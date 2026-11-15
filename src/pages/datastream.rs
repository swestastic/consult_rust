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
                    let mut col = 0;
                    for (spec, val) in specs.iter().zip(&values) {
                        if catalog::is_digital(spec) {
                            continue; // rendered in the digital region below
                        }
                        let v = val.unwrap_or(0.0);
                        let warn = match spec.id {
                            "rpm" => v >= rpm_warn,
                            "coolant" => v >= temp_warn,
                            _ => false,
                        };
                        let (min, max) = spec.range_for(&cfg);
                        gauge(ui, &spec.title(&cfg), v, min, max, spec.decimals, warn);
                        col += 1;
                        if col % 3 == 0 {
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
                ui.horizontal_wrapped(|ui| {
                    for (spec, val) in digital {
                        digital_panel(ui, spec, val);
                    }
                });
            }
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

/// Compact indicator panel for one digital bit register: 8 bits in two
/// columns, green dot + name when set, dim when clear.
fn digital_panel(ui: &mut egui::Ui, spec: &catalog::SensorSpec, value: Option<f64>) {
    egui::Frame::group(ui.style())
        .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
        .show(ui, |ui| {
            ui.label(RichText::new(spec.label).strong().small());
            let byte = value.map(|v| v as u8);
            egui::Grid::new(format!("dig_{}", spec.id))
                .num_columns(2)
                .spacing([18.0, 2.0])
                .show(ui, |ui| {
                    for row in 0u8..4 {
                        bit_indicator(ui, spec, byte, 7 - row); // left column: bits 7..4
                        bit_indicator(ui, spec, byte, 3 - row); // right column: bits 3..0
                        ui.end_row();
                    }
                });
            let hex = byte.map(|b| format!("0x{b:02X}")).unwrap_or("--".into());
            ui.label(RichText::new(hex).monospace().weak().small());
        });
}

fn bit_indicator(ui: &mut egui::Ui, spec: &catalog::SensorSpec, byte: Option<u8>, bit: u8) {
    let on = byte.is_some_and(|b| b & (1 << bit) != 0);
    let name = catalog::digital_bit_name(spec.id, bit)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Bit {bit}"));
    let (dot, text) = if on {
        (Color32::from_rgb(80, 220, 100), ui.visuals().text_color())
    } else {
        (Color32::from_rgb(70, 70, 70), ui.visuals().weak_text_color())
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.colored_label(dot, "\u{25CF}");
        ui.label(RichText::new(name).color(text).small());
    })
    .response
    .on_hover_text(format!("{} bit {}: {}", spec.label, bit, if on { "ON" } else { "off" }));
}
