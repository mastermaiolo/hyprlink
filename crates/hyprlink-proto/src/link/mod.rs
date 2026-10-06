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
#[cfg(any(test, feature = "mock"))]
pub mod mock_more;
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
    /// The phone sends `phone.status` (network, storage, RAM, screen,
    /// notifications, now-playing).
    PhoneStatus,
    /// The phone exposes its media session (`phone.media` control).
    MediaSession,
}

/// The phone link as a whole — drives the LINK wordmark colour.
/// Mirrors the daemon's `ConnState` (Pairing/Connecting → `Connecting`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LinkPhase {
    #[default]
    Disconnected,
    Connecting,
    Connected,
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
    /// Everything added for the new pages.
    More(Command2),
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
    Webcam,
    Files,
    Media,
    Dispatch,
    PhoneAudio,
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
    /// Linear 0–1 levels, ≥ 30 Hz. `None` = that channel is OFF (the mic is
    /// not streaming / the tap is not running); `Some` = on, with its level
    /// (`0.0` when on but not measured yet). The GUI derives the on/off
    /// state of both toggles from this, so it never shows a stale "ABERTO".
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
    Link(LinkPhase),
    More(Event2),
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

// ═══════════════════ pages added after Fase 0 (see RESPOSTA-fase0.md) ═══════════════════

/// One point of the phone's battery history (daemon-side, 12 h).
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct BatteryPoint {
    pub at: u64,
    pub level: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct BatteryAlerts {
    /// Warn below this level; `None` = off.
    pub low: Option<u8>,
    pub full: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActiveWindow {
    pub class: String,
    pub title: String,
    pub workspace: u8,
}

/// A user-defined `hyprctl dispatch`, also offered on the phone.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub label: String,
    pub dispatch: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct TrackpadConfig {
    /// 0.25–3.0
    pub sensitivity: f32,
    /// 0.25–3.0
    pub scroll: f32,
    pub acceleration: bool,
    pub natural_scroll: bool,
    pub keyboard: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CamCodec {
    Mjpeg,
    H264,
}

impl CamCodec {
    pub const ALL: [CamCodec; 2] = [CamCodec::Mjpeg, CamCodec::H264];
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct WebcamConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codec: CamCodec,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
pub struct WebcamStats {
    pub mbps: f32,
    /// The daemon does not know this yet.
    pub fps: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct NetTest {
    pub mbps: f32,
    pub rtt_ms: f32,
    pub loss_pct: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PhoneStream {
    Media,
    Ring,
    Notification,
    Alarm,
}

impl PhoneStream {
    pub const ALL: [PhoneStream; 4] = [
        PhoneStream::Media,
        PhoneStream::Ring,
        PhoneStream::Notification,
        PhoneStream::Alarm,
    ];
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Ringer {
    Normal,
    Vibrate,
    Silent,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamLevel {
    pub stream: PhoneStream,
    pub level: u8,
    pub max: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PhoneAudio {
    pub streams: Vec<StreamLevel>,
    pub ringer: Ringer,
    pub dnd: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Sink {
    pub id: u32,
    /// PipeWire node name, e.g. "alsa_output.pci-0000_04_00.6.analog-stereo".
    pub name: String,
    /// Human description from PipeWire (data, not our wording).
    pub description: String,
    /// 0–150 %.
    pub volume: u8,
    pub muted: bool,
    pub default: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AppStream {
    pub id: u32,
    pub app: String,
    pub volume: u8,
    pub muted: bool,
    pub sink: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PhoneNotification {
    pub key: String,
    pub app: String,
    pub title: String,
    pub text: Option<String>,
    /// Unix seconds.
    pub at: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Phone,
    Pc,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClipEntry {
    pub id: u64,
    pub origin: Origin,
    pub mime: String,
    /// Text content, when the entry is text.
    pub text: Option<String>,
    pub bytes: u64,
    pub at: u64,
    pub pinned: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    Active,
    Done,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Transfer {
    pub id: u64,
    pub name: String,
    /// Tx = PC → phone, Rx = phone → PC.
    pub dir: Dir,
    pub bytes: u64,
    pub done: u64,
    pub state: TransferState,
    pub at: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Player {
    /// MPRIS bus name suffix, e.g. "spotify".
    pub id: String,
    pub identity: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub playing: bool,
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    Previous,
    PlayPause,
    Next,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Settings {
    pub downloads_dir: String,
    pub daemon_version: String,
    pub socket: String,
    /// When the daemon started (Unix seconds) — the GUI shows its uptime.
    /// `0` = unknown (an older daemon).
    #[serde(default)]
    pub started_unix: u64,
}

/// Commands for the new pages. Kept apart so the original enum stays readable.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Command2 {
    SetBatteryAlerts(BatteryAlerts),
    RunDispatch(String),
    SetShortcuts(Vec<Shortcut>),
    SetTrackpad(TrackpadConfig),
    StartWebcam(WebcamConfig),
    StopWebcam,
    TestNetwork,
    SetPhoneVolume(PhoneStream, u8),
    SetRinger(Ringer),
    SetDnd(bool),
    SetSinkVolume(u32, u8),
    SetSinkMute(u32, bool),
    SetDefaultSink(u32),
    SetAppVolume(u32, u8),
    SetAppMute(u32, bool),
    DismissNotification(String),
    DismissAllNotifications,
    CopyClip(u64),
    SendClipToPhone(u64),
    PinClip(u64, bool),
    DeleteClip(u64),
    SendFile(String),
    CancelTransfer(u64),
    OpenDownloads,
    Media {
        player: String,
        action: MediaAction,
    },
    /// Proposed: needs a media-session channel on Android.
    PhoneMedia(MediaAction),
    /// Headset (C1): the phone becomes the PC's full-duplex headset —
    /// speaker mode (PC audio plays on the phone) + the phone mic as PC
    /// input, one toggle. Composition of `SetSpeakerMode(true)` +
    /// `SetMic(true)`; the disconnect cleanup (speaker OFF + mic stop)
    /// já existe no daemon.
    SetHeadset(bool),
    SetDownloadsDir(String),
    /// Nome escolhido no PC para um dispositivo (vazio = voltar ao nome que o
    /// telemóvel envia). Guardado no daemon, por certificado.
    RenameDevice(DeviceId, String),
    RestartDaemon,
}

/// Events for the new pages.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Event2 {
    BatteryHistory(Vec<BatteryPoint>),
    BatteryAlerts(BatteryAlerts),
    ActiveWindow(Option<ActiveWindow>),
    Shortcuts(Vec<Shortcut>),
    Trackpad(TrackpadConfig),
    Webcam(Option<WebcamStats>),
    NetTest(NetTest),
    PhoneAudio(PhoneAudio),
    Mixer {
        sinks: Vec<Sink>,
        apps: Vec<AppStream>,
    },
    Notifications(Vec<PhoneNotification>),
    Clipboard(Vec<ClipEntry>),
    Transfers(Vec<Transfer>),
    Players(Vec<Player>),
    Settings(Settings),
}
