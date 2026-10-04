//! A metade JSON do `hyprlinkctl` contra o `hyprlinkd` real, pelo socket
//! local (`hyprlink_proto::client::Session`). Mesmos subcomandos, mesmas
//! respostas e o mesmo JSON v1 que o backend simulado.

use hyprlink_proto::client::{Session, SessionError};
use hyprlink_proto::host::Probe;
use hyprlink_proto::link::{
    Codec, Command, DeviceId, ErrorKind, Event, LinkState, MirrorConfig, Notice, Op,
};
use hyprlink_proto::snapshot::Snapshot;
use serde_json::json;
use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const OPEN_TIMEOUT: Duration = Duration::from_secs(2);
/// Quanto esperar por um `Notice::Failed` depois de um comando sem resposta
/// positiva própria (mic, tap, ws…). Sem falha nesse tempo = aceite.
const GRACE: Duration = Duration::from_millis(600);

/// O que o JSON v1 precisa e o `Snapshot::apply` não deduz sozinho.
struct State {
    snap: Snapshot,
    linked: Option<(DeviceId, String)>,
    speaker: bool,
}

impl State {
    fn new() -> Self {
        State {
            snap: Snapshot::new("up"),
            linked: None,
            speaker: false,
        }
    }

    fn apply(&mut self, e: &Event) {
        self.snap.apply(e);
        match e {
            Event::Devices(list) => {
                self.linked = list
                    .iter()
                    .find(|d| d.state == LinkState::Linked)
                    .map(|d| (d.id, d.name.clone()));
            }
            // O daemon só mede um canal enquanto ele está ligado.
            Event::Levels { mic, tap } => {
                self.snap.mic.on = mic.is_some();
                self.snap.tap.on = tap.is_some();
            }
            Event::SpeakerMode(on) => self.speaker = *on,
            _ => {}
        }
    }
}

fn open() -> Result<(Session, State), SessionError> {
    let (session, events) = Session::open(OPEN_TIMEOUT)?;
    let mut st = State::new();
    for e in &events {
        st.apply(e);
    }
    Ok((session, st))
}

fn print_json(v: &impl serde::Serialize) {
    println!(
        "{}",
        serde_json::to_string(v).unwrap_or_else(|_| "{}".into())
    );
}

fn ok(json_out: bool, msg: &str, extra: serde_json::Value) -> ExitCode {
    if json_out {
        let mut v = json!({ "ok": true, "message": msg });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        print_json(&v);
    } else {
        println!("{msg}");
    }
    ExitCode::SUCCESS
}

fn fail(json_out: bool, msg: &str, error: &str) -> ExitCode {
    if json_out {
        print_json(&json!({ "ok": false, "message": msg, "error": error }));
    } else {
        eprintln!("erro: {msg}");
    }
    ExitCode::from(1)
}

fn session_fail(json_out: bool, e: &SessionError) -> ExitCode {
    let code = match e {
        SessionError::Down => "daemon_down",
        SessionError::Incompatible { .. } => "incompatible",
        SessionError::Timeout => "timeout",
        SessionError::Frame(_) => "protocol",
    };
    fail(json_out, &e.to_string(), code)
}

fn error_code(e: ErrorKind) -> (&'static str, &'static str) {
    match e {
        ErrorKind::NotImplemented => ("not_implemented", "por implementar"),
        ErrorKind::Offline => ("offline", "sem telemóvel ligado"),
        ErrorKind::Refused => ("refused", "recusado"),
        ErrorKind::Timeout => ("timeout", "sem resposta"),
    }
}

/// Envia `c` e espera `GRACE` por uma falha da mesma operação.
fn send_and_check(s: &mut Session, c: Command, op: Op) -> Result<(), ErrorKind> {
    s.send(c).map_err(|_| ErrorKind::Timeout)?;
    match s.wait_for(GRACE, |e| match e {
        Event::Notice(Notice::Failed { op: o, error }) if *o == op => Some(*error),
        _ => None,
    }) {
        Ok(err) => Err(err),
        Err(_) => Ok(()),
    }
}

fn toggle(arg: Option<&str>, current: bool) -> Option<bool> {
    match arg {
        Some("on") | Some("start") => Some(true),
        Some("off") | Some("stop") => Some(false),
        Some("toggle") | None => Some(!current),
        _ => None,
    }
}

const MIRROR: MirrorConfig = MirrorConfig {
    codec: Codec::Hevc,
    bitrate_mbps: 12.0,
    max_fps: 60,
    scale: 0.75,
};

/// `open`: foca a GUI que já corre, ou lança-a.
pub fn open_gui(json_out: bool) -> ExitCode {
    let sock = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| std::path::PathBuf::from(d).join("hyprlink-gui.sock"));
    if let Some(mut s) = sock.and_then(|p| std::os::unix::net::UnixStream::connect(p).ok()) {
        let _ = s.write_all(b"open\n");
        return ok(json_out, "HYPRLINK em primeiro plano", json!({ "running": true }));
    }
    match std::process::Command::new("hyprlink-gui").spawn() {
        Ok(_) => ok(json_out, "a abrir HYPRLINK", json!({ "running": false })),
        Err(e) => fail(
            json_out,
            &format!("não foi possível abrir hyprlink-gui: {e}"),
            "spawn",
        ),
    }
}

pub fn watch(json_out: bool, interval: Duration, human: fn(&Snapshot) -> String) -> ExitCode {
    let mut probe = Probe::default();
    let mut host = probe.sample();
    let mut last_host = Instant::now();
    let stdout = std::io::stdout();
    loop {
        let mut session = open().ok();
        let mut st = session.as_ref().map(|_| ()).map_or_else(
            || {
                let mut s = State::new();
                s.snap = Snapshot::new("down");
                s
            },
            |_| State::new(),
        );
        if let Ok((s, state)) = session.take().map_or(Err(()), |_| open().map_err(|_| ())) {
            session = Some(s);
            st = state;
        }
        loop {
            let deadline = Instant::now() + interval;
            if let Some(s) = session.as_mut() {
                // Absorve tudo o que chegar até ao próximo instantâneo.
                loop {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    match s.next(left) {
                        Ok(e) => st.apply(&e),
                        Err(SessionError::Timeout) => break,
                        Err(_) => {
                            session = None;
                            break;
                        }
                    }
                }
            } else {
                std::thread::sleep(interval);
            }
            if last_host.elapsed() > Duration::from_secs(2) {
                last_host = Instant::now();
                host = probe.sample();
            }
            st.snap.host = Some(host.clone());
            if session.is_none() {
                st.snap = Snapshot::new("down");
                st.snap.host = Some(host.clone());
            }
            let line = if json_out {
                serde_json::to_string(&st.snap).unwrap_or_default()
            } else {
                human(&st.snap).replace('\n', "  ")
            };
            let mut out = stdout.lock();
            if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
                return ExitCode::SUCCESS;
            }
            if session.is_none() {
                break; // volta a tentar ligar
            }
        }
    }
}

pub fn status(json_out: bool, human: fn(&Snapshot) -> String) -> ExitCode {
    let mut probe = Probe::default();
    let (snap, code) = match open() {
        Ok((_, st)) => (st.snap, ExitCode::SUCCESS),
        Err(_) => (Snapshot::new("down"), ExitCode::from(1)),
    };
    let mut snap = snap;
    snap.host = Some(probe.sample());
    if json_out {
        print_json(&snap);
    } else {
        println!("{}", human(&snap));
    }
    code
}

pub fn ping(json_out: bool) -> ExitCode {
    let (mut s, st) = match open() {
        Ok(x) => x,
        Err(e) => return session_fail(json_out, &e),
    };
    let Some((id, name)) = st.linked else {
        return fail(json_out, "sem telemóvel ligado", "offline");
    };
    if s.send(Command::Ping(id)).is_err() {
        return fail(json_out, "o daemon fechou a ligação", "protocol");
    }
    let r = s.wait_for(Duration::from_secs(3), |e| match e {
        Event::Notice(Notice::PingReply { device, rtt_ms }) if *device == id => Some(Ok(*rtt_ms)),
        Event::Notice(Notice::Failed { op: Op::Ping, error }) => Some(Err(*error)),
        _ => None,
    });
    match r {
        Ok(Ok(rtt)) => {
            let rtt = (rtt * 10.0).round() / 10.0;
            ok(
                json_out,
                &format!("{name} respondeu em {rtt} ms"),
                json!({ "device": name, "rtt_ms": rtt }),
            )
        }
        Ok(Err(e)) => {
            let (code, msg) = error_code(e);
            fail(json_out, msg, code)
        }
        Err(e) => session_fail(json_out, &e),
    }
}

pub fn clipboard(json_out: bool) -> ExitCode {
    let (mut s, st) = match open() {
        Ok(x) => x,
        Err(e) => return session_fail(json_out, &e),
    };
    let Some((id, _)) = st.linked else {
        return fail(json_out, "sem telemóvel ligado", "offline");
    };
    if s.send(Command::SendClipboard(id)).is_err() {
        return fail(json_out, "o daemon fechou a ligação", "protocol");
    }
    let r = s.wait_for(Duration::from_secs(3), |e| match e {
        Event::Notice(Notice::ClipboardSent { .. }) => Some(Ok(())),
        Event::Notice(Notice::Failed {
            op: Op::Clipboard,
            error,
        }) => Some(Err(*error)),
        _ => None,
    });
    match r {
        Ok(Ok(())) => ok(json_out, "área de transferência enviada", json!({})),
        Ok(Err(e)) => {
            let (code, msg) = error_code(e);
            fail(json_out, msg, code)
        }
        Err(e) => session_fail(json_out, &e),
    }
}

pub fn pair(json_out: bool) -> ExitCode {
    let (mut s, _) = match open() {
        Ok(x) => x,
        Err(e) => return session_fail(json_out, &e),
    };
    if s.send(Command::BeginPairing).is_err() {
        return fail(json_out, "o daemon fechou a ligação", "protocol");
    }
    match s.wait_for(Duration::from_secs(2), |e| match e {
        Event::Pairing(Some(t)) => Some(t.clone()),
        _ => None,
    }) {
        Ok(t) => ok(
            json_out,
            "emparelhamento aberto",
            json!({ "payload": t.payload, "code": t.code, "expires_in": t.expires_in }),
        ),
        Err(e) => session_fail(json_out, &e),
    }
}

pub fn channel(json_out: bool, which: &str, arg: Option<&str>) -> Option<ExitCode> {
    let (mut s, st) = match open() {
        Ok(x) => x,
        Err(e) => return Some(session_fail(json_out, &e)),
    };
    let current = match which {
        "mic" => st.snap.mic.on,
        "tap" => st.snap.tap.on,
        "speaker" => st.speaker,
        _ => st.snap.mirror.on,
    };
    let v = toggle(arg, current)?;
    let (cmd, op) = match which {
        "mic" => (Command::SetMic(v), Op::Mic),
        "tap" => (Command::SetTap(v), Op::Tap),
        "speaker" => (Command::SetSpeakerMode(v), Op::Speaker),
        _ => (
            if v {
                Command::StartMirror(MIRROR)
            } else {
                Command::StopMirror
            },
            Op::Mirror,
        ),
    };
    Some(match send_and_check(&mut s, cmd, op) {
        Ok(()) => ok(
            json_out,
            &format!("{which} {}", if v { "on" } else { "off" }),
            json!({ which: v }),
        ),
        Err(e) => {
            let (code, msg) = error_code(e);
            fail(json_out, &format!("{which}: {msg}"), code)
        }
    })
}

pub fn workspace(json_out: bool, n: u8) -> ExitCode {
    let (mut s, _) = match open() {
        Ok(x) => x,
        Err(e) => return session_fail(json_out, &e),
    };
    match send_and_check(&mut s, Command::SwitchWorkspace(n), Op::Workspace) {
        Ok(()) => ok(
            json_out,
            &format!("workspace {n}"),
            json!({ "workspace": n }),
        ),
        Err(e) => {
            let (code, msg) = error_code(e);
            fail(json_out, msg, code)
        }
    }
}
