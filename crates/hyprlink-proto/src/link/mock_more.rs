//! Mock data for the pages added after Fase 0: camera, phone volumes, PC
//! mixer, notifications, clipboard, files, MPRIS, shortcuts, trackpad,
//! battery history, settings. Plausible, deterministic, responsive.

use super::*;

pub struct More {
    booted: bool,
    acc: f32,
    now: u64,
    alerts: BatteryAlerts,
    shortcuts: Vec<Shortcut>,
    trackpad: TrackpadConfig,
    webcam: Option<WebcamConfig>,
    audio: PhoneAudio,
    sinks: Vec<Sink>,
    apps: Vec<AppStream>,
    notifs: Vec<PhoneNotification>,
    clips: Vec<ClipEntry>,
    transfers: Vec<Transfer>,
    players: Vec<Player>,
    settings: Settings,
    next_id: u64,
    active_ws: u8,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1_791_100_000)
}

impl More {
    pub fn new() -> Self {
        let now = now_unix();
        let n =
            |key: &str, app: &str, title: &str, text: Option<&str>, ago: u64| PhoneNotification {
                key: key.into(),
                app: app.into(),
                title: title.into(),
                text: text.map(Into::into),
                at: now - ago,
            };
        let c = |id: u64, origin: Origin, text: &str, ago: u64, pinned: bool| ClipEntry {
            id,
            origin,
            mime: "text/plain".into(),
            text: Some(text.into()),
            bytes: text.len() as u64,
            at: now - ago,
            pinned,
        };
        Self {
            booted: false,
            acc: 0.0,
            now,
            alerts: BatteryAlerts {
                low: Some(20),
                full: true,
            },
            shortcuts: vec![
                Shortcut {
                    label: "Terminal".into(),
                    dispatch: "exec kitty".into(),
                },
                Shortcut {
                    label: "Bloquear".into(),
                    dispatch: "exec hyprlock".into(),
                },
                Shortcut {
                    label: "Ecrã inteiro".into(),
                    dispatch: "fullscreen 1".into(),
                },
                Shortcut {
                    label: "Scratchpad".into(),
                    dispatch: "togglespecialworkspace".into(),
                },
                Shortcut {
                    label: "Captura".into(),
                    dispatch: "exec grimblast copy area".into(),
                },
                Shortcut {
                    label: "Reaper".into(),
                    dispatch: "workspace 4".into(),
                },
            ],
            trackpad: TrackpadConfig {
                sensitivity: 1.4,
                scroll: 1.0,
                acceleration: true,
                natural_scroll: true,
                keyboard: true,
            },
            webcam: None,
            audio: PhoneAudio {
                streams: vec![
                    StreamLevel {
                        stream: PhoneStream::Media,
                        level: 9,
                        max: 15,
                    },
                    StreamLevel {
                        stream: PhoneStream::Ring,
                        level: 5,
                        max: 7,
                    },
                    StreamLevel {
                        stream: PhoneStream::Notification,
                        level: 4,
                        max: 7,
                    },
                    StreamLevel {
                        stream: PhoneStream::Alarm,
                        level: 6,
                        max: 7,
                    },
                ],
                ringer: Ringer::Vibrate,
                dnd: false,
            },
            sinks: vec![
                Sink {
                    id: 48,
                    name: "alsa_output.pci-0000_04_00.6.analog-stereo".into(),
                    description: "Altifalantes internos".into(),
                    volume: 62,
                    muted: false,
                    default: true,
                },
                Sink {
                    id: 71,
                    name: "bluez_output.AC_80_0A_12_9F_31.1".into(),
                    description: "Sony ULT WEAR".into(),
                    volume: 45,
                    muted: false,
                    default: false,
                },
                Sink {
                    id: 93,
                    name: "hyprlink-speaker".into(),
                    description: "HyprLink — telemóvel".into(),
                    volume: 100,
                    muted: false,
                    default: false,
                },
            ],
            apps: vec![
                AppStream {
                    id: 112,
                    app: "Spotify".into(),
                    volume: 80,
                    muted: false,
                    sink: 48,
                },
                AppStream {
                    id: 118,
                    app: "Zen Browser".into(),
                    volume: 100,
                    muted: false,
                    sink: 48,
                },
                AppStream {
                    id: 131,
                    app: "REAPER".into(),
                    volume: 115,
                    muted: false,
                    sink: 48,
                },
                AppStream {
                    id: 140,
                    app: "Telegram".into(),
                    volume: 70,
                    muted: true,
                    sink: 48,
                },
            ],
            notifs: vec![
                n(
                    "0|org.thoughtcrime.securesms|3",
                    "Signal",
                    "Rita",
                    Some("Já chegaste? Estou à porta do Maus Hábitos."),
                    120,
                ),
                n(
                    "0|com.google.android.gm|11",
                    "Gmail",
                    "GitHub",
                    Some("[HyprLink] PR #42: ipc: socket Unix + CBOR"),
                    900,
                ),
                n(
                    "0|org.telegram.messenger|7",
                    "Telegram",
                    "HEISHI.食.ARCHON",
                    Some("Novo mix enviado: OMNIS v3 — 4:12"),
                    2400,
                ),
                n(
                    "0|pt.ctt.outsystems.ctt|2",
                    "CTT",
                    "Encomenda a caminho",
                    None,
                    5400,
                ),
                n(
                    "0|com.google.android.calendar|1",
                    "Calendário",
                    "Ensaio às 21:00",
                    Some("Estúdio · 2 h"),
                    7200,
                ),
            ],
            clips: vec![
                c(
                    9,
                    Origin::Phone,
                    "https://docs.noctalia.dev/noctalia/plugins/development/runtime-api/",
                    300,
                    false,
                ),
                c(
                    8,
                    Origin::Pc,
                    "windowrulev2 = float, class:^(hyprlink-mirror)$",
                    1500,
                    true,
                ),
                c(
                    7,
                    Origin::Phone,
                    "Rua de Passos Manuel 178, 4000-382 Porto",
                    4000,
                    false,
                ),
                c(
                    6,
                    Origin::Pc,
                    "cargo install --path ~/Projectos/iced/hyprlink-gui",
                    6000,
                    false,
                ),
                c(
                    5,
                    Origin::Pc,
                    "9f3a c21e 77b0 4d19 e8a2 0c5f b913 6a7d",
                    9000,
                    true,
                ),
            ],
            transfers: vec![
                Transfer {
                    id: 3,
                    name: "IMG_20261004_183245.jpg".into(),
                    dir: Dir::Rx,
                    bytes: 4_812_334,
                    done: 4_812_334,
                    state: TransferState::Done,
                    at: now - 1800,
                },
                Transfer {
                    id: 2,
                    name: "OMNIS_v3_master.wav".into(),
                    dir: Dir::Tx,
                    bytes: 52_430_112,
                    done: 52_430_112,
                    state: TransferState::Done,
                    at: now - 7400,
                },
                Transfer {
                    id: 1,
                    name: "contrato.pdf".into(),
                    dir: Dir::Rx,
                    bytes: 1_204_550,
                    done: 380_000,
                    state: TransferState::Failed,
                    at: now - 86_000,
                },
            ],
            players: vec![
                Player {
                    id: "spotify".into(),
                    identity: "Spotify".into(),
                    title: Some("Glass Weather".into()),
                    artist: Some("Fieldnotes".into()),
                    playing: false,
                    position_ms: Some(83_000),
                    duration_ms: Some(214_000),
                },
                Player {
                    id: "firefox.instance_1_52".into(),
                    identity: "Zen Browser".into(),
                    title: Some("Hyprland 0.51 — what's new".into()),
                    artist: Some("Vaxry".into()),
                    playing: true,
                    position_ms: Some(412_000),
                    duration_ms: Some(1_386_000),
                },
            ],
            settings: Settings {
                downloads_dir: "~/Transferências/HyprLink".into(),
                daemon_version: env!("CARGO_PKG_VERSION").into(),
                socket: "$XDG_RUNTIME_DIR/hyprlink.sock".into(),
                // A believable uptime for design captures: 3 h 17 min ago.
                started_unix: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs())
                    .saturating_sub(3 * 3600 + 17 * 60),
            },
            next_id: 100,
            active_ws: 3,
        }
    }

    fn more(out: &mut Vec<Event>, e: Event2) {
        out.push(Event::More(e));
    }

    fn packet(
        out: &mut Vec<Event>,
        t: f32,
        dir: Dir,
        kind: &str,
        bytes: usize,
        note: impl Into<String>,
    ) {
        out.push(Event::Packet(Packet {
            at: t,
            dir,
            kind: kind.into(),
            bytes,
            note: note.into(),
        }));
    }

    fn history(&self, battery: u8) -> Vec<BatteryPoint> {
        // 12 h, one point every 10 min: overnight charge, then a working day.
        // 12 h, one point every 10 min: an evening discharge, a charge,
        // then the morning drain down to the current level.
        (0..72)
            .map(|i| {
                let h = i as f32 / 6.0; // 0 = 12 h ago … 12 = now
                let level = if h < 3.0 {
                    64.0 - h * 8.5
                } else if h < 5.5 {
                    38.5 + (h - 3.0) * 24.6
                } else if h < 6.5 {
                    100.0
                } else {
                    100.0 - (h - 6.5) / 5.5 * (100.0 - battery as f32)
                };
                let level = (level + (i as f32 * 1.7).sin() * 0.8).clamp(5.0, 100.0);
                BatteryPoint {
                    at: self.now - (72 - i) as u64 * 600,
                    level: level as u8,
                }
            })
            .collect()
    }

    pub fn boot(&mut self, out: &mut Vec<Event>, battery: u8) {
        if self.booted {
            return;
        }
        self.booted = true;
        let mut hist = self.history(battery);
        // Make the curve end exactly at the current level.
        if let Some(last) = hist.last_mut() {
            last.level = battery;
        }
        Self::more(out, Event2::BatteryHistory(hist));
        Self::more(out, Event2::BatteryAlerts(self.alerts));
        Self::more(out, Event2::Shortcuts(self.shortcuts.clone()));
        Self::more(out, Event2::Trackpad(self.trackpad));
        Self::more(out, Event2::PhoneAudio(self.audio.clone()));
        Self::more(
            out,
            Event2::Mixer {
                sinks: self.sinks.clone(),
                apps: self.apps.clone(),
            },
        );
        Self::more(out, Event2::Notifications(self.notifs.clone()));
        Self::more(out, Event2::Clipboard(self.clips.clone()));
        Self::more(out, Event2::Transfers(self.transfers.clone()));
        Self::more(out, Event2::Players(self.players.clone()));
        Self::more(out, Event2::Settings(self.settings.clone()));
        Self::more(out, Event2::Webcam(None));
        self.active_window(out);
    }

    fn active_window(&self, out: &mut Vec<Event>) {
        let (class, title) = match self.active_ws {
            1 => ("kitty", "~/Projectos/iced/hyprlink-gui — nvim"),
            2 => ("zen", "Noctalia — Runtime API"),
            3 => ("code", "views.rs — hyprlink-gui"),
            4 => ("REAPER", "OMNIS_v3.rpp — REAPER"),
            5 => ("obsidian", "HyprLink — roadmap"),
            7 => ("Spotify", "Spotify Premium"),
            9 => ("org.telegram.desktop", "Telegram"),
            _ => ("", ""),
        };
        Self::more(
            out,
            Event2::ActiveWindow((!class.is_empty()).then(|| ActiveWindow {
                class: class.into(),
                title: title.into(),
                workspace: self.active_ws,
            })),
        );
    }

    pub fn on_workspace(&mut self, ws: u8, out: &mut Vec<Event>) {
        self.active_ws = ws;
        self.active_window(out);
    }

    pub fn send(&mut self, cmd: Command2, out: &mut Vec<Event>, t: f32) {
        use packets as p;
        match cmd {
            // Intercepado no mock.rs (composição SetMic + SetSpeakerMode,
            // ver `Command2::SetHeadset` lá) — aqui nunca chega, mas o
            // match tem de cobrir a variante.
            Command2::SetHeadset(_) => {}
            Command2::SetBatteryAlerts(a) => {
                self.alerts = a;
                Self::more(out, Event2::BatteryAlerts(a));
            }
            Command2::RunDispatch(d) => {
                Self::packet(out, t, Dir::Tx, p::HYPR_DISPATCH, 64, d);
            }
            Command2::SetShortcuts(s) => {
                self.shortcuts = s.clone();
                Self::more(out, Event2::Shortcuts(s));
            }
            Command2::SetTrackpad(c) => {
                self.trackpad = c;
                Self::more(out, Event2::Trackpad(c));
                Self::packet(
                    out,
                    t,
                    Dir::Tx,
                    p::INPUT_CONFIG,
                    40,
                    format!("sens {:.2} scroll {:.2}", c.sensitivity, c.scroll),
                );
            }
            Command2::StartWebcam(c) => {
                self.webcam = Some(c);
                Self::packet(
                    out,
                    t,
                    Dir::Tx,
                    p::WEBCAM_START,
                    48,
                    format!("{}x{} {}fps {:?}", c.width, c.height, c.fps, c.codec).to_lowercase(),
                );
            }
            Command2::StopWebcam => {
                self.webcam = None;
                Self::packet(out, t, Dir::Tx, p::WEBCAM_STOP, 24, "");
                Self::more(out, Event2::Webcam(None));
            }
            Command2::TestNetwork => {
                Self::packet(out, t, Dir::Tx, p::WEBCAM_NETTEST, 24, "8 MB");
                Self::more(
                    out,
                    Event2::NetTest(NetTest {
                        mbps: 86.4,
                        rtt_ms: 7.8,
                        loss_pct: 0.2,
                    }),
                );
            }
            Command2::SetPhoneVolume(stream, level) => {
                if let Some(s) = self.audio.streams.iter_mut().find(|s| s.stream == stream) {
                    s.level = level.min(s.max);
                }
                Self::packet(
                    out,
                    t,
                    Dir::Tx,
                    p::PHONE_VOLUME,
                    32,
                    format!("{stream:?} {level}").to_lowercase(),
                );
                Self::more(out, Event2::PhoneAudio(self.audio.clone()));
            }
            Command2::SetRinger(r) => {
                self.audio.ringer = r;
                Self::packet(
                    out,
                    t,
                    Dir::Tx,
                    p::PHONE_RINGER,
                    24,
                    format!("{r:?}").to_lowercase(),
                );
                Self::more(out, Event2::PhoneAudio(self.audio.clone()));
            }
            Command2::SetDnd(b) => {
                self.audio.dnd = b;
                Self::packet(out, t, Dir::Tx, p::PHONE_DND, 24, b.to_string());
                Self::more(out, Event2::PhoneAudio(self.audio.clone()));
            }
            Command2::SetSinkVolume(id, v) => {
                if let Some(s) = self.sinks.iter_mut().find(|s| s.id == id) {
                    s.volume = v.min(150);
                }
                self.mixer(out);
            }
            Command2::SetSinkMute(id, m) => {
                if let Some(s) = self.sinks.iter_mut().find(|s| s.id == id) {
                    s.muted = m;
                }
                self.mixer(out);
            }
            Command2::SetDefaultSink(id) => {
                for s in &mut self.sinks {
                    s.default = s.id == id;
                }
                self.mixer(out);
            }
            Command2::SetAppVolume(id, v) => {
                if let Some(a) = self.apps.iter_mut().find(|a| a.id == id) {
                    a.volume = v.min(150);
                }
                self.mixer(out);
            }
            Command2::SetAppMute(id, m) => {
                if let Some(a) = self.apps.iter_mut().find(|a| a.id == id) {
                    a.muted = m;
                }
                self.mixer(out);
            }
            Command2::DismissNotification(key) => {
                self.notifs.retain(|n| n.key != key);
                Self::packet(out, t, Dir::Tx, p::NOTIF_DISMISS, 48, key);
                Self::more(out, Event2::Notifications(self.notifs.clone()));
            }
            Command2::DismissAllNotifications => {
                self.notifs.clear();
                Self::packet(out, t, Dir::Tx, p::NOTIF_DISMISS, 24, "all");
                Self::more(out, Event2::Notifications(Vec::new()));
            }
            Command2::CopyClip(id) => {
                if let Some(c) = self.clips.iter_mut().find(|c| c.id == id) {
                    c.at = now_unix();
                }
                self.clips.sort_by(|a, b| b.at.cmp(&a.at));
                Self::more(out, Event2::Clipboard(self.clips.clone()));
            }
            Command2::SendClipToPhone(id) => {
                let bytes = self
                    .clips
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.bytes)
                    .unwrap_or(0);
                Self::packet(
                    out,
                    t,
                    Dir::Tx,
                    p::CLIPBOARD_SET,
                    bytes as usize + 16,
                    "text/plain",
                );
            }
            Command2::PinClip(id, b) => {
                if let Some(c) = self.clips.iter_mut().find(|c| c.id == id) {
                    c.pinned = b;
                }
                Self::more(out, Event2::Clipboard(self.clips.clone()));
            }
            Command2::DeleteClip(id) => {
                self.clips.retain(|c| c.id != id);
                Self::more(out, Event2::Clipboard(self.clips.clone()));
            }
            Command2::SendFile(path) => {
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                self.next_id += 1;
                self.transfers.insert(
                    0,
                    Transfer {
                        id: self.next_id,
                        name: name.clone(),
                        dir: Dir::Tx,
                        bytes: 286_400_000,
                        done: 0,
                        state: TransferState::Active,
                        at: now_unix(),
                    },
                );
                Self::packet(out, t, Dir::Tx, p::SHARE_OFFER, 96, name);
                Self::more(out, Event2::Transfers(self.transfers.clone()));
            }
            Command2::CancelTransfer(id) => {
                if let Some(tr) = self.transfers.iter_mut().find(|x| x.id == id) {
                    tr.state = TransferState::Cancelled;
                }
                Self::more(out, Event2::Transfers(self.transfers.clone()));
            }
            Command2::OpenDownloads => {}
            Command2::Media { player, action } => {
                if let Some(pl) = self.players.iter_mut().find(|x| x.id == player) {
                    match action {
                        MediaAction::PlayPause => pl.playing = !pl.playing,
                        MediaAction::Next | MediaAction::Previous => pl.position_ms = Some(0),
                    }
                }
                Self::more(out, Event2::Players(self.players.clone()));
            }
            Command2::PhoneMedia(_) => {
                out.push(Event::Notice(Notice::Failed {
                    op: Op::Media,
                    error: ErrorKind::NotImplemented,
                }));
            }
            Command2::SetDownloadsDir(d) => {
                self.settings.downloads_dir = d;
                Self::more(out, Event2::Settings(self.settings.clone()));
            }
            Command2::RestartDaemon => {
                Self::packet(out, t, Dir::Tx, p::CORE_HELLO, 88, "restart");
            }
        }
    }

    fn mixer(&self, out: &mut Vec<Event>) {
        Self::more(
            out,
            Event2::Mixer {
                sinks: self.sinks.clone(),
                apps: self.apps.clone(),
            },
        );
    }

    pub fn poll(&mut self, dt: f32, t: f32, out: &mut Vec<Event>) {
        self.acc += dt;
        if self.acc < 0.25 {
            return;
        }
        let step = self.acc;
        self.acc = 0.0;

        // Webcam stats.
        if self.webcam.is_some() {
            let mbps = 6.2 + (t * 1.3).sin() * 0.6;
            Self::more(out, Event2::Webcam(Some(WebcamStats { mbps, fps: None })));
        }

        // Active transfers progress (≈ 9 MB/s).
        let mut changed = false;
        for tr in &mut self.transfers {
            if tr.state == TransferState::Active {
                tr.done = (tr.done + (9_000_000.0 * step) as u64).min(tr.bytes);
                if tr.done >= tr.bytes {
                    tr.state = TransferState::Done;
                }
                changed = true;
            }
        }
        if changed {
            Self::more(out, Event2::Transfers(self.transfers.clone()));
        }

        // Players advance.
        for pl in &mut self.players {
            if pl.playing {
                if let (Some(p), Some(d)) = (pl.position_ms, pl.duration_ms) {
                    pl.position_ms = Some((p + (step * 1000.0) as u64) % d);
                }
            }
        }
        Self::more(out, Event2::Players(self.players.clone()));
    }
}

impl Default for More {
    fn default() -> Self {
        Self::new()
    }
}
