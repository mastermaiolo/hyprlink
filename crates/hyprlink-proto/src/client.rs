//! Cliente do socket local (`$XDG_RUNTIME_DIR/hyprlink.sock`), só com `std`.
//!
//! - [`Socket`]: o [`Transport`] da GUI. Uma thread liga ao daemon, volta a
//!   ligar com backoff (250 ms → 5 s) e entrega os eventos por canal; o
//!   `poll()` da GUI nunca bloqueia. O estado da ligação está em
//!   [`Socket::status`].
//! - [`Session`]: uma ligação bloqueante para o `hyprlinkctl` (lê o estado
//!   completo até `Ready`, envia um comando, espera pela resposta).

use crate::ipc::{self, ClientMsg, FrameError, PROTO_VERSION, ServerMsg};
use crate::link::{Command, Event, Notice, Op, Transport};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Estado da ligação ao daemon (do lado do cliente; não vem no wire).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkStatus {
    Connecting,
    /// Sem socket ou ninguém a responder.
    Down,
    /// O daemon fala outra versão do contrato.
    Incompatible {
        daemon: u32,
    },
    Up {
        daemon_version: String,
    },
}

enum Item {
    Status(LinkStatus),
    Event(Event),
}

const BACKOFF_MIN: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);

pub struct Socket {
    commands: Sender<Command>,
    items: Receiver<Item>,
    status: LinkStatus,
}

impl Socket {
    /// Começa a ligar em segundo plano; nunca falha nem bloqueia.
    pub fn connect() -> Self {
        Self::connect_to(ipc::socket_path())
    }

    pub fn connect_to(path: Option<PathBuf>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (item_tx, item_rx) = mpsc::channel();
        let cmd_rx = Arc::new(Mutex::new(cmd_rx));
        std::thread::Builder::new()
            .name("hyprlink-ipc".into())
            .spawn(move || run(path, cmd_rx, item_tx))
            .expect("thread do cliente IPC");
        Socket {
            commands: cmd_tx,
            items: item_rx,
            status: LinkStatus::Connecting,
        }
    }

    pub fn status(&self) -> &LinkStatus {
        &self.status
    }
}

impl Transport for Socket {
    /// Com o daemon em baixo, a thread responde `Failed{op, Offline}` em vez
    /// de guardar o comando para mais tarde.
    fn send(&mut self, command: Command) {
        let _ = self.commands.send(command);
    }

    fn poll(&mut self, _dt: Duration) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(item) = self.items.try_recv() {
            match item {
                Item::Event(e) => out.push(e),
                Item::Status(s) => {
                    let was_up = matches!(self.status, LinkStatus::Up { .. });
                    let is_up = matches!(s, LinkStatus::Up { .. });
                    if was_up && !is_up {
                        // O daemon caiu: o que a UI sabia já não é verdade.
                        out.extend(forget_all());
                    }
                    self.status = s;
                }
            }
        }
        out
    }
}

/// Eventos que levam a UI aos estados vazios quando o daemon desaparece.
fn forget_all() -> Vec<Event> {
    vec![
        Event::Devices(Vec::new()),
        Event::Pairing(None),
        Event::Levels {
            mic: None,
            tap: None,
        },
        Event::Mirror(None),
    ]
}

/// A operação a que um comando pertence, para responder com
/// `Notice::Failed` quando não há daemon.
pub fn op_of(c: &Command) -> Option<Op> {
    use crate::link::Command2 as C2;
    Some(match c {
        Command::Ping(_) => Op::Ping,
        Command::SendClipboard(_) => Op::Clipboard,
        Command::SwitchWorkspace(_) => Op::Workspace,
        Command::SetMic(_) => Op::Mic,
        Command::SetTap(_) => Op::Tap,
        Command::SetSpeakerMode(_) => Op::Speaker,
        Command::StartMirror(_) | Command::StopMirror => Op::Mirror,
        Command::SetSensorBridge(..) => Op::Sensors,
        Command::SetRule(..) => Op::Presence,
        Command::BeginPairing | Command::CancelPairing | Command::Unpair(_) => Op::Pairing,
        Command::More(m) => match m {
            C2::RunDispatch(_) | C2::SetShortcuts(_) | C2::SetGesture(..) => Op::Dispatch,
            C2::StartWebcam(_) | C2::StopWebcam | C2::TestNetwork => Op::Webcam,
            C2::SetPhoneVolume(..) | C2::SetRinger(_) | C2::SetDnd(_) => Op::PhoneAudio,
            C2::CopyClip(_) | C2::SendClipToPhone(_) | C2::PinClip(..) | C2::DeleteClip(_) => {
                Op::Clipboard
            }
            C2::SendFile(_) | C2::CancelTransfer(_) | C2::OpenDownloads | C2::ClearFileHistory => {
                Op::Files
            }
            C2::Media { .. } | C2::PhoneMedia(_) => Op::Media,
            C2::SetHeadset(_) => Op::Speaker,
            C2::DismissNotification(_)
            | C2::DismissAllNotifications
            | C2::SetBatteryAlerts(_)
            | C2::SetTrackpad(_)
            | C2::SetSinkVolume(..)
            | C2::SetSinkMute(..)
            | C2::SetDefaultSink(_)
            | C2::SetAppVolume(..)
            | C2::SetAppMute(..)
            | C2::SetDownloadsDir(_)
            | C2::RenameDevice(..)
            | C2::RestartDaemon => return None,
        },
    })
}

fn run(path: Option<PathBuf>, commands: Arc<Mutex<Receiver<Command>>>, items: Sender<Item>) {
    let mut backoff = BACKOFF_MIN;
    loop {
        let stream = path.as_ref().and_then(|p| UnixStream::connect(p).ok());
        let Some(mut stream) = stream else {
            if items.send(Item::Status(LinkStatus::Down)).is_err() {
                return;
            }
            // Comandos pedidos sem daemon: falham já, não ficam à espera.
            drain_offline(&commands, &items, backoff);
            backoff = (backoff * 2).min(BACKOFF_MAX);
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let hello: Result<ServerMsg, _> = ipc::read(&mut stream);
        let daemon_version = match hello {
            Ok(ServerMsg::Hello {
                proto_version,
                daemon_version,
            }) if proto_version == PROTO_VERSION => daemon_version,
            Ok(ServerMsg::Hello { proto_version, .. }) => {
                if items
                    .send(Item::Status(LinkStatus::Incompatible {
                        daemon: proto_version,
                    }))
                    .is_err()
                {
                    return;
                }
                drain_offline(&commands, &items, BACKOFF_MAX);
                continue;
            }
            _ => {
                drain_offline(&commands, &items, backoff);
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        backoff = BACKOFF_MIN;
        let _ = stream.set_read_timeout(None);
        if items
            .send(Item::Status(LinkStatus::Up { daemon_version }))
            .is_err()
        {
            return;
        }

        let alive = Arc::new(AtomicBool::new(true));
        let writer = stream.try_clone().ok().map(|mut w| {
            let (alive, commands) = (alive.clone(), commands.clone());
            std::thread::spawn(move || {
                while alive.load(Ordering::Relaxed) {
                    let next = commands
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_millis(100));
                    match next {
                        Ok(c) => {
                            if ipc::write(&mut w, &ClientMsg::Command(c)).is_err() {
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
        });

        loop {
            match ipc::read::<ServerMsg>(&mut stream) {
                Ok(ServerMsg::Event(e)) => {
                    if items.send(Item::Event(e)).is_err() {
                        alive.store(false, Ordering::Relaxed);
                        return;
                    }
                }
                Ok(ServerMsg::Hello { .. } | ServerMsg::Ready) => {}
                Err(_) => break,
            }
        }
        alive.store(false, Ordering::Relaxed);
        let _ = stream.shutdown(std::net::Shutdown::Both);
        if let Some(w) = writer {
            let _ = w.join();
        }
        if items.send(Item::Status(LinkStatus::Connecting)).is_err() {
            return;
        }
        std::thread::sleep(BACKOFF_MIN);
    }
}

/// Espera `wait` a responder `Failed{op, Offline}` aos comandos que chegarem.
fn drain_offline(commands: &Arc<Mutex<Receiver<Command>>>, items: &Sender<Item>, wait: Duration) {
    let until = Instant::now() + wait;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        let next = commands.lock().unwrap().recv_timeout(left);
        match next {
            Ok(c) => {
                if let Some(op) = op_of(&c) {
                    let _ = items.send(Item::Event(Event::Notice(Notice::Failed {
                        op,
                        error: crate::link::ErrorKind::Offline,
                    })));
                }
            }
            Err(RecvTimeoutError::Timeout) => return,
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

// ───────────────────────── sessão bloqueante ─────────────────────────

#[derive(Debug)]
pub enum SessionError {
    /// Sem `XDG_RUNTIME_DIR` ou sem daemon no socket.
    Down,
    Incompatible {
        daemon: u32,
    },
    Frame(FrameError),
    Timeout,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Down => write!(f, "daemon parado (sem hyprlink.sock)"),
            SessionError::Incompatible { daemon } => write!(
                f,
                "versão incompatível: daemon fala v{daemon}, este cliente v{PROTO_VERSION}"
            ),
            SessionError::Frame(e) => write!(f, "{e}"),
            SessionError::Timeout => write!(f, "o daemon não respondeu a tempo"),
        }
    }
}

impl std::error::Error for SessionError {}

pub struct Session {
    stream: UnixStream,
    pub daemon_version: String,
}

impl Session {
    /// Liga e lê o estado completo (até `Ready`).
    pub fn open(timeout: Duration) -> Result<(Session, Vec<Event>), SessionError> {
        let path = ipc::socket_path().ok_or(SessionError::Down)?;
        let mut stream = UnixStream::connect(&path).map_err(|_| SessionError::Down)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| SessionError::Frame(FrameError::Io(e)))?;
        let daemon_version = match ipc::read(&mut stream).map_err(frame_err)? {
            ServerMsg::Hello {
                proto_version,
                daemon_version,
            } if proto_version == PROTO_VERSION => daemon_version,
            ServerMsg::Hello { proto_version, .. } => {
                return Err(SessionError::Incompatible {
                    daemon: proto_version,
                });
            }
            _ => return Err(SessionError::Frame(FrameError::Decode("sem Hello".into()))),
        };
        let mut state = Vec::new();
        loop {
            match ipc::read(&mut stream).map_err(frame_err)? {
                ServerMsg::Event(e) => state.push(e),
                ServerMsg::Ready => break,
                ServerMsg::Hello { .. } => {}
            }
        }
        Ok((
            Session {
                stream,
                daemon_version,
            },
            state,
        ))
    }

    pub fn send(&mut self, c: Command) -> Result<(), SessionError> {
        ipc::write(&mut self.stream, &ClientMsg::Command(c)).map_err(SessionError::Frame)
    }

    /// Próximo evento, ou `Timeout`.
    pub fn next(&mut self, timeout: Duration) -> Result<Event, SessionError> {
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| SessionError::Frame(FrameError::Io(e)))?;
        loop {
            match ipc::read(&mut self.stream).map_err(frame_err)? {
                ServerMsg::Event(e) => return Ok(e),
                ServerMsg::Hello { .. } | ServerMsg::Ready => {}
            }
        }
    }

    /// Espera até `f` aceitar um evento (devolve o que `f` devolver).
    pub fn wait_for<T>(
        &mut self,
        timeout: Duration,
        mut f: impl FnMut(&Event) -> Option<T>,
    ) -> Result<T, SessionError> {
        let until = Instant::now() + timeout;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(SessionError::Timeout);
            }
            let e = self.next(left)?;
            if let Some(t) = f(&e) {
                return Ok(t);
            }
        }
    }
}

fn frame_err(e: FrameError) -> SessionError {
    match e {
        FrameError::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            SessionError::Timeout
        }
        other => SessionError::Frame(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::ErrorKind;
    use std::os::unix::net::UnixListener;

    fn temp_socket(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "hyprlink-client-{}-{name}-{}.sock",
            std::process::id(),
            Instant::now().elapsed().as_nanos() ^ (name.len() as u128 * 7919)
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn poll_until(s: &mut Socket, f: impl Fn(&Socket, &[Event]) -> bool) -> Vec<Event> {
        let until = Instant::now() + Duration::from_secs(3);
        let mut all = Vec::new();
        while Instant::now() < until {
            all.extend(s.poll(Duration::ZERO));
            if f(s, &all) {
                return all;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "condição não chegou; status {:?}, eventos {all:?}",
            s.status()
        );
    }

    #[test]
    fn down_then_up_then_down_again() {
        let path = temp_socket("updown");
        let mut s = Socket::connect_to(Some(path.clone()));
        // Sem daemon: Down, e um comando falha logo.
        poll_until(&mut s, |s, _| *s.status() == LinkStatus::Down);
        s.send(Command::SetMic(true));
        let ev = poll_until(&mut s, |_, ev| !ev.is_empty());
        assert!(matches!(
            ev[0],
            Event::Notice(Notice::Failed {
                op: Op::Mic,
                error: ErrorKind::Offline
            })
        ));

        // Daemon falso aparece.
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            ipc::write(
                &mut c,
                &ServerMsg::Hello {
                    proto_version: PROTO_VERSION,
                    daemon_version: "9.9.9".into(),
                },
            )
            .unwrap();
            ipc::write(&mut c, &ServerMsg::Event(Event::Rssi(-40))).unwrap();
            ipc::write(&mut c, &ServerMsg::Ready).unwrap();
            // Recebe um comando e sai (o daemon "cai").
            let got: ClientMsg = ipc::read(&mut c).unwrap();
            got
        });
        let ev = poll_until(&mut s, |s, ev| {
            matches!(s.status(), LinkStatus::Up { .. }) && !ev.is_empty()
        });
        assert_eq!(
            *s.status(),
            LinkStatus::Up {
                daemon_version: "9.9.9".into()
            }
        );
        assert!(matches!(ev[0], Event::Rssi(-40)));
        s.send(Command::SetTap(true));
        let got = server.join().unwrap();
        assert!(matches!(got, ClientMsg::Command(Command::SetTap(true))));

        // O daemon caiu: a UI recebe os eventos que a esvaziam.
        let ev = poll_until(&mut s, |s, _| !matches!(s.status(), LinkStatus::Up { .. }));
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Devices(d) if d.is_empty()))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn incompatible_daemon_is_reported() {
        let path = temp_socket("incompat");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let mut c = c.unwrap();
                let _ = ipc::write(
                    &mut c,
                    &ServerMsg::Hello {
                        proto_version: PROTO_VERSION + 1,
                        daemon_version: "x".into(),
                    },
                );
            }
        });
        let mut s = Socket::connect_to(Some(path.clone()));
        poll_until(&mut s, |s, _| {
            *s.status()
                == LinkStatus::Incompatible {
                    daemon: PROTO_VERSION + 1,
                }
        });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn every_command_maps_to_an_op_or_is_silent() {
        // Compila só se o match de op_of for exaustivo; aqui só o uso.
        assert_eq!(op_of(&Command::Ping(1)), Some(Op::Ping));
        assert_eq!(
            op_of(&Command::More(crate::link::Command2::RestartDaemon)),
            None
        );
    }
}
