//! Testing page: Consult active tests (0x0A commands) and raw command entry.

use egui::RichText;

use crate::app::ConsultApp;
use crate::catalog;
use crate::consult::link::LinkCommand;
use crate::consult::protocol::{self, activate};

/// Registers streamed while the Testing page is open, feeding the live
/// adjustment readouts: RPM MSB/LSB, TPS, digital 0x13, ignition timing.
pub(crate) const HELPER_REGS: [u8; 5] = [0x00, 0x01, 0x0D, 0x13, 0x16];

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

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                show_live_readouts(self, ui);
                ui.add_space(6.0);

                ui.columns(2, |cols| {
                    show_controls(self, &mut cols[0]);
                    show_romdump(self, &mut cols[1]);
                    show_raw(self, &mut cols[1]);
                });
            });
    }

    /// Create the dump file and mark the dump active; the caller then
    /// sends LinkCommand::RomDump.
    fn start_rom_dump_file(&mut self, start: u16, len: u32) -> Result<(), String> {
        std::fs::create_dir_all("dumps").map_err(|e| format!("Cannot create dumps dir: {e}"))?;
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let name = format!("dumps/romdump_{start:04X}_{len:X}_{stamp}.bin");
        let file =
            std::fs::File::create(&name).map_err(|e| format!("Cannot create {name}: {e}"))?;
        self.romdump.writer = Some(std::io::BufWriter::new(file));
        self.romdump.file_name = Some(name);
        self.romdump.active = true;
        self.romdump.total = len;
        self.romdump.done = 0;
        Ok(())
    }

    fn send_activation(&mut self, kind: u8, data: u8) {
        self.send(LinkCommand::Activate { kind, data });
        self.testing.push_log(match kind {
            k if k == activate::FUEL_PUMP => {
                format!("Fuel pump {}", if data == 0x00 { "ON" } else { "OFF" })
            }
            k if k == activate::POWER_BALANCE => {
                if self.testing.pb_single_cyl {
                    if data == 0 {
                        "Power balance: all cylinders on".to_string()
                    } else {
                        format!("Power balance: cut cylinder {data}")
                    }
                } else {
                    format!("Power balance mask 0x{data:02X}")
                }
            }
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

/// Live readouts for the under-hood adjustment workflows: TPS adjustment
/// (closed-throttle switch + sensor voltage) and base timing/idle.
fn show_live_readouts(app: &mut ConsultApp, ui: &mut egui::Ui) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Live adjustment readouts").strong());
        let frame: Option<&[u8]> = app.frame.as_deref();
        let cfg = &app.config;
        let get = |id: &str| {
            catalog::find_spec(id).and_then(|s| frame.and_then(|f| s.decode(f, &HELPER_REGS, cfg)))
        };

        ui.horizontal(|ui| {
            let tps = get("tps");
            ui.label(format!("TPS: {}", fmt_opt(tps, 2, "V")));
            let closed = get("d13").is_some_and(|v| v as u8 & 1 != 0);
            let (dot, text) = if closed {
                (egui::Color32::from_rgb(80, 220, 100), "closed-throttle switch ON")
            } else {
                (egui::Color32::from_rgb(70, 70, 70), "closed-throttle switch OFF")
            };
            ui.colored_label(dot, "\u{25CF}");
            ui.label(text);
            ui.label("(set closed throttle to ~0.45 V with the switch ON)");
        });

        ui.horizontal(|ui| {
            ui.label(format!(
                "Timing: {}   RPM: {}",
                fmt_opt(get("timing"), 0, "°BTDC"),
                fmt_opt(get("rpm"), 0, ""),
            ));
            ui.label("(use Base idle / timing mode below instead of unplugging the TPS)");
        });
    });
}

fn fmt_opt(v: Option<f64>, decimals: usize, unit: &str) -> String {
    match v {
        Some(v) if unit.is_empty() => format!("{v:.decimals$}"),
        Some(v) => format!("{v:.decimals$} {unit}"),
        None => "--".into(),
    }
}

/// ROM dump controls (0xC9): address range in, progress, cancel.
fn show_romdump(app: &mut ConsultApp, ui: &mut egui::Ui) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("ROM dump").strong());

        if app.romdump.active {
            let frac = if app.romdump.total > 0 {
                app.romdump.done as f32 / app.romdump.total as f32
            } else {
                0.0
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .text(format!("0x{:X} / 0x{:X} bytes", app.romdump.done, app.romdump.total)),
            );
            if let Some(name) = &app.romdump.file_name {
                ui.label(format!("Writing {name}"));
            }
            if ui.button("Cancel dump").clicked() {
                app.send(LinkCommand::CancelRomDump);
            }
            return;
        }

        ui.label("Reads ECU memory 8 bytes per request; a full 32 KB dump takes a few minutes.");
        ui.horizontal(|ui| {
            ui.label("Start (hex):");
            ui.add(
                egui::TextEdit::singleline(&mut app.romdump.start_text).desired_width(60.0),
            );
            ui.label("Length (hex):");
            ui.add(egui::TextEdit::singleline(&mut app.romdump.len_text).desired_width(60.0));
        });
        if ui
            .button("Start dump")
            .on_hover_text("ROM space is 0x8000-0xFFFF; RAM below. Output goes to dumps/.")
            .clicked()
        {
            let range =
                protocol::parse_rom_range(&app.romdump.start_text, &app.romdump.len_text);
            match range {
                Ok((start, len)) => match app.start_rom_dump_file(start, len) {
                    Ok(()) => {
                        app.send(LinkCommand::RomDump { start, len });
                        app.testing
                            .push_log(format!("ROM dump started: 0x{start:04X} +0x{len:X}"));
                    }
                    Err(e) => app.testing.push_log(e),
                },
                Err(e) => app.testing.push_log(e),
            }
        }
    });
    ui.add_space(6.0);
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
        ui.label(RichText::new("Base idle / timing mode").strong());
        ui.label(
            "Holds the AAC valve and ignition timing at base — the software equivalent \
             of unplugging the TPS — so base idle and timing can be set at the \
             adjusters. Stays active until stopped.",
        );
        ui.horizontal(|ui| {
            let active = app.testing.base_idle_active;
            if ui.add_enabled(!active, egui::Button::new("Start")).clicked() {
                app.send(LinkCommand::ActivateSet(vec![
                    (activate::AAC, 0x00),
                    (activate::IGN_TIMING, 0x00),
                ]));
                app.testing.base_idle_active = true;
                app.testing
                    .push_log("Base idle mode ON (AAC valve + timing held)".into());
            }
            if ui.add_enabled(active, egui::Button::new("Stop")).clicked() {
                app.send(LinkCommand::CancelActivations);
                app.testing.base_idle_active = false;
                app.testing.push_log("Base idle mode OFF".into());
            }
            if active {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 90, 90),
                    RichText::new("ACTIVE").strong(),
                );
            }
        });
    });

    ui.add_space(6.0);
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
        if ui
            .checkbox(
                &mut app.testing.pb_single_cyl,
                "SR20-style single-cylinder select",
            )
            .on_hover_text(
                "Some ECUs (SR20 etc.) take 0x01-0x08 to select one cylinder \
                 instead of a bitmask.",
            )
            .changed()
        {
            app.testing.cylinders = [false; 8];
            app.send_activation(activate::POWER_BALANCE, 0x00);
        }
        let single = app.testing.pb_single_cyl;
        for i in 0..8 {
            let mut on = app.testing.cylinders[i];
            if ui
                .checkbox(&mut on, format!("Disable cylinder {}", i + 1))
                .changed()
            {
                if single {
                    // Radio behavior: one cylinder at a time, value = index+1.
                    app.testing.cylinders = [false; 8];
                    app.testing.cylinders[i] = on;
                    let data = if on { (i + 1) as u8 } else { 0x00 };
                    app.send_activation(activate::POWER_BALANCE, data);
                } else {
                    app.testing.cylinders[i] = on;
                    let mask = app
                        .testing
                        .cylinders
                        .iter()
                        .enumerate()
                        .fold(0u8, |m, (i, off)| if *off { m | (1 << i) } else { m });
                    app.send_activation(activate::POWER_BALANCE, mask);
                }
            }
        }
        if ui.button("All cylinders on").clicked() {
            app.testing.cylinders = [false; 8];
            app.send_activation(activate::POWER_BALANCE, 0x00);
        }
    });

    ui.add_space(6.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Manual adjustments").strong());
        ui.label("Adjustments latch until cancelled (or you leave this page).");

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

        if ui
            .button("Cancel all active tests")
            .on_hover_text("Sends STOP (0x30), cancelling every latched adjustment and override.")
            .clicked()
        {
            app.send(LinkCommand::CancelActivations);
            app.testing.base_idle_active = false;
            app.testing.push_log("Cancelled all active tests".into());
        }
    });

    ui.add_space(6.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Solenoid / relay / fan tests").strong());
        ui.label("Overrides latch until you send OFF, cancel, or leave this page.");
        egui::Grid::new("onoff_grid").num_columns(3).show(ui, |ui| {
            on_off_row(app, ui, "IACV/FICD solenoid", activate::FICD_SOLENOID);
            // Hardware-specific tests show only when the identified ECU is
            // known to have it (or the show-all override is on).
            if app.feature_visible(crate::ecuid::activation_feature(activate::PREG_SOLENOID)) {
                on_off_row(app, ui, "P/Reg control solenoid", activate::PREG_SOLENOID);
            }
            on_off_row(app, ui, "IACV-AAC/V control valve", activate::AACV_CONTROL_VALVE);
            if app.feature_visible(crate::ecuid::activation_feature(activate::TIMING_SOLENOID)) {
                on_off_row(app, ui, "Valve timing solenoid (VTC)", activate::TIMING_SOLENOID);
            }
            if app.feature_visible(crate::ecuid::activation_feature(activate::MAP_SW_SOLENOID)) {
                on_off_row(app, ui, "MAP SW solenoid", activate::MAP_SW_SOLENOID);
            }
            on_off_row(app, ui, "Coolant fan low speed", activate::FAN_LOW);
            on_off_row(app, ui, "Coolant fan high speed", activate::FAN_HIGH);
        });
        let any_hidden = [activate::PREG_SOLENOID, activate::MAP_SW_SOLENOID, activate::TIMING_SOLENOID]
            .iter()
            .any(|k| !app.feature_visible(crate::ecuid::activation_feature(*k)));
        if any_hidden {
            ui.label("Some tests are hidden by the ECU filter (Settings > Show all options).");
        }
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
