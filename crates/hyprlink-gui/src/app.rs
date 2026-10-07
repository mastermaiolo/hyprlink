use crate::graphics::Ticker;
use crate::host::{Host, Probe};
use crate::link::{
    self, ActiveWindow, AppStream, BatteryAlerts, BatteryPoint, CamCodec, ClipEntry, Codec,
    Command, Command2, DeviceId, Dir, Event, Event2, MirrorConfig, MirrorStats, NetTest, Packet,
    PairingTicket, PhoneAudio, PhoneNotification, Player, SensorKind, Sensors, Settings, Shortcut,
    Sink, TrackpadConfig, Transfer, Transport, WebcamConfig, WebcamStats, Workspace,
};
use crate::theme::{self, *};
use crate::tray;
use crate::ui::{self, *};
use crate::views;
use hyprlink_gui::fmt;
use hyprlink_gui::i18n::t;
use hyprlink_gui::tr;

use iced::keyboard::{self, Key};
use iced::widget::{button, canvas, column, container, qr_code, row, scrollable, stack, text};
use iced::window;
use iced::{Alignment, Element, Length, Padding, Subscription, Task};
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

/// How long a toggle keeps the user's choice before the daemon's state wins
/// (the phone has to accept a mic request; the tap pipeline takes a moment).
const TOGGLE_GRACE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Capa,
    Dispositivos,
    Secretaria,
    Camera,
    Audio,
    Notificacoes,
    Partilha,
    Multimedia,
    Sensores,
    Diario,
    /// Not numbered: lives at the foot of the rail.
    Definicoes,
}

impl Section {
    /// The ten numbered sections, in reading order (keys 1–9, 0).
    pub const ALL: [Section; 10] = [
        Section::Capa,
        Section::Dispositivos,
        Section::Secretaria,
        Section::Camera,
        Section::Audio,
        Section::Notificacoes,
        Section::Partilha,
        Section::Multimedia,
        Section::Sensores,
        Section::Diario,
    ];

    pub fn num(self) -> &'static str {
        match self {
            Section::Capa => "01",
            Section::Dispositivos => "02",
            Section::Secretaria => "03",
            Section::Camera => "04",
            Section::Audio => "05",
            Section::Notificacoes => "06",
            Section::Partilha => "07",
            Section::Multimedia => "08",
            Section::Sensores => "09",
            Section::Diario => "10",
            Section::Definicoes => "00",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Capa => t("Capa"),
            Section::Dispositivos => t("Dispositivos"),
            Section::Secretaria => t("Secretária"),
            Section::Camera => t("Câmara & Ecrã"),
            Section::Audio => t("Áudio"),
            Section::Notificacoes => t("Notificações"),
            Section::Partilha => t("Partilha"),
            Section::Multimedia => t("Multimédia"),
            Section::Sensores => t("Sensores & Presença"),
            Section::Diario => t("Diário"),
            Section::Definicoes => t("Definições"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CamMode {
    Camera,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Tx,
    Rx,
}

pub struct History {
    buf: VecDeque<f32>,
    cap: usize,
}

impl History {
    pub fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(cap),
            cap,
        }
    }
    pub fn push(&mut self, v: f32) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }
    pub fn to_vec(&self) -> Vec<f32> {
        self.buf.iter().copied().collect()
    }
    pub fn last(&self) -> f32 {
        self.buf.back().copied().unwrap_or(0.0)
    }
    pub fn max(&self) -> f32 {
        self.buf.iter().copied().fold(0.0, f32::max)
    }
}

pub struct Rule {
    pub trigger: &'static str,
    pub detail: &'static str,
    pub action: &'static str,
    pub on: bool,
}

pub struct App {
    link: Box<dyn Transport>,
    pub section: Section,
    pub t: f32,
    last: Option<Instant>,

    pub devices: Vec<link::Device>,
    pub selected: DeviceId,
    pub workspaces: Vec<Workspace>,
    pub active_ws: u8,
    pub follow_phone: bool,
    /// Regras dos gestos do telemóvel, como o daemon as guarda (vazio até a
    /// primeira `Event2::Gestures`).
    pub gestures: Vec<link::GestureRule>,
    /// O último gesto recebido do telemóvel, se algum.
    pub last_gesture: Option<link::GestureLast>,

    pub latency: History,
    pub up: History,
    pub down: History,

    pub sensors: Sensors,
    pub sensor_hist: Vec<(SensorKind, History)>,
    pub bridges: HashSet<SensorKind>,

    pub mic_on: bool,
    pub tap_on: bool,
    pub mic: f32,
    pub tap: f32,
    pub mic_peak: f32,
    pub tap_peak: f32,
    pub mic_gain: f32,
    pub mic_measured: bool,
    pub tap_measured: bool,
    /// When the user last flipped each toggle. For `TOGGLE_GRACE` after a
    /// click the GUI keeps the user's choice; after that the daemon's
    /// `Event::Levels` (None = off) is the truth.
    mic_clicked: Option<Instant>,
    tap_clicked: Option<Instant>,
    /// Tap source is the `hyprlink-speaker` sink (phone as PC output).
    pub speaker: bool,
    pub tap_gain: f32,
    pub mic_hist: History,
    pub tap_hist: History,

    pub mirror_cfg: MirrorConfig,
    pub mirror: Option<MirrorStats>,
    pub mirror_on: bool,

    pub rssi: f32,
    /// `false` até chegar um `Event::Rssi` real — sem isso mostra-se «—».
    pub rssi_known: bool,
    pub rssi_hist: History,
    pub lock_at: f32,
    pub unlock_at: f32,
    pub rules: Vec<Rule>,

    pub packets: VecDeque<Packet>,
    pub filter: Filter,
    pub query: String,
    pub paused: bool,

    pub pairing: Option<PairingTicket>,
    pub qr: Option<qr_code::Data>,

    toasts: Vec<(String, f32)>,

    /// The phone's own report — the headline on the PC side.
    pub phone: Option<link::PhoneStatus>,
    /// This machine. Secondary, real data.
    pub host: Host,
    probe: Probe,
    pub cpu_hist: History,
    host_acc: f32,

    /// The single main window, if open. The process lives on in the tray.
    pub window: Option<window::Id>,
    tray: Option<tray::Link>,
    tray_failed: bool,
    tray_acc: f32,
    tray_snap: tray::Snapshot,

    // ── pages added after Fase 0 ──
    pub battery_hist: Vec<BatteryPoint>,
    pub alerts: BatteryAlerts,
    pub active_window: Option<ActiveWindow>,
    pub shortcuts: Vec<Shortcut>,
    /// Página Secretária: modo de edição dos atalhos, rascunhos por posição
    /// (só as linhas alteradas) e o rascunho do atalho novo.
    /// Página Partilha: URL e pacote a abrir no telemóvel.
    pub phone_url: String,
    pub phone_pkg: String,
    pub sc_edit: bool,
    pub sc_drafts: std::collections::HashMap<usize, (String, String)>,
    pub sc_new: (String, String),
    pub trackpad: TrackpadConfig,
    pub dispatch_input: String,
    pub cam_mode: CamMode,
    pub webcam_cfg: WebcamConfig,
    pub webcam: Option<WebcamStats>,
    pub webcam_on: bool,
    pub nettest: Option<NetTest>,
    pub phone_audio: Option<PhoneAudio>,
    pub sinks: Vec<Sink>,
    pub apps: Vec<AppStream>,
    pub notifs: Vec<PhoneNotification>,
    /// Rascunho de resposta por notificação (chave do Android).
    pub reply_drafts: std::collections::HashMap<String, String>,
    pub notif_app: Option<String>,
    pub notif_query: String,
    pub clips: Vec<ClipEntry>,
    pub clip_query: String,
    pub transfers: Vec<Transfer>,
    pub send_path: String,
    pub players: Vec<Player>,
    pub settings: Option<Settings>,
    pub downloads_input: String,
    /// Texto do campo «NOME» da página Dispositivos (por dispositivo em edição).
    pub rename_input: String,
    pub rename_for: Option<DeviceId>,

    /// The phone link as a whole: drives the LINK wordmark.
    pub phase: link::LinkPhase,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick(Instant),
    Nav(Section),
    Key(keyboard::Event),
    Select(DeviceId),
    Ping(DeviceId),
    Clipboard(DeviceId),
    Unpair(DeviceId),
    BeginPair,
    CancelPair,
    SwitchWs(u8),
    FollowPhone(bool),
    Gesture(usize, bool),
    ClearFileHistory,
    /// Seletor de idioma em Definições: aplica na hora, grava `gui.json` e
    /// manda o idioma novo à bandeja.
    SetLang(hyprlink_gui::i18n::Lang),
    Mic(bool),
    Tap(bool),
    Speaker(bool),
    MicGain(f32),
    TapGain(f32),
    Codec(Codec),
    Bitrate(f32),
    Fps(u32),
    Scale(f32),
    MirrorStart,
    MirrorStop,
    Bridge(SensorKind, bool),
    Rule(usize, bool),
    LockAt(f32),
    UnlockAt(f32),
    Filter(Filter),
    Query(String),
    Pause(bool),
    ClearJournal,
    TrayReady(tray::Link),
    TrayFailed,
    Tray(tray::Action),
    WindowClosed(window::Id),
    /// Anything that maps 1:1 to a daemon command on the new pages.
    Do(Command2),
    CamMode(CamMode),
    CamRes(u32, u32),
    CamFps(u32),
    CamCodec(CamCodec),
    WebcamStart,
    WebcamStop,
    DispatchInput(String),
    DispatchRun,
    LowAlert(bool),
    LowLevel(f32),
    FullAlert(bool),
    Trackpad(TrackpadConfig),
    NotifApp(Option<String>),
    NotifQuery(String),
    PhoneUrl(String),
    PhoneUrlOpen,
    PhonePkg(String),
    PhonePkgOpen,
    ShortcutsEdit(bool),
    ShortcutName(usize, String),
    ShortcutCommand(usize, String),
    ShortcutSave(usize),
    ShortcutRemove(usize),
    ShortcutNewName(String),
    ShortcutNewCommand(String),
    ShortcutAdd,
    /// Modo auricular: coluna + microfone do telemóvel num só interruptor.
    Headset(bool),
    /// Texto da resposta a uma notificação (chave, texto).
    ReplyInput(String, String),
    /// Envia a resposta (chave, índice da ação de resposta).
    ReplySend(String, u32),
    ClipQuery(String),
    SendPath(String),
    SendFile,
    FileDropped(std::path::PathBuf),
    /// Botão «ESCOLHER FICHEIROS…»: abre o seletor do portal XDG.
    PickFiles,
    /// Resultado do seletor: `None` = cancelado; `Some(vazio)` = o portal não abriu.
    FilesPicked(Option<Vec<std::path::PathBuf>>),
    DownloadsInput(String),
    RenameInput(DeviceId, String),
    RenameSave(DeviceId),
    DownloadsSave,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let start = std::env::var("HYPRLINK_SECTION")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|n| {
                if n == 11 {
                    Some(Section::Definicoes)
                } else {
                    Section::ALL.get(n.saturating_sub(1)).copied()
                }
            })
            .unwrap_or(Section::Capa);

        let mut app = Self {
            link: hyprlink_gui::transport(),
            section: start,
            t: 0.0,
            last: None,
            devices: Vec::new(),
            selected: 1,
            workspaces: Vec::new(),
            active_ws: 1,
            follow_phone: true,
            gestures: Vec::new(),
            last_gesture: None,
            latency: History::new(90),
            up: History::new(90),
            down: History::new(90),
            sensors: Sensors::default(),
            sensor_hist: SensorKind::ALL
                .iter()
                .map(|k| (*k, History::new(80)))
                .collect(),
            bridges: [SensorKind::Accel, SensorKind::Light, SensorKind::Proximity]
                .into_iter()
                .collect(),
            mic_on: false,
            tap_on: false,
            mic: 0.0,
            tap: 0.0,
            mic_peak: 0.0,
            tap_peak: 0.0,
            mic_gain: 0.72,
            mic_measured: false,
            tap_measured: false,
            mic_clicked: None,
            tap_clicked: None,
            speaker: true,
            tap_gain: 0.85,
            mic_hist: History::new(160),
            tap_hist: History::new(160),
            mirror_cfg: MirrorConfig {
                codec: Codec::Hevc,
                bitrate_mbps: 12.0,
                max_fps: 60,
                scale: 0.75,
            },
            mirror: None,
            mirror_on: false,
            rssi: -52.0,
            rssi_known: false,
            rssi_hist: History::new(60),
            lock_at: -80.0,
            unlock_at: -65.0,
            rules: vec![
                Rule {
                    trigger: "Telemóvel afasta-se",
                    detail: "rssi < limiar durante 10 s",
                    action: "hyprlock",
                    on: true,
                },
                Rule {
                    trigger: "Telemóvel regressa",
                    detail: "rssi > limiar · mTLS ok",
                    action: "desbloqueia + restaura workspace",
                    on: true,
                },
                Rule {
                    trigger: "Chamada a entrar",
                    detail: "phone.call · proposto",
                    action: "playerctl pause · notify-send",
                    on: true,
                },
                Rule {
                    trigger: "A carregar na secretária",
                    detail: "battery.charging ∧ rssi > −50",
                    action: "modo foco · makoctl mode dnd",
                    on: false,
                },
                Rule {
                    trigger: "Pulseira deteta sono",
                    detail: "Smart Band 10 · sleep_state",
                    action: "systemctl suspend (15 min)",
                    on: false,
                },
            ],
            packets: VecDeque::with_capacity(400),
            filter: Filter::All,
            query: String::new(),
            paused: false,
            pairing: None,
            qr: None,
            toasts: Vec::new(),
            phone: None,
            host: Host::default(),
            probe: Probe::default(),
            cpu_hist: History::new(60),
            host_acc: 10.0,
            window: None,
            tray: None,
            tray_failed: false,
            tray_acc: 0.0,
            tray_snap: tray::Snapshot::default(),
            battery_hist: Vec::new(),
            alerts: BatteryAlerts {
                low: Some(20),
                full: true,
            },
            active_window: None,
            shortcuts: Vec::new(),
            phone_url: String::new(),
            phone_pkg: String::new(),
            sc_edit: false,
            sc_drafts: std::collections::HashMap::new(),
            sc_new: (String::new(), String::new()),
            trackpad: TrackpadConfig {
                sensitivity: 1.0,
                scroll: 1.0,
                acceleration: true,
                natural_scroll: false,
                keyboard: true,
            },
            dispatch_input: String::new(),
            cam_mode: CamMode::Camera,
            webcam_cfg: WebcamConfig {
                width: 1280,
                height: 720,
                fps: 30,
                codec: CamCodec::H264,
            },
            webcam: None,
            webcam_on: false,
            nettest: None,
            phone_audio: None,
            sinks: Vec::new(),
            apps: Vec::new(),
            notifs: Vec::new(),
            reply_drafts: std::collections::HashMap::new(),
            notif_app: None,
            notif_query: String::new(),
            clips: Vec::new(),
            clip_query: String::new(),
            transfers: Vec::new(),
            send_path: String::new(),
            players: Vec::new(),
            settings: None,
            downloads_input: String::new(),
            rename_input: String::new(),
            rename_for: None,
            // Until the daemon says otherwise, we are trying.
            phase: link::LinkPhase::Connecting,
        };

        // Warm the simulation so graphs open full rather than empty.
        for _ in 0..240 {
            let events = app.link.poll(Duration::from_millis(33));
            for e in events {
                app.apply(e);
            }
            app.t += 0.033;
        }
        app.toasts.clear();

        if std::env::var("HYPRLINK_DEMO").as_deref() == Ok("mirror") {
            app.mirror_on = true;
            app.link.send(Command::StartMirror(app.mirror_cfg));
        }
        if std::env::var("HYPRLINK_DEMO").as_deref() == Ok("pair") {
            app.link.send(Command::BeginPairing);
        }
        match std::env::var("HYPRLINK_DEMO").as_deref() {
            Ok("webcam") => {
                app.webcam_on = true;
                app.link
                    .send(Command::More(Command2::StartWebcam(app.webcam_cfg)));
                app.link.send(Command::More(Command2::TestNetwork));
            }
            Ok("screen") => {
                app.cam_mode = CamMode::Screen;
                app.mirror_on = true;
                app.link.send(Command::StartMirror(app.mirror_cfg));
            }
            Ok("transfer") => {
                app.link.send(Command::More(Command2::SendFile(
                    "~/Música/OMNIS_v3_stems.zip".into(),
                )));
            }
            _ => {}
        }
        // Let demo commands land before the first frame.
        for _ in 0..10 {
            for e in app.link.poll(Duration::from_millis(33)) {
                app.apply(e);
            }
        }
        app.toasts.clear();
        app.host = app.probe.sample();

        // `--hidden` starts straight into the tray.
        let task = if std::env::args().any(|a| a == "--hidden") {
            Task::none()
        } else {
            app.open_window()
        };
        (app, task)
    }

    pub fn window_settings() -> window::Settings {
        window::Settings {
            // HYPRLINK_WINDOW_H: taller windows for full-page captures.
            size: iced::Size::new(
                1480.0,
                std::env::var("HYPRLINK_WINDOW_H")
                    .ok()
                    .and_then(|h| h.parse().ok())
                    .unwrap_or(940.0),
            ),
            min_size: Some(iced::Size::new(1240.0, 760.0)),
            platform_specific: window::settings::PlatformSpecific {
                application_id: "dev.hyprlink.gui".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn open_window(&mut self) -> Task<Message> {
        if let Some(id) = self.window {
            return window::gain_focus(id);
        }
        let (id, task) = window::open(Self::window_settings());
        self.window = Some(id);
        task.discard()
    }

    fn push_tray(&mut self) -> Task<Message> {
        let Some(link) = self.tray.clone() else {
            return Task::none();
        };
        let d = self.primary();
        let snap = tray::Snapshot {
            device: d.map(fmt::name),
            battery: d.and_then(|d| d.battery),
            charging: d.map(|d| d.charging).unwrap_or(false),
            network: self
                .phone
                .as_ref()
                .map(fmt::network_line)
                .unwrap_or_default(),
            latency_ms: self.latency.last(),
            mic: self.mic_on,
            mirror: self.mirror_on,
            pairing: self.pairing.is_some(),
            phase: self.phase,
            lang: hyprlink_gui::i18n::get(),
        };
        if snap == self.tray_snap {
            return Task::none();
        }
        self.tray_snap = snap.clone();
        Task::future(async move {
            link.0.update(move |t| t.snap = snap).await;
        })
        .discard()
    }

    pub fn title(&self, _window: window::Id) -> String {
        format!("HYPRLINK — {}", self.section.title())
    }

    pub fn device(&self) -> Option<&link::Device> {
        self.devices
            .iter()
            .find(|d| d.id == self.selected)
            .or(self.devices.first())
    }

    pub fn primary(&self) -> Option<&link::Device> {
        self.devices
            .iter()
            .find(|d| d.state == link::LinkState::Linked)
    }

    fn apply(&mut self, e: Event) {
        match e {
            Event::Devices(d) => {
                self.devices = d;
                if !self.devices.iter().any(|d| d.id == self.selected) {
                    self.selected = self.devices.first().map(|d| d.id).unwrap_or(0);
                }
            }
            Event::Workspaces { list, active } => {
                self.workspaces = list;
                self.active_ws = active;
            }
            Event::Telemetry {
                latency_ms,
                up_kbps,
                down_kbps,
            } => {
                self.latency.push(latency_ms);
                self.up.push(up_kbps);
                self.down.push(down_kbps);
            }
            Event::Sensors(s) => {
                self.sensors = s;
                for (k, h) in &mut self.sensor_hist {
                    h.push(match k {
                        SensorKind::Accel => {
                            (s.accel[0].powi(2) + s.accel[1].powi(2) + s.accel[2].powi(2)).sqrt()
                        }
                        SensorKind::Gyro => {
                            (s.gyro[0].powi(2) + s.gyro[1].powi(2) + s.gyro[2].powi(2)).sqrt()
                        }
                        SensorKind::Light => s.lux,
                        SensorKind::Proximity => s.proximity_cm,
                        SensorKind::Pressure => s.pressure_hpa,
                        SensorKind::Thermal => s.battery_temp,
                    });
                }
            }
            Event::Levels { mic, tap } => {
                // `None` = that channel is off. The toggles follow the
                // daemon, except right after a click (the phone may still be
                // accepting the request).
                self.mic_measured = mic.is_some();
                self.tap_measured = tap.is_some();
                if self.mic_clicked.is_none_or(|t| t.elapsed() > TOGGLE_GRACE) {
                    self.mic_on = mic.is_some();
                }
                if self.tap_clicked.is_none_or(|t| t.elapsed() > TOGGLE_GRACE) {
                    self.tap_on = tap.is_some();
                }
                self.mic = (mic.unwrap_or(0.0) * self.mic_gain * 1.25).min(1.0);
                self.tap = (tap.unwrap_or(0.0) * self.tap_gain).min(1.0);
                self.mic_peak = if self.mic > self.mic_peak {
                    self.mic
                } else {
                    (self.mic_peak - 0.008).max(0.0)
                };
                self.tap_peak = if self.tap > self.tap_peak {
                    self.tap
                } else {
                    (self.tap_peak - 0.008).max(0.0)
                };
                self.mic_hist.push(self.mic);
                self.tap_hist.push(self.tap);
            }
            Event::SpeakerMode(on) => self.speaker = on,
            Event::Mirror(m) => self.mirror = m,
            Event::Rssi(r) => {
                self.rssi = r as f32;
                self.rssi_known = true;
                self.rssi_hist.push(r as f32);
            }
            Event::Packet(p) => {
                if !self.paused {
                    if self.packets.len() == 400 {
                        self.packets.pop_front();
                    }
                    self.packets.push_back(p);
                }
            }
            Event::Pairing(p) => {
                self.qr = p.as_ref().and_then(|p| qr_code::Data::new(&p.payload).ok());
                self.pairing = p;
            }
            Event::Phone(p) => self.phone = Some(p),
            Event::More(e) => self.apply_more(e),
            Event::Link(p) => self.phase = p,
            Event::Notice(n) => {
                let s = fmt::notice(&n, |id| {
                    self.devices.iter().find(|d| d.id == id).map(fmt::name)
                });
                self.toast(s);
            }
        }
    }

    fn apply_more(&mut self, e: Event2) {
        match e {
            Event2::BatteryHistory(h) => self.battery_hist = h,
            Event2::BatteryAlerts(a) => self.alerts = a,
            Event2::ActiveWindow(w) => self.active_window = w,
            Event2::Shortcuts(s) => {
                // Se a lista mudou de tamanho (remover, adicionar, outro
                // cliente), os rascunhos por posição deixam de valer.
                if s.len() != self.shortcuts.len() {
                    self.sc_drafts.clear();
                }
                self.shortcuts = s;
            }
            Event2::Gestures { rules, last } => {
                self.gestures = rules;
                self.last_gesture = last;
            }
            Event2::Trackpad(c) => self.trackpad = c,
            Event2::Webcam(w) => self.webcam = w,
            Event2::NetTest(n) => self.nettest = Some(n),
            Event2::PhoneAudio(a) => self.phone_audio = Some(a),
            Event2::Mixer { sinks, apps } => {
                self.sinks = sinks;
                self.apps = apps;
            }
            Event2::Notifications(n) => self.notifs = n,
            Event2::Clipboard(c) => self.clips = c,
            Event2::Transfers(t) => self.transfers = t,
            Event2::Players(p) => self.players = p,
            Event2::Settings(s) => {
                if self.downloads_input.is_empty() {
                    self.downloads_input = s.downloads_dir.clone();
                }
                self.settings = Some(s);
            }
        }
    }

    /// LINK colour: green connected, golden amber while trying (breathing),
    /// red when down. The only place these three colours are chosen.
    pub fn link_color(&self) -> Color {
        match self.phase {
            link::LinkPhase::Connected => LINK_UP,
            link::LinkPhase::Disconnected => LINK_DOWN,
            link::LinkPhase::Connecting => {
                let breath = 0.5 + 0.5 * (self.t * std::f32::consts::TAU / 1.6).sin();
                alpha(LINK_WAIT, 0.45 + 0.55 * breath)
            }
        }
    }

    fn more(&mut self, c: Command2) {
        self.link.send(Command::More(c));
    }

    /// Nome e comando que a linha `i` mostra: o rascunho, ou o guardado.
    pub fn shortcut_draft(&self, i: usize) -> (String, String) {
        self.sc_drafts.get(&i).cloned().unwrap_or_else(|| {
            self.shortcuts
                .get(i)
                .map(|s| (s.label.clone(), s.dispatch.clone()))
                .unwrap_or_default()
        })
    }

    fn toast(&mut self, s: String) {
        self.toasts.push((s, self.t));
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    /// Ponto único de envio de ficheiros (largados na janela ou escolhidos):
    /// um `SendFile` por ficheiro, pela ordem recebida; as pastas ficam de fora
    /// (o daemon só aceita ficheiros regulares) e avisa-se uma vez.
    fn send_paths(&mut self, paths: Vec<std::path::PathBuf>) {
        let plan = plan_send(paths);
        for f in plan.files {
            self.more(Command2::SendFile(f));
        }
        if plan.folders > 0 {
            self.toast(tr!(
                "{} pasta(s) ignorada(s) — só se enviam ficheiros.",
                plan.folders
            ));
        }
        self.section = Section::Partilha;
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick(now) => {
                let dt = self
                    .last
                    .map(|l| now - l)
                    .unwrap_or(Duration::from_millis(33));
                self.last = Some(now);
                self.t += dt.as_secs_f32();
                for e in self.link.poll(dt) {
                    self.apply(e);
                }
                let t = self.t;
                self.toasts.retain(|(_, born)| t - born < 4.0);

                self.host_acc += dt.as_secs_f32();
                if self.host_acc > 2.0 {
                    self.host_acc = 0.0;
                    self.host = self.probe.sample();
                    self.cpu_hist.push(self.host.cpu_pct);
                }
                self.tray_acc += dt.as_secs_f32();
                if self.tray_acc > 1.0 {
                    self.tray_acc = 0.0;
                    return self.push_tray();
                }
            }
            Message::TrayReady(link) => {
                self.tray = Some(link);
                self.tray_snap = tray::Snapshot {
                    device: Some(String::new()),
                    ..Default::default()
                };
                return self.push_tray();
            }
            Message::TrayFailed => {
                self.tray_failed = true;
                if self.window.is_none() {
                    return self.open_window();
                }
            }
            Message::Tray(action) => {
                let id = self.primary().map(|d| d.id);
                return match action {
                    tray::Action::Open => self.open_window(),
                    tray::Action::Ping => id
                        .map(|i| self.update(Message::Ping(i)))
                        .unwrap_or(Task::none()),
                    tray::Action::Clipboard => id
                        .map(|i| self.update(Message::Clipboard(i)))
                        .unwrap_or(Task::none()),
                    tray::Action::Mic(b) => {
                        Task::batch([self.update(Message::Mic(b)), self.push_tray()])
                    }
                    tray::Action::Mirror(b) => {
                        let m = if b {
                            Message::MirrorStart
                        } else {
                            Message::MirrorStop
                        };
                        Task::batch([self.update(m), self.push_tray()])
                    }
                    tray::Action::Quit => iced::exit(),
                };
            }
            Message::WindowClosed(id) => {
                if self.window == Some(id) {
                    self.window = None;
                }
                // Without a tray there is nothing left to come back to.
                if self.tray_failed || self.tray.is_none() {
                    return iced::exit();
                }
            }
            Message::Nav(s) => self.section = s,
            Message::Key(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if let Key::Character(c) = key.as_ref() {
                    if !modifiers.control() && !modifiers.alt() {
                        if let Ok(n) = c.parse::<usize>() {
                            // 1–9 → §01–§09, 0 → §10.
                            let i = if n == 0 { 9 } else { n - 1 };
                            if let Some(s) = Section::ALL.get(i) {
                                self.section = *s;
                            }
                        }
                    }
                }
                if let Key::Named(keyboard::key::Named::Escape) = key.as_ref() {
                    if self.pairing.is_some() {
                        self.link.send(Command::CancelPairing);
                    }
                }
            }
            Message::Key(_) => {}
            Message::Select(id) => self.selected = id,
            Message::Ping(id) => self.link.send(Command::Ping(id)),
            Message::Clipboard(id) => self.link.send(Command::SendClipboard(id)),
            Message::Unpair(id) => self.link.send(Command::Unpair(id)),
            Message::BeginPair => self.link.send(Command::BeginPairing),
            Message::CancelPair => self.link.send(Command::CancelPairing),
            Message::SwitchWs(n) => self.link.send(Command::SwitchWorkspace(n)),
            Message::FollowPhone(b) => self.follow_phone = b,
            Message::SetLang(l) => {
                hyprlink_gui::langcfg::apply_and_save(l);
                return self.push_tray();
            }
            Message::Gesture(i, b) => {
                // O daemon guarda e devolve o estado; aqui só se pede.
                if let Some(g) = self.gestures.get(i) {
                    let name = g.name.clone();
                    self.more(Command2::SetGesture(name, b));
                }
            }
            Message::ClearFileHistory => {
                self.more(Command2::ClearFileHistory);
                self.toast(t("Histórico limpo").to_string());
            }
            Message::Mic(b) => {
                self.mic_on = b;
                self.mic_clicked = Some(Instant::now());
                self.link.send(Command::SetMic(b));
            }
            Message::Speaker(b) => {
                self.speaker = b;
                self.link.send(Command::SetSpeakerMode(b));
            }
            Message::Tap(b) => {
                self.tap_on = b;
                self.tap_clicked = Some(Instant::now());
                self.link.send(Command::SetTap(b));
            }
            Message::MicGain(g) => self.mic_gain = g,
            Message::TapGain(g) => self.tap_gain = g,
            Message::Codec(c) => self.mirror_cfg.codec = c,
            Message::Bitrate(b) => self.mirror_cfg.bitrate_mbps = b,
            Message::Fps(f) => self.mirror_cfg.max_fps = f,
            Message::Scale(s) => self.mirror_cfg.scale = s,
            Message::MirrorStart => {
                self.mirror_on = true;
                self.link.send(Command::StartMirror(self.mirror_cfg));
            }
            Message::MirrorStop => {
                self.mirror_on = false;
                self.link.send(Command::StopMirror);
            }
            Message::Bridge(k, b) => {
                if b {
                    self.bridges.insert(k);
                } else {
                    self.bridges.remove(&k);
                }
                self.link.send(Command::SetSensorBridge(k, b));
            }
            Message::Rule(i, b) => {
                if let Some(r) = self.rules.get_mut(i) {
                    r.on = b;
                }
                self.link.send(Command::SetRule(i, b));
            }
            Message::LockAt(v) => self.lock_at = v.min(self.unlock_at - 5.0),
            Message::UnlockAt(v) => self.unlock_at = v.max(self.lock_at + 5.0),
            Message::Filter(f) => self.filter = f,
            Message::Query(q) => self.query = q,
            Message::Pause(p) => self.paused = p,
            Message::ClearJournal => self.packets.clear(),
            Message::Do(c) => self.more(c),
            Message::CamMode(m) => self.cam_mode = m,
            Message::CamRes(w, h) => {
                self.webcam_cfg.width = w;
                self.webcam_cfg.height = h;
            }
            Message::CamFps(f) => self.webcam_cfg.fps = f,
            Message::CamCodec(c) => self.webcam_cfg.codec = c,
            Message::WebcamStart => {
                self.webcam_on = true;
                self.more(Command2::StartWebcam(self.webcam_cfg));
            }
            Message::WebcamStop => {
                self.webcam_on = false;
                self.more(Command2::StopWebcam);
            }
            Message::DispatchInput(s) => self.dispatch_input = s,
            Message::DispatchRun => {
                let d = self.dispatch_input.trim().to_string();
                if !d.is_empty() {
                    self.more(Command2::RunDispatch(d));
                    self.dispatch_input.clear();
                }
            }
            Message::LowAlert(b) => {
                let a = BatteryAlerts {
                    low: b.then_some(self.alerts.low.unwrap_or(20)),
                    ..self.alerts
                };
                self.alerts = a;
                self.more(Command2::SetBatteryAlerts(a));
            }
            Message::LowLevel(v) => {
                let a = BatteryAlerts {
                    low: Some(v as u8),
                    ..self.alerts
                };
                self.alerts = a;
                self.more(Command2::SetBatteryAlerts(a));
            }
            Message::FullAlert(b) => {
                let a = BatteryAlerts {
                    full: b,
                    ..self.alerts
                };
                self.alerts = a;
                self.more(Command2::SetBatteryAlerts(a));
            }
            Message::Trackpad(c) => {
                self.trackpad = c;
                self.more(Command2::SetTrackpad(c));
            }
            Message::NotifApp(a) => self.notif_app = a,
            Message::NotifQuery(q) => self.notif_query = q,
            Message::ClipQuery(q) => self.clip_query = q,
            Message::SendPath(p) => self.send_path = p,
            Message::SendFile => {
                let p = self.send_path.trim().to_string();
                if !p.is_empty() {
                    self.more(Command2::SendFile(p));
                    self.send_path.clear();
                }
            }
            Message::FileDropped(path) => self.send_paths(vec![path]),
            Message::PickFiles => {
                if !file_chooser_portal_present() {
                    self.toast(no_picker().to_string());
                    return Task::none();
                }
                return Task::perform(pick_files(), Message::FilesPicked);
            }
            Message::FilesPicked(None) => {}
            Message::FilesPicked(Some(paths)) if paths.is_empty() => {
                self.toast(no_picker().to_string());
            }
            Message::FilesPicked(Some(paths)) => self.send_paths(paths),
            Message::RenameInput(id, s) => {
                self.rename_for = Some(id);
                self.rename_input = s;
            }
            Message::RenameSave(id) => {
                let n = self.rename_input.trim().to_string();
                self.more(Command2::RenameDevice(id, n));
                self.rename_for = None;
                self.rename_input.clear();
            }
            Message::PhoneUrl(s) => self.phone_url = s,
            Message::PhonePkg(s) => self.phone_pkg = s,
            Message::PhoneUrlOpen => {
                let url = self.phone_url.trim().to_string();
                if crate::link::http_url_ok(&url) {
                    self.phone_url.clear();
                    self.more(Command2::OpenOnPhone {
                        url: Some(url),
                        package: None,
                    });
                }
            }
            Message::PhonePkgOpen => {
                let package = self.phone_pkg.trim().to_string();
                if crate::link::package_ok(&package) {
                    self.phone_pkg.clear();
                    self.more(Command2::OpenOnPhone {
                        url: None,
                        package: Some(package),
                    });
                }
            }
            Message::ShortcutsEdit(on) => {
                self.sc_edit = on;
                self.sc_drafts.clear();
                self.sc_new = (String::new(), String::new());
            }
            Message::ShortcutName(i, s) => {
                let cur = self.shortcut_draft(i);
                self.sc_drafts.insert(i, (s, cur.1));
            }
            Message::ShortcutCommand(i, s) => {
                let cur = self.shortcut_draft(i);
                self.sc_drafts.insert(i, (cur.0, s));
            }
            Message::ShortcutSave(i) => {
                let (name, command) = self.shortcut_draft(i);
                if shortcut_valid(&name, &command) && i < self.shortcuts.len() {
                    let mut list = self.shortcuts.clone();
                    list[i] = Shortcut {
                        label: name.trim().into(),
                        dispatch: command.trim().into(),
                    };
                    self.sc_drafts.remove(&i);
                    self.more(Command2::SetShortcuts(list));
                }
            }
            Message::ShortcutRemove(i) => {
                if i < self.shortcuts.len() {
                    let mut list = self.shortcuts.clone();
                    list.remove(i);
                    self.sc_drafts.clear();
                    self.more(Command2::SetShortcuts(list));
                }
            }
            Message::ShortcutNewName(s) => self.sc_new.0 = s,
            Message::ShortcutNewCommand(s) => self.sc_new.1 = s,
            Message::ShortcutAdd => {
                let (name, command) = self.sc_new.clone();
                if shortcut_valid(&name, &command)
                    && self.shortcuts.len() < crate::link::SHORTCUTS_MAX
                {
                    let mut list = self.shortcuts.clone();
                    list.push(Shortcut {
                        label: name.trim().into(),
                        dispatch: command.trim().into(),
                    });
                    self.sc_new = (String::new(), String::new());
                    self.more(Command2::SetShortcuts(list));
                }
            }
            Message::Headset(b) => {
                self.speaker = b;
                self.mic_on = b;
                self.mic_clicked = Some(Instant::now());
                self.more(Command2::SetHeadset(b));
            }
            Message::ReplyInput(key, s) => {
                self.reply_drafts.insert(key, s);
            }
            Message::ReplySend(key, idx) => {
                let text = self.reply_drafts.get(&key).cloned().unwrap_or_default();
                if !text.trim().is_empty() {
                    self.reply_drafts.remove(&key);
                    self.more(Command2::ReplyNotification { key, idx, text });
                }
            }
            Message::DownloadsInput(s) => self.downloads_input = s,
            Message::DownloadsSave => {
                let d = self.downloads_input.trim().to_string();
                if !d.is_empty() {
                    self.more(Command2::SetDownloadsDir(d));
                }
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(33)).map(Message::Tick),
            keyboard::listen().map(Message::Key),
            window::close_events().map(Message::WindowClosed),
            iced::event::listen_with(|e, _, _| match e {
                iced::Event::Window(window::Event::FileDropped(p)) => Some(Message::FileDropped(p)),
                _ => None,
            }),
            tray::subscription(),
            hyprlink_gui::instance::subscription().map(|_| Message::Tray(tray::Action::Open)),
        ])
    }

    // ───────────────────────────── shell ─────────────────────────────

    pub fn view(&self, _window: window::Id) -> Element<'_, Message> {
        let page: El = match self.section {
            Section::Capa => views::cover(self),
            Section::Dispositivos => views::devices(self),
            Section::Secretaria => views::desk(self),
            Section::Camera => crate::pages::camera(self),
            Section::Audio => views::audio(self),
            Section::Notificacoes => crate::pages::notifications(self),
            Section::Partilha => crate::pages::share(self),
            Section::Multimedia => crate::pages::media(self),
            Section::Sensores => views::sensors(self),
            Section::Diario => views::journal(self),
            Section::Definicoes => crate::pages::settings(self),
        };

        let content = column![
            self.masthead(),
            rule(),
            scrollable(container(page).padding(Padding {
                top: space::GUTTER,
                right: space::GUTTER + 8.0,
                bottom: space::GUTTER + 16.0,
                left: space::GUTTER,
            }))
            .height(Length::Fill)
            .style(theme::scroll),
            rule(),
            self.ticker(),
        ];

        let shell = row![self.rail(), content].height(Length::Fill);

        let mut layers: Vec<El> = vec![
            container(shell)
                .style(theme::void)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ];
        if let Some(ticket) = &self.pairing {
            layers.push(views::pairing(self, ticket));
        }
        if !self.toasts.is_empty() {
            layers.push(self.toasts());
        }
        stack(layers).into()
    }

    fn rail(&self) -> El<'_> {
        let mark = column![
            headline("HYPR", 52.0),
            row![
                headline("LINK", 52.0).color(self.link_color()),
                hgap(6.0),
                square(self.link_color(), 11.0)
            ]
            .align_y(Alignment::End),
            gap(space::M),
            kicker(t("EDIÇÃO 0.1 — OUT 2026")),
            kicker("ANDROID ⇄ HYPRLAND"),
            gap(space::S),
            // Colour never carries meaning alone: the state is also spelled out.
            kicker_c(format!("■ {}", fmt::phase(self.phase)), self.link_color()),
        ];

        let mut nav = column![].spacing(2);
        for s in Section::ALL {
            let active = s == self.section;
            let hint: El = match s {
                Section::Dispositivos => kicker(format!("{}", self.devices.len())).into(),
                Section::Secretaria => kicker(format!("W{}", self.active_ws)).into(),
                Section::Camera if self.webcam.is_some() || self.mirror.is_some() => {
                    kicker_c(t("LIVE"), HOT).into()
                }
                Section::Notificacoes if !self.notifs.is_empty() => {
                    kicker_c(format!("{}", self.notifs.len()), ACID).into()
                }
                Section::Partilha
                    if self
                        .transfers
                        .iter()
                        .any(|t| t.state == link::TransferState::Active) =>
                {
                    kicker_c("⇅", ACID).into()
                }
                Section::Multimedia if self.players.iter().any(|p| p.playing) => {
                    kicker_c("▶", ACID).into()
                }
                Section::Audio if self.mic_on || self.tap_on => kicker_c(
                    if self.mic_on && self.tap_on {
                        "⇅"
                    } else if self.mic_on {
                        "MIC"
                    } else {
                        "TAP"
                    },
                    ACID,
                )
                .into(),
                Section::Sensores => kicker(if self.rssi_known {
                    format!("{:.0}", self.rssi)
                } else {
                    "—".to_string()
                })
                .into(),
                Section::Diario => kicker(format!("{}", self.packets.len())).into(),
                _ => kicker("").into(),
            };
            let entry = row![
                container(iced::widget::Space::new().width(2).height(18))
                    .style(theme::fill(if active { ACID } else { Color::TRANSPARENT })),
                hgap(space::M),
                ui::t(
                    s.num(),
                    MONO_MEDIUM,
                    11.0,
                    if active { ACID } else { FAINT }
                ),
                hgap(space::M),
                ui::t(
                    s.title(),
                    if active { SANS_SEMI } else { SANS_MEDIUM },
                    14.0,
                    if active { PAPER } else { SUB }
                ),
                fill_x(),
                hint,
            ]
            .align_y(Alignment::Center);
            nav = nav.push(
                button(entry)
                    .width(Length::Fill)
                    .padding(Padding {
                        top: 7.0,
                        right: space::L,
                        bottom: 7.0,
                        left: 0.0,
                    })
                    .style(theme::nav(active))
                    .on_press(Message::Nav(s)),
            );
        }
        let settings_active = self.section == Section::Definicoes;
        let settings = button(
            row![
                container(iced::widget::Space::new().width(2).height(18)).style(theme::fill(
                    if settings_active {
                        ACID
                    } else {
                        Color::TRANSPARENT
                    }
                )),
                hgap(space::M),
                ui::t(
                    "00",
                    MONO_MEDIUM,
                    11.0,
                    if settings_active { ACID } else { FAINT }
                ),
                hgap(space::M),
                ui::t(
                    t("Definições"),
                    if settings_active {
                        SANS_SEMI
                    } else {
                        SANS_MEDIUM
                    },
                    13.0,
                    if settings_active { PAPER } else { MUTED }
                ),
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding {
            top: 7.0,
            right: space::L,
            bottom: 7.0,
            left: 0.0,
        })
        .style(theme::nav(settings_active))
        .on_press(Message::Nav(Section::Definicoes));

        // Version and uptime come from the daemon (`Settings`); "—" until
        // the first `Settings` arrives or when the daemon is older.
        let started = self.settings.as_ref().map_or(0, |s| s.started_unix);
        let version = self
            .settings
            .as_ref()
            .map_or("—".to_string(), |s| s.daemon_version.clone());
        let uptime = if started == 0 {
            tr!("uptime {}", "—")
        } else {
            let up = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
                .saturating_sub(started);
            let hms = format!("{:02}:{:02}:{:02}", up / 3600, (up / 60) % 60, up % 60);
            tr!("uptime {}", hms)
        };
        let daemon = column![
            rule(),
            gap(space::L),
            row![
                kicker(t("DAEMON")),
                fill_x(),
                row![square(ACID, 6.0), hgap(6.0), kicker_c(t("A ESCUTAR"), ACID)]
                    .align_y(Alignment::Center)
            ]
            .align_y(Alignment::Center),
            gap(space::S),
            ui::mono(format!("hyprlinkd {version}"), PAPER),
            ui::mono("udp/7443 · quic · mtls", MUTED),
            ui::mono(uptime, MUTED),
        ]
        .padding(Padding {
            top: 0.0,
            right: space::XL,
            bottom: space::XL,
            left: space::XL,
        });

        container(
            column![
                container(mark).padding(Padding {
                    top: space::XXL,
                    right: space::XL,
                    bottom: space::XL,
                    left: space::XL
                }),
                rule(),
                gap(space::L),
                nav,
                iced::widget::space::vertical(),
                settings,
                gap(space::S),
                daemon,
            ]
            .height(Length::Fill),
        )
        .width(252)
        .height(Length::Fill)
        .style(theme::rail)
        .into()
    }

    fn masthead(&self) -> El<'_> {
        let now = chrono::Local::now();
        let dev: El = match self.primary() {
            Some(d) => row![
                square(self.link_color(), 7.0),
                hgap(8.0),
                ui::t(fmt::name(d), MONO_SEMI, 11.5, PAPER),
                hgap(space::M),
                ui::mono(format!("{} ms", fmt::latency(d.latency_ms)), SUB),
                hgap(space::M),
                ui::mono(
                    format!("{}%{}", fmt::battery(d), if d.charging { "+" } else { "" }),
                    SUB
                ),
                hgap(space::M),
                ui::mono(
                    if self.rssi_known {
                        format!("{:.0} dBm", self.rssi)
                    } else {
                        "— dBm".to_string()
                    },
                    SUB,
                ),
            ]
            .align_y(Alignment::Center)
            .into(),
            None => row![
                square(self.link_color(), 7.0),
                hgap(8.0),
                ui::mono(
                    match self.phase {
                        link::LinkPhase::Connecting => t("A LIGAR AO TELEMÓVEL…"),
                        _ => t("SEM LIGAÇÃO"),
                    },
                    self.link_color()
                )
            ]
            .align_y(Alignment::Center)
            .into(),
        };
        container(
            row![
                kicker("HYPRLINK"),
                hgap(space::S),
                ui::t("/", MONO, 11.5, FAINT),
                hgap(space::S),
                ui::t(self.section.title(), SANS_SEMI, 13.0, PAPER),
                hgap(space::L),
                kicker(t("TECLAS 1–9 · 0 PARA NAVEGAR")).color(FAINT),
                fill_x(),
                dev,
                hgap(space::XL),
                vrule(),
                hgap(space::XL),
                ui::t(now.format("%H:%M:%S").to_string(), MONO_MEDIUM, 11.5, PAPER),
                hgap(6.0),
                kicker("WEST"),
            ]
            .align_y(Alignment::Center)
            .height(Length::Fill),
        )
        .height(52)
        .padding(Padding::from([0.0, space::GUTTER]))
        .into()
    }

    fn ticker(&self) -> El<'_> {
        let mut items: Vec<(String, Color)> = vec![
            ("HYPRLINK".into(), PAPER),
            (
                tr!(
                    "LATÊNCIA {} MS",
                    fmt::decimal(self.latency.last() as f64, 1)
                ),
                SUB,
            ),
            (
                format!(
                    "↑ {:.0} KB/S  ↓ {:.0} KB/S",
                    self.up.last() / 8.0,
                    self.down.last() / 8.0
                ),
                SUB,
            ),
            (tr!("WORKSPACE {}", format!("{:02}", self.active_ws)), SUB),
        ];
        for p in self.packets.iter().rev().take(4) {
            items.push((
                format!(
                    "{} {}",
                    if p.dir == Dir::Tx { "TX" } else { "RX" },
                    p.kind.to_uppercase()
                ),
                if p.dir == Dir::Tx { ACID } else { COLD },
            ));
        }
        row![
            container(kicker_c(t("AO VIVO"), VOID))
                .padding(Padding::from([0, 10]))
                .height(Length::Fill)
                .align_y(Alignment::Center)
                .style(theme::hot_block),
            container(
                canvas(Ticker { t: self.t, items })
                    .width(Length::Fill)
                    .height(Length::Fill)
            )
            .clip(true)
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .height(28)
        .into()
    }

    fn toasts(&self) -> El<'_> {
        let mut col = column![].spacing(space::S).align_x(Alignment::End);
        for (s, born) in &self.toasts {
            let age = self.t - born;
            let a = if age < 0.15 {
                age / 0.15
            } else if age > 3.4 {
                ((4.0 - age) / 0.6).max(0.0)
            } else {
                1.0
            };
            col = col.push(
                container(
                    row![
                        square(alpha(ACID, a), 7.0),
                        hgap(space::M),
                        text(s.as_str())
                            .font(MONO_SEMI)
                            .size(11.5)
                            .color(alpha(PAPER, a)),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding(Padding::from([12, 16]))
                .style(move |th| {
                    let mut s = theme::toast(th);
                    s.background = Some(alpha(INK_1, a).into());
                    s.border.color = alpha(ACID, 0.6 * a);
                    s
                }),
            );
        }
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .padding(Padding {
                top: 0.0,
                right: space::GUTTER,
                bottom: 28.0 + space::XL,
                left: 0.0,
            })
            .into()
    }
}

use iced::Color;

fn no_picker() -> &'static str {
    t("Sem seletor de ficheiros — instala xdg-desktop-portal-gtk, ou larga o ficheiro aqui.")
}

/// Caminhos a enviar e número de pastas deixadas de fora.
#[derive(Debug, PartialEq, Eq)]
struct SendPlan {
    files: Vec<String>,
    folders: usize,
}

/// Ignora diretórios, mantém a ordem e não deduplica (o mesmo ficheiro duas
/// vezes envia-se duas vezes).
fn plan_send(paths: Vec<std::path::PathBuf>) -> SendPlan {
    let mut plan = SendPlan {
        files: Vec::new(),
        folders: 0,
    };
    for p in paths {
        if p.is_dir() {
            plan.folders += 1;
        } else {
            plan.files.push(p.display().to_string());
        }
    }
    plan
}

/// Há algum backend de portal que declare `FileChooser`? (`gtk.portal`,
/// `kde.portal`…; o `hyprland.portal` não declara.) O `rfd` devolve `None`
/// tanto ao cancelar como quando não há portal, por isso verifica-se antes.
fn file_chooser_portal_present() -> bool {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let data = if data.is_empty() {
        "/usr/local/share:/usr/share".to_string()
    } else {
        data
    };
    for d in data.split(':').filter(|d| !d.is_empty()) {
        dirs.push(std::path::Path::new(d).join("xdg-desktop-portal/portals"));
    }
    dirs.iter().any(|d| {
        std::fs::read_dir(d).is_ok_and(|rd| {
            rd.flatten().any(|e| {
                std::fs::read_to_string(e.path())
                    .is_ok_and(|c| c.contains("org.freedesktop.impl.portal.FileChooser"))
            })
        })
    })
}

/// Pasta inicial do seletor: Transferências/Documentos se existirem.
fn picker_start_dir() -> Option<std::path::PathBuf> {
    let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
    ["Transferências", "Downloads", "Documentos", "Documents"]
        .iter()
        .map(|d| home.join(d))
        .find(|d| d.is_dir())
}

/// Seletor por portal, vários ficheiros, sem filtro. Cancelar dá `None`; um
/// portal que falha logo ao abrir (< 300 ms, impossível para uma pessoa a
/// escolher e cancelar) dá `Some(vazio)` para a GUI avisar.
async fn pick_files() -> Option<Vec<std::path::PathBuf>> {
    let t0 = Instant::now();
    let mut dlg = rfd::AsyncFileDialog::new().set_title(t("Escolher ficheiros para o telemóvel"));
    if let Some(d) = picker_start_dir() {
        dlg = dlg.set_directory(d);
    }
    match dlg.pick_files().await {
        Some(v) => Some(v.into_iter().map(|f| f.path().to_path_buf()).collect()),
        None if t0.elapsed() < Duration::from_millis(300) => Some(Vec::new()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn plan_send_skips_folders_keeps_order_and_repeats() {
        let dir = std::env::temp_dir().join(format!("hyprlink-plan-{}", std::process::id()));
        let sub = dir.join("pasta");
        std::fs::create_dir_all(&sub).unwrap();
        let a = dir.join("a.wav");
        let b = dir.join("b.bin");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();

        let plan = plan_send(vec![b.clone(), sub.clone(), a.clone(), b.clone()]);
        let s = |p: &PathBuf| p.display().to_string();
        assert_eq!(plan.files, vec![s(&b), s(&a), s(&b)]);
        assert_eq!(plan.folders, 1);

        let none = plan_send(Vec::new());
        assert_eq!((none.files.len(), none.folders), (0, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Mesmos limites do daemon (`config::validate_shortcuts`): nada vazio, nome
/// até 40 e comando até 200 caracteres.
pub fn shortcut_valid(name: &str, command: &str) -> bool {
    let (n, c) = (name.trim(), command.trim());
    !n.is_empty()
        && !c.is_empty()
        && n.chars().count() <= crate::link::SHORTCUT_NAME_MAX
        && c.chars().count() <= crate::link::SHORTCUT_COMMAND_MAX
}
