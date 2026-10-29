//! A metade JSON do `hyprlinkctl` contra um daemon falso no
//! `$XDG_RUNTIME_DIR/hyprlink.sock` (o mesmo framing do `hyprlinkd`), com o
//! daemon parado, e com `HYPRLINK_MOCK=1`. Corre o binário a sério.

use hyprlink_proto::ipc::{self, ClientMsg, PROTO_VERSION, ServerMsg};
use hyprlink_proto::link::{
    Command, Command2, Device, Event, Event2, FindPhoneState, FindStoppedBy, Kind, LinkPhase,
    LinkState, Network, Notice, PhoneStatus,
};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command as Proc, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hyprlinkctl");

fn runtime_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hyprlinkctl-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn phone() -> Device {
    Device {
        id: 7,
        name: "Xiaomi 22021211RG".into(),
        manufacturer: Some("Xiaomi".into()),
        model: Some("22021211RG".into()),
        android: Some("16".into()),
        app_version: Some("0.1.0".into()),
        kind: Kind::Phone,
        state: LinkState::Linked,
        fingerprint: "00".repeat(32),
        addr: None,
        battery: Some(49),
        charging: false,
        rssi: None,
        latency_ms: Some(12.0),
        caps: Vec::new(),
        paired_since: None,
    }
}

/// Daemon falso: em cada ligação manda Hello, o estado (telemóvel ligado),
/// Ready e depois telemetria a cada 200 ms; responde a `Ping`.
fn fake_daemon(dir: &std::path::Path) {
    let listener = UnixListener::bind(dir.join(ipc::SOCKET_NAME)).unwrap();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { return };
            std::thread::spawn(move || {
                let hello = ServerMsg::Hello {
                    proto_version: PROTO_VERSION,
                    daemon_version: "teste".into(),
                };
                let status = PhoneStatus {
                    network: Some(Network::Wifi),
                    notifications: Some(3),
                    ..Default::default()
                };
                let mut out = vec![
                    hello,
                    ServerMsg::Event(Event::Devices(vec![phone()])),
                    ServerMsg::Event(Event::Phone(status)),
                    ServerMsg::Event(Event::Link(LinkPhase::Connected)),
                    ServerMsg::Ready,
                ];
                for m in out.drain(..) {
                    if ipc::write(&mut s, &m).is_err() {
                        return;
                    }
                }
                let mut r = s.try_clone().unwrap();
                let mut w = s;
                // Comandos com resposta aqui: `Ping` e o «onde está o
                // telemóvel» (com 7 s finge uma app antiga, que não responde).
                let mut w2 = w.try_clone().unwrap();
                std::thread::spawn(move || {
                    while let Ok(ClientMsg::Command(c)) = ipc::read::<ClientMsg>(&mut r) {
                        let reply = match c {
                            Command::Ping(device) => Event::Notice(Notice::PingReply {
                                device,
                                rtt_ms: 12.34,
                            }),
                            Command::More(Command2::FindPhone(req)) if req.duration_s != 7 => {
                                Event::More(Event2::FindPhone(FindPhoneState {
                                    active: true,
                                    until_ms: Some(
                                        1_791_100_000_000 + u64::from(req.duration_s) * 1000,
                                    ),
                                    ..Default::default()
                                }))
                            }
                            Command::More(Command2::StopFindPhone) => {
                                Event::More(Event2::FindPhone(FindPhoneState {
                                    active: false,
                                    until_ms: None,
                                    last_stopped_by: Some(FindStoppedBy::Pc),
                                    last_error: None,
                                }))
                            }
                            _ => continue,
                        };
                        if ipc::write(&mut w2, &ServerMsg::Event(reply)).is_err() {
                            return;
                        }
                    }
                });
                loop {
                    let t = Event::Telemetry {
                        latency_ms: 12.0,
                        up_kbps: 1.0,
                        down_kbps: 2.0,
                    };
                    if ipc::write(&mut w, &ServerMsg::Event(t)).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            });
        }
    });
}

fn ctl(dir: &std::path::Path, args: &[&str], mock: bool) -> Proc {
    let mut c = Proc::new(BIN);
    c.args(args)
        .env("XDG_RUNTIME_DIR", dir)
        .env("LANG", "pt_PT.UTF-8")
        .env_remove("HYPRLINK_MOCK");
    if mock {
        c.env("HYPRLINK_MOCK", "1");
    }
    c
}

/// Corre um comando de uma só vez e devolve (saída JSON, sucesso).
fn once(dir: &std::path::Path, args: &[&str], mock: bool) -> (Value, bool) {
    let out = ctl(dir, args, mock).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("JSON inválido ({e}): {text:?}"));
    (v, out.status.success())
}

/// As primeiras `n` linhas de `watch --json` (e quanto tardou a primeira).
fn watch_lines(dir: &std::path::Path, n: usize, mock: bool) -> (Vec<Value>, Duration) {
    let mut child = ctl(dir, &["watch", "--json"], mock)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut out = Vec::new();
    let mut first = Duration::ZERO;
    while out.len() < n {
        let l = lines.next().expect("o watch acabou cedo").unwrap();
        if out.is_empty() {
            first = t0.elapsed();
        }
        out.push(serde_json::from_str(&l).unwrap());
    }
    let _ = child.kill();
    let _ = child.wait();
    (out, first)
}

#[test]
fn watch_real_com_telemovel_ligado() {
    let dir = runtime_dir("watch");
    fake_daemon(&dir);
    let (lines, first) = watch_lines(&dir, 2, false);
    // O plugin marca «stale» aos 5 s e espera linhas a cada 500 ms.
    assert!(first < Duration::from_secs(1), "1.ª linha em {first:?}");
    let a = &lines[0];
    assert_eq!(a["v"], 1);
    assert_eq!(a["daemon"], "up");
    assert_eq!(a["link"], "connected");
    assert_eq!(a["device"]["name"], "Xiaomi 22021211RG");
    assert_eq!(a["device"]["state"], "linked");
    assert_eq!(a["device"]["battery"], 49);
    assert_eq!(a["phone"]["network"], "wifi");
    assert_eq!(a["phone"]["notifications"], 3);
    assert!(a["host"]["hostname"].is_string());
    // A partir do 2.º instantâneo já há latência.
    assert_eq!(lines[1]["latency_ms"], 12.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn find_real_toca_para_e_avisa_sem_resposta() {
    let dir = runtime_dir("find");
    fake_daemon(&dir);
    let (v, ok) = once(&dir, &["find", "sound", "--secs", "15", "--json"], false);
    assert!(ok, "{v}");
    assert_eq!(v["find_phone"]["active"], true);
    assert_eq!(v["message"], "a tocar no telemóvel (som) · 15 s");
    let (v, ok) = once(&dir, &["find", "stop", "--json"], false);
    assert!(ok, "{v}");
    assert_eq!(v["find_phone"]["last_stopped_by"], "pc");
    // Sem phone.find.state em 5 s: app antiga.
    let (v, ok) = once(&dir, &["find", "--secs", "7", "--json"], false);
    assert!(!ok);
    assert_eq!(v["error"], "no_reply");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn status_e_ping_reais() {
    let dir = runtime_dir("status");
    fake_daemon(&dir);
    let (v, ok) = once(&dir, &["status", "--json"], false);
    assert!(ok);
    assert_eq!(v["daemon"], "up");
    assert_eq!(v["link"], "connected");
    let (v, ok) = once(&dir, &["ping", "--json"], false);
    assert!(ok, "{v}");
    assert_eq!(v["ok"], true);
    assert_eq!(v["rtt_ms"], 12.3);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn daemon_parado() {
    let dir = runtime_dir("parado");
    for cmd in [
        &["ping", "--json"][..],
        &["mic", "on", "--json"],
        &["ws", "2", "--json"],
        &["pair", "--json"],
    ] {
        let (v, ok) = once(&dir, cmd, false);
        assert!(!ok, "{cmd:?} devia falhar");
        assert_eq!(v["ok"], false, "{cmd:?}");
        assert_eq!(v["error"], "daemon_down", "{cmd:?}");
    }
    // `status` dá o instantâneo «down» (o mesmo formato do watch) e sai ≠ 0.
    let (v, ok) = once(&dir, &["status", "--json"], false);
    assert!(!ok);
    assert_eq!(v["daemon"], "down");
    // `watch` não desiste: linhas «down» seguidas.
    let (lines, _) = watch_lines(&dir, 2, false);
    assert!(lines.iter().all(|l| l["daemon"] == "down"));
    assert!(lines.iter().all(|l| l["link"] == "disconnected"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn hyprlink_mock_continua_a_dar_o_simulador() {
    let dir = runtime_dir("mock");
    // Mesmo com um daemon «real» no socket, o simulador manda.
    fake_daemon(&dir);
    let (v, ok) = once(&dir, &["status", "--json"], true);
    assert!(ok);
    assert_eq!(v["daemon"], "mock");
    let (lines, _) = watch_lines(&dir, 1, true);
    assert_eq!(lines[0]["daemon"], "mock");
    // `--mock` faz o mesmo.
    let (v, _) = once(&dir, &["status", "--json", "--mock"], false);
    assert_eq!(v["daemon"], "mock");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mirror_esta_escondido_e_nao_fala_com_o_daemon() {
    let dir = runtime_dir("mirror");
    let sock = UnixListener::bind(dir.join("hyprlink.sock")).unwrap();
    sock.set_nonblocking(true).unwrap();
    for mock in [false, true] {
        let out = ctl(&dir, &["mirror", "start", "--json"], mock)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("ainda não implementado"));
    }
    assert!(sock.accept().is_err(), "ligou ao daemon");
    let help = ctl(&dir, &["--help"], false).output().unwrap();
    assert!(!String::from_utf8_lossy(&help.stderr).contains("mirror"));
    std::fs::remove_dir_all(&dir).ok();
}
