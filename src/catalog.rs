//! Catalog of selectable data-stream items, mirroring the register options
//! from the consult_box prototype. Each item maps to one or two Consult
//! registers and knows how to decode and display itself.
//!
//! The ECU streams one byte per requested register, in request order; items
//! with two registers (e.g. RPM) read them as a big-endian 16-bit pair.

use crate::config::Config;

/// The ECU accepts at most 20 registers per stream request.
pub const MAX_STREAM_REGISTERS: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Rpm,
    Speed,
    Temp,
    Volts,
    Percent,
    Ms,
    Deg,
    /// Raw byte, shown as hex/binary (digital bit registers).
    Raw,
}

pub struct SensorSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub regs: &'static [u8],
    pub unit: UnitKind,
    /// Base gauge range; Speed/Temp items are re-ranged per unit at display.
    pub range: (f64, f64),
    pub decimals: usize,
}

pub const CATALOG: &[SensorSpec] = &[
    SensorSpec { id: "rpm",       label: "RPM",              regs: &[0x00, 0x01], unit: UnitKind::Rpm,     range: (0.0, 8000.0),  decimals: 0 },
    SensorSpec { id: "speed",     label: "Speed",            regs: &[0x0B],       unit: UnitKind::Speed,   range: (0.0, 160.0),   decimals: 0 },
    SensorSpec { id: "coolant",   label: "Coolant",          regs: &[0x08],       unit: UnitKind::Temp,    range: (100.0, 250.0), decimals: 0 },
    SensorSpec { id: "battery",   label: "Battery",          regs: &[0x0C],       unit: UnitKind::Volts,   range: (8.0, 16.0),    decimals: 1 },
    SensorSpec { id: "tps",       label: "TPS",              regs: &[0x0D],       unit: UnitKind::Volts,   range: (0.0, 5.0),     decimals: 2 },
    SensorSpec { id: "maf",       label: "MAF",              regs: &[0x04, 0x05], unit: UnitKind::Volts,   range: (0.0, 5.0),     decimals: 2 },
    SensorSpec { id: "maf_rh",    label: "MAF RH",           regs: &[0x06, 0x07], unit: UnitKind::Volts,   range: (0.0, 5.0),     decimals: 2 },
    SensorSpec { id: "o2_lh",     label: "O2 LH",            regs: &[0x09],       unit: UnitKind::Volts,   range: (0.0, 1.2),     decimals: 2 },
    SensorSpec { id: "o2_rh",     label: "O2 RH",            regs: &[0x0A],       unit: UnitKind::Volts,   range: (0.0, 1.2),     decimals: 2 },
    SensorSpec { id: "fuel_temp", label: "Fuel Temp",        regs: &[0x0F],       unit: UnitKind::Temp,    range: (-40.0, 250.0), decimals: 0 },
    SensorSpec { id: "iat",       label: "Intake Air Temp",  regs: &[0x11],       unit: UnitKind::Temp,    range: (-40.0, 250.0), decimals: 0 },
    SensorSpec { id: "egt",       label: "EGT",              regs: &[0x12],       unit: UnitKind::Volts,   range: (0.0, 5.0),     decimals: 2 },
    SensorSpec { id: "inj_lh",    label: "Injector LH",      regs: &[0x14, 0x15], unit: UnitKind::Ms,      range: (0.0, 25.0),    decimals: 2 },
    SensorSpec { id: "inj_rh",    label: "Injector RH",      regs: &[0x22, 0x23], unit: UnitKind::Ms,      range: (0.0, 25.0),    decimals: 2 },
    SensorSpec { id: "timing",    label: "Timing",           regs: &[0x16],       unit: UnitKind::Deg,     range: (-10.0, 50.0),  decimals: 0 },
    SensorSpec { id: "aac",       label: "AAC",              regs: &[0x17],       unit: UnitKind::Percent, range: (0.0, 100.0),   decimals: 0 },
    SensorSpec { id: "alpha_lh",  label: "AF Alpha LH",      regs: &[0x1A],       unit: UnitKind::Percent, range: (50.0, 150.0),  decimals: 0 },
    SensorSpec { id: "alpha_rh",  label: "AF Alpha RH",      regs: &[0x1B],       unit: UnitKind::Percent, range: (50.0, 150.0),  decimals: 0 },
    SensorSpec { id: "learn_lh",  label: "AF Learn LH",      regs: &[0x1C],       unit: UnitKind::Percent, range: (50.0, 150.0),  decimals: 0 },
    SensorSpec { id: "learn_rh",  label: "AF Learn RH",      regs: &[0x1D],       unit: UnitKind::Percent, range: (50.0, 150.0),  decimals: 0 },
    SensorSpec { id: "d13",       label: "Digital 0x13",     regs: &[0x13],       unit: UnitKind::Raw,     range: (0.0, 255.0),   decimals: 0 },
    SensorSpec { id: "d1e",       label: "Digital 0x1E",     regs: &[0x1E],       unit: UnitKind::Raw,     range: (0.0, 255.0),   decimals: 0 },
    SensorSpec { id: "d1f",       label: "Digital 0x1F",     regs: &[0x1F],       unit: UnitKind::Raw,     range: (0.0, 255.0),   decimals: 0 },
    SensorSpec { id: "d21",       label: "Digital 0x21",     regs: &[0x21],       unit: UnitKind::Raw,     range: (0.0, 255.0),   decimals: 0 },
];

/// What a fresh install displays (the original 8 gauges + MAF and O2).
pub const DEFAULT_SELECTION: &[&str] = &[
    "rpm", "speed", "coolant", "battery", "tps", "maf", "aac", "inj_lh", "timing", "o2_lh",
];

pub fn find_spec(id: &str) -> Option<&'static SensorSpec> {
    CATALOG.iter().find(|s| s.id == id)
}

/// Name of a digital-register bit, per the consult_box bit map
/// (and the register PDF). Unknown bits return None.
pub fn digital_bit_name(spec_id: &str, bit: u8) -> Option<&'static str> {
    let table: &[(u8, &str)] = match spec_id {
        "d13" => &[
            (4, "A/C Switch"),
            (3, "Power Steering Sw"),
            (2, "Park/Neutral Sw"),
            (1, "Start Signal"),
            (0, "TPS Closed"),
        ],
        "d1e" => &[
            (7, "A/C Relay"),
            (6, "Fuel Pump Relay"),
            (5, "VTC Solenoid"),
            (1, "Coolant Fan Hi"),
            (0, "Coolant Fan Lo"),
        ],
        "d1f" => &[
            (6, "P/Reg Control Valve"),
            (5, "Wastegate Sol"),
            (3, "IACV/FICD Sol"),
            (0, "EGR Solenoid"),
        ],
        "d21" => &[(7, "LH Bank Lean"), (6, "RH Bank Lean")],
        _ => return None,
    };
    table.iter().find(|(b, _)| *b == bit).map(|(_, n)| *n)
}

pub fn is_digital(spec: &SensorSpec) -> bool {
    spec.unit == UnitKind::Raw
}

/// Selected specs in catalog order.
pub fn selected_specs(ids: &[String]) -> Vec<&'static SensorSpec> {
    CATALOG.iter().filter(|s| ids.iter().any(|i| i == s.id)).collect()
}

/// Flat register list for a stream request (MSB/LSB pairs expanded).
pub fn expand_registers(specs: &[&SensorSpec]) -> Vec<u8> {
    let mut regs = Vec::new();
    for s in specs {
        regs.extend_from_slice(s.regs);
    }
    regs.truncate(MAX_STREAM_REGISTERS);
    regs
}

pub fn register_count(specs: &[&SensorSpec]) -> usize {
    specs.iter().map(|s| s.regs.len()).sum()
}

/// Drop unknown/duplicate ids and enforce the 20-register cap.
pub fn sanitize_selection(ids: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut regs = 0usize;
    for id in ids {
        let Some(spec) = find_spec(&id) else { continue };
        if out.contains(&id) {
            continue;
        }
        if regs + spec.regs.len() > MAX_STREAM_REGISTERS {
            continue;
        }
        regs += spec.regs.len();
        out.push(id);
    }
    if out.is_empty() {
        DEFAULT_SELECTION.iter().map(|s| s.to_string()).collect()
    } else {
        out
    }
}

fn u16be(frame: &[u8], stream_regs: &[u8], msb: u8, lsb: u8) -> Option<u16> {
    let hi = byte_of(frame, stream_regs, msb)?;
    let lo = byte_of(frame, stream_regs, lsb)?;
    Some(((hi as u16) << 8) | lo as u16)
}

fn byte_of(frame: &[u8], stream_regs: &[u8], reg: u8) -> Option<u8> {
    stream_regs
        .iter()
        .position(|r| *r == reg)
        .and_then(|p| frame.get(p).copied())
}

impl SensorSpec {
    /// Decode this item's value from a stream frame, converted to the
    /// configured display units.
    pub fn decode(&self, frame: &[u8], stream_regs: &[u8], cfg: &Config) -> Option<f64> {
        let raw = || byte_of(frame, stream_regs, self.regs[0]);
        let v = match self.id {
            "rpm" => u16be(frame, stream_regs, 0x00, 0x01)? as f64 * 12.5,
            "maf" | "maf_rh" => {
                u16be(frame, stream_regs, self.regs[0], self.regs[1])? as f64 * 5.0 / 1000.0
            }
            "inj_lh" | "inj_rh" => {
                u16be(frame, stream_regs, self.regs[0], self.regs[1])? as f64 / 100.0
            }
            "coolant" | "fuel_temp" | "iat" => cfg.display_temp(raw()? as f64 - 50.0),
            "speed" => cfg.display_speed(raw()? as f64 * 2.0),
            "battery" => raw()? as f64 * 80.0 / 1000.0,
            "tps" | "egt" => raw()? as f64 * 20.0 / 1000.0,
            "o2_lh" | "o2_rh" => raw()? as f64 * 10.0 / 1000.0,
            "timing" => 110.0 - raw()? as f64,
            "aac" => raw()? as f64 / 2.0,
            "alpha_lh" | "alpha_rh" | "learn_lh" | "learn_rh" => raw()? as f64,
            _ => raw()? as f64, // digital registers and unknown items
        };
        Some(v)
    }

    pub fn unit_label(&self, cfg: &Config) -> &'static str {
        self.unit_in(cfg.speed_unit.label(), cfg.temp_unit.label())
    }

    pub fn unit_in(&self, speed_unit: &str, temp_unit: &str) -> &'static str {
        match self.unit {
            UnitKind::Rpm => "RPM",
            UnitKind::Speed => {
                if speed_unit == "MPH" { "MPH" } else { "KPH" }
            }
            UnitKind::Temp => {
                if temp_unit == "F" { "°F" } else { "°C" }
            }
            UnitKind::Volts => "V",
            UnitKind::Percent => "%",
            UnitKind::Ms => "ms",
            UnitKind::Deg => "°BTDC",
            UnitKind::Raw => "raw",
        }
    }

    /// Gauge title including unit, e.g. "Speed (MPH)".
    pub fn title(&self, cfg: &Config) -> String {
        format!("{} ({})", self.label, self.unit_label(cfg))
    }

    pub fn range_in(&self, speed_unit: &str, temp_unit: &str) -> (f64, f64) {
        match self.unit {
            UnitKind::Speed => {
                if speed_unit == "MPH" { (0.0, 160.0) } else { (0.0, 260.0) }
            }
            UnitKind::Temp => {
                let (lo, hi) = self.range;
                if temp_unit == "F" {
                    (lo, hi)
                } else {
                    // Ranges above are authored in F.
                    ((lo - 32.0) * 5.0 / 9.0, (hi - 32.0) * 5.0 / 9.0)
                }
            }
            _ => self.range,
        }
    }

    pub fn range_for(&self, cfg: &Config) -> (f64, f64) {
        self.range_in(cfg.speed_unit.label(), cfg.temp_unit.label())
    }

    /// Register list shown in the settings UI, e.g. "0x00/0x01".
    pub fn regs_display(&self) -> String {
        self.regs
            .iter()
            .map(|r| format!("0x{r:02X}"))
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// Map a log column title back to a spec (for replay gauges). Checked in
/// catalog order; longest/most-specific labels first.
pub fn spec_for_column(column: &str) -> Option<&'static SensorSpec> {
    let c = column.trim();
    for (prefix, id) in [
        ("RPM", "rpm"),
        ("Speed", "speed"),
        ("Coolant", "coolant"),
        ("Temp", "coolant"), // legacy Python column name
        ("Fuel Temp", "fuel_temp"),
        ("Intake Air Temp", "iat"),
        ("Battery", "battery"),
        ("TPS", "tps"),
        ("MAF RH", "maf_rh"),
        ("MAF", "maf"),
        ("O2 RH", "o2_rh"),
        ("O2", "o2_lh"),
        ("EGT", "egt"),
        ("Injector RH", "inj_rh"),
        ("Injector", "inj_lh"),
        ("Timing", "timing"),
        ("AAC", "aac"),
        ("AF Alpha LH", "alpha_lh"),
        ("AF Alpha RH", "alpha_rh"),
        ("AF Learn LH", "learn_lh"),
        ("AF Learn RH", "learn_rh"),
        ("Digital 0x13", "d13"),
        ("Digital 0x1E", "d1e"),
        ("Digital 0x1F", "d1f"),
        ("Digital 0x21", "d21"),
    ] {
        if c.starts_with(prefix) {
            return find_spec(id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_selection_fits() {
        let ids: Vec<String> = DEFAULT_SELECTION.iter().map(|s| s.to_string()).collect();
        let specs = selected_specs(&ids);
        assert_eq!(specs.len(), 10);
        assert_eq!(register_count(&specs), 13);
        assert!(register_count(&specs) <= MAX_STREAM_REGISTERS);
    }

    #[test]
    fn decode_rpm_16bit() {
        let cfg = Config::default();
        let spec = find_spec("rpm").unwrap();
        let regs = vec![0x00, 0x01];
        // 7000 RPM -> raw 560 = 0x0230
        let frame = vec![0x02, 0x30];
        let v = spec.decode(&frame, &regs, &cfg).unwrap();
        assert!((v - 7000.0).abs() < 1.0);
    }

    #[test]
    fn decode_speed_mph() {
        let cfg = Config::default();
        let spec = find_spec("speed").unwrap();
        let regs = vec![0x0B];
        let v = spec.decode(&[60], &regs, &cfg).unwrap(); // 120 kph
        assert!((v - 74.56).abs() < 0.1);
    }

    #[test]
    fn sanitize_caps_and_dedupes() {
        let everything: Vec<String> = CATALOG.iter().map(|s| s.id.to_string()).collect();
        let clean = sanitize_selection(everything);
        let specs = selected_specs(&clean);
        assert!(register_count(&specs) <= MAX_STREAM_REGISTERS);
        assert!(sanitize_selection(vec![]).len() == DEFAULT_SELECTION.len());
    }

    #[test]
    fn digital_bit_names_match_consult_box() {
        assert_eq!(digital_bit_name("d13", 4), Some("A/C Switch"));
        assert_eq!(digital_bit_name("d13", 0), Some("TPS Closed"));
        assert_eq!(digital_bit_name("d1e", 6), Some("Fuel Pump Relay"));
        assert_eq!(digital_bit_name("d1f", 5), Some("Wastegate Sol"));
        assert_eq!(digital_bit_name("d21", 7), Some("LH Bank Lean"));
        assert_eq!(digital_bit_name("d13", 7), None);
        assert_eq!(digital_bit_name("rpm", 0), None);
    }
}
