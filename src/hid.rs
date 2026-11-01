//! HID transport: device discovery and a worker thread that owns the keyboard.
//!
//! Every call into the device is blocking (a 225-frame upload takes minutes), so
//! the UI talks to this worker over channels and never blocks its own thread.

use std::ffi::CString;
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, Timelike};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use hidapi::HidApi;

use crate::live::{LiveState, Provider};

pub const VENDOR_ID: u16 = 0x05ac;
pub const PRODUCT_ID: u16 = 0x024f;
const IMAGE_USAGE_PAGE: u16 = 0xFF68;
const CONTROL_USAGE_PAGE: u16 = 0xFF13;
const CMD_DELAY: Duration = Duration::from_millis(35);
const PAGE_ACK_MAGIC: [u8; 3] = [0x01, 0x5a, 0x02];

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub image: String,
    pub control: String,
    pub product: String,
}

#[derive(Debug, Clone)]
pub enum Command {
    Probe,
    Upload(Vec<u8>),
    Light { mode: u8, colour: (u8, u8, u8), brightness: u8, speed: u8, colourful: bool },
    ClockSync,
    StartLive { provider: Provider, interval_secs: u32 },
    StopLive,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum Event {
    Device(Option<DeviceInfo>),
    Stage { label: String, done: usize, total: usize },
    Finished { message: String },
    Failed { message: String },
    LivePush { ok: bool, message: String },
    LiveState { running: bool },
}

pub struct Worker {
    tx: Sender<Command>,
    rx: Receiver<Event>,
}

impl Worker {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("hx450-hid".into())
            .spawn(move || run(cmd_rx, evt_tx))
            .expect("spawn hid worker");
        Self { tx: cmd_tx, rx: evt_rx }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}

struct LiveJob {
    provider: Provider,
    interval: Duration,
    next_at: Instant,
    failures: u32,
}

/// The clock is aligned to the minute boundary so the panel flips exactly when
/// the minute changes instead of lagging up to a full interval behind.
const CLOCK_ALIGN_MS: u64 = 1200;

fn next_due(provider: Provider, interval: Duration) -> Instant {
    if provider == Provider::Clock {
        let now = chrono::Local::now();
        let seconds = now.second() as u64;
        let millis = now.timestamp_subsec_millis() as u64;
        let until_next_minute = (60 - seconds) * 1000 - millis + CLOCK_ALIGN_MS;
        Instant::now() + Duration::from_millis(until_next_minute.max(1500))
    } else {
        Instant::now() + interval
    }
}

fn run(cmd_rx: Receiver<Command>, evt_tx: Sender<Event>) {
    let mut live_state = LiveState::new();
    let mut live: Option<LiveJob> = None;

    loop {
        if let Some(job) = live.as_mut() {
            if Instant::now() >= job.next_at {
                let provider = job.provider;
                let interval = job.interval;
                let result = with_device(|dev| {
                    let frame = live_state.render(provider);
                    let payload = crate::model::Screen::single(frame).encode();
                    upload(dev, &payload, &evt_tx, true)
                });
                match result {
                    Ok(pages) => {
                        job.failures = 0;
                        job.next_at = next_due(provider, interval);
                        let _ = evt_tx.send(Event::LivePush {
                            ok: true,
                            message: format!("{pages} pages pushed"),
                        });
                    }
                    Err(message) => {
                        job.failures = job.failures.saturating_add(1);
                        // Retry quickly at first, then back off so a sleeping
                        // keyboard cannot spin the loop.
                        let backoff = if job.failures >= 5 { 30 } else { 3 };
                        job.next_at = Instant::now() + Duration::from_secs(backoff);
                        let _ = evt_tx.send(Event::LivePush {
                            ok: false,
                            message: format!("push failed ({message}), retrying in {backoff}s"),
                        });
                    }
                }
            }
        }

        let timeout = live
            .as_ref()
            .map(|job| job.next_at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(250))
            .min(Duration::from_millis(250));

        match cmd_rx.recv_timeout(timeout) {
            Ok(Command::Shutdown) => break,
            Ok(Command::Probe) => {
                let info = HidApi::new()
                    .ok()
                    .and_then(|api| locate(&api).ok())
                    .map(|device| device.info());
                let _ = evt_tx.send(Event::Device(info));
            }
            Ok(Command::Upload(payload)) => {
                match with_device(|dev| upload(dev, &payload, &evt_tx, false)) {
                    Ok(pages) => {
                        let _ = evt_tx.send(Event::Finished {
                            message: format!("uploaded {pages} pages"),
                        });
                    }
                    Err(message) => {
                        let _ = evt_tx.send(Event::Failed { message });
                    }
                }
            }
            Ok(Command::Light { mode, colour, brightness, speed, colourful }) => {
                match with_device(|dev| {
                    set_light(dev, mode, colour, brightness, speed, colourful, &evt_tx)
                }) {
                    Ok(_) => {
                        let _ = evt_tx.send(Event::Finished { message: "lighting applied".into() });
                    }
                    Err(message) => {
                        let _ = evt_tx.send(Event::Failed { message });
                    }
                }
            }
            Ok(Command::ClockSync) => match with_device(|dev| clock_sync(dev, &evt_tx)) {
                Ok(_) => {
                    let _ = evt_tx.send(Event::Finished { message: "clock synced".into() });
                }
                Err(message) => {
                    let _ = evt_tx.send(Event::Failed { message });
                }
            },
            Ok(Command::StartLive { provider, interval_secs }) => {
                let interval = Duration::from_secs(interval_secs.max(10) as u64);
                live = Some(LiveJob {
                    provider,
                    interval,
                    next_at: Instant::now(),
                    failures: 0,
                });
                let _ = evt_tx.send(Event::LiveState { running: true });
            }
            Ok(Command::StopLive) => {
                live = None;
                let _ = evt_tx.send(Event::LiveState { running: false });
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
        // keep TryRecvError referenced for the compiler's benefit on older chans
        let _ = TryRecvError::Empty;
    }
}

fn with_device<T>(
    action: impl FnOnce(&Device) -> Result<T, String>,
) -> Result<T, String> {
    let api = HidApi::new().map_err(|e| format!("hidapi init failed: {e}"))?;
    let device = locate(&api)?;
    action(&device)
}

struct Device {
    image: hidapi::HidDevice,
    control: hidapi::HidDevice,
    info: DeviceInfo,
}

fn locate(api: &HidApi) -> Result<Device, String> {
    let mut image_path: Option<CString> = None;
    let mut control_path: Option<CString> = None;
    let mut product = String::new();

    for info in api.device_list() {
        if info.vendor_id() != VENDOR_ID || info.product_id() != PRODUCT_ID {
            continue;
        }
        if product.is_empty() {
            if let Some(name) = info.product_string() {
                product = name.to_string();
            }
        }
        let usage_page = info.usage_page();
        let path = info.path().to_owned();
        if usage_page == IMAGE_USAGE_PAGE || info.interface_number() == 2 {
            image_path = Some(path);
        } else if usage_page == CONTROL_USAGE_PAGE || info.interface_number() == 3 {
            control_path = Some(path);
        }
    }

    let (image_path, control_path) = match (image_path, control_path) {
        (Some(image), Some(control)) => (image, control),
        _ => {
            return Err(
                "HX-KEG450 not found — connect it with the USB-C cable, \
                 install the udev rule and make sure you are in the 'input' group"
                    .into(),
            )
        }
    };

    let image = api
        .open_path(&image_path)
        .map_err(|e| format!("cannot open image interface: {e}"))?;
    let control = api
        .open_path(&control_path)
        .map_err(|e| format!("cannot open control interface: {e}"))?;

    if product.is_empty() {
        product = "HISKA HX-KEG450".into();
    }

    Ok(Device {
        image,
        control,
        info: DeviceInfo {
            image: image_path.to_string_lossy().into_owned(),
            control: control_path.to_string_lossy().into_owned(),
            product,
        },
    })
}

impl Device {
    fn info(&self) -> DeviceInfo {
        self.info.clone()
    }
}

// ------------------------------------------------------------------ framing --

fn feature(payload: &[u8]) -> [u8; 65] {
    let mut buf = [0u8; 65];
    let len = payload.len().min(64);
    buf[1..1 + len].copy_from_slice(&payload[..len]);
    buf
}

fn send_cmd(device: &Device, payload: &[u8]) -> Result<(), String> {
    device
        .control
        .send_feature_report(&feature(payload))
        .map_err(|e| format!("feature report failed: {e}"))?;
    thread::sleep(Duration::from_millis(2));
    Ok(())
}

fn read_reply(device: &Device) -> Option<[u8; 64]> {
    let mut raw = [0u8; 65];
    let read = device.control.get_feature_report(&mut raw).ok()?;
    if read < 2 {
        return None;
    }
    let mut out = [0u8; 64];
    let len = (read - 1).min(64);
    out[..len].copy_from_slice(&raw[1..1 + len]);
    Some(out)
}

fn status_of(reply: Option<[u8; 64]>) -> &'static str {
    match reply {
        None => "no reply",
        Some(reply) => match reply[3] {
            0x01 => "ok",
            0xFF => "rejected",
            0x00 => "ignored",
            _ => "unknown",
        },
    }
}

fn command(device: &Device, payload: &[u8]) -> Result<&'static str, String> {
    send_cmd(device, payload)?;
    let status = status_of(read_reply(device));
    thread::sleep(CMD_DELAY);
    Ok(status)
}

// ---------------------------------------------------------------- operations --

fn upload(
    device: &Device,
    payload: &[u8],
    events: &Sender<Event>,
    quiet: bool,
) -> Result<usize, String> {
    let pages = payload.len().div_ceil(crate::model::PAGE);
    // drain stale replies
    for _ in 0..8 {
        if read_reply(device).is_none() {
            break;
        }
    }

    command(device, &[0x04, 0x18])?;
    command(device, &[0x04, 0x72, 0x00, 0, 0, 0, 0, 0, (pages & 0xFF) as u8, (pages >> 8) as u8])?;

    for index in 0..pages {
        let page = &payload[index * crate::model::PAGE..(index + 1) * crate::model::PAGE];
        let mut buffer = Vec::with_capacity(crate::model::PAGE + 1);
        buffer.push(0x00);
        buffer.extend_from_slice(page);
        device
            .image
            .write(&buffer)
            .map_err(|e| format!("page {}/{} write failed: {e}", index + 1, pages))?;

        let mut ack = [0u8; 64];
        let read = device
            .image
            .read_timeout(&mut ack, 800)
            .map_err(|e| format!("page {} ack failed: {e}", index + 1))?;
        if read < 3 || ack[..3] != PAGE_ACK_MAGIC {
            return Err(format!("page {}/{} was not acknowledged", index + 1, pages));
        }

        if !quiet {
            let _ = events.send(Event::Stage {
                label: "uploading".into(),
                done: index + 1,
                total: pages,
            });
        }
    }

    let commit = command(device, &[0x04, 0x02])?;
    if commit != "ok" {
        return Err(format!("commit returned {commit}"));
    }
    let _ = events.send(Event::Stage { label: "done".into(), done: pages, total: pages });
    Ok(pages)
}

fn set_light(
    device: &Device,
    mode: u8,
    colour: (u8, u8, u8),
    brightness: u8,
    speed: u8,
    colourful: bool,
    events: &Sender<Event>,
) -> Result<(), String> {
    for _ in 0..8 {
        if read_reply(device).is_none() {
            break;
        }
    }
    command(device, &[0x04, 0x18])?;
    command(device, &[0x04, 0x13, 0, 0, 0, 0, 0, 0, 0x01])?;

    let mut packet = [0u8; 64];
    packet[0] = mode;
    packet[1] = colour.0;
    packet[2] = colour.1;
    packet[3] = colour.2;
    packet[8] = u8::from(colourful);
    packet[9] = brightness.min(5);
    packet[10] = speed.min(5);
    packet[14] = 0xAA;
    packet[15] = 0x55;
    send_cmd(device, &packet)?;
    thread::sleep(CMD_DELAY);

    let commit = command(device, &[0x04, 0x02])?;
    command(device, &[0x04, 0xF0])?;
    let _ = events.send(Event::Stage { label: "lighting".into(), done: 1, total: 1 });
    if commit == "ok" {
        Ok(())
    } else {
        Err(format!("lighting commit returned {commit}"))
    }
}

fn clock_sync(device: &Device, _events: &Sender<Event>) -> Result<(), String> {
    let now = Local::now();
    let cmd = [
        0x04,
        0x28,
        (now.year() % 100) as u8,
        now.month() as u8,
        now.day() as u8,
        now.hour() as u8,
        now.minute() as u8,
        now.second() as u8,
    ];
    let status = command(device, &cmd)?;
    if status == "ok" {
        Ok(())
    } else {
        Err(format!("clock sync returned {status}"))
    }
}

