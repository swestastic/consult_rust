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
        let file = std::fs::File::create(&name).map_err(|e| format!("Cannot create {name}: {e}"))?;
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
        let Some(w) = self.writer.as_mut() else { return };
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
