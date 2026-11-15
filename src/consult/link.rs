//! Background serial link to the ECU, plus a simulator with the same
//! interface for test mode. The GUI talks to the link through channels;
//! all blocking serial I/O happens on the worker thread.

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::consult::protocol::*;

pub enum LinkCommand {
    StartStream(Vec<u8>),
    StopStream,
    ReadDtc,
    ClearDtc,
    EcuInfo,
    Activate { kind: u8, data: u8 },
    /// Several activations in one write (protocol allows chaining).
    ActivateSet(Vec<(u8, u8)>),
    /// Send STOP, which also cancels all active test commands.
    CancelActivations,
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
        ecu_candidates: Vec<u8>,
        evt_tx: Sender<LinkEvent>,
    ) -> std::io::Result<LinkHandle> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let join = std::thread::Builder::new()
            .name("consult-link".into())
            .spawn(move || serial_worker(port_name, ecu_candidates, evt_tx, cmd_rx))?;
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
    ecu_candidates: Vec<u8>,
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

    if let Err(e) = handshake(&mut *port, &ecu_candidates) {
        let _ = evt.send(LinkEvent::Error(e));
        return;
    }
    if evt.send(LinkEvent::Connected).is_err() {
        return;
    }

    let mut parser = FrameParser::new();
    let mut streaming = false;
    let mut stream_regs: Vec<u8> = Vec::new();
    // Active test commands latch on the ECU until a STOP (0x30) cancels
    // them, so we track whether a cancel is owed before closing.
    let mut activation_active = false;
    let mut buf = [0u8; 512];

    loop {
        // Drain pending commands first.
        loop {
            match cmd_rx.try_recv() {
                Ok(LinkCommand::Shutdown) => {
                    if streaming || activation_active {
                        stop_and_drain(&mut *port, &mut parser);
                    }
                    return;
                }
                Ok(LinkCommand::StartStream(regs)) => {
                    stream_regs = regs;
                    let _ = port.write_all(&build_stream_command(&stream_regs));
                    let _ = port.flush();
                    streaming = true;
                }
                Ok(LinkCommand::StopStream) => {
                    stop_and_drain(&mut *port, &mut parser);
                    streaming = false;
                }
                // Activations are fire-and-forget: no STOP afterwards, since
                // STOP cancels the test (per the Issue 7 protocol doc). They
                // can be sent mid-stream; echo bytes are skipped by the
                // frame parser.
                Ok(LinkCommand::Activate { kind, data }) => {
                    let bytes = activation_command(kind, data);
                    let _ = port.write_all(&bytes);
                    let _ = port.flush();
                    activation_active = true;
                    let _ = evt.send(LinkEvent::RawLog(format!("TX: {}", to_hex(&bytes))));
                }
                Ok(LinkCommand::ActivateSet(pairs)) => {
                    let mut bytes = Vec::with_capacity(pairs.len() * 3 + 1);
                    for (kind, data) in &pairs {
                        bytes.extend_from_slice(&[CMD_ACTIVATE, *kind, *data]);
                    }
                    bytes.push(TERMINATOR);
                    let _ = port.write_all(&bytes);
                    let _ = port.flush();
                    activation_active = true;
                    let _ = evt.send(LinkEvent::RawLog(format!("TX: {}", to_hex(&bytes))));
                }
                Ok(LinkCommand::CancelActivations) => {
                    if activation_active {
                        stop_and_drain(&mut *port, &mut parser);
                        activation_active = false;
                        // STOP also stopped the stream; restart it.
                        if streaming {
                            let _ = port.write_all(&build_stream_command(&stream_regs));
                            let _ = port.flush();
                        }
                        let _ = evt.send(LinkEvent::RawLog(
                            "TX: 30 (active tests cancelled)".into(),
                        ));
                    }
                }
                Ok(cmd) => {
                    if streaming {
                        stop_and_drain(&mut *port, &mut parser);
                    }
                    run_query(cmd, &mut *port, &mut parser, &evt);
                    if streaming {
                        let _ = port.write_all(&build_stream_command(&stream_regs));
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
                            let is_stream = frame.len() == stream_regs.len();
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

fn handshake(port: &mut dyn SerialPort, candidates: &[u8]) -> Result<(), String> {
    for &ecu_address in candidates {
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
    }
    let tried = candidates
        .iter()
        .map(|a| format!("0x{a:02X}"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "No ECU response (tried {tried}). Check cable, ignition, and ECU address."
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
                .map(|f| format_part_number(f))
                .unwrap_or_else(|| "(no response)".to_string());
            let _ = evt.send(LinkEvent::PartNumber(text));
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
    let mut stream_regs: Vec<u8> = Vec::new();
    let mut next_frame = Instant::now();
    let start = Instant::now();

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                LinkCommand::Shutdown => return,
                LinkCommand::StartStream(regs) => {
                    stream_regs = regs;
                    streaming = true;
                }
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
                LinkCommand::ActivateSet(pairs) => {
                    let mut bytes = Vec::with_capacity(pairs.len() * 3 + 1);
                    for (kind, data) in &pairs {
                        bytes.extend_from_slice(&[CMD_ACTIVATE, *kind, *data]);
                    }
                    bytes.push(TERMINATOR);
                    let _ = evt.send(LinkEvent::RawLog(format!("Sim TX: {}", to_hex(&bytes))));
                }
                LinkCommand::CancelActivations => {
                    let _ = evt.send(LinkEvent::RawLog(
                        "Sim TX: 30 (active tests cancelled)".into(),
                    ));
                }
                LinkCommand::Raw(b) => {
                    let _ = evt.send(LinkEvent::RawLog(format!("Sim TX: {}", to_hex(&b))));
                }
            }
        }

        if streaming && Instant::now() >= next_frame {
            next_frame = Instant::now() + Duration::from_millis(100);
            if evt
                .send(LinkEvent::StreamFrame(sim_frame(
                    &mut rng,
                    &stream_regs,
                    start.elapsed().as_secs_f64(),
                )))
                .is_err()
            {
                return;
            }
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Digital register patterns, ported from the consult_box demo: bits toggle
/// slowly so the indicators are readable.
fn sim_digital(reg: u8, t: f64) -> u8 {
    let s = |freq: f64, phase: f64, thresh: f64| (t * freq + phase).sin() > thresh;
    match reg {
        0x13 => {
            (u8::from(s(0.55, 0.0, 0.55)) << 4)
                | (u8::from(s(0.8, 1.6, 0.35)) << 3)
                | (1 << 2) // park/neutral, mostly on
                | (u8::from(s(1.4, 0.0, 0.75)) << 1)
                | u8::from((t * 0.9).sin() < -0.35)
        }
        0x1E => {
            (u8::from(s(0.3, 0.4, 0.1)) << 7)
                | (u8::from(s(0.85, 0.2, 0.3)) << 5)
                | (u8::from(s(0.4, 0.0, 0.15)) << 1)
                | u8::from(s(0.35, 0.8, 0.35))
        }
        0x1F => {
            (u8::from(s(0.75, 1.0, 0.45)) << 6)
                | (u8::from(s(1.05, 2.1, 0.6)) << 5)
                | (u8::from(s(1.05, 2.1, 0.6)) << 3)
                | u8::from(s(0.55, 0.0, 0.15))
        }
        0x21 => (u8::from(s(0.6, 0.0, 0.25)) << 7) | (u8::from(s(0.7, 2.5, -0.4)) << 6),
        _ => 0,
    }
}

/// Build a raw frame matching the requested register list, with plausible
/// decoded values.
fn sim_frame(rng: &mut XorShift, regs: &[u8], t: f64) -> Vec<u8> {
    // Physical values for this tick.
    let rpm_raw = (rng.range(700.0, 5200.0) / 12.5) as u16;
    let speed = rng.range(0.0, 180.0);
    let coolant_c = rng.range(80.0, 100.0);
    let batt = rng.range(13.5, 14.6);
    let tps = rng.range(0.4, 4.4);
    let maf_raw = (rng.range(0.9, 3.8) * 1000.0 / 5.0) as u16;
    let o2 = rng.range(0.05, 0.95);
    let inj_raw = (rng.range(1.5, 12.0) * 100.0) as u16;
    let timing = rng.range(10.0, 38.0);
    let aac = rng.range(4.0, 40.0);
    let temp_c = rng.range(10.0, 45.0);
    let egt = rng.range(0.5, 4.0);
    let alpha = rng.range(85.0, 115.0);
    let load = rng.range(15.0, 80.0);

    regs.iter()
        .map(|&reg| match reg {
            0x00 => (rpm_raw >> 8) as u8,
            0x01 => rpm_raw as u8,
            // CAS ref raw = RPM / 8 (generic scale).
            0x02 => (((rpm_raw as f64 * 12.5 / 8.0) as u16) >> 8) as u8,
            0x03 => (rpm_raw as f64 * 12.5 / 8.0) as u8,
            0x04 => (maf_raw >> 8) as u8,
            0x05 => maf_raw as u8,
            0x06 => (maf_raw >> 8) as u8,
            0x07 => maf_raw as u8,
            0x08 => (coolant_c + 50.0) as u8,
            0x09 | 0x0A => (o2 * 100.0) as u8,
            0x0B => (speed / 2.0) as u8,
            0x0C => (batt * 1000.0 / 80.0) as u8,
            0x0D => (tps * 1000.0 / 20.0) as u8,
            0x0F | 0x11 => (temp_c + 50.0) as u8,
            0x12 => (egt * 1000.0 / 20.0) as u8,
            0x13 | 0x1E | 0x1F | 0x21 => sim_digital(reg, t),
            0x14 | 0x22 => (inj_raw >> 8) as u8,
            0x15 | 0x23 => inj_raw as u8,
            0x16 => (110.0 - timing) as u8,
            0x17 => (aac * 2.0) as u8,
            0x1A..=0x1D => alpha as u8,
            0x28 => (rng.range(0.0, 60.0) * 2.0) as u8,        // wastegate %
            0x29 => (rng.range(0.8, 4.5) * 1000.0 / 20.0) as u8, // boost V
            0x33 => load as u8,
            0x34 => ((load - load.floor()) * 255.0) as u8,
            0x35 => (o2 * 100.0) as u8,
            0x37 => (tps / 5.0 * 255.0) as u8, // abs TPS %
            0x38 => (maf_raw as f64 * 5.0 / 1000.0 / 5.0 * 100.0) as u8, // MAF %
            0x4A => (rng.range(2.0, 4.8) * 1000.0 / 20.0) as u8, // abs pressure V
            _ => (rng.next() & 0xFF) as u8,
        })
        .collect()
}
