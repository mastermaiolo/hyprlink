use crate::graphics::Ticker;
use crate::host::{Host, Probe};
use crate::link::{
    self, Codec, Command, DeviceId, Dir, Event, MirrorConfig, MirrorStats, Packet, PairingTicket,
    SensorKind, Sensors, Transport, Workspace,
};
use crate::theme::{self, *};
use crate::tray;
use crate::ui::{self, *};
use crate::views;
use hyprlink_gui::fmt;

use iced::keyboard::{self, Key};
use iced::widget::{button, canvas, column, container, qr_code, row, scrollable, stack, text};
use iced::window;
use iced::{Alignment, Element, Length, Padding, Subscription, Task};
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Capa,
    Dispositivos,
    Secretaria,
    Espelho,
    Audio,
    Sensores,
    Presenca,
    Diario,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Capa,
        Section::Dispositivos,
        Section::Secretaria,
        Section::Espelho,
        Section::Audio,
        Section::Sensores,
        Section::Presenca,
        Section::Diario,
    ];

    pub fn num(self) -> &'static str {
        match self {
            Section::Capa => "01",
            Section::Dispositivos => "02",
            Section::Secretaria => "03",
            Section::Espelho => "04",
            Section::Audio => "05",
            Section::Sensores => "06",
            Section::Presenca => "07",
            Section::Diario => "08",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Capa => "Capa",
            Section::Dispositivos => "Dispositivos",
            Section::Secretaria => "Secretária",
            Section::Espelho => "Espelho",
            Section::Audio => "Áudio",
            Section::Sensores => "Sensores",
            Section::Presenca => "Presença",
            Section::Diario => "Diário",
        }
    }
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

pub struct Gesture {
    pub gesture: &'static str,
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
    pub gestures: Vec<Gesture>,

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
    /// Tap source is the `hyprlink-speaker` sink (phone as PC output).
    pub speaker: bool,
    pub tap_gain: f32,
    pub mic_hist: History,
    pub tap_hist: History,

    pub mirror_cfg: MirrorConfig,
    pub mirror: Option<MirrorStats>,
    pub mirror_on: bool,

    pub rssi: f32,
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
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let start = std::env::var("HYPRLINK_SECTION")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|n| Section::ALL.get(n.saturating_sub(1)).copied())
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
            gestures: vec![
                Gesture {
                    gesture: "Deslizar ←  (3 dedos)",
                    action: "workspace e-1",
                    on: true,
                },
                Gesture {
                    gesture: "Deslizar →  (3 dedos)",
                    action: "workspace e+1",
                    on: true,
                },
                Gesture {
                    gesture: "Toque duplo no verso",
                    action: "togglespecialworkspace",
                    on: true,
                },
                Gesture {
                    gesture: "Rodar para horizontal",
                    action: "fullscreen 1",
                    on: false,
                },
                Gesture {
                    gesture: "Volume + / −",
                    action: "wpctl set-volume @DEFAULT_SINK@",
                    on: true,
                },
            ],
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
            mic_on: true,
            tap_on: false,
            mic: 0.0,
            tap: 0.0,
            mic_peak: 0.0,
            tap_peak: 0.0,
            mic_gain: 0.72,
            mic_measured: false,
            tap_measured: false,
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
                // `None` = the daemon does not measure that channel (yet).
                self.mic_measured = mic.is_some();
                self.tap_measured = tap.is_some();
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
            Event::Notice(n) => {
                let s = fmt::notice(&n, |id| {
                    self.devices.iter().find(|d| d.id == id).map(fmt::name)
                });
                self.toasts.push((s, self.t));
                if self.toasts.len() > 3 {
                    self.toasts.remove(0);
                }
            }
        }
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
                            if (1..=8).contains(&n) {
                                self.section = Section::ALL[n - 1];
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
            Message::Gesture(i, b) => {
                if let Some(g) = self.gestures.get_mut(i) {
                    g.on = b;
                }
            }
            Message::Mic(b) => {
                self.mic_on = b;
                self.link.send(Command::SetMic(b));
            }
            Message::Speaker(b) => {
                self.speaker = b;
                self.link.send(Command::SetSpeakerMode(b));
            }
            Message::Tap(b) => {
                self.tap_on = b;
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
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(33)).map(Message::Tick),
            keyboard::listen().map(Message::Key),
            window::close_events().map(Message::WindowClosed),
            tray::subscription(),
        ])
    }

    // ───────────────────────────── shell ─────────────────────────────

    pub fn view(&self, _window: window::Id) -> Element<'_, Message> {
        let page: El = match self.section {
            Section::Capa => views::cover(self),
            Section::Dispositivos => views::devices(self),
            Section::Secretaria => views::desk(self),
            Section::Espelho => views::mirror(self),
            Section::Audio => views::audio(self),
            Section::Sensores => views::sensors(self),
            Section::Presenca => views::presence(self),
            Section::Diario => views::journal(self),
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
            row![headline("LINK", 52.0), hgap(6.0), square(ACID, 11.0)].align_y(Alignment::End),
            gap(space::M),
            kicker("EDIÇÃO 0.1 — OUT 2026"),
            kicker("ANDROID ⇄ HYPRLAND"),
        ];

        let mut nav = column![].spacing(2);
        for s in Section::ALL {
            let active = s == self.section;
            let hint: El = match s {
                Section::Dispositivos => kicker(format!("{}", self.devices.len())).into(),
                Section::Secretaria => kicker(format!("W{}", self.active_ws)).into(),
                Section::Espelho if self.mirror.is_some() => kicker_c("LIVE", HOT).into(),
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
                Section::Presenca => kicker(format!("{:.0}", self.rssi)).into(),
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
                        top: 9.0,
                        right: space::L,
                        bottom: 9.0,
                        left: 0.0,
                    })
                    .style(theme::nav(active))
                    .on_press(Message::Nav(s)),
            );
        }

        let up = self.t as u64 + 3 * 3600 + 17 * 60;
        let daemon = column![
            rule(),
            gap(space::L),
            row![
                kicker("DAEMON"),
                fill_x(),
                row![square(ACID, 6.0), hgap(6.0), kicker_c("A ESCUTAR", ACID)]
                    .align_y(Alignment::Center)
            ]
            .align_y(Alignment::Center),
            gap(space::S),
            ui::mono("hyprlinkd 0.4.2", PAPER),
            ui::mono("udp/7443 · quic · mtls", MUTED),
            ui::mono(
                format!(
                    "uptime {:02}:{:02}:{:02}",
                    up / 3600,
                    (up / 60) % 60,
                    up % 60
                ),
                MUTED
            ),
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
                square(ACID, 7.0),
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
                ui::mono(format!("{:.0} dBm", self.rssi), SUB),
            ]
            .align_y(Alignment::Center)
            .into(),
            None => row![square(HOT, 7.0), hgap(8.0), ui::mono("SEM LIGAÇÃO", HOT)]
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
                kicker("TECLAS 1–8 PARA NAVEGAR").color(FAINT),
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
            (format!("LATÊNCIA {:.1} MS", self.latency.last()), SUB),
            (
                format!(
                    "↑ {:.0} KB/S  ↓ {:.0} KB/S",
                    self.up.last() / 8.0,
                    self.down.last() / 8.0
                ),
                SUB,
            ),
            (format!("WORKSPACE {:02}", self.active_ws), SUB),
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
            container(kicker_c("AO VIVO", VOID))
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
