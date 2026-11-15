//! Data Stream page: live gauges for the user-selected catalog items,
//! plus CSV logging.

use egui::{Color32, RichText};
use std::time::Instant;

use crate::app::ConsultApp;
use crate::catalog;
use crate::consult::link::LinkCommand;
use crate::widgets::{self, SERIES_COLORS, Series, gauge};

/// Gap between the gauge grid and the window's right edge / scrollbar.
const RIGHT_PAD: f32 = 12.0;

impl ConsultApp {
    pub(crate) fn show_datastream(&mut self, ui: &mut egui::Ui) {
        if !self.connected() {
            ui.label("Not connected.");
            return;
        }

        let cfg = self.config.clone();
        let specs = cfg.selected_specs();
        let regs = catalog::expand_registers(&specs);
        let frame: Option<&[u8]> = self.frame.as_deref();
        let values: Vec<Option<f64>> = specs
            .iter()
            .map(|s| frame.and_then(|f| s.decode(f, &regs, &cfg)))
            .collect();

        // Track per-session peaks (max per item) from live values.
        for (spec, val) in specs.iter().zip(&values) {
            if catalog::is_digital(spec) {
                continue;
            }
            if let Some(v) = val {
                let peak = self.peaks.entry(spec.id).or_insert(*v);
                if *v > *peak {
                    *peak = *v;
                }
            }
        }

        // Record the rolling history for the live plot (last 90 s kept).
        if values.iter().any(Option::is_some) {
            let start = *self.history_start.get_or_insert_with(Instant::now);
            let t = start.elapsed().as_secs_f64();
            let row: Vec<f64> = values.iter().map(|v| v.unwrap_or(0.0)).collect();
            self.history.push_back((t, row));
            while self
                .history
                .front()
                .is_some_and(|(t0, _)| t - t0 > 90.0)
            {
                self.history.pop_front();
            }
        }

        let rpm_warn = cfg.rpm_warning as f64;
        let temp_warn = cfg.coolant_warning_display();
        let batt_warn = cfg.battery_low_v;

        // Controls docked to the bottom of the page so they are always
        // visible regardless of scroll position or window height.
        egui::Panel::bottom("stream_controls").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if self.logger.is_logging() {
                    if ui.button("Stop Data Logging").clicked() {
                        self.logger.stop();
                    }
                    if let Some(name) = &self.logger.file_name {
                        ui.label(RichText::new(format!("Logging to {name}")).color(Color32::GREEN));
                    }
                } else if ui.button("Start Data Logging").clicked() {
                    let columns: Vec<String> = specs.iter().map(|s| s.title(&cfg)).collect();
                    match self.logger.start(&columns) {
                        Ok(_) => self.status = "Logging started.".into(),
                        Err(e) => self.status = e,
                    }
                }

                ui.separator();
                if self.streaming {
                    if ui.button("Pause Stream").clicked() {
                        self.send(LinkCommand::StopStream);
                        self.streaming = false;
                    }
                } else if ui.button("Resume Stream").clicked() {
                    self.send(LinkCommand::StartStream(self.config.stream_registers()));
                    self.streaming = true;
                }

                ui.separator();
                if ui
                    .button("Reset peaks")
                    .on_hover_text("Clear the per-session peak (max) shown on each gauge")
                    .clicked()
                {
                    self.peaks.clear();
                }

                if self.streaming {
                    ui.separator();
                    let fps = self.stream_fps();
                    let color = if fps >= 8.0 {
                        Color32::from_rgb(80, 220, 100)
                    } else if fps >= 4.0 {
                        Color32::from_rgb(230, 200, 60)
                    } else {
                        Color32::from_rgb(255, 90, 90)
                    };
                    ui.label(
                        RichText::new(format!("{fps:.1} fps"))
                            .color(color)
                            .size(11.0),
                    )
                    .on_hover_text("Stream frame rate over the last 2 s");
                }
            });
        });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail = (ui.available_width() - RIGHT_PAD).max(100.0);
                let (cols, gw) = widgets::fit_columns(avail, 250.0, 360.0, 6, 12.0);
                let gsize = egui::vec2(gw, gw / 1.31);

                egui::Grid::new("gauges")
                    .num_columns(cols)
                    .spacing([12.0, 12.0])
                    .show(ui, |ui| {
                        let mut col = 0;
                        for (spec, val) in specs.iter().zip(&values) {
                            if catalog::is_digital(spec) {
                                continue; // rendered in the digital region below
                            }
                            let v = val.unwrap_or(0.0);
                            let warn = val.is_some()
                                && match spec.id {
                                    "rpm" => v >= rpm_warn,
                                    "coolant" => v >= temp_warn,
                                    "battery" => v <= batt_warn,
                                    _ => false,
                                };
                            let (min, max) = spec.range_for(&cfg);
                            let rect = gauge(
                                ui,
                                gsize,
                                &spec.title(&cfg),
                                v,
                                min,
                                max,
                                spec.decimals,
                                warn,
                            );
                            if let Some(pk) = self.peaks.get(spec.id) {
                                ui.painter_at(rect).text(
                                    rect.right_top() + egui::vec2(-8.0, 6.0),
                                    egui::Align2::RIGHT_TOP,
                                    format!("pk {:.*}", spec.decimals, pk),
                                    egui::FontId::proportional(10.0),
                                    ui.visuals().weak_text_color(),
                                );
                            }
                            col += 1;
                            if col % cols == 0 {
                                ui.end_row();
                            }
                        }
                    });

                // ---- Digital register region (compact on/off indicators)
                let digital: Vec<(&catalog::SensorSpec, Option<f64>)> = specs
                    .iter()
                    .zip(&values)
                    .filter(|(s, _)| catalog::is_digital(s))
                    .map(|(s, v)| (*s, *v))
                    .collect();
                if !digital.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Digital Registers").strong());
                    ui.add_space(4.0);
                    let (dcols, dw) = widgets::fit_columns(avail, 260.0, 420.0, 4, 12.0);
                    egui::Grid::new("digital_region")
                        .num_columns(dcols)
                        .spacing([12.0, 12.0])
                        .show(ui, |ui| {
                            for (i, (spec, val)) in digital.iter().enumerate() {
                                digital_panel(ui, spec, *val, dw);
                                if (i + 1) % dcols == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                }

                // ---- Live plot (rolling last 60 s)
                let plot_idx: Vec<usize> = specs
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !catalog::is_digital(s))
                    .map(|(i, _)| i)
                    .collect();
                if self.plot_channels.len() != plot_idx.len() {
                    self.plot_channels = (0..plot_idx.len()).map(|i| i < 2).collect();
                }
                ui.add_space(10.0);
                egui::CollapsingHeader::new("Live plot (last 60 s)")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (k, &col) in plot_idx.iter().enumerate() {
                                let on = self.plot_channels.get(k).copied().unwrap_or(false);
                                let color = SERIES_COLORS[k % SERIES_COLORS.len()];
                                let text = if on {
                                    RichText::new(specs[col].label).color(color).strong()
                                } else {
                                    RichText::new(specs[col].label)
                                };
                                if ui.selectable_label(on, text).clicked()
                                    && let Some(ch) = self.plot_channels.get_mut(k)
                                {
                                    *ch = !*ch;
                                }
                            }
                            ui.separator();
                            ui.checkbox(&mut self.plot_normalize, "Normalize")
                                .on_hover_text(
                                    "Scale each channel to 0..1 so different units can be compared.",
                                );
                        });

                        let Some(&(latest_t, _)) = self.history.back() else {
                            ui.label("Waiting for stream data...");
                            return;
                        };
                        let window = 60.0;
                        let normalize = self.plot_normalize;
                        let mut enabled: Vec<(usize, Vec<[f64; 2]>)> = Vec::new();
                        for (k, &col) in plot_idx.iter().enumerate() {
                            if !self.plot_channels.get(k).copied().unwrap_or(false) {
                                continue;
                            }
                            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                            let mut pts: Vec<[f64; 2]> = Vec::with_capacity(self.history.len());
                            for (t, row) in &self.history {
                                let v = row.get(col).copied().unwrap_or(0.0);
                                lo = lo.min(v);
                                hi = hi.max(v);
                                pts.push([window - (latest_t - t), v]);
                            }
                            if normalize && hi > lo {
                                for p in &mut pts {
                                    p[1] = (p[1] - lo) / (hi - lo);
                                }
                            }
                            enabled.push((k, pts));
                        }
                        let series: Vec<Series> = enabled
                            .iter()
                            .map(|(k, pts)| Series {
                                color: SERIES_COLORS[k % SERIES_COLORS.len()],
                                points: pts,
                            })
                            .collect();
                        widgets::time_plot(ui, &series, window, None, 160.0);
                    });
            });

        // Log once per incoming frame.
        if self.logger.is_logging() && self.frame_seq != self.last_logged_seq {
            let row: Vec<(f64, usize)> = specs
                .iter()
                .zip(&values)
                .map(|(s, v)| (v.unwrap_or(0.0), s.decimals))
                .collect();
            self.logger.log(&row);
            self.last_logged_seq = self.frame_seq;
        }
    }
}

/// Compact indicator panel for one digital bit register: 8 bits in two
/// columns, green dot + name when set, dim when clear.
fn digital_panel(ui: &mut egui::Ui, spec: &catalog::SensorSpec, value: Option<f64>, width: f32) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 124.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let dim = ui.visuals().weak_text_color();
    let fg = ui.visuals().text_color();

    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(
            1.0,
            ui.style().visuals.widgets.noninteractive.bg_stroke.color,
        ),
        egui::StrokeKind::Inside,
    );

    let pad = 10.0;
    painter.text(
        egui::pos2(rect.left() + pad, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        spec.label,
        egui::FontId::proportional(12.0),
        fg,
    );

    let byte = value.map(|v| v as u8);
    let grid_top = rect.top() + 26.0;
    let row_h = 18.0;
    let col_x = [rect.left() + pad, rect.left() + rect.width() / 2.0];
    for row in 0u8..4 {
        for (col, base) in col_x.iter().enumerate() {
            // left column: bits 7..4, right column: bits 3..0
            let bit = if col == 0 { 7 - row } else { 3 - row };
            let on = byte.is_some_and(|b| b & (1 << bit) != 0);
            let y = grid_top + row as f32 * row_h + row_h / 2.0;
            let (dot, text) = if on {
                (Color32::from_rgb(80, 220, 100), fg)
            } else {
                (Color32::from_rgb(70, 70, 70), dim)
            };
            painter.circle_filled(egui::pos2(base + 5.0, y), 4.0, dot);
            let name = catalog::digital_bit_name(spec.id, bit)
                .map(String::from)
                .unwrap_or_else(|| format!("Bit {bit}"));
            painter.text(
                egui::pos2(base + 14.0, y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(11.0),
                text,
            );
        }
    }

    let hex = byte.map(|b| format!("0x{b:02X}")).unwrap_or("--".into());
    painter.text(
        egui::pos2(rect.right() - pad, rect.bottom() - 6.0),
        egui::Align2::RIGHT_BOTTOM,
        hex,
        egui::FontId::monospace(10.0),
        dim,
    );

    if let Some(b) = byte {
        resp.on_hover_text(format!("{} = 0x{b:02X} (0b{b:08b})", spec.label));
    }
}
