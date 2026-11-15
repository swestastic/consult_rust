//! Application configuration, stored as JSON in `consult_config.json`
//! next to the working directory.

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "consult_config.json";

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SpeedUnit {
    #[default]
    Mph,
    Kph,
}

impl SpeedUnit {
    pub fn label(self) -> &'static str {
        match self {
            SpeedUnit::Mph => "MPH",
            SpeedUnit::Kph => "KPH",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TempUnit {
    #[default]
    F,
    C,
}

impl TempUnit {
    pub fn label(self) -> &'static str {
        match self {
            TempUnit::F => "F",
            TempUnit::C => "C",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub speed_unit: SpeedUnit,
    pub temp_unit: TempUnit,
    /// Multiplier applied to the ECU's indicated speed (e.g. 1.03 if the
    /// speedometer reads 3% low) to account for tire/gear changes.
    pub speed_correction: f64,
    pub rpm_warning: u32,
    /// Stored canonically in Celsius regardless of display unit.
    pub coolant_warning_c: f64,
    /// Battery gauge warns below this voltage (charging trouble).
    pub battery_low_v: f64,
    /// ECU init address as hex text, e.g. "EF" (some '96+ ECUs use "ED").
    pub ecu_address: String,
    /// Show every catalog option regardless of the identified ECU
    /// (escape hatch for mislabeled/unknown ECUs).
    pub show_all_options: bool,
    /// Last serial port that connected successfully, restored on launch.
    pub last_port: Option<String>,
    /// Selected data-stream items (catalog ids), user-editable in Settings.
    pub display_items: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            speed_unit: SpeedUnit::Mph,
            temp_unit: TempUnit::F,
            speed_correction: 1.0,
            rpm_warning: 7000,
            coolant_warning_c: 93.5,
            battery_low_v: 12.0,
            ecu_address: "EF".into(),
            show_all_options: false,
            last_port: None,
            display_items: crate::catalog::DEFAULT_SELECTION
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        let mut cfg: Config = std::fs::read_to_string(CONFIG_FILE)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        cfg.display_items = crate::catalog::sanitize_selection(cfg.display_items);
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(CONFIG_FILE, text).map_err(|e| format!("Cannot write {CONFIG_FILE}: {e}"))
    }

    pub fn ecu_address_byte(&self) -> Result<u8, String> {
        let trimmed = self
            .ecu_address
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        u8::from_str_radix(trimmed, 16).map_err(|_| {
            format!(
                "Invalid ECU address \"{}\" (expected hex, e.g. EF)",
                self.ecu_address
            )
        })
    }

    /// Init addresses to try at connect: "AUTO" probes the common ones
    /// (EF for most ECUs, ED for many '96+), otherwise just the one given.
    pub fn ecu_address_candidates(&self) -> Result<Vec<u8>, String> {
        if self.ecu_address.trim().eq_ignore_ascii_case("auto") {
            return Ok(vec![0xEF, 0xED]);
        }
        self.ecu_address_byte().map(|b| vec![b])
    }

    /// Indicated kph -> display-unit, corrected speed.
    pub fn display_speed(&self, kph: f64) -> f64 {
        let corrected = kph * self.speed_correction;
        match self.speed_unit {
            SpeedUnit::Kph => corrected,
            SpeedUnit::Mph => corrected * 0.621371192237334,
        }
    }

    /// Celsius -> display-unit temperature.
    pub fn display_temp(&self, c: f64) -> f64 {
        match self.temp_unit {
            TempUnit::C => c,
            TempUnit::F => c * 9.0 / 5.0 + 32.0,
        }
    }

    /// Display-unit temperature -> Celsius.
    pub fn temp_to_c(&self, v: f64) -> f64 {
        match self.temp_unit {
            TempUnit::C => v,
            TempUnit::F => (v - 32.0) * 5.0 / 9.0,
        }
    }

    pub fn coolant_warning_display(&self) -> f64 {
        self.display_temp(self.coolant_warning_c)
    }

    /// Selected catalog items in display order.
    pub fn selected_specs(&self) -> Vec<&'static crate::catalog::SensorSpec> {
        crate::catalog::selected_specs(&self.display_items)
    }

    /// Flat register list for the stream request.
    pub fn stream_registers(&self) -> Vec<u8> {
        crate::catalog::expand_registers(&self.selected_specs())
    }
}
