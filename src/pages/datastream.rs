//! Data Stream page: live gauges for the user-selected catalog items,
//! plus CSV logging.

use egui::{Color32, RichText};

use crate::app::ConsultApp;
use crate::catalog;
use crate::consult::link::LinkCommand;
use crate::widgets::gauge;

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

        let rpm_warn = cfg.rpm_warning as f64;
        let temp_warn = cfg.coolant_warning_display();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("gauges")
                .num_columns(3)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    for (i, (spec, val)) in specs.iter().zip(&values).enumerate() {
                        let v = val.unwrap_or(0.0);
                        let warn = match spec.id {
                            "rpm" => v >= rpm_warn,
                            "coolant" => v >= temp_warn,
                            _ => false,
                        };
                        if spec.unit == catalog::UnitKind::Raw {
                            raw_box(ui, &spec.title(&cfg), *val);
                        } else {
                            let (min, max) = spec.range_for(&cfg);
                            gauge(ui, &spec.title(&cfg), v, min, max, spec.decimals, warn);
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
        });

        ui.add_space(8.0);
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

/// Display box for raw digital bit registers: hex and binary.
fn raw_box(ui: &mut egui::Ui, title: &str, value: Option<f64>) {
    egui::Frame::group(ui.style())
        .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(230.0, 60.0));
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(title).weak());
                match value {
                    Some(v) => {
                        let b = v as u8;
                        ui.label(
                            RichText::new(format!("0x{b:02X}"))
                                .monospace()
                                .size(18.0),
                        );
                        ui.label(
                            RichText::new(format!("0b{b:08b}"))
                                .monospace()
                                .weak(),
                        );
                    }
                    None => {
                        ui.label(RichText::new("--").monospace().size(18.0));
                    }
                }
            });
        });
}
