//! The seam between the GUI and the HyprLink daemon — destined to become the
//! `hyprlink-proto` crate.
//!
//! Rule: the daemon sends **data, never presentation**. No formatted strings,
//! no UI wording, no display units baked into text. Everything a human reads
//! is produced by the GUI (`crate::fmt`). Unknown values are `None`.
//!
//! The GUI never talks QUIC itself. It speaks to the local daemon through a
//! [`Transport`]. `mock::Simulator` is a self-contained fake used for design
//! work; the real implementation wraps `$XDG_RUNTIME_DIR/hyprlink.sock`
//! (u32 BE length + CBOR frames) behind the same contract.

#![allow(dead_code)] // protocol surface: not every variant is produced yet

#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod packets;

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub type DeviceId = u32;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Phone,
    Tablet,
    Wearable,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    Linked,
    Idle,
    Offline,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    Workspaces,
    Mirror,
    Webcam,
    Mic,
    Tap,
    Speaker,
    Sensors,
    Presence,
    Clipboard,
    Files,
    Notifications,
    Input,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Device {
    pub id: DeviceId,
    /// The name the user gave the phone (raw; the GUI uppercases it).
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    /// Android release, e.g. "15".
    pub android: Option<String>,
    pub app_version: Option<String>,
    pub kind: Kind,
    pub state: LinkState,
    /// SHA-256 of the device's mTLS certificate, lowercase hex, no separators.
    pub fingerprint: String,
    pub addr: Option<String>,
    pub battery: Option<u8>,
    pub charging: bool,
    pub rssi: Option<i32>,
    pub latency_ms: Option<f32>,
    pub caps: Vec<Cap>,
    /// Unix seconds.
    pub paired_since: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Workspace {
    pub id: u8,
    pub monitor: String,
    pub clients: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
pub struct Sensors {
    pub accel: [f32; 3],
    pub gyro: [f32; 3],
    pub lux: f32,
    pub proximity_cm: f32,
    pub pressure_hpa: f32,
    pub battery_temp: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    Accel,
    Gyro,
    Light,
    Proximity,
    Pressure,
    Thermal,
}

impl SensorKind {
    pub const ALL: [SensorKind; 6] = [
        SensorKind::Accel,
        SensorKind::Gyro,
        SensorKind::Light,
        SensorKind::Proximity,
        SensorKind::Pressure,
        SensorKind::Thermal,
    ];
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    Tx,
    Rx,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Packet {
    /// Seconds since the daemon started.
    pub at: f32,
    pub dir: Dir,
    /// Wire name, `module.action` — see [`packets`].
    pub kind: String,
    pub bytes: usize,
    /// Short machine-ish detail (argument, size, id). Not prose.
    pub note: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

impl Codec {
    pub const ALL: [Codec; 3] = [Codec::H264, Codec::Hevc, Codec::Av1];
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct MirrorConfig {
    pub codec: Codec,
    pub bitrate_mbps: f32,
    pub max_fps: u32,
    pub scale: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
pub struct MirrorStats {
    pub fps: f32,
    pub decode_ms: f32,
    pub dropped: u32,
    pub kbps: f32,
}

/// Commands the GUI asks the daemon to perform.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Command {
    Ping(DeviceId),
    SendClipboard(DeviceId),
    SwitchWorkspace(u8),
    SetMic(bool),
    SetTap(bool),
    /// Tap source: `true` = the virtual sink `hyprlink-speaker` (the phone is
    /// an output device on the PC); `false` = everything the PC plays.
    SetSpeakerMode(bool),
    StartMirror(MirrorConfig),
    StopMirror,
    SetSensorBridge(SensorKind, bool),
    SetRule(usize, bool),
    BeginPairing,
    CancelPairing,
    Unpair(DeviceId),
}

/// Typed replies the GUI turns into toasts. No wording on the wire.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Notice {
    PingReply { device: DeviceId, rtt_ms: f32 },
    ClipboardSent { device: DeviceId },
    Paired { device: DeviceId, name: String },
    Revoked { device: DeviceId, name: String },
    MirrorStarted,
    Failed { op: Op, error: ErrorKind },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Ping,
    Clipboard,
    Mic,
    Tap,
    Speaker,
    Mirror,
    Sensors,
    Presence,
    Pairing,
    Workspace,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The feature does not exist yet (daemon or Android side).
    NotImplemented,
    /// No phone connected.
    Offline,
    /// The phone said no.
    Refused,
    Timeout,
}

/// Things the daemon tells the GUI.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Event {
    Devices(Vec<Device>),
    Workspaces {
        list: Vec<Workspace>,
        active: u8,
    },
    Telemetry {
        latency_ms: f32,
        up_kbps: f32,
        down_kbps: f32,
    },
    Sensors(Sensors),
    /// Linear 0–1 levels, ≥ 30 Hz. `None` = not measured (e.g. the mic has
    /// no meter yet).
    Levels {
        mic: Option<f32>,
        tap: Option<f32>,
    },
    SpeakerMode(bool),
    Mirror(Option<MirrorStats>),
    Rssi(i32),
    Packet(Packet),
    Pairing(Option<PairingTicket>),
    Notice(Notice),
    Phone(PhoneStatus),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PairingTicket {
    /// Exactly what goes into the QR code. Today: `fp|host:port|token`.
    pub payload: String,
    /// Short verification code to type by hand, if the daemon offers one.
    pub code: Option<String>,
    /// `None` = the ticket does not expire (current daemon behaviour).
    pub expires_in: Option<f32>,
}

pub trait Transport {
    fn send(&mut self, command: Command);
    fn poll(&mut self, dt: Duration) -> Vec<Event>;
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Wifi,
    Cellular,
    Ethernet,
    None,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Wifi {
    /// Needs ACCESS_FINE_LOCATION on Android; often `None`.
    pub ssid: Option<String>,
    pub rssi_dbm: Option<i32>,
}

/// `phone.status` — what the phone reports about itself. Sent with the hello,
/// on change, and every 30 s. Every field is optional.
///
/// The PC GUI shows this *first*: the remote device matters more than the
/// machine you are sitting at.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PhoneStatus {
    pub network: Option<Network>,
    /// "5G", "4G"… (RAT generation, not prose).
    pub cell_gen: Option<String>,
    pub carrier: Option<String>,
    /// 0–4.
    pub signal_bars: Option<u8>,
    pub wifi: Option<Wifi>,
    pub storage_used_b: Option<u64>,
    pub storage_total_b: Option<u64>,
    pub ram_used_b: Option<u64>,
    pub ram_total_b: Option<u64>,
    pub battery_temp_c: Option<f32>,
    pub screen_on: Option<bool>,
    pub dnd: Option<bool>,
    pub notifications: Option<u32>,
    pub now_playing: Option<NowPlaying>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct NowPlaying {
    pub title: String,
    pub artist: Option<String>,
    pub app: Option<String>,
    pub playing: bool,
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
}

impl NowPlaying {
    /// 0–1, computed on our side.
    pub fn progress(&self) -> Option<f32> {
        match (self.position_ms, self.duration_ms) {
            (Some(p), Some(d)) if d > 0 => Some((p as f32 / d as f32).clamp(0.0, 1.0)),
            _ => None,
        }
    }
}
