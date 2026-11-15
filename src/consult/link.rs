//! Background serial link to the ECU, plus a simulator with the same
//! interface for test mode. The GUI talks to the link through channels;
//! all blocking serial I/O happens on the worker thread.

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::consult::protocol::*;

pub enum LinkCommand {
    StartStream,
    StopStream,
    ReadDtc,
    ClearDtc,
    EcuInfo,
    Activate { kind: u8, data: u8 },
    Raw(Vec<u8>),
    Shutdown,
}

pub enum LinkEvent {
    Connected,
    StreamFrame(Vec<u8>),
    DtcReport(Vec<(u8, u8)>),
    PartNumber(String),
    DtcCleared,
    RawLog(String),
    Error(String),
}

pub struct LinkHandle {
    cmd_tx: Sender<LinkCommand>,
    join: Option<JoinHandle<()>>,
    pub test_mode: bool,
}

impl LinkHandle {
    pub fn send(&self, cmd: LinkCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn connect(
        port_name: String,
        ecu_address: u8,
        evt_tx: Sender<LinkEvent>,
    ) -> std::io::Result<LinkHandle> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let join = std::thread::Builder::new()
            .name("consult-link".into())
            .spawn(move || serial_worker(port_name, ecu_address, evt_tx, cmd_rx))?;
        Ok(LinkHandle {
            cmd_tx,
            join: Some(join),
            test_mode: false,
        })
    }

    pub fn start_simulator(evt_tx: Sender<LinkEvent>) -> std::io::Result<LinkHandle> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let join = std::thread::Builder::new()
            .name("consult-sim".into())
            .spawn(move || sim_worker(evt_tx, cmd_rx))?;
        Ok(LinkHandle {
            cmd_tx,
            join: Some(join),
            test_mode: true,
        })
    }
}

impl Drop for LinkHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(LinkCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

// ---------------------------------------------------------------- serial worker

fn serial_worker(
    port_name: String,
    ecu_address: u8,
    evt: Sender<LinkEvent>,
    cmd_rx: Receiver<LinkCommand>,
) {
    let mut port = match serialport::new(&port_name, BAUD_RATE)
        .timeout(Duration::from_millis(60))
        .open()
    {
        Ok(p) => p,
        Err(e) => {
            let _ = evt.send(LinkEvent::Error(format!("Cannot open {port_name}: {e}")));
            return;
        }
    };

    if let Err(e) = handshake(&mut *port, ecu_address) {
        let _ = evt.send(LinkEvent::Error(e));
        return;
    }
    if evt.send(LinkEvent::Connected).is_err() {
        return;
    }

    let mut parser = FrameParser::new();
    let mut streaming = false;
    let mut buf = [0u8; 512];

    loop {
        // Drain pending commands first.
        loop {
            match cmd_rx.try_recv() {
                Ok(LinkCommand::Shutdown) => {
                    if streaming {
                        stop_and_drain(&mut *port, &mut parser);
                    }
                    return;
                }
                Ok(LinkCommand::StartStream) => {
                    let _ = port.write_all(&stream_command());
                    let _ = port.flush();
                    streaming = true;
                }
                Ok(LinkCommand::StopStream) => {
                    stop_and_drain(&mut *port, &mut parser);
                    streaming = false;
                }
                Ok(cmd) => {
                    if streaming {
                        stop_and_drain(&mut *port, &mut parser);
                    }
                    run_query(cmd, &mut *port, &mut parser, &evt);
                    if streaming {
                        let _ = port.write_all(&stream_command());
                        let _ = port.flush();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        match port.read(&mut buf) {
            Ok(n) => {
                for &b in &buf[..n] {
                    match parser.feed(b) {
                        Some(ParserOutput::Frame(frame)) => {
                            let is_stream = frame.len() == STREAM_REGISTERS.len();
                            let out = if streaming && is_stream {
                                LinkEvent::StreamFrame(frame)
                            } else {
                                LinkEvent::RawLog(format!("RX: {}", to_hex(&frame)))
                            };
                            if evt.send(out).is_err() {
                                return;
                            }
                        }
                        Some(ParserOutput::StopAck) => {}
                        None => {}
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                let _ = evt.send(LinkEvent::Error(format!("Serial error: {e}")));
                return;
            }
        }
    }
}

fn handshake(port: &mut dyn SerialPort, ecu_address: u8) -> Result<(), String> {
    let expected = expected_init_reply(ecu_address);
    for _ in 0..3 {
        let _ = port.clear(serialport::ClearBuffer::All);
        port.write_all(&init_sequence(ecu_address))
            .map_err(|e| format!("Init write failed: {e}"))?;
        let _ = port.flush();
        std::thread::sleep(Duration::from_millis(100));

        let deadline = Instant::now() + Duration::from_millis(900);
        let mut b = [0u8; 1];
        while Instant::now() < deadline {
            match port.read(&mut b) {
                Ok(1) if b[0] == expected => return Ok(()),
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(format!("Init read failed: {e}")),
            }
        }
    }
    Err(format!(
        "No ECU response (expected reply 0x{expected:02X}). Check cable, ignition, and ECU address."
    ))
}

/// Send STOP and read until the ECU acknowledges (0xCF) or we time out.
fn stop_and_drain(port: &mut dyn SerialPort, parser: &mut FrameParser) {
    let _ = port.write_all(&[STOP]);
    let _ = port.flush();
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut buf = [0u8; 256];
    while Instant::now() < deadline {
        match port.read(&mut buf) {
            Ok(n) => {
                for &b in &buf[..n] {
                    if matches!(parser.feed(b), Some(ParserOutput::StopAck)) {
                        return;
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
}

/// Run a one-shot query: send bytes, collect response frames until the ECU
/// goes quiet, then stop the response stream. Returns collected frames.
fn run_query(
    cmd: LinkCommand,
    port: &mut dyn SerialPort,
    parser: &mut FrameParser,
    evt: &Sender<LinkEvent>,
) {
    let (bytes, max_wait, silence) = match &cmd {
        LinkCommand::ReadDtc => (vec![CMD_READ_DTC, TERMINATOR], 3000, 600),
        LinkCommand::ClearDtc => (vec![CMD_CLEAR_DTC, TERMINATOR], 1500, 400),
        LinkCommand::EcuInfo => (vec![CMD_ECU_INFO, TERMINATOR], 1500, 400),
        LinkCommand::Activate { kind, data } => {
            (activation_command(*kind, *data).to_vec(), 600, 200)
        }
        LinkCommand::Raw(b) => (b.clone(), 900, 300),
        _ => return,
    };

    if let LinkCommand::Raw(b) = &cmd {
        let _ = evt.send(LinkEvent::RawLog(format!("TX: {}", to_hex(b))));
    }

    let _ = port.clear(serialport::ClearBuffer::Input);
    if port.write_all(&bytes).is_err() {
        let _ = evt.send(LinkEvent::Error("Write failed".into()));
        return;
    }
    let _ = port.flush();

    let start = Instant::now();
    let mut last_frame: Option<Instant> = None;
    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut buf = [0u8; 256];

    while start.elapsed() < Duration::from_millis(max_wait) {
        match port.read(&mut buf) {
            Ok(n) => {
                for &b in &buf[..n] {
                    if let Some(ParserOutput::Frame(f)) = parser.feed(b) {
                        last_frame = Some(Instant::now());
                        frames.push(f);
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
        // The ECU repeats the response frame until stopped; three identical
        // frames means we have the complete answer.
        let n = frames.len();
        if n >= 3 && frames[n - 1] == frames[n - 2] && frames[n - 2] == frames[n - 3] {
            break;
        }
        if !frames.is_empty()
            && last_frame.is_some_and(|t| t.elapsed() > Duration::from_millis(silence))
        {
            break;
        }
    }

    stop_and_drain(port, parser);

    match cmd {
        LinkCommand::ReadDtc => {
            let _ = evt.send(LinkEvent::DtcReport(parse_dtc_frames(&frames)));
        }
        LinkCommand::ClearDtc => {
            let _ = evt.send(LinkEvent::DtcCleared);
        }
        LinkCommand::EcuInfo => {
            let text = frames
                .first()
                .map(|f| {
                    f.iter()
                        .filter(|b| b.is_ascii_graphic() || **b == b' ')
                        .map(|b| *b as char)
                        .collect::<String>()
                })
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "(no response)".to_string());
            let _ = evt.send(LinkEvent::PartNumber(text));
        }
        LinkCommand::Activate { kind, data } => {
            let _ = evt.send(LinkEvent::RawLog(format!(
                "TX: {}",
                to_hex(&activation_command(kind, data))
            )));
        }
        LinkCommand::Raw(_) => {
            for f in &frames {
                let _ = evt.send(LinkEvent::RawLog(format!("RX: {}", to_hex(f))));
            }
            if frames.is_empty() {
                let _ = evt.send(LinkEvent::RawLog("RX: (no response)".into()));
            }
        }
        _ => {}
    }
}

use serialport::SerialPort;

// ---------------------------------------------------------------- simulator

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() as f64) / (u64::MAX as f64)
    }
}

fn sim_worker(evt: Sender<LinkEvent>, cmd_rx: Receiver<LinkCommand>) {
    if evt.send(LinkEvent::Connected).is_err() {
        return;
    }
    let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
    let mut streaming = false;
    let mut next_frame = Instant::now();

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                LinkCommand::Shutdown => return,
                LinkCommand::StartStream => streaming = true,
                LinkCommand::StopStream => streaming = false,
                LinkCommand::ReadDtc => {
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = evt.send(LinkEvent::DtcReport(vec![(0x55, 0x00)]));
                }
                LinkCommand::ClearDtc => {
                    std::thread::sleep(Duration::from_millis(200));
                    let _ = evt.send(LinkEvent::DtcCleared);
                }
                LinkCommand::EcuInfo => {
                    let _ = evt.send(LinkEvent::PartNumber("SIMU 23710-LATED".into()));
                }
                LinkCommand::Activate { kind, data } => {
                    let _ = evt.send(LinkEvent::RawLog(format!(
                        "Sim TX: {}",
                        to_hex(&activation_command(kind, data))
                    )));
                }
                LinkCommand::Raw(b) => {
                    let _ = evt.send(LinkEvent::RawLog(format!("Sim TX: {}", to_hex(&b))));
                }
            }
        }

        if streaming && Instant::now() >= next_frame {
            next_frame = Instant::now() + Duration::from_millis(100);
            if evt.send(LinkEvent::StreamFrame(sim_frame(&mut rng))).is_err() {
                return;
            }
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Build a raw 13-byte frame whose decoded readings look plausible.
fn sim_frame(rng: &mut XorShift) -> Vec<u8> {
    let mut f = vec![0u8; STREAM_REGISTERS.len()];
    let put16 = |f: &mut Vec<u8>, i: usize, v: u16| {
        f[i] = (v >> 8) as u8;
        f[i + 1] = v as u8;
    };
    put16(&mut f, 0, (rng.range(700.0, 5200.0) / 12.5) as u16); // RPM
    f[2] = (rng.range(0.0, 180.0) / 2.0) as u8; // speed kph
    f[3] = (rng.range(80.0, 100.0) + 50.0) as u8; // coolant C
    f[4] = (rng.range(13.5, 14.6) * 1000.0 / 80.0) as u8; // battery V
    f[5] = (rng.range(0.4, 4.4) * 1000.0 / 20.0) as u8; // TPS V
    put16(&mut f, 6, (rng.range(0.9, 3.8) * 1000.0 / 5.0) as u16); // MAF V
    f[8] = (rng.range(4.0, 40.0) * 2.0) as u8; // AAC %
    put16(&mut f, 9, (rng.range(1.5, 12.0) * 100.0) as u16); // injector ms
    f[11] = (110.0 - rng.range(10.0, 38.0)) as u8; // timing deg
    f[12] = (rng.range(0.05, 0.95) * 100.0) as u8; // O2 V
    f
}
