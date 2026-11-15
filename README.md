# Consult Rust

A cross-platform Nissan Consult (14-pin, pre-OBD-II) diagnostic tool, rewritten in
Rust from the PyConsult prototype (`PyConsult/`). Works with most 90s-era Nissans
(240sx, 300zx, Skyline, Laurel, ...) through a USB Consult cable.

## Features

- Live data stream with gauges: RPM, speed, coolant temp, TPS, timing, battery,
  AAC, injector pulse, plus MAF/O2 readouts
- CSV data logging to `logs/`
- Log replay: load saved logs, play them back through the gauges with
  play/pause/speed/scrub controls, and inspect channels on a time-series plot
  (works without an ECU connection)
- DTC read/erase with code descriptions, ECU part number readout
- Active tests: fuel pump relay, power balance (per-cylinder cut), coolant temp
  bias, fuel trim, timing adjust, AAC adjust, clear self-learn
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
