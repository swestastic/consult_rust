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

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let busy = self.dtc.busy;
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
            ui.separator();
            ui.label(&self.dtc.status);
        });

        if let Some(part) = &self.dtc.part_number {
            ui.add_space(4.0);
            ui.label(format!("ECU part number: {part}"));
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

        ui.add_space(6.0);
        ui.label(
            "Codes clear automatically after 50 engine starts if the fault does not re-occur.",
        );
    }
}
