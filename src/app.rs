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
/// Mid-session link failures trigger this many auto-reconnect attempts
/// (2 s apart) before giving up.
const MAX_RECONNECT_ATTEMPTS: u8 = 5;

/// A detected serial port, with a USB description when available.
pub struct PortInfo {
    pub name: String,
    pub description: Option<String>,
}

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
    /// SR20-style power balance: send 0x01-0x08 to cut one cylinder
    /// instead of a bitmask.
    pub pb_single_cyl: bool,
    /// Base idle/timing mode is holding the AAC valve and timing.
    pub base_idle_active: bool,
    pub fuel_trim_pct: f32,
    pub timing_deg: i32,
    pub aac_pct: f32,
    pub iacv_pct: f32,
    pub temp_display: f32,
    pub raw_input: String,
    pub log: VecDeque<String>,
}

impl Default for TestingState {
    fn default() -> Self {
        TestingState {
            cylinders: [false; 8],
            pb_single_cyl: false,
            base_idle_active: false,
            fuel_trim_pct: 0.0,
            timing_deg: 0,
            aac_pct: 0.0,
            iacv_pct: 0.0,
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
    pub battery_low: String,
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
    /// Selected display item ids (edits config.display_items on save).
    pub items: Vec<String>,
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
    pub ports: Vec<PortInfo>,
    pub last_port_scan: Instant,
    pub selected_port: String,
    /// Set once a lone detected port has been auto-selected, so we don't
    /// keep overriding a deliberate user choice.
    pub port_autoselected: bool,
    /// Auto-reconnect state: attempts so far (0 = not retrying) and when
    /// the next attempt fires.
    pub reconnect_attempts: u8,
    pub reconnect_at: Option<Instant>,
    pub streaming: bool,
    pub frame: Option<Vec<u8>>,
    pub frame_seq: u64,
    /// Arrival times of recent stream frames, for the health readout.
    pub frame_times: VecDeque<Instant>,
    /// Rolling decoded-value history for the live plot: (seconds since the
    /// first frame, one value per selected item in catalog order).
    pub history: VecDeque<(f64, Vec<f64>)>,
    pub history_start: Option<Instant>,
    /// Live-plot channel toggles (non-digital items) and normalize flag.
    pub plot_channels: Vec<bool>,
    pub plot_normalize: bool,
    pub last_logged_seq: u64,
    pub logger: DataLogger,
    pub dtc: DtcState,
    pub testing: TestingState,
    pub settings: SettingsState,
    pub replay: ReplayState,
    /// Per-item session peak (max) values, keyed by catalog id. Cleared on
    /// connect and from the Data Stream controls.
    pub peaks: std::collections::HashMap<&'static str, f64>,
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
    /// One toggle per column of the loaded log.
    pub channels: Vec<bool>,
    pub normalize: bool,
    /// Two-click confirm state for deleting the selected log file.
    pub confirm_delete: bool,
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
            channels: Vec::new(),
            normalize: false,
            confirm_delete: false,
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
        let restored_port = config.last_port.clone();
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
            selected_port: restored_port.unwrap_or_else(|| TEST_MODE_LABEL.into()),
            port_autoselected: false,
            reconnect_attempts: 0,
            reconnect_at: None,
            streaming: false,
            frame: None,
            frame_seq: 0,
            frame_times: VecDeque::new(),
            history: VecDeque::new(),
            history_start: None,
            plot_channels: Vec::new(),
            plot_normalize: false,
            last_logged_seq: 0,
            logger: DataLogger::new(),
            dtc: DtcState::default(),
            testing: TestingState::default(),
            settings: SettingsState::default(),
            replay: ReplayState::default(),
            peaks: std::collections::HashMap::new(),
        };
        app.refresh_ports();
        app.load_settings_buffer();

        // Demo hook for layout verification: CONSULT_DEMO=1 auto-connects
        // the simulator on the Data Stream page with digital registers shown.
        if std::env::var_os("CONSULT_DEMO").is_some() {
            for d in ["d13", "d1e", "d1f", "d21"] {
                app.config.display_items.push(d.to_string());
            }
            app.page = Page::DataStream;
            app.connect();
        }
        app
    }

    pub fn connected(&self) -> bool {
        self.conn == ConnState::Connected
    }

    pub fn test_mode(&self) -> bool {
        self.link.as_ref().is_some_and(|l| l.test_mode)
    }

    /// Stream frames per second over the trailing 2 s window.
    pub fn stream_fps(&self) -> f64 {
        self.frame_times.len() as f64 / 2.0
    }

    pub fn send(&self, cmd: LinkCommand) {
        if let Some(link) = &self.link {
            link.send(cmd);
        }
    }

    pub fn refresh_ports(&mut self) {
        self.ports = serialport::available_ports()
            .map(|list| {
                list.into_iter()
                    .map(|p| {
                        let description = match p.port_type {
                            serialport::SerialPortType::UsbPort(u) => {
                                u.product.or(u.manufacturer)
                            }
                            serialport::SerialPortType::BluetoothPort => {
                                Some("Bluetooth".into())
                            }
                            _ => None,
                        };
                        PortInfo {
                            name: p.port_name,
                            description,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.last_port_scan = Instant::now();
        if self.selected_port != TEST_MODE_LABEL
            && !self.ports.iter().any(|p| p.name == self.selected_port)
            && let Some(first) = self.ports.first()
        {
            self.selected_port = first.name.clone();
        }
        // Auto-select the adapter when exactly one port is present.
        if !self.port_autoselected
            && self.selected_port == TEST_MODE_LABEL
            && self.ports.len() == 1
        {
            self.selected_port = self.ports[0].name.clone();
            self.port_autoselected = true;
        }
    }

    pub fn connect(&mut self) {
        self.reconnect_at = None;
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
        let candidates = match self.config.ecu_address_candidates() {
            Ok(c) => c,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        match LinkHandle::connect(self.selected_port.clone(), candidates, self.evt_tx.clone()) {
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
        self.reconnect_attempts = 0;
        self.reconnect_at = None;
        self.status = "Disconnected.".into();
    }

    /// Drop the dead link and schedule another connect attempt.
    fn schedule_reconnect(&mut self, reason: &str) {
        self.link = None;
        self.conn = ConnState::Disconnected;
        self.connecting_since = None;
        self.streaming = false;
        self.frame = None;
        self.logger.stop();
        self.reconnect_attempts += 1;
        self.reconnect_at = Some(Instant::now() + Duration::from_secs(2));
        self.status = format!(
            "{reason} Reconnecting in 2 s (attempt {}/{MAX_RECONNECT_ATTEMPTS}).",
            self.reconnect_attempts
        );
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
        // The Testing page runs its own small stream for the adjustment
        // readouts; stop it when leaving.
        if self.page == Page::Testing && self.connected() {
            // The STOP also cancels any latched active tests.
            self.send(LinkCommand::StopStream);
            self.frame = None;
            self.testing.base_idle_active = false;
        }
        if page == Page::DataStream && self.connected() && !self.streaming {
            let regs = self.config.stream_registers();
            self.send(LinkCommand::StartStream(regs));
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
            if self.connected() {
                self.send(LinkCommand::StartStream(
                    crate::pages::testing::HELPER_REGS.to_vec(),
                ));
                self.frame = None;
            }
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
                    self.reconnect_attempts = 0;
                    self.reconnect_at = None;
                    self.peaks.clear();
                    self.history.clear();
                    self.history_start = None;
                    // Remember the port that worked for the next launch.
                    if !self.test_mode()
                        && self.config.last_port.as_deref() != Some(self.selected_port.as_str())
                    {
                        self.config.last_port = Some(self.selected_port.clone());
                        let _ = self.config.save();
                    }
                    self.status = if self.test_mode() {
                        "Test mode: simulated data.".into()
                    } else {
                        format!("Connected to ECU on {}.", self.selected_port)
                    };
                    if self.page == Page::DataStream && !self.streaming {
                        let regs = self.config.stream_registers();
                        self.send(LinkCommand::StartStream(regs));
                        self.streaming = true;
                    }
                }
                LinkEvent::StreamFrame(f) => {
                    self.frame = Some(f);
                    self.frame_seq += 1;
                    let now = Instant::now();
                    self.frame_times.push_back(now);
                    while self
                        .frame_times
                        .front()
                        .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(2))
                    {
                        self.frame_times.pop_front();
                    }
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
                    self.dtc.busy = false;
                    let mid_session = self.conn == ConnState::Connected;
                    let retrying =
                        self.conn == ConnState::Connecting && self.reconnect_attempts > 0;
                    if (mid_session || retrying) && !self.test_mode() {
                        if self.reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                            self.link = None;
                            self.conn = ConnState::Disconnected;
                            self.connecting_since = None;
                            self.streaming = false;
                            self.reconnect_attempts = 0;
                            self.reconnect_at = None;
                            self.status = format!("Gave up reconnecting: {e}");
                        } else {
                            self.schedule_reconnect(&e);
                        }
                    } else {
                        self.status = e.clone();
                        if self.conn == ConnState::Connecting {
                            self.link = None;
                            self.conn = ConnState::Disconnected;
                            self.connecting_since = None;
                        } else if self.conn == ConnState::Connected {
                            // Test-mode simulator failure.
                            self.disconnect();
                            self.status = e;
                        }
                    }
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

        // Fire a scheduled auto-reconnect attempt.
        if self.conn == ConnState::Disconnected
            && let Some(at) = self.reconnect_at
            && Instant::now() >= at
        {
            self.status = format!(
                "Reconnecting (attempt {}/{MAX_RECONNECT_ATTEMPTS})...",
                self.reconnect_attempts
            );
            self.connect();
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

        // No right margin so page scroll bars sit flush against the window
        // edge; pages pad their own content as needed.
        let central_frame = egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin {
            left: 8,
            right: 0,
            top: 8,
            bottom: 8,
        });
        egui::CentralPanel::default_margins()
            .frame(central_frame)
            .show(ui, |ui| match self.page {
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
