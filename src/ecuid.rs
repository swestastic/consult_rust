//! ECU identification: match the part number read from the ECU (0xD0)
//! against the Jim Wolf Technology core list and derive which data-stream
//! items and active tests that ECU is known to support.
//!
//! The source table (tools/extract_ecuid.py output) says nothing about
//! registers directly, so support is inferred from the row attributes:
//! turbo flag/engine, V-engine layout, and '96+ model year. When a part
//! number matches several rows (the list has known mislabels), the
//! union of their attributes is taken so nothing is hidden by one bad row.

mod table;

pub use table::ECU_TABLE;

/// One row of the JWT core list: what a Nissan ECU part number ships in.
pub struct EcuEntry {
    /// The five characters after `23710-`, e.g. "41P70".
    pub part: &'static str,
    /// First listed model year, two digits (0 = unknown).
    pub year: u8,
    pub model: &'static str,
    pub chassis: &'static str,
    pub engine: &'static str,
    pub turbo: bool,
    /// Transmission/region code as printed (AF, MC, A, ...), "" if unknown.
    pub trans: &'static str,
}

/// A capability a data-stream item or active test can require. `Base`
/// items work on every Consult ECU.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    Base,
    /// Turbo hardware: boost/wastegate/EGT sensors, boost-control solenoids.
    Turbo,
    /// V-engine with two banks: RH O2/injector/fuel-trim registers.
    VEngine,
    /// Twin-MAF engines (e.g. RB26DETT); none appear in the current table.
    TwinMaf,
    /// '96-and-newer extended registers (CAL/LD, rear O2, abs TPS, ...).
    Late96,
    /// Variable valve timing (NVCS/VTC) solenoid.
    Vtc,
}

/// The connected ECU, resolved from the table: union of all matching rows.
#[derive(Clone)]
pub struct EcuInfo {
    /// Matched part suffix, e.g. "41P70".
    pub part: String,
    /// One-line description for the UI, e.g.
    /// "'88-'89 300ZX (late) · Z32-VG30DET · turbo · auto".
    pub summary: String,
    pub turbo: bool,
    pub v_engine: bool,
    pub twin_maf: bool,
    pub late96: bool,
    pub vtc: bool,
}

impl EcuInfo {
    pub fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Base => true,
            Feature::Turbo => self.turbo,
            Feature::VEngine => self.v_engine,
            Feature::TwinMaf => self.twin_maf,
            Feature::Late96 => self.late96,
            Feature::Vtc => self.vtc,
        }
    }
}

/// Engines with Nissan VTC/NVCS variable valve timing (active test 0x8F).
/// The Z32's VG30s carry a VTC solenoid on each bank (all but USDM '96);
/// the ECU drives both together through the single 0x8F command. SR20DE is
/// deliberately excluded: the US-market versions in this table shipped
/// without VTC.
const VTC_ENGINES: &[&str] = &["VE30DE", "VG30DE", "VG30DET", "VH41DE", "VH45DE", "VQ30DE"];

/// Twin-MAF engines (RH MAF registers 0x06/0x07).
const TWIN_MAF_ENGINES: &[&str] = &["RB26DETT"];

/// Pull the five-character Nissan part-number suffix out of a 0xD0 response
/// as formatted by `format_part_number`: "0488 23710-41P70", a bare
/// "23710-41P70", the '96+ 32-byte "version 4L618" form, or a bare suffix.
fn part_suffix(text: &str) -> Option<String> {
    let take5 = |s: &str| -> Option<String> {
        let s: String = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(5)
            .collect::<String>()
            .to_uppercase();
        (s.len() == 5).then_some(s)
    };
    if let Some(pos) = text.find("23710-") {
        return take5(&text[pos + 6..]);
    }
    if let Some(pos) = text.to_lowercase().find("version ") {
        return take5(&text[pos + 8..]);
    }
    let trimmed = text.trim();
    if trimmed.len() == 5 {
        return take5(trimmed);
    }
    None
}

/// Identify an ECU from its part-number text. Returns None when the number
/// is unreadable or absent from the table; callers then show all options.
pub fn identify(part_text: &str) -> Option<EcuInfo> {
    let suffix = part_suffix(part_text)?;
    let rows: Vec<&EcuEntry> = ECU_TABLE.iter().filter(|e| e.part == suffix).collect();
    if rows.is_empty() {
        return None;
    }

    let turbo = rows.iter().any(|r| r.turbo);
    let v_engine = rows.iter().any(|r| r.engine.starts_with('V'));
    let twin_maf = rows.iter().any(|r| TWIN_MAF_ENGINES.contains(&r.engine));
    let late96 = rows.iter().any(|r| r.year >= 96);
    let vtc = rows.iter().any(|r| VTC_ENGINES.contains(&r.engine));

    let dedup = |pick: fn(&EcuEntry) -> &'static str| -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for r in &rows {
            let v = pick(r);
            if !v.is_empty() && !out.contains(&v) {
                out.push(v);
            }
        }
        out
    };
    let mut parts: Vec<String> = Vec::new();
    let years: Vec<u8> = rows.iter().map(|r| r.year).filter(|y| *y > 0).collect();
    if let (Some(lo), Some(hi)) = (years.iter().min(), years.iter().max()) {
        parts.push(if lo == hi {
            format!("'{lo:02}")
        } else {
            format!("'{lo:02}-'{hi:02}")
        });
    }
    parts.extend(dedup(|r| r.model).into_iter().map(str::to_string));
    // Chassis-engine pairs dedup together (one chassis can pair with
    // several engines across rows).
    let mut engines: Vec<String> = Vec::new();
    for r in &rows {
        if r.engine.is_empty() {
            continue;
        }
        let s = if r.chassis.is_empty() {
            r.engine.to_string()
        } else {
            format!("{}-{}", r.chassis, r.engine)
        };
        if !engines.contains(&s) {
            engines.push(s);
        }
    }
    if !engines.is_empty() {
        parts.push(engines.join(" / "));
    }
    if turbo {
        parts.push("turbo".into());
    }
    let mut trans: Vec<&str> = Vec::new();
    for t in dedup(|r| r.trans) {
        let label = match t.chars().next() {
            Some('A') => "auto",
            _ => "manual",
        };
        if !trans.contains(&label) {
            trans.push(label);
        }
    }
    for t in trans {
        parts.push(t.to_string());
    }

    Some(EcuInfo {
        part: suffix,
        summary: parts.join(" · "),
        turbo,
        v_engine,
        twin_maf,
        late96,
        vtc,
    })
}

/// Capability gating for the active tests (0x0A kinds); everything not
/// listed is considered universal.
pub fn activation_feature(kind: u8) -> Feature {
    use crate::consult::protocol::activate as a;
    match kind {
        // P/Reg and MAP-switching solenoids are boost-control hardware.
        k if k == a::PREG_SOLENOID || k == a::MAP_SW_SOLENOID => Feature::Turbo,
        k if k == a::TIMING_SOLENOID => Feature::Vtc,
        _ => Feature::Base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_extraction() {
        assert_eq!(part_suffix("0488 23710-41P70").as_deref(), Some("41P70"));
        assert_eq!(part_suffix("23710-56u66").as_deref(), Some("56U66"));
        assert_eq!(part_suffix("version 4L618").as_deref(), Some("4L618"));
        assert_eq!(part_suffix("41P70").as_deref(), Some("41P70"));
        assert_eq!(part_suffix("(no response)"), None);
        assert_eq!(part_suffix(""), None);
    }

    #[test]
    fn identifies_turbo_v6() {
        let info = identify("0488 23710-41P70").expect("41P70 is a Z32 300ZX Turbo");
        assert!(info.turbo);
        assert!(info.v_engine);
        assert!(!info.late96);
        assert!(info.vtc); // VG30DET has VTC on both banks
        assert!(info.supports(Feature::Turbo));
        assert!(info.supports(Feature::VEngine));
        assert!(!info.supports(Feature::Late96));
        assert!(info.summary.contains("300ZX"));
        assert!(info.summary.contains("'91-'92"));
    }

    #[test]
    fn identifies_late_v6() {
        let info = identify("version 56U66").expect("56U66 is a '96 Maxima");
        assert!(info.late96);
        assert!(info.v_engine);
        assert!(!info.turbo);
        assert!(info.vtc); // VQ30DE has VTC
    }

    #[test]
    fn identifies_inline_four() {
        let info = identify("23710-53F10").expect("53F10 is a '90 240SX KA24E");
        assert!(!info.turbo);
        assert!(!info.v_engine);
        assert!(!info.late96);
        assert!(info.supports(Feature::Base));
        assert!(!info.supports(Feature::Turbo));
    }

    #[test]
    fn mislabelled_part_unions_attributes() {
        // 32F12 is listed twice: once as S12-CA18ET (turbo I4) and once as
        // S12-VG30E (NA V6). The union keeps both hardware families.
        let info = identify("23710-32F12").expect("32F12 is in the table");
        assert!(info.turbo);
        assert!(info.v_engine);
    }

    #[test]
    fn unknown_numbers_are_unidentified() {
        assert!(identify("SIMU 23710-LATED").is_none());
        assert!(identify("version ZZZZZ").is_none());
        assert!(identify("(no response)").is_none());
    }

    #[test]
    fn activation_features() {
        use crate::consult::protocol::activate as a;
        assert_eq!(activation_feature(a::PREG_SOLENOID), Feature::Turbo);
        assert_eq!(activation_feature(a::MAP_SW_SOLENOID), Feature::Turbo);
        assert_eq!(activation_feature(a::TIMING_SOLENOID), Feature::Vtc);
        assert_eq!(activation_feature(a::FUEL_PUMP), Feature::Base);
        assert_eq!(activation_feature(a::FAN_LOW), Feature::Base);
    }
}
