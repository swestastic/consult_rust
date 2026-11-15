#!/usr/bin/env python3
"""Extract the Jim Wolf Technology ECU core identification list into the
Rust table used by the ECU-based option filter (src/ecuid/table.rs).

Usage:
    python tools/extract_ecuid.py [ecu-id.pdf] [--text extracted.txt]

Fetches https://jimwolftechnology.com/wolfpdf/ecu-id.pdf when no file is
given. Requires `pdftotext` (poppler) on PATH. The PDF layout is irregular
(misaligned columns, merged tokens, unknown fields), so parsing is
heuristic; rows without a usable 23710-XXXXX Nissan part number are
skipped. Review the printed stats before committing the output.
"""

import re
import subprocess
import sys
import tempfile
import urllib.request

PDF_URL = "https://jimwolftechnology.com/wolfpdf/ecu-id.pdf"

# Engine codes as printed in the table, longest first so e.g. VG30DETT wins
# over VG30DET. "DH41DE" is the PDF's typo for the Q45's VH41DE; it is
# normalised below.
ENGINES = [
    "VG30DETT", "VG30DET", "VG30DE", "VG30ET", "VG30E", "VG30i",
    "VH45DE", "DH41DE", "VQ30DE", "VE30DE",
    "KA24DE", "KA24E", "SR20DE", "GA16DE", "GA16i",
    "CA18ET", "CA18DE", "CA16DE", "L28ET",
    "E15ET", "E15", "E16i", "E16", "Z24i",
]
NORMALIZE_ENGINE = {"DH41DE": "VH41DE"}
ENGINE_RE = re.compile(
    r"\b(?:([A-Za-z0-9]{2,6})-)?(" + "|".join(ENGINES) + r")\b"
)

PART_RE = re.compile(r"23710-([0-9A-Z?]{5})\b")
YEAR_RE = re.compile(r"^(\d{2}(?:\.\d)?(?:-\d{2})?|\?\?)$")
TRANS_RE = re.compile(r"^[AM][FCUM]$")
MERGED_RE = re.compile(r"^([NT])([AM][FCUM])$")  # e.g. "NA" = N + AF
# Turbo flag + bare trans letter when the region letter was lost: "NA" = N + A.
MERGED_SHORT_RE = re.compile(r"^([NT])([AM])$")
# Manufacturer P/N fragments: "A11-B15", "MECS-G270", "A18-", "JA18-B88".
MFR_RE = re.compile(r"^[A-Z]{0,4}\d{1,2}-[0-9A-Z?]{0,4}$")
# The bare 3-char serial that can follow the P/N ("G11", "475", "0E1").
MFR_SERIAL_RE = re.compile(r"^(?=.*[0-9A-Z])[0-9A-Z]{3}$")

# Tokens that may sit between the year and the turbo/trans flags without
# being part of the car description.
HEADER_WORDS = {"NISSAN", "INFINITI", "ECU", "CORE", "IDENTIFICATION", "LIST"}
# Option-column values that can drift into the description region.
SKIP_WORDS = {"OBD2", "ACT.", "SUS."}


def pdf_text(path: str) -> str:
    out = subprocess.run(
        ["pdftotext", "-layout", path, "-"], capture_output=True, check=True
    )
    return out.stdout.decode("utf-8", errors="replace")


def parse_year(tok: str) -> int:
    """First year of a (possibly ranged/fractional) year token, 0 if unknown."""
    if tok == "??":
        return 0
    m = re.match(r"(\d{2})", tok)
    return int(m.group(1)) if m else 0


def parse_line(line: str, prev_year: int) -> dict | None:
    m = PART_RE.search(line)
    if not m:
        return None
    part = m.group(1)
    if part == "?????":
        return None

    tokens = line[: m.start()].split()

    # Drop the manufacturer P/N: everything up to and including the last
    # year-like token is header material; that token is the year.
    year_idx = None
    for i, tok in enumerate(tokens):
        if YEAR_RE.match(tok):
            year_idx = i
    year = prev_year
    if year_idx is not None:
        year = parse_year(tokens[year_idx])
        tokens = tokens[year_idx + 1 :]
    else:
        # Continuation row (no year of its own): strip any leading
        # manufacturer P/N tokens so they don't leak into the description.
        stripped = False
        while tokens and (MFR_RE.match(tokens[0]) or tokens[0] == "??"):
            tokens.pop(0)
            stripped = True
        if stripped and tokens and MFR_SERIAL_RE.match(tokens[0]):
            tokens.pop(0)

    # Walk the remaining tokens: description words come first, then the
    # turbo flag and transmission code, then the chassis-engine token and
    # trailing noise (OBD2 marks, compression ratios, MEXICO notes...).
    desc: list[str] = []
    turbo_flag = ""
    trans = ""
    engine = ""
    chassis = ""
    i = 0
    while i < len(tokens):
        tok = tokens[i]
        em = ENGINE_RE.search(tok)
        if em:
            chassis = em.group(1) or ""
            engine = em.group(2)
            break
        if tok in ("N", "T") and not turbo_flag:
            turbo_flag = tok
        elif not turbo_flag and (mm := MERGED_RE.match(tok)):
            turbo_flag, trans = mm.group(1), mm.group(2)
        elif not turbo_flag and (mm := MERGED_SHORT_RE.match(tok)):
            turbo_flag, trans = mm.group(1), mm.group(2)
        elif TRANS_RE.match(tok) and not trans:
            trans = tok
        elif tok in ("A", "M") and i + 1 < len(tokens) and tokens[i + 1] in (
            "F",
            "C",
            "U",
            "M",
        ):
            trans = tok + tokens[i + 1]
            i += 1
        elif not turbo_flag and not trans and tok not in HEADER_WORDS | SKIP_WORDS:
            desc.append(tok)
        # Anything after the flags (OBD2, ACT. SUS., ratios) is ignored.
        i += 1

    model = " ".join(desc).strip()
    engine = NORMALIZE_ENGINE.get(engine, engine)
    # Turbo by flag, or by engine suffix (ET/DET/DETT are turbo by design);
    # covers rows where the flag column was lost in the PDF conversion.
    turbo = turbo_flag == "T" or engine.endswith("T")
    return {
        "part": part,
        "year": year,
        "model": model,
        "chassis": chassis,
        "engine": engine,
        "turbo": turbo,
        "trans": trans,
    }


def main() -> None:
    args = sys.argv[1:]
    text = None
    if "--text" in args:
        i = args.index("--text")
        with open(args[i + 1], encoding="utf-8", errors="replace") as f:
            text = f.read()
        args = args[:i] + args[i + 2 :]
    if text is None:
        if args:
            pdf_path = args[0]
        else:
            pdf_path = tempfile.mktemp(suffix=".pdf")
            urllib.request.urlretrieve(PDF_URL, pdf_path)
        text = pdf_text(pdf_path)

    rows = []
    seen = set()
    prev_year = 0
    for line in text.splitlines():
        row = parse_line(line, prev_year)
        if row is None:
            continue
        # Rows carrying only a year (no model/engine/trans) add noise.
        if not row["model"] and not row["engine"] and not row["trans"]:
            continue
        prev_year = row["year"] or prev_year
        key = tuple(row.values())
        if key not in seen:
            seen.add(key)
            rows.append(row)

    rows.sort(key=lambda r: (r["part"], r["year"], r["trans"]))

    out = [
        "//! Generated by tools/extract_ecuid.py from the Jim Wolf Technology",
        "//! ECU core identification list (https://jimwolftechnology.com/wolfpdf/ecu-id.pdf).",
        "//! The source list is incomplete and may contain errors; it is only a",
        "//! guide. Regenerate with: python tools/extract_ecuid.py",
        "",
        "use super::EcuEntry;",
        "",
        "/// Sorted by part suffix. `year` is the first listed year (0 = unknown).",
        "pub static ECU_TABLE: &[EcuEntry] = &[",
    ]
    for r in rows:
        model = r["model"].replace('"', "'")
        out.append(
            '    EcuEntry {{ part: "{part}", year: {year}, model: "{model}", '
            'chassis: "{chassis}", engine: "{engine}", turbo: {turbo}, '
            'trans: "{trans}" }},'.format(
                part=r["part"],
                year=r["year"],
                model=model,
                chassis=r["chassis"],
                engine=r["engine"],
                turbo=str(r["turbo"]).lower(),
                trans=r["trans"],
            )
        )
    out.append("];\n")

    with open("src/ecuid/table.rs", "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(out))

    engines = {r["engine"] for r in rows}
    print(f"wrote src/ecuid/table.rs: {len(rows)} rows, "
          f"{len({r['part'] for r in rows})} unique part numbers")
    print(f"engines seen: {sorted(engines)}")
    print(f"turbo rows: {sum(r['turbo'] for r in rows)}")
    missing_year = sum(1 for r in rows if r['year'] == 0)
    missing_engine = sum(1 for r in rows if not r['engine'])
    print(f"rows with unknown year: {missing_year}, unknown engine: {missing_engine}")


if __name__ == "__main__":
    main()
