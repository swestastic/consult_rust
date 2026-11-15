//! Log Replay page: load a saved CSV log, play it back through the gauges,
//! scrub the timeline, and inspect channels on a time-series plot.

use egui::RichText;

use crate::app::{ConsultApp, REPLAY_SPEEDS};
use crate::logfile::{CHANNEL_NAMES, LogFile};
use crate::widgets::{self, SERIES_COLORS, Series};

impl ConsultApp {
    pub(crate) fn show_replay(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);

        // ---- File selection row
        ui.horizontal(|ui| {
            ui.label("Log file:");
            let selected = self
                .replay
                .selected_file
                .clone()
                .unwrap_or_else(|| "(none)".into());
            egui::ComboBox::from_id_salt("log_file")
                .selected_text(selected)
                .width(280.0)
                .show_ui(ui, |ui| {
                    for name in self.replay.files.clone() {
                        ui.selectable_value(
                            &mut self.replay.selected_file,
                            Some(name.clone()),
                            name,
                        );
                    }
                });
            if ui.button("Refresh").clicked() {
                self.refresh_log_files();
            }
            if ui
                .add_enabled(
                    self.replay.selected_file.is_some(),
                    egui::Button::new("Load"),
                )
                .clicked()
                && let Some(name) = self.replay.selected_file.clone()
            {
                let path = std::path::Path::new("logs").join(&name);
                match LogFile::load(&path) {
                    Ok(log) => {
                        self.replay.status = format!(
                            "{}: {} rows, {:.1} s",
                            log.name,
                            log.rows.len(),
                            log.duration()
                        );
                        self.replay.t = 0.0;
                        self.replay.playing = false;
                        self.replay.log = Some(log);
                    }
                    Err(e) => {
                        self.replay.status = e;
                        self.replay.log = None;
                    }
                }
            }
            ui.separator();
            ui.label(&self.replay.status);
        });

        if self.replay.log.is_none() {
            ui.add_space(20.0);
            ui.label(
                "No log loaded. Record one with \"Start Data Logging\" on the Data Stream page.",
            );
            return;
        }

        let (duration, speed_unit, temp_unit) = {
            let log = self.replay.log.as_ref().unwrap();
            (
                log.duration(),
                log.speed_unit.clone(),
                log.temp_unit.clone(),
            )
        };

        // ---- Transport controls
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .button(if self.replay.playing { "⏸ Pause" } else { "⏵ Play" })
                .clicked()
            {
                self.replay.toggle_play();
            }
            egui::ComboBox::from_id_salt("replay_speed")
                .selected_text(format!("{}x", self.replay.speed))
                .show_ui(ui, |ui| {
                    for s in REPLAY_SPEEDS {
                        ui.selectable_value(&mut self.replay.speed, s, format!("{s}x"));
                    }
                });
            ui.label(format!("{:.1} / {:.1} s", self.replay.t, duration));
        });

        // ---- Timeline scrubber
        let mut t = self.replay.t;
        let slider = egui::Slider::new(&mut t, 0.0..=duration.max(0.001))
            .show_value(false)
            .clamping(egui::SliderClamping::Always);
        if ui
            .add_sized([ui.available_width(), 18.0], slider)
            .changed()
        {
            self.replay.t = t;
            self.replay.last_tick = std::time::Instant::now();
        }

        // ---- Sample data for the current position (immutable borrow phase)
        let (row, points) = {
            let log = self.replay.log.as_ref().unwrap();
            let row = log.sample(self.replay.t);
            let normalize = self.replay.normalize;
            let mut points: Vec<Vec<[f64; 2]>> = Vec::new();
            for i in 0..CHANNEL_NAMES.len() {
                if !self.replay.channels[i] {
                    continue;
                }
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                let mut pts: Vec<[f64; 2]> = Vec::with_capacity(log.rows.len());
                for r in &log.rows {
                    let v = r.channel(i);
                    lo = lo.min(v);
                    hi = hi.max(v);
                    pts.push([r.t, v]);
                }
                if normalize && hi > lo {
                    for p in &mut pts {
                        p[1] = (p[1] - lo) / (hi - lo);
                    }
                }
                points.push(pts);
            }
            (row, points)
        };

        // ---- Gauges (recorded units)
        let speed_max = if speed_unit == "MPH" { 160.0 } else { 260.0 };
        let (temp_min, temp_max) = if temp_unit == "F" {
            (100.0, 250.0)
        } else {
            (40.0, 120.0)
        };
        let temp_warn = self.config.coolant_warning_display();
        let rpm_warn = self.config.rpm_warning as f64;

        ui.add_space(4.0);
        egui::Grid::new("replay_gauges")
            .num_columns(3)
            .spacing([12.0, 12.0])
            .show(ui, |ui| {
                widgets::gauge(ui, "RPM", row.rpm, 0.0, 8000.0, 0, row.rpm >= rpm_warn);
                widgets::gauge(ui, &format!("Speed ({speed_unit})"), row.speed, 0.0, speed_max, 0, false);
                widgets::gauge(ui, &format!("Coolant (°{temp_unit})"), row.temp, temp_min, temp_max, 0, row.temp >= temp_warn);
                ui.end_row();
                widgets::gauge(ui, "TPS (V)", row.tps, 0.0, 5.0, 2, false);
                widgets::gauge(ui, "Timing (°BTDC)", row.timing, -10.0, 50.0, 0, false);
                widgets::gauge(ui, "Battery (V)", row.batt, 8.0, 16.0, 1, false);
                ui.end_row();
                widgets::gauge(ui, "AAC (%)", row.aac, 0.0, 100.0, 0, false);
                widgets::gauge(ui, "Injector (ms)", row.inj, 0.0, 20.0, 1, false);
                ui.end_row();
            });

        // ---- Channel toggles (double as plot legend)
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Plot:");
            for (i, name) in CHANNEL_NAMES.iter().enumerate() {
                let on = self.replay.channels[i];
                let text = if on {
                    RichText::new(*name).color(SERIES_COLORS[i]).strong()
                } else {
                    RichText::new(*name)
                };
                if ui.selectable_label(on, text).clicked() {
                    self.replay.channels[i] = !on;
                }
            }
            ui.separator();
            ui.checkbox(&mut self.replay.normalize, "Normalize")
                .on_hover_text("Scale each channel to 0..1 so different units can be compared.");
        });

        // ---- Plot
        let series: Vec<Series> = points
            .iter()
            .zip(CHANNEL_NAMES.iter().enumerate().filter(|(i, _)| self.replay.channels[*i]))
            .map(|(pts, (i, _))| Series {
                color: SERIES_COLORS[i],
                points: pts,
            })
            .collect();

        ui.add_space(4.0);
        widgets::time_plot(ui, &series, duration, Some(self.replay.t), 180.0);
    }
}
