//! Testing page: Consult active tests (0x0A commands) and raw command entry.

use egui::RichText;

use crate::app::ConsultApp;
use crate::consult::link::LinkCommand;
use crate::consult::protocol::{self, activate};

impl ConsultApp {
    pub(crate) fn show_testing(&mut self, ui: &mut egui::Ui) {
        if !self.connected() {
            ui.label("Not connected.");
            return;
        }

        ui.label(
            RichText::new(
                "Active tests change live ECU outputs. Use with care; engine should be running \
                 (or key-on) in a safe state.",
            )
            .color(egui::Color32::from_rgb(230, 160, 60)),
        );
        ui.add_space(6.0);

        ui.columns(2, |cols| {
            show_controls(self, &mut cols[0]);
            show_raw(self, &mut cols[1]);
        });
    }

    fn send_activation(&mut self, kind: u8, data: u8) {
        self.send(LinkCommand::Activate { kind, data });
        self.testing.push_log(match kind {
            k if k == activate::FUEL_PUMP => {
                format!("Fuel pump {}", if data == 0x00 { "ON" } else { "OFF" })
            }
            k if k == activate::POWER_BALANCE => format!("Power balance mask 0x{data:02X}"),
            k if k == activate::COOLANT_TEMP => {
                format!("Coolant temp test: {} C", data as i32 - 50)
            }
            k if k == activate::FUEL_TRIM => format!("Fuel trim: {}%", data as i32 - 100),
            k if k == activate::IGN_TIMING => format!("Timing adjust: {} deg", data as i8),
            k if k == activate::AAC => format!("AAC adjust: {:.1}%", data as i8 as f32 / 2.0),
            k if k == activate::IACV_OPENING => {
                format!("IACV opening: {:.0}%", data as f32 / 1.4)
            }
            k if k == activate::FICD_SOLENOID => {
                format!("IACV/FICD solenoid {}", on_off(data))
            }
            k if k == activate::PREG_SOLENOID => {
                format!("P/Reg control solenoid {}", on_off(data))
            }
            k if k == activate::AACV_CONTROL_VALVE => {
                format!("IACV-AAC/V control valve {}", on_off(data))
            }
            k if k == activate::TIMING_SOLENOID => {
                format!("Valve timing solenoid {}", on_off(data))
            }
            k if k == activate::MAP_SW_SOLENOID => {
                format!("MAP SW solenoid {}", on_off(data))
            }
            k if k == activate::FAN_LOW => format!("Coolant fan low {}", on_off(data)),
            k if k == activate::FAN_HIGH => format!("Coolant fan high {}", on_off(data)),
            k if k == activate::CLEAR_SELF_LEARN => "Cleared self-learn values".to_string(),
            _ => format!("Activation 0x{kind:02X} = 0x{data:02X}"),
        });
    }
}

fn on_off(data: u8) -> &'static str {
    if data == 0x00 { "OFF" } else { "ON" }
}

/// ON/OFF button pair for a solenoid/relay activation (0x01 = on, 0x00 = off).
fn on_off_row(app: &mut ConsultApp, ui: &mut egui::Ui, label: &str, kind: u8) {
    ui.label(label);
    if ui.button("ON").clicked() {
        app.send_activation(kind, 0x01);
    }
    if ui.button("OFF").clicked() {
        app.send_activation(kind, 0x00);
    }
    ui.end_row();
}

fn show_controls(app: &mut ConsultApp, ui: &mut egui::Ui) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Fuel pump relay").strong());
        ui.horizontal(|ui| {
            if ui.button("Pump ON").clicked() {
                app.send_activation(activate::FUEL_PUMP, 0x00);
            }
            if ui.button("Pump OFF").clicked() {
                app.send_activation(activate::FUEL_PUMP, 0x01);
            }
        });
        ui.label("Note: on some ECUs (SR20 etc.) 0x00/0x01 are inverted.");
    });

    ui.add_space(6.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Power balance (cylinder cut)").strong());
        let mut changed = false;
        for i in 0..8 {
            if ui
                .checkbox(
                    &mut app.testing.cylinders[i],
                    format!("Disable cylinder {}", i + 1),
                )
                .changed()
            {
                changed = true;
            }
        }
        if changed {
            let mask = app
                .testing
                .cylinders
                .iter()
                .enumerate()
                .fold(0u8, |m, (i, off)| if *off { m | (1 << i) } else { m });
            app.send_activation(activate::POWER_BALANCE, mask);
        }
        if ui.button("All cylinders on").clicked() {
            app.testing.cylinders = [false; 8];
            app.send_activation(activate::POWER_BALANCE, 0x00);
        }
    });

    ui.add_space(6.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Manual adjustments").strong());

        let temp_unit = app.config.temp_unit.label();
        let (lo, hi) = match app.config.temp_unit {
            crate::config::TempUnit::C => (-10.0, 130.0),
            crate::config::TempUnit::F => (14.0, 266.0),
        };
        ui.horizontal(|ui| {
            ui.label(format!("Coolant temp (°{temp_unit}):"));
            ui.add(egui::Slider::new(&mut app.testing.temp_display, lo..=hi));
            if ui.button("Send").clicked() {
                let c = app.config.temp_to_c(app.testing.temp_display as f64);
                app.send_activation(activate::COOLANT_TEMP, (c as i32 + 50).clamp(0, 255) as u8);
            }
        });

        ui.horizontal(|ui| {
            ui.label("Fuel trim (%):       ");
            ui.add(
                egui::Slider::new(&mut app.testing.fuel_trim_pct, -10.0..=10.0)
                    .step_by(1.0)
                    .suffix("%"),
            );
            if ui.button("Send").clicked() {
                app.send_activation(
                    activate::FUEL_TRIM,
                    (100.0 + app.testing.fuel_trim_pct).clamp(0.0, 255.0) as u8,
                );
            }
        });

        ui.horizontal(|ui| {
            ui.label("Timing adjust (°):   ");
            ui.add(egui::Slider::new(&mut app.testing.timing_deg, -10..=10).suffix("°"));
            if ui.button("Send").clicked() {
                app.send_activation(activate::IGN_TIMING, app.testing.timing_deg as i8 as u8);
            }
        });

        ui.horizontal(|ui| {
            ui.label("AAC adjust (%):     ");
            ui.add(
                egui::Slider::new(&mut app.testing.aac_pct, -10.0..=10.0)
                    .step_by(0.5)
                    .suffix("%"),
            );
            if ui.button("Send").clicked() {
                app.send_activation(activate::AAC, (app.testing.aac_pct * 2.0) as i8 as u8);
            }
        });

        ui.horizontal(|ui| {
            ui.label("IACV opening (%):  ");
            ui.add(
                egui::Slider::new(&mut app.testing.iacv_pct, 0.0..=100.0)
                    .step_by(1.0)
                    .suffix("%"),
            );
            if ui.button("Send").clicked() {
                // 0x85 scale: 0x00 = 0%, 0x8C (140) = 100%.
                app.send_activation(
                    activate::IACV_OPENING,
                    (app.testing.iacv_pct * 1.4).round().clamp(0.0, 140.0) as u8,
                );
            }
        });

        if ui
            .button("Clear self-learn values")
            .on_hover_text("Resets learned closed-loop fuel trims (register 0x1C).")
            .clicked()
        {
            app.send_activation(activate::CLEAR_SELF_LEARN, 0x00);
        }
    });

    ui.add_space(6.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Solenoid / relay / fan tests").strong());
        ui.label("Momentary output overrides; send OFF when done.");
        egui::Grid::new("onoff_grid").num_columns(3).show(ui, |ui| {
            on_off_row(app, ui, "IACV/FICD solenoid", activate::FICD_SOLENOID);
            on_off_row(app, ui, "P/Reg control solenoid", activate::PREG_SOLENOID);
            on_off_row(app, ui, "IACV-AAC/V control valve", activate::AACV_CONTROL_VALVE);
            on_off_row(app, ui, "Valve timing solenoid (VTC)", activate::TIMING_SOLENOID);
            on_off_row(app, ui, "MAP SW solenoid", activate::MAP_SW_SOLENOID);
            on_off_row(app, ui, "Coolant fan low speed", activate::FAN_LOW);
            on_off_row(app, ui, "Coolant fan high speed", activate::FAN_HIGH);
        });
    });
}

fn show_raw(app: &mut ConsultApp, ui: &mut egui::Ui) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Raw command (hex)").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.testing.raw_input)
                    .desired_width(200.0)
                    .hint_text("e.g. 5A 01 F0"),
            );
            if ui.button("Send").clicked() {
                match protocol::parse_hex(&app.testing.raw_input) {
                    Ok(bytes) => app.send(LinkCommand::Raw(bytes)),
                    Err(e) => app.testing.push_log(format!("Parse error: {e}")),
                }
            }
            if ui.button("Clear log").clicked() {
                app.testing.log.clear();
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Response log").strong());
        let text = app
            .testing
            .log
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        egui::ScrollArea::vertical()
            .max_height(430.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut text.as_str())
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
            });
    });
}
