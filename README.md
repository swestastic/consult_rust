# Consult Rust

A cross-platform Nissan Consult (14-pin, pre-OBD-II) diagnostic tool, rewritten in
Rust from the PyConsult prototype (`PyConsult/`). Works with most 90s-era Nissans
(240sx, 300zx, Skyline, Laurel, ...) through a USB Consult cable.

## Features

- Live data stream with gauges: RPM, speed, coolant temp, TPS, timing, battery,
  AAC, injector pulse, plus MAF/O2 readouts
- Per-session peak recall: each gauge shows its observed maximum; reset from
  the Data Stream controls
- Rolling live plot (last 60 s) under the gauges, with per-channel toggles and
  a normalize option
- Selectable display items: pick which ECU registers to stream (Settings ->
  Displayed items); 24 known sensors/registers including dual-bank MAF/O2/
  injector, AF alpha/self-learn, and raw digital bit registers
- Late-model registers: CAS reference, wastegate duty, boost sensor, CAL/LD
  engine load, rear O2, absolute TPS, MAF %, absolute pressure sensor
- Digital bit registers (0x13/0x1E/0x1F/0x21) decode to named on/off
  indicators (fuel pump relay, A/C switch, coolant fans, ...) in a compact
  region below the gauges
- CSV data logging to `logs/`
- Log replay: load saved logs, play them back through the gauges with
  play/pause/speed/scrub controls, and inspect channels on a time-series plot
  (works without an ECU connection)
- DTC read/erase with code descriptions, ECU part number readout
- Transient DTC watch (0x51): clears the transient code set, then reports
  codes as faults re-occur — useful for wiggle-testing connectors
- ROM dump (0xC9): read ECU memory 8 bytes per request to a `.bin` file in
  `dumps/`, with progress and cancel
- Active tests: fuel pump relay, power balance (per-cylinder cut), coolant temp
  bias, fuel trim, timing adjust, AAC adjust, IACV opening, clear self-learn,
  plus on/off overrides for the FICD/P-Reg/AAC-V/timing/MAP-SW solenoids and
  the low/high coolant fans. Tests latch until cancelled with STOP (0x30), per
  the protocol docs; a Base idle / timing mode holds the AAC valve and timing
  so base idle can be set without unplugging the TPS
- Raw hex command console
- Test mode with simulated data (no hardware needed)
- Unit selection (MPH/KPH, F/C), speedometer correction for tire/final-drive
  changes, configurable warning thresholds and ECU address

## Protocol notes

Implemented from the documentation in `PyConsult/Documentation/`:

- 9600 8N1, init `FF FF <addr>` (default `EF`; some '96+ ECUs use `ED`), ECU
  replies with the inverted address byte
- Commands are ACKed with the inverted command byte; `F0` terminates a command
  and starts the response stream; `30` stops it; the ECU answers `CF`
- Response frames: `FF <len> <len data bytes>`
- DTC read `D1`, clear `C1`, ECU part number `D0`
- Register streaming: `5A <reg>` pairs + `F0` (max 20 registers)
- Active tests: `0A <type> <data> F0`
- Transient DTC watch: `51 F0` clears the transient set, then the ECU reports
  codes as they re-occur; `C9 <hi> <lo>` (max 8 per request) reads ROM/RAM

RPM and injector pulse are read as proper 16-bit MSB/LSB pairs (the Python
prototype read only the LSB, which saturates above ~3200 RPM). Vehicle speed
uses the documented x2 kph scale.

## Building

Requires Rust 1.95+ (edition 2024).

```
cargo build --release
```

- **Windows**: works out of the box (MSVC toolchain).
- **macOS**: works out of the box (Apple Silicon and Intel).
- **Linux**: needs a few system packages first:
  ```
  sudo apt-get install libudev-dev libxcb-render0-dev libxcb-shape0-dev \
      libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
  ```

macOS and Linux binaries can't be cross-compiled from a Windows host; the
GitHub Actions workflow in `.github/workflows/build.yml` builds all four
targets (Windows, macOS ARM, macOS Intel, Linux) and uploads binaries as
artifacts.

## Running

```
cargo run --release
```

On the Connect tab, pick your adapter's serial port (or "Test Mode") and hit
Connect. Settings are stored in `consult_config.json` in the working directory;
logs go to `logs/`.
