//! Saved CSV log files: parsing, sampling, and channel access for replay.

use std::path::Path;

pub const CHANNEL_NAMES: [&str; 9] = [
    "RPM", "Speed", "Coolant", "TPS", "Timing", "Battery", "AAC", "Injector", "O2",
];

#[derive(Clone, Copy, Default)]
pub struct LogRow {
    /// Seconds since log start.
    pub t: f64,
    pub rpm: f64,
    pub speed: f64,
    pub tps: f64,
    pub temp: f64,
    pub timing: f64,
    pub batt: f64,
    pub aac: f64,
    pub inj: f64,
    pub o2: f64,
}

impl LogRow {
    pub fn channel(&self, idx: usize) -> f64 {
        match idx {
            0 => self.rpm,
            1 => self.speed,
            2 => self.temp,
            3 => self.tps,
            4 => self.timing,
            5 => self.batt,
            6 => self.aac,
            7 => self.inj,
            8 => self.o2,
            _ => 0.0,
        }
    }

    fn lerp(a: &LogRow, b: &LogRow, f: f64) -> LogRow {
        let l = |x: f64, y: f64| x + (y - x) * f;
        LogRow {
            t: l(a.t, b.t),
            rpm: l(a.rpm, b.rpm),
            speed: l(a.speed, b.speed),
            tps: l(a.tps, b.tps),
            temp: l(a.temp, b.temp),
            timing: l(a.timing, b.timing),
            batt: l(a.batt, b.batt),
            aac: l(a.aac, b.aac),
            inj: l(a.inj, b.inj),
            o2: l(a.o2, b.o2),
        }
    }
}

pub struct LogFile {
    pub name: String,
    pub rows: Vec<LogRow>,
    /// Unit labels as recorded in the file header ("MPH"/"KPH", "F"/"C").
    pub speed_unit: String,
    pub temp_unit: String,
}

impl LogFile {
    pub fn load(path: &Path) -> Result<LogFile, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let mut lines = text.lines();
        let header = lines.next().ok_or("Empty log file")?;

        let speed_unit = if header.contains("MPH") { "MPH" } else { "KPH" }.to_string();
        let temp_unit = if header.contains("degF") || header.contains("\u{00B0}F") {
            "F"
        } else {
            "C"
        }
        .to_string();

        let mut rows = Vec::new();
        let mut t0: Option<chrono::NaiveDateTime> = None;
        for line in lines {
            let cols: Vec<&str> = line.split(',').collect();
            if cols.len() < 9 {
                continue;
            }
            let Ok(ts) =
                chrono::NaiveDateTime::parse_from_str(cols[0].trim(), "%Y-%m-%d %H:%M:%S%.3f")
            else {
                continue;
            };
            let start = *t0.get_or_insert(ts);
            let t = (ts - start).num_milliseconds() as f64 / 1000.0;
            let num = |i: usize| cols[i].trim().parse::<f64>().unwrap_or(0.0);
            rows.push(LogRow {
                t,
                rpm: num(1),
                speed: num(2),
                tps: num(3),
                temp: num(4),
                timing: num(5),
                batt: num(6),
                aac: num(7),
                inj: num(8),
                o2: if cols.len() > 9 { num(9) } else { 0.0 },
            });
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
            rows,
            speed_unit,
            temp_unit,
        })
    }

    pub fn duration(&self) -> f64 {
        self.rows.last().map(|r| r.t).unwrap_or(0.0)
    }

    /// Linearly interpolated row at time `t`.
    pub fn sample(&self, t: f64) -> LogRow {
        if self.rows.is_empty() {
            return LogRow::default();
        }
        let idx = self.rows.partition_point(|r| r.t < t);
        if idx == 0 {
            return self.rows[0];
        }
        if idx >= self.rows.len() {
            return *self.rows.last().unwrap();
        }
        let a = &self.rows[idx - 1];
        let b = &self.rows[idx];
        let frac = if b.t > a.t { (t - a.t) / (b.t - a.t) } else { 0.0 };
        LogRow::lerp(a, b, frac.clamp(0.0, 1.0))
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
            "Timestamp,RPM,Speed (MPH),TPS (V),Coolant (degF),Timing (deg BTDC),Battery (V),AAC (%),Injector (ms),O2 (V)\n\
             2026-11-14 17:00:00.000,800,0,0.5,180,15,14.1,10,2.0,0.40\n\
             2026-11-14 17:00:01.000,1600,10,1.0,182,16,14.2,12,3.0,0.50\n",
        )
        .unwrap();
        let log = LogFile::load(&path).unwrap();
        assert_eq!(log.rows.len(), 2);
        assert_eq!(log.speed_unit, "MPH");
        assert_eq!(log.temp_unit, "F");
        assert!((log.duration() - 1.0).abs() < 1e-6);
        let mid = log.sample(0.5);
        assert!((mid.rpm - 1200.0).abs() < 1e-6);
        assert!((mid.temp - 181.0).abs() < 1e-6);
        std::fs::remove_dir_all(&dir).ok();
    }
}
