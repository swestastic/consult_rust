//! CSV data logging, one row per incoming frame while active.
//! Columns are captured at start from the selected display items.

use std::io::Write;

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

    pub fn start(&mut self, columns: &[String]) -> Result<String, String> {
        if self.is_logging() {
            return Ok(self.file_name.clone().unwrap_or_default());
        }
        std::fs::create_dir_all("logs").map_err(|e| format!("Cannot create logs dir: {e}"))?;
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let name = format!("logs/datalog_{stamp}.csv");
        let file =
            std::fs::File::create(&name).map_err(|e| format!("Cannot create {name}: {e}"))?;
        let mut w = std::io::BufWriter::new(file);
        let mut header = String::from("Timestamp");
        for c in columns {
            header.push(',');
            header.push_str(c);
        }
        writeln!(w, "{header}").map_err(|e| e.to_string())?;
        self.writer = Some(w);
        self.file_name = Some(name.clone());
        Ok(name)
    }

    /// `values` are (value, decimals) pairs in column order.
    pub fn log(&mut self, values: &[(f64, usize)]) {
        let Some(w) = self.writer.as_mut() else {
            return;
        };
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let mut row = ts.to_string();
        for (v, d) in values {
            row.push(',');
            row.push_str(&format!("{v:.d$}"));
        }
        let _ = writeln!(w, "{row}");
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

/// Raw ECU communication log: every byte written to and read from the
/// serial port, timestamped. One file per connection attempt, meant for
/// diagnosing user-reported connection and protocol issues.
pub struct CommLogger {
    writer: Option<std::io::BufWriter<std::fs::File>>,
    pub file_name: Option<String>,
}

impl Default for CommLogger {
    fn default() -> Self {
        Self::disabled()
    }
}

impl CommLogger {
    /// A logger that discards everything (logging disabled, or the file
    /// could not be created). All methods are no-ops.
    pub fn disabled() -> Self {
        CommLogger {
            writer: None,
            file_name: None,
        }
    }

    pub fn start(port_name: &str) -> Self {
        Self::start_in("logs", port_name)
    }

    fn start_in(dir: &str, port_name: &str) -> Self {
        let mut log = CommLogger::disabled();
        if std::fs::create_dir_all(dir).is_err() {
            return log;
        }
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let name = format!("{dir}/commlog_{stamp}.txt");
        match std::fs::File::create(&name) {
            Ok(file) => {
                log.writer = Some(std::io::BufWriter::new(file));
                log.file_name = Some(name);
                log.note(&format!(
                    "{} v{} — ECU communication log",
                    env!("CARGO_PKG_NAME"),
                    env!("CARGO_PKG_VERSION")
                ));
                log.note(&format!(
                    "Port: {port_name} @ {} baud",
                    crate::consult::protocol::BAUD_RATE
                ));
            }
            Err(_) => return log,
        }
        log
    }

    fn line(&mut self, text: &str) {
        let Some(w) = self.writer.as_mut() else {
            return;
        };
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        // Flush eagerly: a diagnostics log that loses its tail when the app
        // is killed or crashes is worthless, and line rates are tiny.
        let _ = writeln!(w, "{ts} {text}");
        let _ = w.flush();
    }

    pub fn tx(&mut self, data: &[u8]) {
        self.line(&format!("TX: {}", crate::consult::protocol::to_hex(data)));
    }

    pub fn rx(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.line(&format!("RX: {}", crate::consult::protocol::to_hex(data)));
    }

    /// Free-form note (lifecycle events, errors), marked with '#'.
    pub fn note(&mut self, msg: &str) {
        self.line(&format!("# {msg}"));
    }

    pub fn stop(&mut self) {
        if let Some(mut w) = self.writer.take() {
            let _ = w.flush();
        }
    }
}

impl Drop for CommLogger {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comm_log_roundtrip() {
        let dir = std::env::temp_dir().join(format!("consult_commtest_{}", std::process::id()));
        let mut log = CommLogger::start_in(dir.to_str().unwrap(), "COM3");
        assert!(log.file_name.is_some());
        log.tx(&[0x5A, 0x01, 0xF0]);
        log.rx(&[0xFF, 0x02, 0x55, 0x00]);
        log.note("test note");
        let name = log.file_name.clone().unwrap();
        log.stop();
        let text = std::fs::read_to_string(&name).unwrap();
        assert!(text.contains("TX: 5A 01 F0"));
        assert!(text.contains("RX: FF 02 55 00"));
        assert!(text.contains("# test note"));
        assert!(text.contains("Port: COM3"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn disabled_logger_is_quiet() {
        let mut log = CommLogger::disabled();
        log.tx(&[0x30]);
        log.rx(&[0xCF]);
        log.note("nothing");
        log.stop();
        assert!(log.file_name.is_none());
    }
}
