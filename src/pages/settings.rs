//! Settings page: edit and persist `consult_config.json`.

use crate::app::ConsultApp;
use crate::catalog;
use crate::config::{CONFIG_FILE, SpeedUnit, TempUnit};

impl ConsultApp {
    /// Copy the live config into the text edit buffer.
    pub(crate) fn load_settings_buffer(&mut self) {
        let c = &self.config;
        self.settings = crate::app::SettingsState {
            speed_unit: c.speed_unit,
            temp_unit: c.temp_unit,
            rpm_warning: c.rpm_warning.to_string(),
            coolant_warning: format!("{:.1}", c.coolant_warning_display()),
            battery_low: format!("{:.1}", c.battery_low_v),
            speed_correction: fmt(c.speed_correction),
            ecu_address: c.ecu_address.clone(),
            items: c.display_items.clone(),
            status: String::new(),
            about_open: false,
        };
    }

    pub(crate) fn show_settings(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                let s = &mut self.settings;

                egui::Grid::new("settings_grid")
                    .num_columns(4)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Speed units:");
                        egui::ComboBox::from_id_salt("speed_unit")
                            .selected_text(s.speed_unit.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut s.speed_unit, SpeedUnit::Mph, "MPH");
                                ui.selectable_value(&mut s.speed_unit, SpeedUnit::Kph, "KPH");
                            });
                        ui.end_row();

                        ui.label("Temp units:");
                        egui::ComboBox::from_id_salt("temp_unit")
                            .selected_text(s.temp_unit.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut s.temp_unit, TempUnit::F, "F");
                                ui.selectable_value(&mut s.temp_unit, TempUnit::C, "C");
                            });
                        ui.end_row();

                        ui.label("RPM warning:");
                        ui.add(egui::TextEdit::singleline(&mut s.rpm_warning).desired_width(80.0));
                        ui.end_row();

                        ui.label("Coolant warning:");
                        ui.add(
                            egui::TextEdit::singleline(&mut s.coolant_warning).desired_width(80.0),
                        );
                        ui.label(format!("°{}", s.temp_unit.label()));
                        ui.end_row();

                        ui.label("Battery warning (V):");
                        ui.add(egui::TextEdit::singleline(&mut s.battery_low).desired_width(80.0));
                        ui.label("warns below this voltage");
                        ui.end_row();

                        ui.label("ECU address (hex):");
                        ui.add(egui::TextEdit::singleline(&mut s.ecu_address).desired_width(80.0));
                        ui.label("EF for most ECUs; ED for many '96+; AUTO to probe both");
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.label(egui::RichText::new("Speedometer correction").strong());
                ui.add_space(4.0);

                let s = &mut self.settings;
                egui::Grid::new("correction_grid")
                    .num_columns(3)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Correction factor:");
                        ui.add(
                            egui::TextEdit::singleline(&mut s.speed_correction)
                                .desired_width(80.0),
                        );
                        ui.label("actual = indicated x factor (1 = no correction)");
                        ui.end_row();
                    });

                // ---- Displayed items (stream register selection)
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Displayed items").strong());
                ui.label("Choose what the Data Stream page requests and shows.");
                ui.add_space(4.0);

                // ECU-based filtering: once the connected ECU is identified,
                // only items it is known to support are listed. The show-all
                // toggle applies immediately and covers mislabeled ECUs.
                let mut show_all = self.config.show_all_options;
                if ui
                    .checkbox(&mut show_all, "Show all options (ignore ECU filter)")
                    .on_hover_text("The connected ECU's part number limits this list to known-working items. Enable to see everything.")
                    .changed()
                {
                    self.config.show_all_options = show_all;
                    let _ = self.config.save();
                }
                let filter = if show_all { None } else { self.ecu.clone() };
                if let Some(info) = &filter {
                    let hidden = catalog::CATALOG
                        .iter()
                        .filter(|sp| !info.supports(catalog::feature_of(sp.id)))
                        .count();
                    ui.label(format!(
                        "Filtered to 23710-{}: {} — {} item(s) hidden.",
                        info.part, info.summary, hidden
                    ));
                } else if self.ecu.is_none() && !show_all {
                    ui.label("ECU not identified — showing all items.");
                }
                ui.add_space(4.0);

                let s = &mut self.settings;
                let selected = catalog::selected_specs(&s.items);
                let used = catalog::register_count(&selected);
                ui.label(format!(
                    "{used}/{} registers used",
                    catalog::MAX_STREAM_REGISTERS
                ));

                let mut vis = 0usize;
                let mut hidden_selected: Vec<&'static catalog::SensorSpec> = Vec::new();
                egui::Grid::new("items_grid")
                    .num_columns(3)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for spec in catalog::CATALOG.iter() {
                            if let Some(info) = &filter
                                && !info.supports(catalog::feature_of(spec.id))
                            {
                                if s.items.iter().any(|id| id == spec.id) {
                                    hidden_selected.push(spec);
                                }
                                continue;
                            }
                            let mut on = s.items.iter().any(|id| id == spec.id);
                            let text = format!("{} ({})", spec.label, spec.regs_display());
                            let resp = ui.checkbox(&mut on, text);
                            if resp.changed() {
                                if on {
                                    let would = used + spec.regs.len();
                                    if would > catalog::MAX_STREAM_REGISTERS {
                                        s.status = format!(
                                            "Cannot add {}: would exceed {} registers.",
                                            spec.label,
                                            catalog::MAX_STREAM_REGISTERS
                                        );
                                    } else {
                                        s.items.push(spec.id.to_string());
                                    }
                                } else {
                                    s.items.retain(|id| id != spec.id);
                                }
                            }
                            resp.on_hover_ui(|ui| spec_hover(ui, spec, s.speed_unit, s.temp_unit));
                            if vis % 3 == 2 {
                                ui.end_row();
                            }
                            vis += 1;
                        }
                    });

                // Selected items the filter hides keep streaming, but stay
                // listed here so they can be turned off.
                if !hidden_selected.is_empty() {
                    ui.add_space(6.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 160, 60),
                        "Selected but not listed for this ECU (still requested):",
                    );
                    ui.horizontal_wrapped(|ui| {
                        for spec in hidden_selected {
                            let mut on = true;
                            if ui.checkbox(&mut on, spec.label).changed() && !on {
                                s.items.retain(|id| id != spec.id);
                            }
                        }
                    });
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        match self.apply_settings() {
                            Ok(()) => {
                                self.settings.status = format!("Saved to {CONFIG_FILE}.");
                                self.status = "Settings saved.".into();
                            }
                            Err(e) => self.settings.status = e,
                        }
                    }
                    if ui.button("About").clicked() {
                        self.settings.about_open = true;
                    }
                });
                if !self.settings.status.is_empty() {
                    ui.label(&self.settings.status);
                }
            });

        let mut about_open = self.settings.about_open;
        egui::Window::new("About")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut about_open)
            .show(ui.ctx(), |ui| {
                ui.label(egui::RichText::new(env!("CARGO_PKG_NAME")).strong());
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label(env!("CARGO_PKG_DESCRIPTION"));
                ui.add_space(6.0);
                ui.label(format!("Author: {}", env!("CARGO_PKG_AUTHORS")));
            });
        self.settings.about_open = about_open;
    }

    /// Parse the edit buffer into the live config and write it to disk.
    fn apply_settings(&mut self) -> Result<(), String> {
        let s = &self.settings;
        let f = |name: &str, v: &str| -> Result<f64, String> {
            v.trim()
                .parse::<f64>()
                .map_err(|_| format!("{name}: \"{v}\" is not a number"))
        };
        let rpm_warning: u32 = s
            .rpm_warning
            .trim()
            .parse()
            .map_err(|_| format!("RPM warning: \"{}\" is not a number", s.rpm_warning))?;

        let mut cfg = self.config.clone();
        cfg.speed_unit = s.speed_unit;
        cfg.temp_unit = s.temp_unit;
        cfg.rpm_warning = rpm_warning;
        cfg.coolant_warning_c = cfg.temp_to_c(f("Coolant warning", &s.coolant_warning)?);
        cfg.battery_low_v = f("Battery warning", &s.battery_low)?;
        cfg.speed_correction = f("Speed correction factor", &s.speed_correction)?;
        cfg.ecu_address = s.ecu_address.trim().to_uppercase();
        cfg.ecu_address_candidates()?; // validate

        let new_items = catalog::sanitize_selection(s.items.clone());
        let selection_changed = new_items != cfg.display_items;
        cfg.display_items = new_items;

        cfg.save()?;
        let selection_changed = selection_changed && self.connected();
        self.config = cfg;

        if selection_changed {
            // Column set changes invalidate an in-progress log, and the ECU
            // needs a fresh stream request for the new registers.
            self.history.clear();
            self.history_start = None;
            if self.logger.is_logging() {
                self.logger.stop();
                self.status = "Logging stopped (displayed items changed).".into();
            }
            if self.streaming {
                self.send(crate::consult::link::LinkCommand::StopStream);
                let regs = self.config.stream_registers();
                self.send(crate::consult::link::LinkCommand::StartStream(regs));
            }
        }
        Ok(())
    }
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Hover card for a Displayed-items checkbox: description, decoded range in
/// the selected units, and typical readings. Digital registers list their
/// named bits instead.
fn spec_hover(
    ui: &mut egui::Ui,
    spec: &catalog::SensorSpec,
    speed_unit: SpeedUnit,
    temp_unit: TempUnit,
) {
    use egui::RichText;
    ui.set_max_width(300.0);
    ui.label(RichText::new(format!("{} — {}", spec.label, spec.regs_display())).strong());
    ui.label(spec.desc);
    ui.add_space(4.0);

    if spec.unit == catalog::UnitKind::Raw {
        ui.label(RichText::new("Bits:").strong());
        let mut any = false;
        for bit in (0..8).rev() {
            if let Some(name) = catalog::digital_bit_name(spec.id, bit) {
                ui.label(format!("  b{bit}: {name}"));
                any = true;
            }
        }
        if !any {
            ui.label("  (no named bits)");
        }
        return;
    }

    let (lo, hi) = spec.range_in(speed_unit.label(), temp_unit.label());
    ui.label(format!(
        "Range: {} – {} {}",
        fmt(lo),
        fmt(hi),
        spec.unit_in(speed_unit.label(), temp_unit.label())
    ));
    ui.label(format!("Typical: {}", spec.typical));
}
