//! Saved CSV log files: parsing, sampling, and channel access for replay.
//! Column layout is dynamic — it follows whatever items were selected for
//! display when the log was recorded.

use std::path::Path;

/// Open the logs directory in the OS file manager.
pub fn open_logs_folder() -> Result<(), String> {
    std::fs::create_dir_all("logs").map_err(|e| format!("Cannot create logs dir: {e}"))?;
    let abs = std::fs::canonicalize("logs").map_err(|e| e.to_string())?;
    let mut path = abs.to_string_lossy().into_owned();
    // Explorer doesn't understand the \\?\ verbatim prefix.
    #[cfg(target_os = "windows")]
    {
        if let Some(stripped) = path.strip_prefix(r"\\?\") {
            path = stripped.to_string();
        }
    }
    let (cmd, arg): (&str, &str) = if cfg!(target_os = "windows") {
        ("explorer", &path)
    } else if cfg!(target_os = "macos") {
        ("open", &path)
    } else {
        ("xdg-open", &path)
    };
    std::process::Command::new(cmd)
        .arg(arg)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot launch {cmd}: {e}"))
}

#[derive(Clone, Default)]
pub struct LogFile {
    pub name: String,
    /// Column titles, excluding the leading Timestamp column.
    pub columns: Vec<String>,
    /// (seconds since start, one value per column)
    pub rows: Vec<(f64, Vec<f64>)>,
    /// Unit labels as recorded in the file ("MPH"/"KPH", "F"/"C").
    pub speed_unit: String,
    pub temp_unit: String,
}

impl LogFile {
    pub fn load(path: &Path) -> Result<LogFile, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let mut lines = text.lines();
        let header = lines.next().ok_or("Empty log file")?;
        let head_cols: Vec<&str> = header.split(',').collect();
        if head_cols.len() < 2 {
            return Err("Log file has no data columns".into());
        }
        let columns: Vec<String> = head_cols[1..]
            .iter()
            .map(|s| s.trim().to_string())
            .collect();
        let ncols = columns.len();

        let speed_unit = columns
            .iter()
            .find(|c| c.starts_with("Speed"))
            .map(|c| if c.contains("MPH") { "MPH" } else { "KPH" })
            .unwrap_or("MPH")
            .to_string();
        let temp_unit = columns
            .iter()
            .find(|c| c.starts_with("Coolant") || c.starts_with("Temp"))
            .map(|c| if c.contains('F') { "F" } else { "C" })
            .unwrap_or("F")
            .to_string();

        let mut rows = Vec::new();
        let mut t0: Option<chrono::NaiveDateTime> = None;
        for line in lines {
            let cols: Vec<&str> = line.split(',').collect();
            if cols.len() < ncols + 1 {
                continue;
            }
            let Ok(ts) =
                chrono::NaiveDateTime::parse_from_str(cols[0].trim(), "%Y-%m-%d %H:%M:%S%.3f")
            else {
                continue;
            };
            let start = *t0.get_or_insert(ts);
            let t = (ts - start).num_milliseconds() as f64 / 1000.0;
            let values: Vec<f64> = (1..=ncols)
                .map(|i| cols[i].trim().parse::<f64>().unwrap_or(0.0))
                .collect();
            rows.push((t, values));
        }
        if rows.is_empty() {
            return Err("No data rows in log file".into());
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(LogFile {
            name,
            columns,
            rows,
            speed_unit,
            temp_unit,
        })
    }

    pub fn duration(&self) -> f64 {
        self.rows.last().map(|r| r.0).unwrap_or(0.0)
    }

    /// Linearly interpolated values at time `t`.
    pub fn sample(&self, t: f64) -> Vec<f64> {
        let n = self.columns.len();
        if self.rows.is_empty() {
            return vec![0.0; n];
        }
        let idx = self.rows.partition_point(|r| r.0 < t);
        if idx == 0 {
            return self.rows[0].1.clone();
        }
        if idx >= self.rows.len() {
            return self.rows.last().unwrap().1.clone();
        }
        let (ta, a) = &self.rows[idx - 1];
        let (tb, b) = &self.rows[idx];
        let frac = if tb > ta {
            ((t - ta) / (tb - ta)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        a.iter().zip(b).map(|(x, y)| x + (y - x) * frac).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_sample() {
        let dir = std::env::temp_dir().join(format!("consult_logtest_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.csv");
        std::fs::write(
            &path,
            "Timestamp,RPM,Speed (MPH),Coolant (°F)\n\
             2026-11-14 17:00:00.000,800,0,180\n\
             2026-11-14 17:00:01.000,1600,10,182\n",
        )
        .unwrap();
        let log = LogFile::load(&path).unwrap();
        assert_eq!(log.columns, vec!["RPM", "Speed (MPH)", "Coolant (°F)"]);
        assert_eq!(log.rows.len(), 2);
        assert_eq!(log.speed_unit, "MPH");
        assert_eq!(log.temp_unit, "F");
        assert!((log.duration() - 1.0).abs() < 1e-6);
        let mid = log.sample(0.5);
        assert!((mid[0] - 1200.0).abs() < 1e-6);
        assert!((mid[2] - 181.0).abs() < 1e-6);
        std::fs::remove_dir_all(&dir).ok();
    }
}
