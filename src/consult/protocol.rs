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
pub const CMD_ECU_INFO: u8 = 0xD0;
pub const CMD_READ_REGISTER: u8 = 0x5A;
pub const CMD_ACTIVATE: u8 = 0x0A;

/// Active-test ("activation") command type bytes, used as `0x0A <kind> <data> 0xF0`.
pub mod activate {
    pub const COOLANT_TEMP: u8 = 0x80; // data = temp_C + 50
    pub const FUEL_TRIM: u8 = 0x81; // data = 100 + percent (0x64 = normal)
    pub const IGN_TIMING: u8 = 0x82; // data = signed degrees, +1 per degree
    pub const AAC: u8 = 0x84; // data = signed, +1 per 0.5%
    pub const POWER_BALANCE: u8 = 0x88; // data = cylinder bitmask, 1 = off
    pub const FUEL_PUMP: u8 = 0x89; // data = 0x00 on, 0x01 off (some ECUs inverted)
    pub const CLEAR_SELF_LEARN: u8 = 0x8B; // data = 0x00
}

/// Registers requested in data-stream mode, in request order. The ECU answers
/// each frame with one byte per register in this same order.
pub const STREAM_REGISTERS: [u8; 13] = [
    0x00, // CAS position (RPM) MSB
    0x01, // CAS position (RPM) LSB
    0x0B, // vehicle speed
    0x08, // coolant temp
    0x0C, // battery voltage
    0x0D, // TPS voltage
    0x04, // MAF voltage MSB
    0x05, // MAF voltage LSB
    0x17, // AAC valve %
    0x14, // injection time MSB
    0x15, // injection time LSB
    0x16, // ignition timing
    0x09, // LH O2 sensor voltage
];

pub fn init_sequence(ecu_address: u8) -> [u8; 3] {
    [0xFF, 0xFF, ecu_address]
}

/// The ECU replies to init with the bitwise inverse of the address byte
/// (0xEF -> 0x10, 0xED -> 0x12, ...).
pub fn expected_init_reply(ecu_address: u8) -> u8 {
    !ecu_address
}

pub fn stream_command() -> Vec<u8> {
    let mut cmd = Vec::with_capacity(STREAM_REGISTERS.len() * 2 + 1);
    for reg in STREAM_REGISTERS {
        cmd.push(CMD_READ_REGISTER);
        cmd.push(reg);
    }
    cmd.push(TERMINATOR);
    cmd
}

pub fn activation_command(kind: u8, data: u8) -> [u8; 4] {
    [CMD_ACTIVATE, kind, data, TERMINATOR]
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

/// One decoded set of live sensor readings, in raw physical units
/// (kph, deg C, volts) before unit/calibration conversion.
#[derive(Clone, Copy)]
pub struct Readings {
    pub rpm: f64,
    pub speed_kph: f64,
    pub coolant_c: f64,
    pub battery_v: f64,
    pub tps_v: f64,
    pub maf_v: f64,
    pub aac_pct: f64,
    pub inj_ms: f64,
    pub timing_deg: f64,
    pub o2_v: f64,
}

pub fn parse_readings(frame: &[u8]) -> Option<Readings> {
    if frame.len() < STREAM_REGISTERS.len() {
        return None;
    }
    let u16be = |i: usize| ((frame[i] as u16) << 8) | frame[i + 1] as u16;
    Some(Readings {
        rpm: u16be(0) as f64 * 12.5,
        speed_kph: frame[2] as f64 * 2.0,
        coolant_c: frame[3] as f64 - 50.0,
        battery_v: frame[4] as f64 * 80.0 / 1000.0,
        tps_v: frame[5] as f64 * 20.0 / 1000.0,
        maf_v: u16be(6) as f64 * 5.0 / 1000.0,
        aac_pct: frame[8] as f64 / 2.0,
        inj_ms: u16be(9) as f64 / 100.0,
        timing_deg: 110.0 - frame[11] as f64,
        o2_v: frame[12] as f64 * 10.0 / 1000.0,
    })
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
}
