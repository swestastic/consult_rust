//! Landing page: serial port selection and connection.

use crate::app::{ConnState, ConsultApp, TEST_MODE_LABEL};

impl ConsultApp {
    pub(crate) fn show_connect(&mut self, ui: &mut egui::Ui) {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.heading("Connect to ECU");
            ui.add_space(10.0);
            ui.label(
                "Select your USB Consult adapter, or Test Mode to try the app without hardware.",
            );
            ui.add_space(20.0);
        });

        egui::Grid::new("connect_grid")
            .num_columns(2)
            .spacing([10.0, 10.0])
            .show(ui, |ui| {
                ui.label("Port:");
                ui.horizontal(|ui| {
                    let mut choices = vec![TEST_MODE_LABEL.to_string()];
                    choices.extend(self.ports.iter().cloned());
                    egui::ComboBox::from_id_salt("port_combo")
                        .selected_text(self.selected_port.as_str())
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            for choice in choices {
                                ui.selectable_value(
                                    &mut self.selected_port,
                                    choice.clone(),
                                    choice,
                                );
                            }
                        });
                    if ui.button("Refresh").clicked() {
                        self.refresh_ports();
                    }
                });
                ui.end_row();

                ui.label("ECU address:");
                ui.label(format!(
                    "0x{} (change in Settings; most pre-'96 ECUs use EF)",
                    self.config.ecu_address
                ));
                ui.end_row();
            });

        ui.add_space(20.0);
        ui.vertical_centered(|ui| match self.conn {
            ConnState::Disconnected => {
                if ui
                    .add_sized([160.0, 36.0], egui::Button::new("Connect"))
                    .clicked()
                {
                    self.connect();
                }
            }
            ConnState::Connecting => {
                ui.add(egui::Spinner::new().size(28.0));
                ui.label("Connecting...");
            }
            ConnState::Connected => {
                if ui
                    .add_sized([160.0, 36.0], egui::Button::new("Disconnect"))
                    .clicked()
                {
                    self.disconnect();
                }
                ui.add_space(10.0);
                ui.label("Use the tabs above to open Data Stream, DTCs, or Testing.");
            }
        });
    }
}
