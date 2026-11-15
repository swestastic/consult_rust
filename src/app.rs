//! Top-level application state and the eframe::App implementation.
//! Individual pages live in `src/pages/*` as `impl ConsultApp` blocks.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::consult::link::{LinkCommand, LinkEvent, LinkHandle};
use crate::logfile::LogFile;
use crate::logging::DataLogger;

pub const TEST_MODE_LABEL: &str = "Test Mode (Simulated Data)";
pub const REPLAY_SPEEDS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Connect,
    DataStream,
    Dtc,
    Testing,
    Replay,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Disconnected,
    Connecting,
    Connected,
}

pub struct DtcState {
    pub entries: Vec<(u8, u8)>,
    pub part_number: Option<String>,
    pub busy: bool,
    pub status: String,
}

impl Default for DtcState {
    fn default() -> Self {
        DtcState {
            entries: Vec::new(),
            part_number: None,
            busy: false,
            status: "Ready".into(),
        }
    }
}

pub struct TestingState {
    pub cylinders: [bool; 8],
    pub fuel_trim_pct: f32,
    pub timing_deg: i32,
    pub aac_pct: f32,
    pub temp_display: f32,
    pub raw_input: String,
    pub log: VecDeque<String>,
}

impl Default for TestingState {
    fn default() -> Self {
        TestingState {
            cylinders: [false; 8],
            fuel_trim_pct: 0.0,
            timing_deg: 0,
            aac_pct: 0.0,
            temp_display: 180.0,
            raw_input: String::new(),
            log: VecDeque::new(),
        }
    }
}

impl TestingState {
    pub fn push_log(&mut self, line: String) {
        let ts = chrono::Local::now().format("%H:%M:%S");
        self.log.push_back(format!("[{ts}] {line}"));
        while self.log.len() > 500 {
            self.log.pop_front();
        }
    }
}

/// Settings are edited as text (matching the original entry-box UX) and
/// parsed/validated on save.
#[derive(Default)]
pub struct SettingsState {
    pub rpm_warning: String,
    pub coolant_warning: String,
    pub stock_width: String,
    pub stock_aspect: String,
    pub stock_rim: String,
    pub new_width: String,
    pub new_aspect: String,
    pub new_rim: String,
    pub stock_final: String,
    pub new_final: String,
    pub ecu_address: String,
    pub speed_unit: crate::config::SpeedUnit,
    pub temp_unit: crate::config::TempUnit,
    pub status: String,
}

pub struct ConsultApp {
    pub config: Config,
    pub page: Page,
    pub link: Option<LinkHandle>,
    pub evt_tx: Sender<LinkEvent>,
    pub evt_rx: Receiver<LinkEvent>,
    pub conn: ConnState,
    pub connecting_since: Option<Instant>,
    pub status: String,
    pub ports: Vec<String>,
    pub last_port_scan: Instant,
    pub selected_port: String,
    pub streaming: bool,
    pub frame: Option<Vec<u8>>,
    pub frame_seq: u64,
    pub last_logged_seq: u64,
    pub logger: DataLogger,
    pub dtc: DtcState,
    pub testing: TestingState,
    pub settings: SettingsState,
    pub replay: ReplayState,
}

pub struct ReplayState {
    /// CSV file names found in `logs/`.
    pub files: Vec<String>,
    pub selected_file: Option<String>,
    pub log: Option<LogFile>,
    pub playing: bool,
    pub speed: f32,
    /// Playback position in seconds.
    pub t: f64,
    pub last_tick: Instant,
    pub channels: [bool; 9],
    pub normalize: bool,
    pub status: String,
}

impl Default for ReplayState {
    fn default() -> Self {
        ReplayState {
            files: Vec::new(),
            selected_file: None,
            log: None,
            playing: false,
            speed: 1.0,
            t: 0.0,
            last_tick: Instant::now(),
            channels: [true, true, false, false, false, false, false, false, false],
            normalize: false,
            status: String::new(),
        }
    }
}

impl ReplayState {
    /// Advance playback by wall-clock delta; call once per UI frame.
    pub fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        if !self.playing {
            return;
        }
        if let Some(log) = &self.log {
            self.t += dt * self.speed as f64;
            if self.t >= log.duration() {
                self.t = log.duration();
                self.playing = false;
            }
        }
    }

    pub fn toggle_play(&mut self) {
        if self.log.is_none() {
            return;
        }
        // Restart from the beginning if we are parked at the end.
        if !self.playing && self.log.as_ref().is_some_and(|l| self.t >= l.duration()) {
            self.t = 0.0;
        }
        self.playing = !self.playing;
        self.last_tick = Instant::now();
    }
}

impl ConsultApp {
    pub fn new() -> Self {
        let (evt_tx, evt_rx) = std::sync::mpsc::channel();
        let config = Config::load();
        let mut app = ConsultApp {
            config,
            page: Page::Connect,
            link: None,
            evt_tx,
            evt_rx,
            conn: ConnState::Disconnected,
            connecting_since: None,
            status: "Select a port and connect.".into(),
            ports: Vec::new(),
            last_port_scan: Instant::now() - Duration::from_secs(10),
            selected_port: TEST_MODE_LABEL.into(),
            streaming: false,
            frame: None,
            frame_seq: 0,
            last_logged_seq: 0,
            logger: DataLogger::new(),
            dtc: DtcState::default(),
            testing: TestingState::default(),
            settings: SettingsState::default(),
            replay: ReplayState::default(),
        };
        app.refresh_ports();
        app.load_settings_buffer();
        app
    }

    pub fn connected(&self) -> bool {
        self.conn == ConnState::Connected
    }

    pub fn test_mode(&self) -> bool {
        self.link.as_ref().is_some_and(|l| l.test_mode)
    }

    pub fn send(&self, cmd: LinkCommand) {
        if let Some(link) = &self.link {
            link.send(cmd);
        }
    }

    pub fn refresh_ports(&mut self) {
        self.ports = serialport::available_ports()
            .map(|list| list.into_iter().map(|p| p.port_name).collect())
            .unwrap_or_default();
        self.last_port_scan = Instant::now();
        if self.selected_port != TEST_MODE_LABEL
            && !self.ports.contains(&self.selected_port)
            && let Some(first) = self.ports.first()
        {
            self.selected_port = first.clone();
        }
    }

    pub fn connect(&mut self) {
        if self.selected_port == TEST_MODE_LABEL {
            match LinkHandle::start_simulator(self.evt_tx.clone()) {
                Ok(link) => {
                    self.link = Some(link);
                    self.conn = ConnState::Connecting;
                    self.connecting_since = Some(Instant::now());
                    self.status = "Starting simulator...".into();
                }
                Err(e) => self.status = format!("Cannot start simulator: {e}"),
            }
            return;
        }
        let addr = match self.config.ecu_address_byte() {
            Ok(a) => a,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        match LinkHandle::connect(self.selected_port.clone(), addr, self.evt_tx.clone()) {
            Ok(link) => {
                self.link = Some(link);
                self.conn = ConnState::Connecting;
                self.connecting_since = Some(Instant::now());
                self.status = format!("Connecting to {}...", self.selected_port);
            }
            Err(e) => self.status = format!("Cannot start link: {e}"),
        }
    }

    pub fn disconnect(&mut self) {
        self.link = None; // Drop shuts the worker down.
        self.conn = ConnState::Disconnected;
        self.connecting_since = None;
        self.streaming = false;
        self.frame = None;
        self.logger.stop();
        self.status = "Disconnected.".into();
    }

    pub fn set_page(&mut self, page: Page) {
        if page == self.page {
            return;
        }
        // Stop the stream when navigating away from the data page.
        if self.page == Page::DataStream && self.streaming {
            self.send(LinkCommand::StopStream);
            self.streaming = false;
        }
        if page == Page::DataStream && self.connected() && !self.streaming {
            self.send(LinkCommand::StartStream);
            self.streaming = true;
        }
        if page == Page::Settings {
            self.load_settings_buffer();
        }
        if page == Page::Replay {
            self.refresh_log_files();
            self.replay.last_tick = Instant::now();
        }
        if page == Page::Testing {
            self.testing.temp_display = self.config.display_temp(80.0) as f32;
        }
        self.page = page;
    }

    pub fn refresh_log_files(&mut self) {
        let mut files: Vec<String> = std::fs::read_dir("logs")
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.to_lowercase().ends_with(".csv"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files.reverse(); // newest (timestamped names) first
        if self
            .replay
            .selected_file
            .as_ref()
            .is_none_or(|sel| !files.contains(sel))
        {
            self.replay.selected_file = files.first().cloned();
        }
        self.replay.files = files;
    }

    fn poll_events(&mut self) {
        while let Ok(evt) = self.evt_rx.try_recv() {
            match evt {
                LinkEvent::Connected => {
                    self.conn = ConnState::Connected;
                    self.connecting_since = None;
                    self.status = if self.test_mode() {
                        "Test mode: simulated data.".into()
                    } else {
                        format!("Connected to ECU on {}.", self.selected_port)
                    };
                    if self.page == Page::DataStream && !self.streaming {
                        self.send(LinkCommand::StartStream);
                        self.streaming = true;
                    }
                }
                LinkEvent::StreamFrame(f) => {
                    self.frame = Some(f);
                    self.frame_seq += 1;
                }
                LinkEvent::DtcReport(entries) => {
                    self.dtc.busy = false;
                    if entries.is_empty() {
                        self.dtc.status = "No response from ECU.".into();
                    } else {
                        self.dtc.status = format!("{} code(s) reported.", entries.len());
                    }
                    self.dtc.entries = entries;
                }
                LinkEvent::DtcCleared => {
                    self.dtc.busy = false;
                    self.dtc.status = "DTCs cleared.".into();
                }
                LinkEvent::PartNumber(p) => {
                    self.dtc.busy = false;
                    self.dtc.status = "ECU info received.".into();
                    self.dtc.part_number = Some(p);
                }
                LinkEvent::RawLog(line) => self.testing.push_log(line),
                LinkEvent::Error(e) => {
                    self.status = e.clone();
                    if self.conn == ConnState::Connecting {
                        self.link = None;
                        self.conn = ConnState::Disconnected;
                        self.connecting_since = None;
                    } else if self.conn == ConnState::Connected {
                        // Link died mid-session (cable unplugged, etc.).
                        self.disconnect();
                        self.status = e;
                    }
                    self.dtc.busy = false;
                }
            }
        }

        // Connection attempt timeout.
        if self.conn == ConnState::Connecting
            && self
                .connecting_since
                .is_some_and(|t| t.elapsed() > Duration::from_secs(8))
        {
            self.link = None;
            self.conn = ConnState::Disconnected;
            self.connecting_since = None;
            self.status = "Connection timed out.".into();
        }

        // Periodic port re-scan while disconnected.
        if self.conn == ConnState::Disconnected
            && self.last_port_scan.elapsed() > Duration::from_secs(2)
        {
            self.refresh_ports();
        }
    }

}

impl eframe::App for ConsultApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_events();

        egui::Panel::top("nav").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Consult Rust");
                ui.separator();
                if ui
                    .selectable_label(self.page == Page::Connect, "Connect")
                    .clicked()
                {
                    self.set_page(Page::Connect);
                }
                let can_browse = self.connected();
                for (page, label) in [
                    (Page::DataStream, "Data Stream"),
                    (Page::Dtc, "DTCs"),
                    (Page::Testing, "Testing"),
                ] {
                    ui.add_enabled_ui(can_browse, |ui| {
                        if ui.selectable_label(self.page == page, label).clicked() {
                            self.set_page(page);
                        }
                    });
                }
                if ui
                    .selectable_label(self.page == Page::Settings, "Settings")
                    .clicked()
                {
                    self.set_page(Page::Settings);
                }
                if ui
                    .selectable_label(self.page == Page::Replay, "Log Replay")
                    .clicked()
                {
                    self.set_page(Page::Replay);
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let (color, text) = match self.conn {
                    ConnState::Disconnected => (egui::Color32::GRAY, "Disconnected"),
                    ConnState::Connecting => (egui::Color32::YELLOW, "Connecting"),
                    ConnState::Connected => (egui::Color32::GREEN, "Connected"),
                };
                ui.colored_label(color, "\u{25CF}");
                ui.label(text);
                ui.separator();
                ui.label(&self.status);
            });
        });

        egui::CentralPanel::default_margins().show(ui, |ui| match self.page {
            Page::Connect => self.show_connect(ui),
            Page::DataStream => self.show_datastream(ui),
            Page::Dtc => self.show_dtc(ui),
            Page::Testing => self.show_testing(ui),
            Page::Replay => self.show_replay(ui),
            Page::Settings => self.show_settings(ui),
        });

        // Advance log playback with wall-clock time.
        if self.page == Page::Replay && self.replay.playing {
            self.replay.tick();
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        } else {
            self.replay.last_tick = Instant::now();
        }

        ui.ctx().request_repaint_after(Duration::from_millis(75));
    }
}
