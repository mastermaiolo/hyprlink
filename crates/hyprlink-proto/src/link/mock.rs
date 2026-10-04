//! A deterministic fake daemon. Good enough to design against: values drift,
//! spike and respond to commands the way the real link would.

use super::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Uniform in [0, 1).
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniform in [-1, 1).
    fn s(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() % items.len() as u64) as usize]
    }
}

pub struct Simulator {
    rng: Rng,
    t: f32,
    devices: Vec<Device>,
    workspaces: Vec<Workspace>,
    active_ws: u8,
    latency: f32,
    rssi: f32,
    mic: bool,
    tap: bool,
    speaker: bool,
    mic_env: f32,
    mirror: Option<MirrorConfig>,
    bridges: Vec<SensorKind>,
    pairing: Option<(PairingTicket, f32)>,
    outbox: Vec<Event>,
    acc: [f32; 4],
    booted: bool,
    notifications: u32,
    ram: f32,
    more: super::mock_more::More,
}

impl Default for Simulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Simulator {
    pub fn new() -> Self {
        let all = vec![
            Cap::Workspaces,
            Cap::Webcam,
            Cap::Mic,
            Cap::Tap,
            Cap::Speaker,
            Cap::Clipboard,
            Cap::Files,
            Cap::Notifications,
            Cap::Input,
        ];
        let devices = vec![
            Device {
                id: 1,
                // Raw, as the user named it on the phone; the GUI uppercases.
                name: "Poco F4".into(),
                manufacturer: Some("Xiaomi".into()),
                model: Some("22021211RG".into()),
                android: Some("15".into()),
                app_version: Some("1.0.009-alpha".into()),
                kind: Kind::Phone,
                state: LinkState::Linked,
                fingerprint: "9f3ac21e77b04d19e8a20c5fb9136a7d".into(),
                addr: Some("192.168.1.42".into()),
                battery: Some(78),
                charging: false,
                rssi: Some(-52),
                latency_ms: Some(4.1),
                caps: all,
                paired_since: Some(1_786_665_600), // 2026-08-14
            },
            Device {
                id: 3,
                name: "Tab P12".into(),
                manufacturer: Some("Lenovo".into()),
                model: Some("TB370FU".into()),
                android: Some("14".into()),
                app_version: None,
                kind: Kind::Tablet,
                state: LinkState::Offline,
                fingerprint: "e07c3a19d2b48f6015cea93d47f20b81".into(),
                addr: None,
                battery: None,
                charging: false,
                rssi: None,
                latency_ms: None,
                caps: vec![Cap::Workspaces, Cap::Clipboard, Cap::Files],
                paired_since: Some(1_782_777_600), // 2026-06-30
            },
        ];

        let ws = |id: u8, mon: &str, c: &[&str]| Workspace {
            id,
            monitor: mon.into(),
            clients: c.iter().map(|s| s.to_string()).collect(),
        };
        let workspaces = vec![
            ws(1, "eDP-1", &["kitty", "kitty", "nvim"]),
            ws(2, "eDP-1", &["zen-browser"]),
            ws(3, "eDP-1", &["code", "kitty"]),
            ws(4, "eDP-1", &["reaper", "carla"]),
            ws(5, "eDP-1", &["obsidian"]),
            ws(6, "eDP-1", &[]),
            ws(7, "eDP-1", &["spotify"]),
            ws(8, "eDP-1", &[]),
            ws(9, "eDP-1", &["telegram-desktop"]),
            ws(10, "eDP-1", &[]),
        ];

        Self {
            rng: Rng(0x9E3779B97F4A7C15),
            t: 0.0,
            devices,
            workspaces,
            active_ws: 3,
            latency: 4.0,
            rssi: -52.0,
            mic: true,
            tap: false,
            speaker: true,
            mic_env: 0.0,
            mirror: None,
            bridges: vec![SensorKind::Accel, SensorKind::Light, SensorKind::Proximity],
            pairing: None,
            outbox: Vec::new(),
            acc: [0.0; 4],
            booted: false,
            notifications: 3,
            ram: 5.1,
            more: super::mock_more::More::new(),
        }
    }

    fn packet(&mut self, dir: Dir, kind: &str, bytes: usize, note: impl Into<String>) {
        self.outbox.push(Event::Packet(Packet {
            at: self.t,
            dir,
            kind: kind.to_string(),
            bytes,
            note: note.into(),
        }));
    }

    fn notice(&mut self, n: Notice) {
        self.outbox.push(Event::Notice(n));
    }
}

impl Transport for Simulator {
    fn send(&mut self, command: Command) {
        match command {
            Command::More(c) => {
                let t = self.t;
                self.more.send(c, &mut self.outbox, t);
            }
            Command::Ping(id) => {
                let rtt = self.latency * 2.0;
                self.packet(Dir::Tx, packets::CORE_PING, 24, format!("device {id}"));
                self.packet(Dir::Rx, packets::CORE_PING, 24, format!("rtt_ms {rtt:.1}"));
                self.notice(Notice::PingReply {
                    device: id,
                    rtt_ms: rtt,
                });
            }
            Command::SendClipboard(id) => {
                self.packet(Dir::Tx, packets::CLIPBOARD_SET, 312, "text/plain");
                self.notice(Notice::ClipboardSent { device: id });
            }
            Command::SwitchWorkspace(n) => {
                self.active_ws = n;
                self.packet(
                    Dir::Tx,
                    packets::HYPR_DISPATCH,
                    64,
                    format!("workspace {n}"),
                );
                self.outbox.push(Event::Workspaces {
                    list: self.workspaces.clone(),
                    active: n,
                });
                self.more.on_workspace(n, &mut self.outbox);
            }
            Command::SetMic(on) => {
                self.mic = on;
                self.packet(
                    Dir::Tx,
                    if on {
                        packets::MIC_START_REQUEST
                    } else {
                        packets::MIC_STOP_REQUEST
                    },
                    32,
                    "pcm s16le 48000 mono",
                );
            }
            Command::SetTap(on) => {
                self.tap = on;
                self.packet(
                    Dir::Tx,
                    if on {
                        packets::TAP_START
                    } else {
                        packets::TAP_STOP
                    },
                    48,
                    if self.speaker {
                        "source hyprlink-speaker"
                    } else {
                        "source @DEFAULT_MONITOR@"
                    },
                );
            }
            Command::SetSpeakerMode(on) => {
                self.speaker = on;
                self.outbox.push(Event::SpeakerMode(on));
                if self.tap {
                    self.packet(
                        Dir::Tx,
                        packets::TAP_START,
                        48,
                        if on {
                            "source hyprlink-speaker"
                        } else {
                            "source @DEFAULT_MONITOR@"
                        },
                    );
                }
            }
            Command::StartMirror(cfg) => {
                self.mirror = Some(cfg);
                self.packet(
                    Dir::Tx,
                    packets::MIRROR_START,
                    128,
                    format!(
                        "{:?} {}mbps {}fps",
                        cfg.codec, cfg.bitrate_mbps, cfg.max_fps
                    )
                    .to_lowercase(),
                );
                self.notice(Notice::MirrorStarted);
            }
            Command::StopMirror => {
                self.mirror = None;
                self.packet(Dir::Tx, packets::MIRROR_STOP, 32, "");
                self.outbox.push(Event::Mirror(None));
            }
            Command::SetSensorBridge(kind, on) => {
                self.bridges.retain(|k| *k != kind);
                if on {
                    self.bridges.push(kind);
                }
                self.packet(
                    Dir::Tx,
                    packets::SENSOR_SUBSCRIBE,
                    72,
                    format!("{kind:?} = {}", if on { "on" } else { "off" }).to_lowercase(),
                );
            }
            Command::SetRule(i, on) => {
                self.packet(
                    Dir::Tx,
                    packets::PRESENCE_RULE,
                    88,
                    format!("rule[{i}] = {}", if on { "armed" } else { "disarmed" }),
                );
            }
            Command::BeginPairing => {
                // Same shape as the real daemon's QR: fp|host:port|token.
                let token: String = (0..16)
                    .map(|_| format!("{:02x}", self.rng.next() as u8))
                    .collect();
                let ticket = PairingTicket {
                    payload: format!("5d1e0a9c4b7f2e83|192.168.1.10:7443|{token}"),
                    code: Some(token[..6].to_uppercase()),
                    expires_in: Some(120.0),
                };
                self.pairing = Some((ticket.clone(), 0.0));
                self.outbox.push(Event::Pairing(Some(ticket)));
            }
            Command::CancelPairing => {
                self.pairing = None;
                self.outbox.push(Event::Pairing(None));
            }
            Command::Unpair(id) => {
                let name = self.name(id);
                self.devices.retain(|d| d.id != id);
                self.packet(Dir::Tx, packets::PAIR_REVOKE, 64, format!("device {id}"));
                self.notice(Notice::Revoked { device: id, name });
                self.outbox.push(Event::Devices(self.devices.clone()));
            }
        }
    }

    fn poll(&mut self, dt: Duration) -> Vec<Event> {
        let dt = dt.as_secs_f32().min(0.1);
        self.t += dt;
        let t = self.t;

        if !self.booted {
            self.booted = true;
            self.outbox.push(Event::Devices(self.devices.clone()));
            self.outbox.push(Event::Workspaces {
                list: self.workspaces.clone(),
                active: self.active_ws,
            });
            // Seed the journal so it never opens empty.
            let seed: [(Dir, &str, usize, &str); 6] = [
                (Dir::Rx, packets::CORE_HELLO, 96, "device_name Poco F4"),
                (Dir::Tx, packets::CORE_HELLO, 88, "hyprlink/1"),
                (
                    Dir::Rx,
                    packets::BATTERY_STATE,
                    24,
                    "level 78 charging false",
                ),
                (Dir::Rx, packets::PHONE_STATUS, 212, "proposto"),
                (
                    Dir::Tx,
                    packets::MIC_START_REQUEST,
                    32,
                    "pcm s16le 48000 mono",
                ),
                (Dir::Rx, packets::MIC_START, 24, "ok"),
            ];
            for (d, k, b, n) in seed {
                self.packet(d, k, b, n);
            }
        }

        // Telemetry (4 Hz).
        self.acc[0] += dt;
        if self.acc[0] > 0.25 {
            self.acc[0] = 0.0;
            let spike = if self.rng.f() > 0.94 {
                self.rng.f() * 14.0
            } else {
                0.0
            };
            self.latency += (4.0 - self.latency) * 0.3 + self.rng.s() * 0.8;
            self.latency = self.latency.clamp(1.6, 9.0);
            let up = 180.0
                + self.rng.f() * 60.0
                + if self.mic { 64.0 } else { 0.0 }
                + self.mirror.map(|_| 0.0).unwrap_or(0.0);
            let down = 90.0
                + self.rng.f() * 40.0
                + self.mirror.map(|c| c.bitrate_mbps * 1000.0).unwrap_or(0.0)
                + if self.tap { 128.0 } else { 0.0 };
            self.outbox.push(Event::Telemetry {
                latency_ms: self.latency + spike,
                up_kbps: up,
                down_kbps: down,
            });

            self.rssi += (-54.0 - self.rssi) * 0.08 + self.rng.s() * 2.5;
            if self.rng.f() > 0.985 {
                self.rssi -= 18.0;
            }
            self.rssi = self.rssi.clamp(-95.0, -38.0);
            self.outbox.push(Event::Rssi(self.rssi as i32));

            if let Some(cfg) = self.mirror {
                let fps = cfg.max_fps as f32 * (0.93 + self.rng.f() * 0.07);
                self.outbox.push(Event::Mirror(Some(MirrorStats {
                    fps,
                    decode_ms: 3.2 + self.rng.f() * 1.8,
                    dropped: (t * 0.3) as u32,
                    kbps: cfg.bitrate_mbps * 1000.0 * (0.85 + self.rng.f() * 0.15),
                })));
            }
        }

        // Devices (1 Hz): battery, rssi, latency.
        self.acc[1] += dt;
        if self.acc[1] > 1.0 {
            self.acc[1] = 0.0;
            let lat = self.latency;
            let rssi = self.rssi as i32;
            for d in &mut self.devices {
                if d.id == 1 {
                    d.latency_ms = Some(lat);
                    d.rssi = Some(rssi);
                    if (t as u32) % 45 == 0 {
                        d.battery = d.battery.map(|b| b.saturating_sub(1).max(5));
                    }
                }
            }
            self.outbox.push(Event::Devices(self.devices.clone()));

            self.ram = (self.ram + self.rng.s() * 0.15).clamp(4.2, 6.4);
            let track = 214.0;
            let pos = (t * 1.0) % track;
            self.outbox.push(Event::Phone(PhoneStatus {
                network: Some(Network::Cellular),
                cell_gen: Some("5G".into()),
                carrier: Some("Woo".into()),
                signal_bars: Some(if self.rssi > -60.0 {
                    4
                } else if self.rssi > -72.0 {
                    3
                } else {
                    2
                }),
                wifi: Some(Wifi {
                    ssid: Some("Casa-5G".into()),
                    rssi_dbm: Some(self.rssi as i32),
                }),
                storage_used_b: Some(168_400_000_000),
                storage_total_b: Some(256_000_000_000),
                ram_used_b: Some((self.ram * 1e9) as u64),
                ram_total_b: Some(8_000_000_000),
                battery_temp_c: Some(((32.6 + (t * 0.05).sin() * 1.2) * 10.0).round() / 10.0),
                screen_on: Some((t * 0.02).sin() > -0.6),
                dnd: Some(false),
                notifications: Some(self.notifications),
                now_playing: Some(NowPlaying {
                    title: "Night Transit".into(),
                    artist: Some("Lumen Drift".into()),
                    app: Some("Spotify".into()),
                    playing: true,
                    position_ms: Some((pos * 1000.0) as u64),
                    duration_ms: Some((track * 1000.0) as u64),
                }),
            }));
        }

        // Sensors (20 Hz).
        self.acc[2] += dt;
        if self.acc[2] > 0.05 {
            self.acc[2] = 0.0;
            let tilt = (t * 0.35).sin() * 0.6;
            let roll = (t * 0.21).cos() * 0.4;
            let n = |r: &mut Rng| r.s() * 0.06;
            let accel = [
                9.81 * tilt.sin() + n(&mut self.rng),
                9.81 * roll.sin() + n(&mut self.rng),
                9.81 * tilt.cos() * roll.cos() + n(&mut self.rng),
            ];
            let gyro = [
                0.35 * (t * 0.35).cos() * 0.6 + n(&mut self.rng),
                -0.21 * (t * 0.21).sin() * 0.4 + n(&mut self.rng),
                n(&mut self.rng) * 0.5,
            ];
            let lux = 320.0 + 140.0 * (t * 0.12).sin() + self.rng.s() * 8.0;
            let proximity = if (t * 0.1).sin() > 0.93 { 0.0 } else { 5.0 };
            self.outbox.push(Event::Sensors(Sensors {
                accel,
                gyro,
                lux,
                proximity_cm: proximity,
                pressure_hpa: 1012.4 + (t * 0.02).sin() * 0.6 + self.rng.s() * 0.05,
                battery_temp: 31.2 + (t * 0.05).sin() * 0.8,
            }));
        }

        // Audio levels (every poll).
        let mic = if self.mic {
            // Syllabic envelope: bursts of speech separated by pauses.
            let phrase = ((t * 0.7).sin() * 0.5 + 0.5).powf(0.6);
            let syll = ((t * 7.3).sin() * 0.5 + 0.5) * ((t * 3.1).sin() * 0.5 + 0.5);
            let target = (phrase * syll * 0.95 + self.rng.f() * 0.08).min(1.0);
            self.mic_env += (target - self.mic_env) * 0.35;
            self.mic_env
        } else {
            self.mic_env *= 0.8;
            0.0
        };
        let tap = if self.tap {
            let beat = (t * 2.0).fract(); // 120 bpm
            let kick = (1.0 - beat).powf(5.0);
            (0.45 + 0.5 * kick + self.rng.f() * 0.08).min(1.0)
        } else {
            0.0
        };
        // The real mic has no meter yet; keep the mock honest-ish but useful
        // for design: report it, the GUI also handles `None`.
        self.outbox.push(Event::Levels {
            mic: self.mic.then_some(mic),
            tap: self.tap.then_some(tap),
        });

        // Background chatter (≈2.5 Hz).
        self.acc[3] += dt;
        if self.acc[3] > 0.4 {
            self.acc[3] = 0.0;
            if self.rng.f() > 0.35 {
                let r = self.rng.f();
                if r < 0.30 && !self.bridges.is_empty() {
                    let k = *self.rng.pick(&self.bridges);
                    self.packet(
                        Dir::Rx,
                        packets::SENSOR_FRAME,
                        44,
                        format!("{k:?}").to_lowercase(),
                    );
                } else if r < 0.45 {
                    let lat = self.latency;
                    self.packet(
                        Dir::Rx,
                        packets::CORE_PING,
                        24,
                        format!("rtt {:.1} ms", lat * 2.0),
                    );
                } else if r < 0.58 {
                    let rssi = self.rssi as i32;
                    self.packet(
                        Dir::Rx,
                        packets::PRESENCE_RSSI,
                        32,
                        format!("rssi {rssi} dBm"),
                    );
                } else if r < 0.70 && self.mic {
                    self.packet(
                        Dir::Rx,
                        packets::MIC_DATA,
                        1280,
                        "uni-stream #7 · 64 frames",
                    );
                } else if r < 0.78 {
                    let apps = ["Signal", "Gmail", "Telegram", "Calendário", "GitHub"];
                    let a = *self.rng.pick(&apps);
                    self.packet(Dir::Rx, packets::NOTIF_POSTED, 690, format!("{a} · 1 nova"));
                } else if r < 0.86 {
                    let ws = self.active_ws;
                    self.packet(
                        Dir::Tx,
                        packets::HYPR_DISPATCH,
                        512,
                        format!("workspace {ws}"),
                    );
                } else if self.tap && r < 0.92 {
                    self.packet(Dir::Tx, packets::TAP_DATA, 1920, "datagram");
                } else {
                    let tx: [(&'static str, usize, &str); 4] = [
                        (packets::BATTERY_STATE, 24, "level 78 charging false"),
                        (packets::HYPR_DISPATCH, 64, "focuswindow class:code"),
                        (packets::CLIPBOARD_SET, 140, "text/plain"),
                        (packets::CORE_PING, 24, "keepalive"),
                    ];
                    let (k, b, n) = *self.rng.pick(&tx);
                    self.packet(Dir::Tx, k, b, n);
                }
            }
        }

        // Pairing countdown → success after ~9s.
        if let Some((ticket, elapsed)) = &mut self.pairing {
            *elapsed += dt;
            ticket.expires_in = Some((120.0 - *elapsed).max(0.0));
            if *elapsed > 9.0 {
                self.pairing = None;
                self.devices.push(Device {
                    id: 4,
                    name: "Pixel 9a".into(),
                    manufacturer: Some("Google".into()),
                    model: Some("GTF7P".into()),
                    android: Some("16".into()),
                    app_version: Some("1.0.009-alpha".into()),
                    kind: Kind::Phone,
                    state: LinkState::Idle,
                    fingerprint: "2ba977e1c04d5f329e06b1c84a7d03f5".into(),
                    addr: Some("192.168.1.57".into()),
                    battery: Some(91),
                    charging: true,
                    rssi: None,
                    latency_ms: None,
                    caps: vec![Cap::Clipboard, Cap::Files, Cap::Notifications],
                    paired_since: Some(1_791_072_000),
                });
                self.packet(Dir::Rx, packets::CORE_HELLO, 96, "device_name Pixel 9a");
                self.notice(Notice::Paired {
                    device: 4,
                    name: "Pixel 9a".into(),
                });
                self.outbox.push(Event::Pairing(None));
                self.outbox.push(Event::Devices(self.devices.clone()));
            } else if (*elapsed * 4.0) as u32 != ((*elapsed - dt) * 4.0) as u32 {
                self.outbox.push(Event::Pairing(Some(ticket.clone())));
            }
        }

        let battery = self.devices.first().and_then(|d| d.battery).unwrap_or(78);
        self.more.boot(&mut self.outbox, battery);
        self.more.poll(dt, t, &mut self.outbox);

        std::mem::take(&mut self.outbox)
    }
}

impl Simulator {
    fn name(&self, id: DeviceId) -> String {
        self.devices
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    }
}
