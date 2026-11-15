//! Log Replay page: load a saved CSV log, play it back through gauges,
//! scrub the timeline, and inspect columns on a time-series plot.
//! Columns are dynamic, following whatever was selected when the log
//! was recorded; known columns get gauges, the rest get value boxes.

use egui::RichText;

use crate::app::{ConsultApp, REPLAY_SPEEDS};
use crate::catalog;
use crate::logfile::LogFile;
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
                .button("Open Folder")
                .on_hover_text("Open the logs folder in the file manager")
                .clicked()
            {
                match crate::logfile::open_logs_folder() {
                    Ok(()) => self.replay.status = "Opened logs folder.".into(),
                    Err(e) => self.replay.status = e,
                }
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
                        // Default: plot the first two columns.
                        self.replay.channels = (0..log.columns.len()).map(|i| i < 2).collect();
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

        let (duration, speed_unit, temp_unit, columns) = {
            let log = self.replay.log.as_ref().unwrap();
            (
                log.duration(),
                log.speed_unit.clone(),
                log.temp_unit.clone(),
                log.columns.clone(),
            )
        };
        if self.replay.channels.len() != columns.len() {
            self.replay.channels = (0..columns.len()).map(|i| i < 2).collect();
        }

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

        // ---- Sample + plot data for the current position (immutable phase)
        let (values, points) = {
            let log = self.replay.log.as_ref().unwrap();
            let values = log.sample(self.replay.t);
            let normalize = self.replay.normalize;
            let mut points: Vec<Vec<[f64; 2]>> = Vec::new();
            for (i, _) in columns.iter().enumerate() {
                if !self.replay.channels.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                let mut pts: Vec<[f64; 2]> = Vec::with_capacity(log.rows.len());
                for r in &log.rows {
                    let v = r.1.get(i).copied().unwrap_or(0.0);
                    lo = lo.min(v);
                    hi = hi.max(v);
                    pts.push([r.0, v]);
                }
                if normalize && hi > lo {
                    for p in &mut pts {
                        p[1] = (p[1] - lo) / (hi - lo);
                    }
                }
                points.push(pts);
            }
            (values, points)
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            // ---- Gauges / value boxes for known columns
            let rpm_warn = self.config.rpm_warning as f64;
            let temp_warn = self.config.coolant_warning_display();
            let avail = (ui.available_width() - 12.0).max(100.0);
            let (cols, gw) = widgets::fit_columns(avail, 250.0, 360.0, 6, 12.0);
            let gsize = egui::vec2(gw, gw / 1.31);
            let vsize = egui::vec2(gw, 60.0);
            ui.add_space(4.0);
            egui::Grid::new("replay_gauges")
                .num_columns(cols)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    for (i, name) in columns.iter().enumerate() {
                        let v = values.get(i).copied().unwrap_or(0.0);
                        match catalog::spec_for_column(name) {
                            Some(spec) if spec.unit == catalog::UnitKind::Raw => {
                                value_box(ui, name, v, 0, vsize);
                            }
                            Some(spec) => {
                                let (min, max) = spec.range_in(&speed_unit, &temp_unit);
                                let warn = match spec.id {
                                    "rpm" => v >= rpm_warn,
                                    "coolant" => v >= temp_warn,
                                    _ => false,
                                };
                                widgets::gauge(ui, gsize, name, v, min, max, spec.decimals, warn);
                            }
                            None => value_box(ui, name, v, 2, vsize),
                        }
                        if (i + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });

            // ---- Channel toggles (double as plot legend)
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("Plot:");
                for (i, name) in columns.iter().enumerate() {
                    let on = self.replay.channels.get(i).copied().unwrap_or(false);
                    let color = SERIES_COLORS[i % SERIES_COLORS.len()];
                    let text = if on {
                        RichText::new(name).color(color).strong()
                    } else {
                        RichText::new(name.as_str())
                    };
                    if ui.selectable_label(on, text).clicked()
                        && let Some(ch) = self.replay.channels.get_mut(i)
                    {
                        *ch = !*ch;
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.replay.normalize, "Normalize")
                    .on_hover_text("Scale each channel to 0..1 so different units can be compared.");
            });

            // ---- Plot
            let series: Vec<Series> = points
                .iter()
                .zip(
                    columns
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| self.replay.channels.get(*i).copied().unwrap_or(false)),
                )
                .map(|(pts, (i, _))| Series {
                    color: SERIES_COLORS[i % SERIES_COLORS.len()],
                    points: pts,
                })
                .collect();

            ui.add_space(4.0);
            widgets::time_plot(ui, &series, duration, Some(self.replay.t), 180.0);
        });
    }
}

/// Plain value box for columns without a known gauge.
fn value_box(ui: &mut egui::Ui, title: &str, value: f64, decimals: usize, size: egui::Vec2) {
    egui::Frame::group(ui.style())
        .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
        .show(ui, |ui| {
            ui.set_min_size(size);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(title).weak());
                ui.label(
                    RichText::new(format!("{value:.decimals$}"))
                        .monospace()
                        .size(18.0),
                );
            });
        });
}
