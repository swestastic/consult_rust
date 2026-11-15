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
    /// What the item measures, shown in the settings hover tooltip.
    pub desc: &'static str,
    /// Expected readings, shown in the settings hover tooltip.
    pub typical: &'static str,
}

pub const CATALOG: &[SensorSpec] = &[
    SensorSpec {
        id: "rpm",
        label: "RPM",
        regs: &[0x00, 0x01],
        unit: UnitKind::Rpm,
        range: (0.0, 8000.0),
        decimals: 0,
        desc: "Engine speed from the crank angle sensor (CAS). 16-bit value x 12.5.",
        typical: "~700-900 at warm idle; redline ~6400-7500 depending on engine.",
    },
    SensorSpec {
        id: "speed",
        label: "Speed",
        regs: &[0x0B],
        unit: UnitKind::Speed,
        range: (0.0, 160.0),
        decimals: 0,
        desc: "Vehicle speed sensor, raw x 2 km/h, then corrected by your tire/final-drive settings.",
        typical: "0 at rest. Compare against the speedometer to dial in correction.",
    },
    SensorSpec {
        id: "coolant",
        label: "Coolant",
        regs: &[0x08],
        unit: UnitKind::Temp,
        range: (100.0, 250.0),
        decimals: 0,
        desc: "Engine coolant temperature sensor; raw - 50 = deg C.",
        typical: "Fully warm ~180-200 F. Sustained >220 F is overheating territory.",
    },
    SensorSpec {
        id: "battery",
        label: "Battery",
        regs: &[0x0C],
        unit: UnitKind::Volts,
        range: (8.0, 16.0),
        decimals: 1,
        desc: "Battery/charging voltage as seen by the ECU; raw x 80 mV.",
        typical: "12.4-12.8 V engine off, 13.5-14.5 V running. Below ~12 V running means charging trouble.",
    },
    SensorSpec {
        id: "tps",
        label: "TPS",
        regs: &[0x0D],
        unit: UnitKind::Volts,
        range: (0.0, 5.0),
        decimals: 2,
        desc: "Throttle position sensor voltage; raw x 20 mV. These ECUs report volts, not percent.",
        typical: "~0.4 V closed throttle, ~4.0-4.2 V wide open (adjustable at the sensor).",
    },
    SensorSpec {
        id: "maf",
        label: "MAF",
        regs: &[0x04, 0x05],
        unit: UnitKind::Volts,
        range: (0.0, 5.0),
        decimals: 2,
        desc: "Mass airflow sensor signal voltage, 16-bit x 5 mV.",
        typical: "~1.0-1.4 V at idle, rising with load; ~3.5 V+ at high load.",
    },
    SensorSpec {
        id: "maf_rh",
        label: "MAF RH",
        regs: &[0x06, 0x07],
        unit: UnitKind::Volts,
        range: (0.0, 5.0),
        decimals: 2,
        desc: "Right-bank MAF signal on twin-MAF cars (e.g. Z32 TT). 16-bit x 5 mV.",
        typical: "Should closely track the LH MAF.",
    },
    SensorSpec {
        id: "o2_lh",
        label: "O2 LH",
        regs: &[0x09],
        unit: UnitKind::Volts,
        range: (0.0, 1.2),
        decimals: 2,
        desc: "Left O2 sensor voltage; raw x 10 mV.",
        typical: "Switches between ~0.1-0.9 V at warm cruise (closed loop). Stuck near 0.45 V = open loop or dead sensor.",
    },
    SensorSpec {
        id: "o2_rh",
        label: "O2 RH",
        regs: &[0x0A],
        unit: UnitKind::Volts,
        range: (0.0, 1.2),
        decimals: 2,
        desc: "Right O2 sensor voltage; raw x 10 mV.",
        typical: "Same behavior as LH; the two banks should look similar.",
    },
    SensorSpec {
        id: "fuel_temp",
        label: "Fuel Temp",
        regs: &[0x0F],
        unit: UnitKind::Temp,
        range: (-40.0, 250.0),
        decimals: 0,
        desc: "Fuel temperature sensor; raw - 50 = deg C. Used by the ECU for fuel density correction.",
        typical: "Near ambient, climbing on hot days or with a hot rail.",
    },
    SensorSpec {
        id: "iat",
        label: "Intake Air Temp",
        regs: &[0x11],
        unit: UnitKind::Temp,
        range: (-40.0, 250.0),
        decimals: 0,
        desc: "Intake air temperature; raw - 50 = deg C.",
        typical: "Near ambient while cruising; heat-soaks 20-50 F above ambient at idle.",
    },
    SensorSpec {
        id: "egt",
        label: "EGT",
        regs: &[0x12],
        unit: UnitKind::Volts,
        range: (0.0, 5.0),
        decimals: 2,
        desc: "Exhaust gas temperature sensor signal; raw x 20 mV. Mostly present on turbo models.",
        typical: "Rises under sustained load. Watch for unusual drops (sensor failure).",
    },
    SensorSpec {
        id: "inj_lh",
        label: "Injector LH",
        regs: &[0x14, 0x15],
        unit: UnitKind::Ms,
        range: (0.0, 25.0),
        decimals: 2,
        desc: "Injector pulse width, left bank. 16-bit value / 100 = ms.",
        typical: "~1.5-3 ms at idle, rising with RPM and load.",
    },
    SensorSpec {
        id: "inj_rh",
        label: "Injector RH",
        regs: &[0x22, 0x23],
        unit: UnitKind::Ms,
        range: (0.0, 25.0),
        decimals: 2,
        desc: "Injector pulse width, right bank. 16-bit value / 100 = ms.",
        typical: "Should closely match the LH bank.",
    },
    SensorSpec {
        id: "timing",
        label: "Timing",
        regs: &[0x16],
        unit: UnitKind::Deg,
        range: (-10.0, 50.0),
        decimals: 0,
        desc: "Ignition timing; 110 - raw = degrees BTDC.",
        typical: "~15-20 deg BTDC at idle, 30-40+ at light cruise.",
    },
    SensorSpec {
        id: "aac",
        label: "AAC",
        regs: &[0x17],
        unit: UnitKind::Percent,
        range: (0.0, 100.0),
        decimals: 0,
        desc: "AAC (auxiliary air control / idle air) valve duty cycle; raw / 2 = %.",
        typical: "~5-15% at warm idle with no load; higher with A/C on or a cold engine.",
    },
    SensorSpec {
        id: "alpha_lh",
        label: "AF Alpha LH",
        regs: &[0x1A],
        unit: UnitKind::Percent,
        range: (50.0, 150.0),
        decimals: 0,
        desc: "Short-term fuel trim (A/F alpha), left bank, % of base injection pulse.",
        typical: "Oscillates around 100% in closed loop. Pegged high/low = fueling problem.",
    },
    SensorSpec {
        id: "alpha_rh",
        label: "AF Alpha RH",
        regs: &[0x1B],
        unit: UnitKind::Percent,
        range: (50.0, 150.0),
        decimals: 0,
        desc: "Short-term fuel trim (A/F alpha), right bank, % of base injection pulse.",
        typical: "Oscillates around 100%, similar to LH.",
    },
    SensorSpec {
        id: "learn_lh",
        label: "AF Learn LH",
        regs: &[0x1C],
        unit: UnitKind::Percent,
        range: (50.0, 150.0),
        decimals: 0,
        desc: "Self-learned long-term fuel trim, left bank, %.",
        typical: "85-115%. Far from 100% suggests vacuum leaks or a tired O2 sensor. Reset via Testing > Clear self-learn.",
    },
    SensorSpec {
        id: "learn_rh",
        label: "AF Learn RH",
        regs: &[0x1D],
        unit: UnitKind::Percent,
        range: (50.0, 150.0),
        decimals: 0,
        desc: "Self-learned long-term fuel trim, right bank, %.",
        typical: "85-115%, similar to LH.",
    },
    SensorSpec {
        id: "d13",
        label: "Digital 0x13",
        regs: &[0x13],
        unit: UnitKind::Raw,
        range: (0.0, 255.0),
        decimals: 0,
        desc: "Digital inputs: switch states read by the ECU.",
        typical: "Park/Neutral and TPS-closed set at idle; Start Signal only while cranking.",
    },
    SensorSpec {
        id: "d1e",
        label: "Digital 0x1E",
        regs: &[0x1E],
        unit: UnitKind::Raw,
        range: (0.0, 255.0),
        decimals: 0,
        desc: "Digital outputs: relay and solenoid drive states.",
        typical: "Fuel Pump Relay set while running; fans cycle with temperature.",
    },
    SensorSpec {
        id: "d1f",
        label: "Digital 0x1F",
        regs: &[0x1F],
        unit: UnitKind::Raw,
        range: (0.0, 255.0),
        decimals: 0,
        desc: "Digital outputs: emissions/idle solenoids.",
        typical: "Mostly clear at idle; wastegate/EGR bits move under load.",
    },
    SensorSpec {
        id: "d21",
        label: "Digital 0x21",
        regs: &[0x21],
        unit: UnitKind::Raw,
        range: (0.0, 255.0),
        decimals: 0,
        desc: "Closed-loop status flags per bank (1 = running lean).",
        typical: "Flags toggle as the ECU chases stoichiometric in closed loop.",
    },
    // ---- Late-model / extended registers (0x02-0x03, 0x25-0x53). Present
    // only on some ECUs; unsupported registers simply read back nothing.
    SensorSpec {
        id: "cas_ref",
        label: "CAS Ref",
        regs: &[0x02, 0x03],
        unit: UnitKind::Rpm,
        range: (0.0, 8000.0),
        decimals: 0,
        desc: "CAS reference RPM, 16-bit x 8. Alternate RPM source; on '96+ ECUs it is 2.494e6 / raw instead.",
        typical: "Should roughly track RPM.",
    },
    SensorSpec {
        id: "wastegate",
        label: "Wastegate",
        regs: &[0x28],
        unit: UnitKind::Percent,
        range: (0.0, 100.0),
        decimals: 0,
        desc: "Wastegate solenoid duty, raw / 2 = % (scale not confirmed in the register PDF). Turbo ECUs.",
        typical: "0% at idle and cruise, rising under boost.",
    },
    SensorSpec {
        id: "boost",
        label: "Boost",
        regs: &[0x29],
        unit: UnitKind::Volts,
        range: (0.0, 5.0),
        decimals: 2,
        desc: "Turbo boost (pressure) sensor signal; raw x 20 mV. Not a direct psi readout.",
        typical: "~1.5-2.5 V at atmospheric pressure, rising with boost.",
    },
    SensorSpec {
        id: "calld",
        label: "Load",
        regs: &[0x33, 0x34],
        unit: UnitKind::Percent,
        range: (0.0, 100.0),
        decimals: 1,
        desc: "CAL/LD engine load: reg 0x33 + reg 0x34/255 = % (per the '96+ specifics doc).",
        typical: "~15-25% at idle, 60-100% at full throttle.",
    },
    SensorSpec {
        id: "rear_o2",
        label: "Rear O2",
        regs: &[0x35],
        unit: UnitKind::Volts,
        range: (0.0, 1.2),
        decimals: 2,
        desc: "Rear (downstream) O2 sensor voltage; raw x 10 mV. '96+ OBD-II-era ECUs.",
        typical: "Fairly steady ~0.5-0.7 V with a healthy catalyst.",
    },
    SensorSpec {
        id: "abs_tps",
        label: "Abs TPS",
        regs: &[0x37],
        unit: UnitKind::Percent,
        range: (0.0, 100.0),
        decimals: 1,
        desc: "Absolute throttle position; raw / 2.55 = %. Later ECUs only.",
        typical: "0% closed, ~100% wide open.",
    },
    SensorSpec {
        id: "maf_pct",
        label: "MAF %",
        regs: &[0x38],
        unit: UnitKind::Percent,
        range: (0.0, 100.0),
        decimals: 1,
        desc: "MAF flow as % of the ECU's reference flow (per the '96+ specifics doc).",
        typical: "Low single digits at idle, climbing with load.",
    },
    SensorSpec {
        id: "abs_pressure",
        label: "Abs Pressure",
        regs: &[0x4A],
        unit: UnitKind::Volts,
        range: (0.0, 5.0),
        decimals: 2,
        desc: "Absolute pressure sensor signal; raw x 20 mV. Used for barometric/manifold pressure.",
        typical: "Higher with the key on/engine off (baro), lower at idle (manifold vacuum).",
    },
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
    CATALOG
        .iter()
        .filter(|s| ids.iter().any(|i| i == s.id))
        .collect()
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
            "cas_ref" => u16be(frame, stream_regs, 0x02, 0x03)? as f64 * 8.0,
            "maf" | "maf_rh" => {
                u16be(frame, stream_regs, self.regs[0], self.regs[1])? as f64 * 5.0 / 1000.0
            }
            "calld" => {
                let whole = byte_of(frame, stream_regs, 0x33)? as f64;
                let frac = byte_of(frame, stream_regs, 0x34)? as f64;
                whole + frac / 255.0
            }
            "inj_lh" | "inj_rh" => {
                u16be(frame, stream_regs, self.regs[0], self.regs[1])? as f64 / 100.0
            }
            "coolant" | "fuel_temp" | "iat" => cfg.display_temp(raw()? as f64 - 50.0),
            "speed" => cfg.display_speed(raw()? as f64 * 2.0),
            "battery" => raw()? as f64 * 80.0 / 1000.0,
            "tps" | "egt" | "boost" | "abs_pressure" => raw()? as f64 * 20.0 / 1000.0,
            "o2_lh" | "o2_rh" | "rear_o2" => raw()? as f64 * 10.0 / 1000.0,
            "timing" => 110.0 - raw()? as f64,
            "aac" | "wastegate" => raw()? as f64 / 2.0,
            "abs_tps" => raw()? as f64 / 2.55,
            "alpha_lh" | "alpha_rh" | "learn_lh" | "learn_rh" => raw()? as f64,
            _ => raw()? as f64, // maf_pct, digital registers, and unknown items
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
                if speed_unit == "MPH" {
                    "MPH"
                } else {
                    "KPH"
                }
            }
            UnitKind::Temp => {
                if temp_unit == "F" {
                    "°F"
                } else {
                    "°C"
                }
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
                if speed_unit == "MPH" {
                    (0.0, 160.0)
                } else {
                    (0.0, 260.0)
                }
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
        ("MAF %", "maf_pct"),
        ("MAF RH", "maf_rh"),
        ("MAF", "maf"),
        ("O2 RH", "o2_rh"),
        ("O2", "o2_lh"),
        ("EGT", "egt"),
        ("CAS Ref", "cas_ref"),
        ("Wastegate", "wastegate"),
        ("Boost", "boost"),
        ("Load", "calld"),
        ("Rear O2", "rear_o2"),
        ("Abs TPS", "abs_tps"),
        ("Abs Pressure", "abs_pressure"),
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
    fn decode_late_model_registers() {
        let cfg = Config::default();
        // CAL/LD: 0x33 = 40, 0x34 = 127 -> 40 + 127/255 = 40.5%
        let spec = find_spec("calld").unwrap();
        let v = spec.decode(&[40, 127], &[0x33, 0x34], &cfg).unwrap();
        assert!((v - 40.5).abs() < 0.01);
        // Abs TPS: 127.5 raw -> 50%
        let spec = find_spec("abs_tps").unwrap();
        let v = spec.decode(&[128], &[0x37], &cfg).unwrap();
        assert!((v - 50.2).abs() < 0.1);
        // CAS ref: 0x0064 = 100 -> 800 RPM
        let spec = find_spec("cas_ref").unwrap();
        let v = spec.decode(&[0x00, 0x64], &[0x02, 0x03], &cfg).unwrap();
        assert!((v - 800.0).abs() < 1.0);
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
