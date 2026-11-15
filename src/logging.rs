//! CSV data logging, one row per UI update while active.

use std::io::Write;

use crate::config::Config;
use crate::consult::protocol::Readings;

pub struct DataLogger {
    writer: Option<std::io::BufWriter<std::fs::File>>,
    pub file_name: Option<String>,
}

impl Default for DataLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl DataLogger {
    pub fn new() -> Self {
        DataLogger {
            writer: None,
            file_name: None,
        }
    }

    pub fn is_logging(&self) -> bool {
        self.writer.is_some()
    }

    pub fn start(&mut self, cfg: &Config) -> Result<String, String> {
        if self.is_logging() {
            return Ok(self.file_name.clone().unwrap_or_default());
        }
        std::fs::create_dir_all("logs").map_err(|e| format!("Cannot create logs dir: {e}"))?;
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let name = format!("logs/datalog_{stamp}.csv");
        let file = std::fs::File::create(&name).map_err(|e| format!("Cannot create {name}: {e}"))?;
        let mut w = std::io::BufWriter::new(file);
        writeln!(
            w,
            "Timestamp,RPM,Speed ({}),TPS (V),Coolant (deg{}),Timing (deg BTDC),Battery (V),AAC (%),Injector (ms),O2 (V)",
            cfg.speed_unit.label(),
            cfg.temp_unit.label(),
        )
        .map_err(|e| e.to_string())?;
        self.writer = Some(w);
        self.file_name = Some(name.clone());
        Ok(name)
    }

    pub fn log(&mut self, r: &Readings, cfg: &Config) {
        let Some(w) = self.writer.as_mut() else { return };
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(
            w,
            "{},{:.0},{:.1},{:.2},{:.1},{:.1},{:.1},{:.1},{:.2},{:.2}",
            ts,
            r.rpm,
            cfg.display_speed(r.speed_kph),
            r.tps_v,
            cfg.display_temp(r.coolant_c),
            r.timing_deg,
            r.battery_v,
            r.aac_pct,
            r.inj_ms,
            r.o2_v,
        );
    }

    pub fn stop(&mut self) {
        if let Some(mut w) = self.writer.take() {
            let _ = w.flush();
        }
    }
}

impl Drop for DataLogger {
    fn drop(&mut self) {
        self.stop();
    }
}
