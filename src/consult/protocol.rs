//! Nissan Consult (14-pin, pre-OBD-II) protocol definitions.
//!
//! Frame format from the ECU: `0xFF <len> <len data bytes>`, streamed
//! continuously until the host sends `STOP` (0x30). Commands are echoed
//! back with all bits inverted as an ACK; raw data bytes are echoed as-is.

pub const BAUD_RATE: u32 = 9600;

pub const TERMINATOR: u8 = 0xF0;
pub const STOP: u8 = 0x30;
pub const STOP_ACK: u8 = 0xCF;
pub const FRAME_START: u8 = 0xFF;

pub const CMD_READ_DTC: u8 = 0xD1;
pub const CMD_CLEAR_DTC: u8 = 0xC1;
/// Clears the transient DTC set, then reports codes as faults re-occur.
pub const CMD_TRANSIENT_DTC: u8 = 0x51;
pub const CMD_ECU_INFO: u8 = 0xD0;
pub const CMD_READ_REGISTER: u8 = 0x5A;
pub const CMD_ACTIVATE: u8 = 0x0A;
/// Reads internal ROM/RAM bytes, up to 8 per request.
pub const CMD_READ_ROM: u8 = 0xC9;
/// Max bytes per ROM read request.
pub const ROM_CHUNK: u8 = 8;

/// Active-test ("activation") command type bytes, used as `0x0A <kind> <data> 0xF0`.
pub mod activate {
    pub const COOLANT_TEMP: u8 = 0x80; // data = temp_C + 50
    pub const FUEL_TRIM: u8 = 0x81; // data = 100 + percent (0x64 = normal)
    pub const IGN_TIMING: u8 = 0x82; // data = signed degrees, +1 per degree
    pub const AAC: u8 = 0x84; // data = signed, +1 per 0.5%
    pub const IACV_OPENING: u8 = 0x85; // data = 0x00..0x8C = 0..100%
    pub const FICD_SOLENOID: u8 = 0x87; // data = 0x00 off, 0x01 on
    pub const POWER_BALANCE: u8 = 0x88; // data = cylinder bitmask, 1 = off
    pub const FUEL_PUMP: u8 = 0x89; // data = 0x00 on, 0x01 off (some ECUs inverted)
    pub const PREG_SOLENOID: u8 = 0x8A; // data = 0x00 off, 0x01 on
    pub const CLEAR_SELF_LEARN: u8 = 0x8B; // data = 0x00
    pub const AACV_CONTROL_VALVE: u8 = 0x8C; // data = 0x00 off, 0x01 on
    pub const TIMING_SOLENOID: u8 = 0x8F; // data = 0x00 off, 0x01 on
    pub const MAP_SW_SOLENOID: u8 = 0x90; // data = 0x00 off, 0x01 on
    pub const FAN_LOW: u8 = 0x93; // data = 0x00 off, 0x01 on
    pub const FAN_HIGH: u8 = 0x94; // data = 0x00 off, 0x01 on
}

pub fn init_sequence(ecu_address: u8) -> [u8; 3] {
    [0xFF, 0xFF, ecu_address]
}

/// The ECU replies to init with the bitwise inverse of the address byte
/// (0xEF -> 0x10, 0xED -> 0x12, ...).
pub fn expected_init_reply(ecu_address: u8) -> u8 {
    !ecu_address
}

/// Build a stream request for the given registers (max 20 per the protocol).
/// The ECU answers each frame with one byte per register in this same order.
pub fn build_stream_command(registers: &[u8]) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(registers.len() * 2 + 1);
    for reg in registers {
        cmd.push(CMD_READ_REGISTER);
        cmd.push(*reg);
    }
    cmd.push(TERMINATOR);
    cmd
}

pub fn activation_command(kind: u8, data: u8) -> [u8; 4] {
    [CMD_ACTIVATE, kind, data, TERMINATOR]
}

/// Build a ROM read request for `n` consecutive bytes starting at `addr`
/// (n <= 8). Response is one frame with the n data bytes, repeated until
/// stopped like any other query.
pub fn build_rom_request(addr: u16, n: u8) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(n as usize * 3 + 1);
    for i in 0..n {
        let a = addr.wrapping_add(i as u16);
        cmd.extend_from_slice(&[CMD_READ_ROM, (a >> 8) as u8, a as u8]);
    }
    cmd.push(TERMINATOR);
    cmd
}

/// Parse and validate a ROM dump range from hex text fields.
/// Address space is 16-bit (ROM 0x8000-0xFFFF; RAM below, per the docs).
pub fn parse_rom_range(start: &str, len: &str) -> Result<(u16, u32), String> {
    let parse = |name: &str, v: &str| -> Result<u32, String> {
        let t = v
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        u32::from_str_radix(t, 16).map_err(|_| format!("{name}: \"{v}\" is not hex"))
    };
    let start = parse("Start address", start)?;
    let len = parse("Length", len)?;
    if start > 0xFFFF {
        return Err("Start address is 16-bit (0000-FFFF)".into());
    }
    if len == 0 {
        return Err("Length must be non-zero".into());
    }
    if start + len > 0x10000 {
        return Err("Start + length runs past 0xFFFF".into());
    }
    Ok((start as u16, len))
}

/// Incremental parser for ECU response frames.
pub struct FrameParser {
    state: State,
}

enum State {
    Hunt,
    Len,
    Data { buf: Vec<u8>, need: usize },
}

pub enum ParserOutput {
    Frame(Vec<u8>),
    StopAck,
}

impl Default for FrameParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameParser {
    pub fn new() -> Self {
        Self { state: State::Hunt }
    }

    pub fn feed(&mut self, byte: u8) -> Option<ParserOutput> {
        loop {
            match &mut self.state {
                State::Hunt => match byte {
                    FRAME_START => {
                        self.state = State::Len;
                        return None;
                    }
                    STOP_ACK => return Some(ParserOutput::StopAck),
                    // Command echoes and noise land here and are skipped.
                    _ => return None,
                },
                State::Len => {
                    if byte == 0 || byte > 64 {
                        // Implausible length: treat as noise, resync (the byte
                        // may itself be a frame start).
                        self.state = State::Hunt;
                        continue;
                    }
                    self.state = State::Data {
                        buf: Vec::with_capacity(byte as usize),
                        need: byte as usize,
                    };
                    return None;
                }
                State::Data { buf, need } => {
                    buf.push(byte);
                    if buf.len() >= *need {
                        let done = std::mem::replace(&mut self.state, State::Hunt);
                        if let State::Data { buf, .. } = done {
                            return Some(ParserOutput::Frame(buf));
                        }
                    }
                    return None;
                }
            }
        }
    }
}

/// DTCs are reported as (code, engine-starts-since-last-occurrence) pairs.
pub fn parse_dtc_frames(frames: &[Vec<u8>]) -> Vec<(u8, u8)> {
    let mut out: Vec<(u8, u8)> = Vec::new();
    for frame in frames {
        for pair in frame.chunks_exact(2) {
            let entry = (pair[0], pair[1]);
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
    }
    out
}

/// Assemble a readable ECU part number from a 0xD0 response frame.
/// Generic ECUs send 22 bytes containing an ASCII `... 23710-YYYYYY`
/// run; '96+ ECUs send 32 bytes with the version in the last 5 bytes.
/// Falls back to whatever printable ASCII the frame contains.
pub fn format_part_number(frame: &[u8]) -> String {
    let ascii: String = frame
        .iter()
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .map(|b| *b as char)
        .collect();
    let trimmed = ascii.trim();
    if trimmed.is_empty() {
        return "(no response)".to_string();
    }
    if let Some(pos) = trimmed.find("23710") {
        let before = trimmed[..pos].trim_end();
        let prefix: String = before
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let version: String = trimmed[pos + 5..]
            .trim_start_matches(['-', ' '])
            .chars()
            .take(6)
            .collect();
        if !version.is_empty() {
            return if prefix.is_empty() {
                format!("23710-{version}")
            } else {
                format!("{prefix} 23710-{version}")
            };
        }
    }
    if frame.len() == 32 {
        let tail: String = frame[27..]
            .iter()
            .filter(|b| b.is_ascii_graphic())
            .map(|b| *b as char)
            .collect();
        if !tail.is_empty() {
            return format!("version {tail}");
        }
    }
    trimmed.to_string()
}

/// Classic Nissan Consult self-diagnosis codes (two-digit hex, e.g. 0x13).
pub fn dtc_description(code: u8) -> &'static str {
    match code {
        0x11 => "Crank angle sensor circuit",
        0x12 => "Mass air flow sensor circuit",
        0x13 => "Engine coolant temperature sensor circuit",
        0x14 => "Vehicle speed sensor circuit",
        0x21 => "Ignition signal circuit",
        0x22 => "Fuel pump circuit",
        0x23 => "Throttle position sensor (idle switch)",
        0x24 => "Park/Neutral position switch",
        0x25 => "AAC valve / idle air control circuit",
        0x26 => "Boost pressure sensor circuit",
        0x28 => "Engine overheat",
        0x31 => "ECU internal fault",
        0x32 => "EGR function",
        0x33 => "Heated oxygen sensor (LH)",
        0x34 => "Knock sensor circuit",
        0x35 => "Exhaust gas temperature sensor",
        0x36 => "EGR back-pressure transducer",
        0x37 => "Closed loop control",
        0x41 => "Intake air temperature sensor circuit",
        0x42 => "Fuel temperature sensor circuit",
        0x43 => "Throttle position sensor circuit",
        0x44 => "System OK",
        0x45 => "Injector leak",
        0x46 => "Secondary throttle position sensor",
        0x51 => "Injector circuit",
        0x53 => "Heated oxygen sensor (RH)",
        0x54 => "A/T control communication",
        0x55 => "No malfunction",
        _ => "Unknown code",
    }
}

pub fn to_hex(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .collect();
    if cleaned.is_empty() {
        return Err("Empty command".into());
    }
    if cleaned.len() % 2 != 0 {
        return Err("Odd number of hex digits".into());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|_| format!("Invalid hex byte: {}", &cleaned[i..i + 2]))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_parser_basic() {
        let mut p = FrameParser::new();
        let bytes = [0x2E, 0xFF, 0x02, 0x55, 0x00, 0xCF];
        let mut frames = vec![];
        let mut stops = 0;
        for b in bytes {
            match p.feed(b) {
                Some(ParserOutput::Frame(f)) => frames.push(f),
                Some(ParserOutput::StopAck) => stops += 1,
                None => {}
            }
        }
        assert_eq!(frames, vec![vec![0x55, 0x00]]);
        assert_eq!(stops, 1);
    }

    #[test]
    fn frame_parser_resync() {
        let mut p = FrameParser::new();
        assert!(p.feed(0xFF).is_none());
        assert!(p.feed(0xFF).is_none()); // bogus length -> resync, FF restarts
        assert!(p.feed(0x01).is_none());
        assert_eq!(
            matches!(p.feed(0x42), Some(ParserOutput::Frame(ref f)) if f == &vec![0x42u8]),
            true
        );
    }

    #[test]
    fn dtc_pairs() {
        let frames = vec![vec![0x13, 0x00, 0x08, 0x03], vec![0x13, 0x00, 0x08, 0x03]];
        assert_eq!(parse_dtc_frames(&frames), vec![(0x13, 0x00), (0x08, 0x03)]);
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(parse_hex("5A 01 f0").unwrap(), vec![0x5A, 0x01, 0xF0]);
        assert_eq!(parse_hex("5a01F0").unwrap(), vec![0x5A, 0x01, 0xF0]);
        assert!(parse_hex("5A1").is_err());
    }

    #[test]
    fn part_number_formats() {
        // Generic 22-byte frame with the part number as ASCII.
        let mut frame = vec![0u8; 22];
        let text = b"0488 23710-50F00";
        frame[3..3 + text.len()].copy_from_slice(text);
        assert_eq!(format_part_number(&frame), "0488 23710-50F00");

        // '96+ 32-byte frame: version is the last 5 bytes.
        let mut frame = vec![0u8; 32];
        frame[27..].copy_from_slice(b"4L618");
        assert_eq!(format_part_number(&frame), "version 4L618");

        // No printable content at all.
        assert_eq!(format_part_number(&[0x01, 0x02]), "(no response)");
    }

    #[test]
    fn rom_request_builds() {
        // 3 bytes at 0x8000: C9 80 00 C9 80 01 C9 80 02 F0
        assert_eq!(
            build_rom_request(0x8000, 3),
            vec![0xC9, 0x80, 0x00, 0xC9, 0x80, 0x01, 0xC9, 0x80, 0x02, 0xF0]
        );
    }

    #[test]
    fn rom_range_validates() {
        assert_eq!(parse_rom_range("8000", "8000").unwrap(), (0x8000, 0x8000));
        assert_eq!(parse_rom_range("0x8000", "100").unwrap(), (0x8000, 0x100));
        assert!(parse_rom_range("8000", "8001").is_err()); // runs past 0xFFFF
        assert!(parse_rom_range("8000", "0").is_err());
        assert!(parse_rom_range("10000", "10").is_err());
        assert!(parse_rom_range("xyz", "10").is_err());
    }
}
