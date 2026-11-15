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
                egui::Grid::new("digital_region")
                    .num_columns(3)
                    .spacing([12.0, 12.0])
                    .show(ui, |ui| {
                        for (i, (spec, val)) in digital.iter().enumerate() {
                            digital_panel(ui, spec, *val);
                            if i % 3 == 2 {
                                ui.end_row();
                            }
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
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(230.0, 124.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let dim = ui.visuals().weak_text_color();
    let fg = ui.visuals().text_color();

    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, ui.style().visuals.widgets.noninteractive.bg_stroke.color),
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
