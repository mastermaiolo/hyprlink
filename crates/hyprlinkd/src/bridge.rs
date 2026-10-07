//! Liga o daemon ao contrato `hyprlink-proto`: projeta o estado interno em
//! `Event`s no [`Hub`](crate::hub) e executa os `Command`s que chegam pelo
//! socket local (`ipc.rs`).
//!
//! Projeção por amostragem, em três ritmos: níveis de áudio a 30 Hz,
//! transferências a 4 Hz, o resto a 1 Hz (misturador e MPRIS a cada 2 s,
//! volumes do telemóvel a cada 15 s). O hub só difunde o que mudou, por isso
//! amostrar não custa tráfego. Só dados: nenhum texto para humanos sai daqui.

use crate::config;
use crate::hub::{self, Hub};
use crate::pairing::{DeviceMeta, PairingStore};
use crate::server::Ctx;
use crate::state::{self, ConnState};
use hyprlink_proto::link::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Duração do teste de rede da câmara.
const NET_TEST: Duration = Duration::from_secs(60);
/// Um nível de microfone mais velho do que isto já não é "agora".
const LEVEL_STALE: Duration = Duration::from_millis(500);
/// Histórico de bateria mostrado na ficha do dispositivo.
const BATTERY_WINDOW_S: u64 = 12 * 3600;

static START: OnceLock<Instant> = OnceLock::new();

fn uptime_s() -> f32 {
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// Id estável de um dispositivo: os primeiros 4 bytes do SHA-256 do
/// certificado. Cabe no `DeviceId` (u32) e não muda entre arranques.
pub fn device_id(fingerprint: &str) -> DeviceId {
    let hex = wire_fingerprint(fingerprint);
    u32::from_str_radix(hex.get(..8).unwrap_or("0"), 16).unwrap_or(0)
}

/// `AA:BB:…` (formato interno) → `aabb…` (formato do contrato).
pub fn wire_fingerprint(fingerprint: &str) -> String {
    fingerprint.replace(':', "").to_ascii_lowercase()
}

// ───────────────────────── registo de pacotes ─────────────────────────

pub fn log_packet_rx(kind: &str, bytes: usize, id: u64) {
    log_packet(Dir::Rx, kind, bytes, id);
}

pub fn log_packet_tx(kind: &str, bytes: usize, id: u64) {
    log_packet(Dir::Tx, kind, bytes, id);
}

fn log_packet(dir: Dir, kind: &str, bytes: usize, id: u64) {
    hub::global().publish(Event::Packet(Packet {
        at: uptime_s(),
        dir,
        kind: kind.to_string(),
        bytes,
        note: format!("#{id}"),
    }));
}

/// Chamado pelo servidor quando um telemóvel novo emparelha com o token.
pub fn on_paired(fingerprint: &str, name: &str) {
    if let Some(b) = BRIDGE.get() {
        b.pairing_open.store(false, Ordering::Relaxed);
    }
    let hub = hub::global();
    hub.publish(Event::Pairing(None));
    hub.publish(Event::Notice(Notice::Paired {
        device: device_id(fingerprint),
        name: name.to_string(),
    }));
}

// ───────────────────────────── a ponte ─────────────────────────────

struct Bridge {
    ctx: Ctx,
    pairing: Arc<Mutex<PairingStore>>,
    server_fingerprint: String,
    local_addr: String,
    socket: String,
    pairing_open: AtomicBool,
    /// Último estado de volumes do telemóvel (pedido sob procura).
    phone_audio: Mutex<Option<PhoneAudio>>,
    net_test_running: AtomicBool,
}

static BRIDGE: OnceLock<Arc<Bridge>> = OnceLock::new();

/// Arranca o socket local, a projeção e o executor de comandos.
pub async fn spawn(
    ctx: Ctx,
    pairing: Arc<Mutex<PairingStore>>,
    server_fingerprint: String,
    local_addr: String,
) {
    START.get_or_init(Instant::now);
    let Some(path) = hyprlink_proto::ipc::socket_path() else {
        state::push_log(
            &ctx.hud,
            "[!] ipc: sem XDG_RUNTIME_DIR, socket local desativado",
        );
        return;
    };
    let listener = match crate::ipc::bind(&path).await {
        Ok(l) => l,
        Err(e) => {
            state::push_log(&ctx.hud, format!("[!] ipc: {e}"));
            return;
        }
    };
    let bridge = Arc::new(Bridge {
        ctx,
        pairing,
        server_fingerprint,
        local_addr,
        socket: path.display().to_string(),
        pairing_open: AtomicBool::new(false),
        phone_audio: Mutex::new(None),
        net_test_running: AtomicBool::new(false),
    });
    let _ = BRIDGE.set(bridge.clone());
    let hub = hub::global().clone();

    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::ipc::serve(listener, hub.clone(), tx));
    tokio::spawn(execute_forever(bridge.clone(), rx));
    tokio::spawn(project_fast(bridge.clone(), hub.clone()));
    tokio::spawn(project_slow(bridge.clone(), hub.clone()));
    tokio::spawn(project_phone_audio(bridge.clone(), hub.clone()));
    state::push_log(
        &bridge.ctx.hud,
        format!("[+] ipc: socket local em {}", bridge.socket),
    );
}

impl Bridge {
    fn connection(&self) -> Option<quinn::Connection> {
        self.ctx.active.lock().unwrap().clone()
    }

    fn connected_fingerprint(&self) -> Option<String> {
        match &self.ctx.hud.lock().unwrap().conn {
            ConnState::Connected {
                fingerprint_hex, ..
            } => Some(fingerprint_hex.clone()),
            _ => None,
        }
    }

    fn linked_id(&self) -> Option<DeviceId> {
        self.connected_fingerprint().map(|fp| device_id(&fp))
    }

    fn fingerprint_of(&self, id: DeviceId) -> Option<String> {
        self.pairing
            .lock()
            .unwrap()
            .list_fingerprints()
            .into_iter()
            .find(|fp| device_id(fp) == id)
    }

    fn fail(&self, op: Op, error: ErrorKind) {
        hub::global().publish(Event::Notice(Notice::Failed { op, error }));
    }

    // ─────────────── projeções ───────────────

    fn devices(&self) -> Vec<Device> {
        let (conn_fp, conn_name, battery, charging) = {
            let hud = self.ctx.hud.lock().unwrap();
            let (fp, name) = match &hud.conn {
                ConnState::Connected {
                    device_name,
                    fingerprint_hex,
                } => (Some(fingerprint_hex.clone()), Some(device_name.clone())),
                _ => (None, None),
            };
            (
                fp,
                name,
                hud.modules.phone_battery_pct,
                hud.modules.phone_battery_charging,
            )
        };
        let connection = self.connection();
        let store = self.pairing.lock().unwrap();
        let mut fps = store.list_fingerprints();
        if let Some(fp) = &conn_fp
            && !fps.contains(fp)
        {
            fps.push(fp.clone());
        }
        let mut out: Vec<Device> = fps
            .iter()
            .map(|fp| {
                let linked = conn_fp.as_deref() == Some(fp.as_str());
                let meta = store.meta(fp).cloned().unwrap_or_else(|| DeviceMeta {
                    name: if linked {
                        conn_name.clone().unwrap_or_default()
                    } else {
                        String::new()
                    },
                    ..Default::default()
                });
                let live = connection.as_ref().filter(|_| linked);
                Device {
                    id: device_id(fp),
                    name: meta.display_name(),
                    manufacturer: meta.manufacturer.clone(),
                    model: meta.model.clone(),
                    android: meta.android.clone(),
                    app_version: meta.app_version.clone(),
                    kind: Kind::Phone,
                    state: if linked {
                        LinkState::Linked
                    } else {
                        LinkState::Offline
                    },
                    fingerprint: wire_fingerprint(fp),
                    addr: live.map(|c| c.remote_address().to_string()),
                    battery: if linked {
                        battery.and_then(|b| u8::try_from(b.clamp(0, 100)).ok())
                    } else {
                        None
                    },
                    charging: linked && charging,
                    rssi: None,
                    latency_ms: live.map(|c| c.rtt().as_secs_f32() * 1000.0),
                    caps: caps(&meta.capabilities),
                    paired_since: meta.paired_since,
                }
            })
            .collect();
        out.sort_by_key(|d| d.state != LinkState::Linked);
        out
    }

    fn levels(&self) -> Event {
        let hud = self.ctx.hud.lock().unwrap();
        let m = &hud.modules;
        let tap = m
            .audio_tap_active
            .then(|| m.audio_vu.last().map_or(0.0, |p| f32::from(*p) / 100.0));
        let mic = m.mic_active.then(|| match m.mic_level {
            Some((v, at)) if at.elapsed() < LEVEL_STALE => v,
            _ => 0.0,
        });
        Event::Levels { mic, tap }
    }

    fn transfers(&self) -> Vec<Transfer> {
        let hud = self.ctx.hud.lock().unwrap();
        let m = &hud.modules;
        let dir = |d: &str| if d == "enviando" { Dir::Tx } else { Dir::Rx };
        let mut out = Vec::new();
        if let Some(t) = &m.file_transfer {
            out.push(Transfer {
                id: t.id,
                name: t.name.clone(),
                dir: dir(t.direction),
                bytes: t.total,
                done: t.bytes,
                state: TransferState::Active,
                at: t.at_unix,
            });
        }
        out.extend(m.file_history.iter().map(|r| Transfer {
            id: r.id,
            name: r.name.clone(),
            dir: dir(r.direction),
            bytes: r.total,
            done: r.bytes,
            state: if r.ok {
                TransferState::Done
            } else if r.cancelled {
                TransferState::Cancelled
            } else {
                TransferState::Failed
            },
            at: r.at_unix,
        }));
        out
    }

    fn clipboard(&self) -> Vec<ClipEntry> {
        let hud = self.ctx.hud.lock().unwrap();
        hud.modules
            .clip_history
            .iter()
            .map(|c| ClipEntry {
                id: c.id,
                origin: if c.from_phone {
                    Origin::Phone
                } else {
                    Origin::Pc
                },
                mime: c.mime.to_string(),
                text: c.content.clone(),
                bytes: c.bytes,
                at: c.at_unix,
                pinned: c.pinned,
            })
            .collect()
    }

    fn notifications(&self) -> Vec<PhoneNotification> {
        let hud = self.ctx.hud.lock().unwrap();
        hud.modules
            .notif_active
            .iter()
            .map(|n| PhoneNotification {
                key: n.key.clone(),
                app: n.app.clone(),
                title: n.title.clone(),
                text: (!n.text.is_empty()).then(|| n.text.clone()),
                at: n.at_unix,
                actions: n.actions.clone(),
            })
            .collect()
    }

    fn battery_history(&self) -> Vec<BatteryPoint> {
        let since = state::now_unix().saturating_sub(BATTERY_WINDOW_S);
        let hud = self.ctx.hud.lock().unwrap();
        hud.modules
            .battery_history
            .iter()
            .filter(|s| s.at_unix >= since)
            .filter_map(|s| {
                Some(BatteryPoint {
                    at: s.at_unix,
                    level: u8::try_from(s.phone?.clamp(0, 100)).ok()?,
                })
            })
            .collect()
    }

    fn webcam(&self) -> Option<WebcamStats> {
        let hud = self.ctx.hud.lock().unwrap();
        hud.modules.webcam_active.then(|| WebcamStats {
            mbps: hud.modules.webcam_mbps.unwrap_or(0.0) as f32,
            fps: None,
            codec: match hud.modules.webcam_codec {
                Some("H.265") => Some(CamCodec::H265),
                Some("H.264") => Some(CamCodec::H264),
                _ => None,
            },
        })
    }

    /// Último `phone.status` do telemóvel — polia a cada 1 s no
    /// `project_slow` e o hub deduplica (só difunde em mudança). Vazio
    /// (tudo `None`) quando não há telemóvel ligado.
    fn phone_status(&self) -> PhoneStatus {
        let hud = self.ctx.hud.lock().unwrap();
        hud.modules.phone_status.clone()
    }

    fn pairing_ticket(&self) -> Option<PairingTicket> {
        if !self.pairing_open.load(Ordering::Relaxed) {
            return None;
        }
        let token = self.pairing.lock().unwrap().current_token_hex.clone();
        Some(PairingTicket {
            payload: format!("{}|{}|{token}", self.server_fingerprint, self.local_addr),
            code: None,
            expires_in: None,
        })
    }

    fn settings(&self) -> Settings {
        Settings {
            downloads_dir: config::download_dir(&self.ctx.config).display().to_string(),
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
            socket: self.socket.clone(),
            started_unix: crate::state::now_unix().saturating_sub(uptime_s() as u64),
        }
    }
}

/// Regras de gestos guardadas + o último gesto recebido, como evento.
pub fn gestures_event(config: &config::SharedConfig) -> Event {
    Event::More(Event2::Gestures {
        rules: config::gesture_rules(config),
        last: crate::gesture::last(),
    })
}

/// `capabilities` do `core.hello` → capacidades do contrato. A app atual
/// (1.0.009) só declara `core, clipboard, notification, media, battery,
/// share`, mas já faz câmara, microfone, retorno, trackpad e workspaces:
/// sem nenhum nome estendido no hello, assume-se esse conjunto.
fn caps(declared: &[String]) -> Vec<Cap> {
    let mut out = Vec::new();
    let mut push = |c: Cap| {
        if !out.contains(&c) {
            out.push(c);
        }
    };
    let mut extended = false;
    for name in declared {
        match name.as_str() {
            "clipboard" => push(Cap::Clipboard),
            "notification" => push(Cap::Notifications),
            "share" => push(Cap::Files),
            "webcam" => {
                extended = true;
                push(Cap::Webcam);
                push(Cap::Mic);
            }
            "audio" => {
                extended = true;
                push(Cap::Tap);
                push(Cap::Speaker);
            }
            "input" => {
                extended = true;
                push(Cap::Input);
            }
            "hypr" => {
                extended = true;
                push(Cap::Workspaces);
            }
            "mirror" => push(Cap::Mirror),
            "sensors" => push(Cap::Sensors),
            "presence" => push(Cap::Presence),
            // Capabilities que a app passa a declarar no core.hello
            // (prompts de 2026-10-04): o estado do telemóvel e a sessão de
            // mídia dele. A GUI/ctl só ativam as ações quando as veem.
            "phone_status" => {
                extended = true;
                push(Cap::PhoneStatus);
            }
            "media_session" => {
                extended = true;
                push(Cap::MediaSession);
            }
            _ => {}
        }
    }
    if !declared.is_empty() && !extended {
        for c in [
            Cap::Workspaces,
            Cap::Webcam,
            Cap::Mic,
            Cap::Tap,
            Cap::Speaker,
            Cap::Input,
        ] {
            push(c);
        }
    }
    out
}

fn workspaces() -> Option<Event> {
    #[derive(serde::Deserialize)]
    struct Ws {
        id: i64,
        monitor: String,
    }
    #[derive(serde::Deserialize)]
    struct WsRef {
        id: i64,
    }
    #[derive(serde::Deserialize)]
    struct Client {
        class: String,
        workspace: WsRef,
    }
    let list: Vec<Ws> = serde_json::from_str(&crate::hypr::workspaces_json()).ok()?;
    let clients: Vec<Client> =
        serde_json::from_str(&crate::hypr::clients_json()).unwrap_or_default();
    let active: WsRef = serde_json::from_str(&crate::hypr::active_workspace_json()).ok()?;
    let mut by_ws: HashMap<i64, Vec<String>> = HashMap::new();
    for c in clients {
        by_ws.entry(c.workspace.id).or_default().push(c.class);
    }
    // Workspaces especiais têm id negativo; o contrato só leva os normais.
    let mut list: Vec<Workspace> = list
        .into_iter()
        .filter_map(|w| {
            Some(Workspace {
                id: u8::try_from(w.id).ok().filter(|id| *id > 0)?,
                monitor: w.monitor,
                clients: by_ws.remove(&w.id).unwrap_or_default(),
            })
        })
        .collect();
    list.sort_by_key(|w| w.id);
    Some(Event::Workspaces {
        list,
        active: u8::try_from(active.id).unwrap_or(0),
    })
}

fn active_window() -> Option<hyprlink_proto::link::ActiveWindow> {
    #[derive(serde::Deserialize)]
    struct WsRef {
        id: i64,
    }
    #[derive(serde::Deserialize)]
    struct Win {
        class: String,
        title: String,
        workspace: WsRef,
    }
    let w: Win = serde_json::from_str(&crate::hypr::active_window_json()).ok()?;
    Some(hyprlink_proto::link::ActiveWindow {
        class: w.class,
        title: w.title,
        workspace: u8::try_from(w.workspace.id).unwrap_or(0),
    })
}

fn mixer() -> Event {
    let snap = crate::audio::snapshot();
    let pct = |v: i64| u8::try_from(v.clamp(0, 150)).unwrap_or(0);
    Event::More(Event2::Mixer {
        sinks: snap
            .sinks
            .iter()
            .map(|s| Sink {
                id: u32::try_from(s.id).unwrap_or(0),
                name: s.name.clone(),
                description: s.description.clone(),
                volume: pct(s.volume),
                muted: s.muted,
                default: s.is_default,
            })
            .collect(),
        apps: snap
            .apps
            .iter()
            .map(|a| AppStream {
                id: u32::try_from(a.id).unwrap_or(0),
                app: a.name.clone(),
                volume: pct(a.volume),
                muted: a.muted,
                sink: u32::try_from(a.sink_id).unwrap_or(0),
            })
            .collect(),
    })
}

/// Executa uma função bloqueante (hyprctl, pactl) fora do executor.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

async fn project_fast(b: Arc<Bridge>, hub: Hub) {
    let mut tick = tokio::time::interval(Duration::from_millis(33));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut n: u64 = 0;
    loop {
        tick.tick().await;
        n += 1;
        hub.publish(b.levels());
        if n.is_multiple_of(8) {
            hub.publish(Event::More(Event2::Transfers(b.transfers())));
        }
    }
}

async fn project_slow(b: Arc<Bridge>, hub: Hub) {
    hub.publish(Event::Mirror(None));
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut n: u64 = 0;
    let mut last_bytes: Option<(u64, u64, Instant)> = None;
    loop {
        tick.tick().await;
        n += 1;
        hub.publish(Event::Devices(b.devices()));
        hub.publish(Event::Pairing(b.pairing_ticket()));
        // Rede de segurança: os `state::set_*` já publicam a fase quando muda.
        hub.publish(Event::Link(crate::state::link_phase(
            &b.ctx.hud.lock().unwrap().conn,
        )));
        hub.publish(Event::SpeakerMode(crate::speaker::is_active(
            &b.ctx.speaker,
        )));

        // Telemetria: RTT do QUIC e débito pelos bytes UDP por segundo.
        if let Some(c) = b.connection() {
            let s = c.stats();
            let (tx, rx, now) = (s.udp_tx.bytes, s.udp_rx.bytes, Instant::now());
            if let Some((ptx, prx, at)) = last_bytes {
                let dt = now.duration_since(at).as_secs_f32().max(0.001);
                hub.publish(Event::Telemetry {
                    latency_ms: c.rtt().as_secs_f32() * 1000.0,
                    up_kbps: tx.saturating_sub(ptx) as f32 * 8.0 / 1000.0 / dt,
                    down_kbps: rx.saturating_sub(prx) as f32 * 8.0 / 1000.0 / dt,
                });
            }
            last_bytes = Some((tx, rx, now));
        } else {
            last_bytes = None;
        }

        hub.publish(Event::More(Event2::Webcam(b.webcam())));
        hub.publish(Event::Phone(b.phone_status()));
        hub.publish(Event::More(Event2::Clipboard(b.clipboard())));
        hub.publish(Event::More(Event2::Notifications(b.notifications())));
        hub.publish(Event::More(Event2::BatteryHistory(b.battery_history())));
        let alerts = config::battery_alerts(&b.ctx.config);
        hub.publish(Event::More(Event2::BatteryAlerts(BatteryAlerts {
            low: alerts.low.then_some(alerts.low_pct),
            full: alerts.full,
        })));
        hub.publish(shortcuts_event(&b.ctx.config));
        hub.publish(gestures_event(&b.ctx.config));
        let t = config::track_settings(&b.ctx.config);
        hub.publish(Event::More(Event2::Trackpad(TrackpadConfig {
            sensitivity: t.sensitivity,
            scroll: t.scroll_speed,
            acceleration: t.acceleration,
            natural_scroll: t.invert_scroll,
            keyboard: t.virtual_keyboard,
        })));
        hub.publish(Event::More(Event2::Settings(b.settings())));

        if let Some(Some(ws)) = blocking(workspaces).await {
            hub.publish(ws);
        }
        if let Some(w) = blocking(active_window).await {
            hub.publish(Event::More(Event2::ActiveWindow(w)));
        }
        if n.is_multiple_of(2) {
            if let Some(m) = blocking(mixer).await {
                hub.publish(m);
            }
            if let Some(dbus) = &b.ctx.dbus {
                let players = crate::media::players(dbus)
                    .await
                    .into_iter()
                    .map(|p| Player {
                        id: p.id,
                        identity: p.identity,
                        title: p.title,
                        artist: p.artist,
                        playing: p.playing,
                        position_ms: p.position_ms,
                        duration_ms: p.duration_ms,
                    })
                    .collect();
                hub.publish(Event::More(Event2::Players(players)));
            }
        }
    }
}

/// Volumes do telemóvel: pedidos ao ligar e a cada 15 s (é uma ida e volta
/// pela rede; não se faz a 1 Hz).
async fn project_phone_audio(b: Arc<Bridge>, hub: Hub) {
    let mut was_linked = false;
    let mut last = Instant::now() - Duration::from_secs(3600);
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let linked = b.connection().is_some();
        if linked && (!was_linked || last.elapsed() >= Duration::from_secs(15)) {
            refresh_phone_audio(&b, &hub).await;
            last = Instant::now();
        }
        if !linked && was_linked {
            *b.phone_audio.lock().unwrap() = None;
        }
        was_linked = linked;
    }
}

async fn refresh_phone_audio(b: &Bridge, hub: &Hub) {
    let Some(s) = crate::phone_audio::get_state(&b.ctx.active).await else {
        return;
    };
    let pct = |v: i64| u8::try_from(v.clamp(0, 100)).unwrap_or(0);
    let audio = PhoneAudio {
        streams: vec![
            StreamLevel {
                stream: PhoneStream::Media,
                level: pct(s.media_percent),
                max: 100,
            },
            StreamLevel {
                stream: PhoneStream::Ring,
                level: pct(s.ring_percent),
                max: 100,
            },
            StreamLevel {
                stream: PhoneStream::Alarm,
                level: pct(s.alarm_percent),
                max: 100,
            },
        ],
        ringer: match s.ringer_mode.as_str() {
            "vibrate" => Ringer::Vibrate,
            "silent" => Ringer::Silent,
            _ => Ringer::Normal,
        },
        dnd: s.dnd_enabled,
    };
    *b.phone_audio.lock().unwrap() = Some(audio.clone());
    hub.publish(Event::More(Event2::PhoneAudio(audio)));
}

// ─────────────── comandos ───────────────

async fn execute_forever(b: Arc<Bridge>, mut rx: mpsc::UnboundedReceiver<Command>) {
    while let Some(c) = rx.recv().await {
        // Cada comando na sua tarefa: um envio de ficheiro ou o teste de
        // rede não seguram os comandos seguintes.
        let b = b.clone();
        tokio::spawn(async move { execute(&b, c).await });
    }
}

async fn execute(b: &Arc<Bridge>, c: Command) {
    let hub = hub::global();
    let ctx = &b.ctx;
    match c {
        Command::Ping(id) => match (b.connection(), b.linked_id()) {
            (Some(conn), Some(linked)) if linked == id => {
                hub.publish(Event::Notice(Notice::PingReply {
                    device: id,
                    rtt_ms: conn.rtt().as_secs_f32() * 1000.0,
                }));
            }
            _ => b.fail(Op::Ping, ErrorKind::Offline),
        },
        Command::SendClipboard(_) => {
            let text = blocking(|| {
                std::process::Command::new("wl-paste")
                    .args(["--no-newline", "--type", "text"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            })
            .await
            .flatten();
            match text {
                Some(t) if !t.is_empty() => send_text_to_phone(b, t).await,
                _ => b.fail(Op::Clipboard, ErrorKind::Refused),
            }
        }
        Command::SwitchWorkspace(n) => {
            let out = blocking(move || crate::hypr::dispatch(&format!("workspace {n}"))).await;
            if out.is_none_or(|o| o.starts_with("erro")) {
                b.fail(Op::Workspace, ErrorKind::Refused);
            }
        }
        Command::SetMic(on) => {
            let ok = if on {
                crate::mic::request_start(&ctx.active).await
            } else {
                crate::mic::request_stop(&ctx.active).await
            };
            if !ok {
                b.fail(Op::Mic, ErrorKind::Offline);
            }
        }
        Command::SetTap(on) => {
            if on {
                match b.connection() {
                    Some(conn) => {
                        tokio::spawn(crate::tap::start(
                            conn,
                            tap_sink(&ctx.speaker),
                            ctx.tap.clone(),
                            ctx.hud.clone(),
                        ));
                    }
                    None => b.fail(Op::Tap, ErrorKind::Offline),
                }
            } else {
                crate::tap::stop(&ctx.tap, &ctx.hud);
            }
        }
        Command::SetSpeakerMode(on) => {
            let ok = if on {
                if b.connection().is_none() {
                    b.fail(Op::Speaker, ErrorKind::Offline);
                    return;
                }
                crate::speaker::enable(&ctx.active, &ctx.tap, &ctx.hud, &ctx.config, &ctx.speaker)
                    .await
            } else {
                crate::speaker::disable(&ctx.tap, &ctx.hud, &ctx.config, &ctx.speaker).await
                    || !crate::speaker::is_active(&ctx.speaker)
            };
            if !ok {
                b.fail(Op::Speaker, ErrorKind::Refused);
            }
            hub.publish(Event::SpeakerMode(crate::speaker::is_active(&ctx.speaker)));
        }
        Command::StartMirror(_) => b.fail(Op::Mirror, ErrorKind::NotImplemented),
        Command::StopMirror => {
            hub.publish(Event::Mirror(None));
        }
        Command::SetSensorBridge(..) => b.fail(Op::Sensors, ErrorKind::NotImplemented),
        Command::SetRule(..) => b.fail(Op::Presence, ErrorKind::NotImplemented),
        Command::BeginPairing => {
            b.pairing_open.store(true, Ordering::Relaxed);
            hub.publish(Event::Pairing(b.pairing_ticket()));
        }
        Command::CancelPairing => {
            b.pairing_open.store(false, Ordering::Relaxed);
            hub.publish(Event::Pairing(None));
        }
        Command::Unpair(id) => {
            let Some(fp) = b.fingerprint_of(id) else {
                b.fail(Op::Pairing, ErrorKind::Refused);
                return;
            };
            let name = b
                .pairing
                .lock()
                .unwrap()
                .meta(&fp)
                .map(|m| m.name.clone())
                .unwrap_or_default();
            if b.pairing.lock().unwrap().revoke(&fp).is_err() {
                b.fail(Op::Pairing, ErrorKind::Refused);
                return;
            }
            if b.connected_fingerprint().as_deref() == Some(fp.as_str())
                && let Some(conn) = ctx.active.lock().unwrap().take()
            {
                conn.close(0u32.into(), b"HyprLink: dispositivo revogado");
            }
            state::push_log(&ctx.hud, format!("[i] dispositivo revogado · {fp}"));
            hub.publish(Event::Notice(Notice::Revoked { device: id, name }));
            hub.publish(Event::Devices(b.devices()));
        }
        Command::More(m) => execute_more(b, m).await,
    }
}

fn shortcuts_event(config: &config::SharedConfig) -> Event {
    Event::More(Event2::Shortcuts(
        config::shortcuts(config)
            .into_iter()
            .map(|s| Shortcut {
                label: s.name,
                dispatch: s.command,
            })
            .collect(),
    ))
}

async fn execute_more(b: &Arc<Bridge>, c: Command2) {
    let hub = hub::global();
    let ctx = &b.ctx;
    match c {
        Command2::SetBatteryAlerts(a) => {
            let mut cur = config::battery_alerts(&ctx.config);
            cur.low = a.low.is_some();
            if let Some(pct) = a.low {
                cur.low_pct = pct.min(100);
            }
            cur.full = a.full;
            config::set_battery_alerts(&ctx.config, cur);
        }
        Command2::RunDispatch(cmd) => {
            let out = blocking(move || crate::hypr::dispatch(&cmd)).await;
            if out.is_none_or(|o| o.starts_with("erro")) {
                b.fail(Op::Dispatch, ErrorKind::Refused);
            }
        }
        Command2::SetShortcuts(list) => {
            let list: Vec<config::Shortcut> = list
                .into_iter()
                .map(|s| config::Shortcut {
                    name: s.label,
                    command: s.dispatch,
                })
                .collect();
            match config::validate_shortcuts(list) {
                Ok(list) => config::set_shortcuts(&ctx.config, list),
                Err(why) => {
                    state::push_log(&ctx.hud, format!("[!] atalhos recusados · {why}"));
                }
            }
            // Devolve a lista guardada (a nova, ou a antiga se foi recusada).
            hub.publish(shortcuts_event(&ctx.config));
        }
        Command2::SetGesture(name, on) => {
            if config::set_gesture_on(&ctx.config, &name, on) {
                hub.publish(gestures_event(&ctx.config));
            } else {
                b.fail(Op::Dispatch, ErrorKind::Refused);
            }
        }
        Command2::ClearFileHistory => {
            // Só o histórico; a transferência em curso (`file_transfer`) e os
            // ficheiros no disco ficam.
            state::clear_file_history(&ctx.hud);
            hub.publish(Event::More(Event2::Transfers(b.transfers())));
        }
        Command2::SetTrackpad(t) => config::set_track_settings(
            &ctx.config,
            config::TrackSettings {
                sensitivity: t.sensitivity.clamp(0.25, 3.0),
                scroll_speed: t.scroll.clamp(0.25, 3.0),
                acceleration: t.acceleration,
                invert_scroll: t.natural_scroll,
                virtual_keyboard: t.keyboard,
            },
        ),
        Command2::StartWebcam(cfg) => {
            let codec = match cfg.codec {
                CamCodec::H264 => "h264",
                // O telemóvel usa HEVC se tiver encoder por hardware, senão
                // volta a H.264 (o efetivo vem no byte do stream).
                CamCodec::H265 => "h265",
                // O telemóvel só codifica H.264/H.265 (WebcamStreamer).
                CamCodec::Mjpeg => {
                    b.fail(Op::Webcam, ErrorKind::NotImplemented);
                    return;
                }
            };
            let ok = crate::webcam::request_start(
                &ctx.active,
                &ctx.pending_webcam,
                i64::from(cfg.width),
                i64::from(cfg.height),
                i64::from(cfg.fps),
                codec,
            )
            .await;
            if !ok {
                b.fail(Op::Webcam, ErrorKind::Offline);
            }
        }
        Command2::StopWebcam => stop_webcam(b).await,
        Command2::TestNetwork => {
            if b.net_test_running.swap(true, Ordering::Relaxed) {
                return;
            }
            let result = net_test(b).await;
            b.net_test_running.store(false, Ordering::Relaxed);
            match result {
                Some(t) => {
                    hub.publish(Event::More(Event2::NetTest(t)));
                }
                None => b.fail(Op::Webcam, ErrorKind::Offline),
            }
        }
        Command2::SetPhoneVolume(stream, level) => {
            let name = match stream {
                PhoneStream::Media => "media",
                PhoneStream::Ring => "ring",
                PhoneStream::Alarm => "alarm",
                // O protocolo phone_audio não tem o canal de notificações.
                PhoneStream::Notification => {
                    b.fail(Op::PhoneAudio, ErrorKind::NotImplemented);
                    return;
                }
            };
            if b.connection().is_none() {
                b.fail(Op::PhoneAudio, ErrorKind::Offline);
                return;
            }
            crate::phone_audio::set_volume(&ctx.active, name, i64::from(level)).await;
            refresh_phone_audio(b, hub).await;
        }
        Command2::SetRinger(r) => {
            if b.connection().is_none() {
                b.fail(Op::PhoneAudio, ErrorKind::Offline);
                return;
            }
            let mode = match r {
                Ringer::Normal => "normal",
                Ringer::Vibrate => "vibrate",
                Ringer::Silent => "silent",
            };
            crate::phone_audio::set_ringer_mode(&ctx.active, mode).await;
            refresh_phone_audio(b, hub).await;
        }
        Command2::SetDnd(on) => {
            if b.connection().is_none() {
                b.fail(Op::PhoneAudio, ErrorKind::Offline);
                return;
            }
            crate::phone_audio::set_dnd(&ctx.active, on).await;
            refresh_phone_audio(b, hub).await;
        }
        Command2::SetSinkVolume(id, v) => {
            mixer_then(hub, move || {
                crate::audio::set_volume("sink", i64::from(id), i64::from(v))
            })
            .await
        }
        Command2::SetSinkMute(id, m) => {
            mixer_then(hub, move || {
                crate::audio::set_mute("sink", i64::from(id), m)
            })
            .await
        }
        Command2::SetAppVolume(id, v) => {
            mixer_then(hub, move || {
                crate::audio::set_volume("app", i64::from(id), i64::from(v))
            })
            .await
        }
        Command2::SetAppMute(id, m) => {
            mixer_then(hub, move || crate::audio::set_mute("app", i64::from(id), m)).await
        }
        Command2::SetDefaultSink(id) => {
            mixer_then(hub, move || {
                let snap = crate::audio::snapshot();
                if let Some(s) = snap.sinks.iter().find(|s| s.id == i64::from(id)) {
                    crate::audio::set_default_sink(&s.name);
                }
            })
            .await
        }
        Command2::ReplyNotification { key, idx, text } => {
            let active = ctx.hud.lock().unwrap().modules.notif_active.clone();
            match crate::notif::reply_body(&active, &key, idx, &text) {
                Ok(body) => {
                    if crate::active::push(&ctx.active, "notification.reply", Some(body))
                        .await
                        .is_some()
                    {
                        state::push_log(&ctx.hud, format!("[i] resposta enviada · {key}"));
                    } else {
                        state::push_log(
                            &ctx.hud,
                            "[!] resposta não enviada · sem telemóvel ligado",
                        );
                    }
                }
                Err(why) => state::push_log(&ctx.hud, format!("[!] resposta recusada · {why}")),
            }
        }
        Command2::OpenOnPhone { url, package } => {
            let url = url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
            let package = package
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty());
            if url.is_none() && package.is_none() {
                state::push_log(&ctx.hud, "[!] abrir no telemóvel recusado · nada a abrir");
                return;
            }
            if let Some(u) = &url
                && !crate::ctl::http_url_ok(u)
            {
                state::push_log(&ctx.hud, "[!] abrir no telemóvel recusado · só http(s)://");
                return;
            }
            if let Some(p) = &package
                && !crate::ctl::package_ok(p)
            {
                state::push_log(
                    &ctx.hud,
                    "[!] abrir no telemóvel recusado · nome de package inválido",
                );
                return;
            }
            if b.connection().is_none() {
                b.fail(Op::Phone, ErrorKind::Offline);
                return;
            }
            if let Some(u) = url {
                if crate::ctl::phone_open_url(&ctx.active, &u).await {
                    state::push_log(&ctx.hud, format!("[i] URL enviado ao telemóvel · {u}"));
                } else {
                    b.fail(Op::Phone, ErrorKind::Offline);
                }
            }
            if let Some(p) = package {
                if crate::ctl::phone_run_app(&ctx.active, &p).await {
                    state::push_log(
                        &ctx.hud,
                        format!("[i] pedido para abrir no telemóvel · {p}"),
                    );
                } else {
                    b.fail(Op::Phone, ErrorKind::Offline);
                }
            }
        }
        Command2::DismissNotification(key) => dismiss_notification(b, key).await,
        Command2::DismissAllNotifications => {
            for key in state::active_notif_keys(&ctx.hud) {
                dismiss_notification(b, key).await;
            }
        }
        Command2::CopyClip(id) => match state::clip_entry(&ctx.hud, id).and_then(|e| e.content) {
            Some(text) => crate::clip::set_from_remote(&text, &ctx.clip_guard).await,
            None => b.fail(Op::Clipboard, ErrorKind::NotImplemented),
        },
        Command2::SendClipToPhone(id) => {
            match state::clip_entry(&ctx.hud, id).and_then(|e| e.content) {
                Some(text) => send_text_to_phone(b, text).await,
                None => b.fail(Op::Clipboard, ErrorKind::NotImplemented),
            }
        }
        Command2::PinClip(id, pinned) => {
            state::set_clip_pin(&ctx.hud, id, pinned);
            hub.publish(Event::More(Event2::Clipboard(b.clipboard())));
        }
        Command2::DeleteClip(id) => {
            state::delete_clip(&ctx.hud, id);
            hub.publish(Event::More(Event2::Clipboard(b.clipboard())));
        }
        Command2::SendFile(path) => {
            let path = std::path::PathBuf::from(path);
            if !path.is_file() {
                b.fail(Op::Files, ErrorKind::Refused);
            } else if b.connection().is_none() {
                b.fail(Op::Files, ErrorKind::Offline);
            } else {
                crate::share::send_file(&ctx.active, &ctx.hud, &path).await;
            }
        }
        Command2::CancelTransfer(id) => {
            if !state::cancel_file_transfer_id(&ctx.hud, id) {
                b.fail(Op::Files, ErrorKind::Refused);
            }
        }
        Command2::OpenDownloads => {
            let dir = config::download_dir(&ctx.config);
            let _ = std::fs::create_dir_all(&dir);
            if std::process::Command::new("xdg-open")
                .arg(&dir)
                .spawn()
                .is_err()
            {
                b.fail(Op::Files, ErrorKind::Refused);
            }
        }
        Command2::Media { player, action } => {
            let method = match action {
                MediaAction::Previous => "Previous",
                MediaAction::PlayPause => "PlayPause",
                MediaAction::Next => "Next",
            };
            let ok = match &ctx.dbus {
                Some(dbus) => crate::media::control(dbus, &player, method).await,
                None => false,
            };
            if !ok {
                b.fail(Op::Media, ErrorKind::Refused);
            }
        }
        // `phone.media` (PROTOCOL.md §phone): controla o que toca NO
        // telemóvel (MediaSession dele) — o inverso do `Command2::Media`
        // acima, que controla o MPRIS do PC. Sem resposta esperada; o
        // telemóvel atualiza sozinho via `phone.status`/`now_playing`
        // depois de agir.
        Command2::PhoneMedia(action) => {
            let action_str = match action {
                MediaAction::Previous => "previous",
                MediaAction::PlayPause => "play_pause",
                MediaAction::Next => "next",
            };
            let body = Some(ciborium::Value::Map(vec![(
                ciborium::Value::Text("action".into()),
                ciborium::Value::Text(action_str.into()),
            )]));
            if crate::active::push(&ctx.active, "phone.media", body)
                .await
                .is_none()
            {
                b.fail(Op::Media, ErrorKind::Offline);
            }
        }
        // Headset (Pista C1): coluna + mic num toggle — composição das duas
        // peças existentes; a limpeza na desconexão já está nos pontos de
        // cleanup do server.rs (speaker OFF + mic stop).
        Command2::SetHeadset(on) => {
            let ok = if on {
                crate::speaker::enable_headset(
                    &ctx.active,
                    &ctx.tap,
                    &b.ctx.hud,
                    &ctx.config,
                    &ctx.speaker,
                )
                .await
            } else {
                crate::speaker::disable_headset(
                    &ctx.active,
                    &ctx.tap,
                    &b.ctx.hud,
                    &ctx.config,
                    &ctx.speaker,
                )
                .await
            };
            if !ok {
                b.fail(Op::Speaker, ErrorKind::Offline);
            }
            hub.publish(Event::SpeakerMode(crate::speaker::is_active(&ctx.speaker)));
        }
        Command2::SetDownloadsDir(dir) => {
            config::set_download_dir(&ctx.config, std::path::Path::new(&dir));
            hub.publish(Event::More(Event2::Settings(b.settings())));
        }
        Command2::RenameDevice(id, alias) => {
            let Some(fp) = b.fingerprint_of(id) else {
                b.fail(Op::Pairing, ErrorKind::Refused);
                return;
            };
            if !b.pairing.lock().unwrap().set_alias(&fp, &alias) {
                b.fail(Op::Pairing, ErrorKind::Refused);
                return;
            }
            state::push_log(&ctx.hud, format!("[i] dispositivo renomeado · {fp}"));
            hub.publish(Event::Devices(b.devices()));
        }
        Command2::RestartDaemon => restart(b),
    }
}

/// Origem do retorno: no modo coluna, o sink virtual; senão o predefinido.
pub fn tap_sink(speaker: &crate::speaker::SpeakerHandle) -> Option<String> {
    crate::speaker::is_active(speaker).then(|| crate::audio::PHONE_SINK_NAME.to_string())
}

async fn send_text_to_phone(b: &Bridge, text: String) {
    let body = ciborium::Value::Map(vec![(
        ciborium::Value::Text("text".into()),
        ciborium::Value::Text(text.clone()),
    )]);
    match crate::active::push(&b.ctx.active, "clipboard.set", Some(body)).await {
        Some(_) => {
            state::push_clip_entry(&b.ctx.hud, state::DIR_PC_TO_PHONE, text);
            if let Some(device) = b.linked_id() {
                hub::global().publish(Event::Notice(Notice::ClipboardSent { device }));
            }
        }
        None => b.fail(Op::Clipboard, ErrorKind::Offline),
    }
}

async fn dismiss_notification(b: &Bridge, key: String) {
    let body = ciborium::Value::Map(vec![(
        ciborium::Value::Text("key".into()),
        ciborium::Value::Text(key.clone()),
    )]);
    crate::active::push(&b.ctx.active, "notification.dismiss", Some(body)).await;
    state::remove_active_notif(&b.ctx.hud, &key);
    crate::notif::dismiss_local(&key, &b.ctx.notif, &b.ctx.dbus).await;
    hub::global().publish(Event::More(Event2::Notifications(b.notifications())));
}

async fn mixer_then(hub: &Hub, f: impl FnOnce() + Send + 'static) {
    blocking(f).await;
    if let Some(m) = blocking(mixer).await {
        hub.publish(m);
    }
}

async fn stop_webcam(b: &Bridge) {
    *b.ctx.pending_webcam.lock().unwrap() = None;
    crate::active::push(&b.ctx.active, "webcam.stop", None).await;
    crate::webcam::stop(&b.ctx.webcam, &b.ctx.hud);
}

/// Teste de rede: 1080p30 H.264 durante 60 s, débito
/// médio medido pelo pipeline da câmara; RTT e perda vêm do QUIC.
async fn net_test(b: &Bridge) -> Option<NetTest> {
    let conn = b.connection()?;
    let before = conn.stats().path;
    if !crate::webcam::request_start(&b.ctx.active, &b.ctx.pending_webcam, 1920, 1080, 30, "h264")
        .await
    {
        return None;
    }
    let started = Instant::now();
    let mut samples = Vec::new();
    while started.elapsed() < NET_TEST {
        tokio::time::sleep(Duration::from_secs(1)).await;
        b.connection()?;
        if let Some(m) = b.ctx.hud.lock().unwrap().modules.webcam_mbps {
            samples.push(m);
        }
    }
    let after = conn.stats().path;
    stop_webcam(b).await;
    let sent = after.sent_packets.saturating_sub(before.sent_packets);
    let lost = after.lost_packets.saturating_sub(before.lost_packets);
    Some(NetTest {
        mbps: if samples.is_empty() {
            0.0
        } else {
            (samples.iter().sum::<f64>() / samples.len() as f64) as f32
        },
        rtt_ms: conn.rtt().as_secs_f32() * 1000.0,
        loss_pct: if sent == 0 {
            0.0
        } else {
            lost as f32 * 100.0 / sent as f32
        },
    })
}

/// Fecha o QUIC com educação e substitui o processo por um novo (mesmos
/// argumentos). Sob systemd o comportamento é o mesmo.
fn restart(b: &Bridge) {
    use std::os::unix::process::CommandExt;
    if let Some(conn) = b.ctx.active.lock().unwrap().take() {
        conn.close(0u32.into(), b"HyprLink: daemon a reiniciar");
    }
    std::thread::sleep(Duration::from_millis(150));
    if let Some(path) = hyprlink_proto::ipc::socket_path() {
        let _ = std::fs::remove_file(path);
    }
    let Ok(exe) = std::env::current_exe() else {
        b.fail(Op::Dispatch, ErrorKind::Refused);
        return;
    };
    let err = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .exec();
    state::push_log(&b.ctx.hud, format!("[!] reiniciar falhou: {err}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_stable_and_format_free() {
        let a = device_id("3F:8A:28:17:D3:25");
        let b = device_id("3f8a2817d325");
        assert_eq!(a, b);
        assert_eq!(a, 0x3f8a_2817);
        assert_eq!(wire_fingerprint("3F:8A:28"), "3f8a28");
    }

    #[test]
    fn legacy_hello_caps_get_the_implicit_set() {
        let legacy: Vec<String> = [
            "core",
            "clipboard",
            "notification",
            "media",
            "battery",
            "share",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let c = caps(&legacy);
        for want in [
            Cap::Clipboard,
            Cap::Notifications,
            Cap::Files,
            Cap::Mic,
            Cap::Webcam,
            Cap::Input,
        ] {
            assert!(c.contains(&want), "falta {want:?} em {c:?}");
        }
        assert!(!c.contains(&Cap::Mirror) && !c.contains(&Cap::Sensors));
        // Uma app que declara as capacidades estendidas só tem as que declara.
        let modern: Vec<String> = ["clipboard", "webcam"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let c = caps(&modern);
        assert!(c.contains(&Cap::Webcam) && !c.contains(&Cap::Input));
        assert!(caps(&[]).is_empty());
    }
}
