//! Data Stream page: live sensor gauges and CSV logging.

use egui::{Color32, RichText};

use crate::app::ConsultApp;
use crate::config::{SpeedUnit, TempUnit};
use crate::consult::link::LinkCommand;
use crate::consult::protocol::{self, Readings};
use crate::widgets::gauge;

impl ConsultApp {
    pub(crate) fn show_datastream(&mut self, ui: &mut egui::Ui) {
        if !self.connected() {
            ui.label("Not connected.");
            return;
        }

        let readings = self
            .frame
            .as_deref()
            .and_then(protocol::parse_readings)
            .unwrap_or(ZERO);

        let cfg = self.config.clone();
        let speed = cfg.display_speed(readings.speed_kph);
        let temp = cfg.display_temp(readings.coolant_c);
        let speed_label = cfg.speed_unit.label();
        let temp_label = cfg.temp_unit.label();

        let speed_max = if cfg.speed_unit == SpeedUnit::Mph { 160.0 } else { 260.0 };
        let (temp_min, temp_max) = if cfg.temp_unit == TempUnit::F {
            (100.0, 250.0)
        } else {
            (40.0, 120.0)
        };
        let rpm_warn = cfg.rpm_warning as f64;
        let temp_warn = cfg.coolant_warning_display();

        egui::Grid::new("gauges")
            .num_columns(3)
            .spacing([12.0, 12.0])
            .show(ui, |ui| {
                gauge(ui, "RPM", readings.rpm, 0.0, 8000.0, 0, warn_high(readings.rpm, rpm_warn));
                gauge(ui, &format!("Speed ({speed_label})"), speed, 0.0, speed_max, 0, false);
                gauge(ui, &format!("Coolant (°{temp_label})"), temp, temp_min, temp_max, 0, warn_high(temp, temp_warn));
                ui.end_row();
                gauge(ui, "TPS (V)", readings.tps_v, 0.0, 5.0, 2, false);
                gauge(ui, "Timing (°BTDC)", readings.timing_deg, -10.0, 50.0, 0, false);
                gauge(ui, "Battery (V)", readings.battery_v, 8.0, 16.0, 1, false);
                ui.end_row();
                gauge(ui, "AAC (%)", readings.aac_pct, 0.0, 100.0, 0, false);
                gauge(ui, "Injector (ms)", readings.inj_ms, 0.0, 20.0, 1, false);
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(format!("MAF: {:.2} V", readings.maf_v));
            ui.separator();
            ui.label(format!("O2: {:.2} V", readings.o2_v));
            ui.separator();
            ui.label(format!("Speed correction: x{:.3}", cfg.speed_correction()));
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if self.logger.is_logging() {
                if ui.button("Stop Data Logging").clicked() {
                    self.logger.stop();
                }
                if let Some(name) = &self.logger.file_name {
                    ui.label(RichText::new(format!("Logging to {name}")).color(Color32::GREEN));
                }
            } else if ui.button("Start Data Logging").clicked() {
                match self.logger.start(&self.config) {
                    Ok(_) => {}
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
                self.send(LinkCommand::StartStream);
                self.streaming = true;
            }
        });

        // Log once per incoming frame.
        if self.logger.is_logging() && self.frame_seq != self.last_logged_seq {
            self.logger.log(&readings, &self.config);
            self.last_logged_seq = self.frame_seq;
        }

    }
}

const ZERO: Readings = Readings {
    rpm: 0.0,
    speed_kph: 0.0,
    coolant_c: 0.0,
    battery_v: 0.0,
    tps_v: 0.0,
    maf_v: 0.0,
    aac_pct: 0.0,
    inj_ms: 0.0,
    timing_deg: 0.0,
    o2_v: 0.0,
};

fn warn_high(value: f64, threshold: f64) -> bool {
    value >= threshold
}
