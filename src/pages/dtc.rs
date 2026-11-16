//! DTC page: read and erase diagnostic trouble codes, read ECU part number.

use crate::app::ConsultApp;
use crate::consult::link::LinkCommand;
use crate::consult::protocol::dtc_description;

impl ConsultApp {
    pub(crate) fn show_dtc(&mut self, ui: &mut egui::Ui) {
        if !self.connected() {
            ui.label("Not connected.");
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let busy = self.dtc.busy || self.dtc.watching;
                    if ui
                        .add_enabled(!busy, egui::Button::new("Read DTCs"))
                        .clicked()
                    {
                        self.send(LinkCommand::ReadDtc);
                        self.dtc.busy = true;
                        self.dtc.status = "Reading...".into();
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Erase DTCs"))
                        .on_hover_text("Clears stored codes. They return if the fault persists.")
                        .clicked()
                    {
                        self.send(LinkCommand::ClearDtc);
                        self.dtc.busy = true;
                        self.dtc.status = "Erasing...".into();
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Read ECU Part Number"))
                        .clicked()
                    {
                        self.send(LinkCommand::EcuInfo);
                        self.dtc.busy = true;
                        self.dtc.status = "Reading ECU info...".into();
                    }
                    if busy {
                        ui.add(egui::Spinner::new());
                    }
                });
                ui.add_space(2.0);
                ui.label(egui::RichText::new(&self.dtc.status).weak());

                if let Some(part) = &self.dtc.part_number {
                    ui.add_space(4.0);
                    ui.label(format!("ECU part number: {part}"));
                    match &self.ecu {
                        Some(info) => ui.label(format!("Identified: {}", info.summary)),
                        None => ui.label("Not in the ECU ID table — showing all options."),
                    };
                }

                ui.add_space(8.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("Stored codes").strong());
                    ui.add_space(4.0);
                    if self.dtc.entries.is_empty() {
                        ui.label("No codes read yet.");
                    } else {
                        egui::Grid::new("dtc_grid")
                            .num_columns(3)
                            .spacing([20.0, 6.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Code").strong());
                                ui.label(egui::RichText::new("Description").strong());
                                ui.label(egui::RichText::new("Starts since").strong());
                                ui.end_row();
                                for (code, starts) in &self.dtc.entries {
                                    ui.label(format!("{code:02X}"));
                                    ui.label(dtc_description(*code));
                                    ui.label(if *code == 0x55 {
                                        "-".to_string()
                                    } else {
                                        starts.to_string()
                                    });
                                    ui.end_row();
                                }
                            });
                    }
                });

                ui.add_space(8.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("Transient DTC watch").strong());
                    ui.label(
                        "Clears the transient DTC set (command 0x51), then reports codes as faults \
                         re-occur — e.g. while wiggling a suspect connector.",
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if self.dtc.watching {
                            if ui.button("Stop watch").clicked() {
                                self.send(LinkCommand::StopTransientWatch);
                                self.dtc.watching = false;
                                self.dtc.status = "Transient watch stopped.".into();
                            }
                            ui.add(egui::Spinner::new());
                            ui.label("Watching...");
                        } else if ui
                            .add_enabled(!self.dtc.busy, egui::Button::new("Start watch"))
                            .clicked()
                        {
                            self.send(LinkCommand::WatchTransientDtc);
                            self.dtc.watching = true;
                            self.dtc.transient.clear();
                            self.dtc.status = "Watching for transient codes...".into();
                        }
                    });
                    if !self.dtc.transient.is_empty() {
                        ui.add_space(4.0);
                        egui::Grid::new("transient_grid")
                            .num_columns(4)
                            .spacing([20.0, 6.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Code").strong());
                                ui.label(egui::RichText::new("Description").strong());
                                ui.label(egui::RichText::new("Starts since").strong());
                                ui.label(egui::RichText::new("First seen").strong());
                                ui.end_row();
                                for (code, starts, ts) in &self.dtc.transient {
                                    ui.label(format!("{code:02X}"));
                                    ui.label(dtc_description(*code));
                                    ui.label(starts.to_string());
                                    ui.label(ts);
                                    ui.end_row();
                                }
                            });
                    }
                });

                ui.add_space(6.0);
                ui.label(
                    "Codes clear automatically after 50 engine starts if the fault does not re-occur.",
                );
            });
    }
}
