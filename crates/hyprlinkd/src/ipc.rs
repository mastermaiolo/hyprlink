//! Socket local `$XDG_RUNTIME_DIR/hyprlink.sock` para a GUI e o
//! `hyprlinkctl` (envelope em `hyprlink_proto::ipc`).
//!
//! Cada cliente recebe `Hello`, o estado completo do [`Hub`] e depois os
//! eventos. Os `Command`s que envia seguem para o canal `commands`; quem os
//! executa (`bridge.rs`) responde com eventos (`Notice`, estado novo) no hub.
//!
//! Coalescência por cliente: entre o hub e o socket há uma fila
//! ([`Pending`]) onde um evento de estado substitui o anterior da mesma
//! chave sem perder a posição. Um cliente lento (ou parado) nunca acumula
//! níveis de áudio atrasados — recebe o mais recente.

use crate::hub::{self, Hub, StateKey};
use hyprlink_proto::ipc::{self, ClientMsg, PROTO_VERSION, ServerMsg};
use hyprlink_proto::link::{Command, Event};
use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, broadcast, mpsc};

/// Teto da fila por cliente. Só os acontecimentos (Packet, Notice) a fazem
/// crescer; o estado ocupa no máximo uma entrada por chave.
const MAX_PENDING: usize = 512;

/// Fila de saída de um cliente, com substituição por chave de estado.
#[derive(Default)]
pub struct Pending {
    q: VecDeque<(Option<StateKey>, Event)>,
}

impl Pending {
    pub fn push(&mut self, e: Event) {
        let k = hub::key(&e);
        if let Some(k) = k
            && let Some(slot) = self.q.iter_mut().find(|(sk, _)| *sk == Some(k))
        {
            slot.1 = e;
            return;
        }
        if self.q.len() >= MAX_PENDING {
            // Larga o acontecimento mais antigo; o estado nunca se perde.
            if let Some(i) = self.q.iter().position(|(sk, _)| sk.is_none()) {
                self.q.remove(i);
            } else {
                return;
            }
        }
        self.q.push_back((k, e));
    }

    pub fn drain(&mut self) -> Vec<Event> {
        self.q.drain(..).map(|(_, e)| e).collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.q.len()
    }
}

#[derive(Debug)]
pub enum ServeError {
    /// Já há um daemon a responder neste socket.
    InUse(PathBuf),
    Io(std::io::Error),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServeError::InUse(p) => write!(f, "outro daemon já responde em {}", p.display()),
            ServeError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Liga o socket (0600). Um socket morto de um arranque anterior é removido;
/// um vivo é de outro daemon e não se toca.
pub async fn bind(path: &Path) -> Result<UnixListener, ServeError> {
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            return Err(ServeError::InUse(path.to_path_buf()));
        }
        let _ = std::fs::remove_file(path);
    }
    let listener = UnixListener::bind(path).map_err(ServeError::Io)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(ServeError::Io)?;
    Ok(listener)
}

/// Aceita clientes para sempre.
pub async fn serve(listener: UnixListener, hub: Hub, commands: mpsc::UnboundedSender<Command>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(client(stream, hub.clone(), commands.clone()));
            }
            Err(e) => {
                eprintln!("[!] ipc: accept falhou: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
}

async fn client(stream: UnixStream, hub: Hub, commands: mpsc::UnboundedSender<Command>) {
    let (mut rd, mut wr) = stream.into_split();
    let pending = Arc::new(Mutex::new(Pending::default()));
    let wake = Arc::new(Notify::new());

    // Subscrever antes de copiar o estado: nada se perde entre os dois
    // (um evento repetido é inofensivo — estado é idempotente).
    let mut rx = hub.subscribe();
    {
        let mut p = pending.lock().unwrap();
        for e in hub.snapshot() {
            p.push(e);
        }
    }
    wake.notify_one();

    let feeder = {
        let (pending, wake, hub) = (pending.clone(), wake.clone(), hub.clone());
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(e) => pending.lock().unwrap().push(e),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Perdeu acontecimentos; o estado repõe-se inteiro.
                        let mut p = pending.lock().unwrap();
                        for e in hub.snapshot() {
                            p.push(e);
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
                wake.notify_one();
            }
        })
    };

    let writer = {
        let (pending, wake) = (pending.clone(), wake.clone());
        tokio::spawn(async move {
            let hello = ServerMsg::Hello {
                proto_version: PROTO_VERSION,
                daemon_version: env!("CARGO_PKG_VERSION").to_string(),
            };
            if wr.write_all(&ipc::encode(&hello)).await.is_err() {
                return;
            }
            loop {
                wake.notified().await;
                let batch = pending.lock().unwrap().drain();
                for e in batch {
                    if wr
                        .write_all(&ipc::encode(&ServerMsg::Event(e)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        })
    };

    // Leitor: corre nesta tarefa; quando o cliente fecha, acaba tudo.
    loop {
        let mut prefix = [0u8; 4];
        if rd.read_exact(&mut prefix).await.is_err() {
            break;
        }
        let Ok(len) = ipc::body_len(prefix) else {
            break;
        };
        let mut body = vec![0u8; len];
        if rd.read_exact(&mut body).await.is_err() {
            break;
        }
        match ipc::decode::<ClientMsg>(&body) {
            Ok(ClientMsg::Command(c)) => {
                if commands.send(c).is_err() {
                    break;
                }
            }
            Err(e) => {
                eprintln!("[!] ipc: frame inválido de um cliente: {e}");
                break;
            }
        }
    }
    feeder.abort();
    writer.abort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprlink_proto::link::{Notice, Op};
    use std::time::Duration;

    fn temp_socket(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "hyprlink-test-{}-{name}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    async fn read_msg(s: &mut UnixStream) -> ServerMsg {
        let mut prefix = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut prefix))
            .await
            .expect("timeout a ler")
            .unwrap();
        let mut body = vec![0u8; ipc::body_len(prefix).unwrap()];
        s.read_exact(&mut body).await.unwrap();
        ipc::decode(&body).unwrap()
    }

    async fn read_event(s: &mut UnixStream) -> Event {
        match read_msg(s).await {
            ServerMsg::Event(e) => e,
            other => panic!("esperava Event, veio {other:?}"),
        }
    }

    /// Daemon com estado falso ↔ cliente real pelo socket.
    #[tokio::test]
    async fn hello_state_events_and_commands() {
        let path = temp_socket("e2e");
        let hub = Hub::new();
        hub.publish(Event::Rssi(-60));
        hub.publish(Event::SpeakerMode(true));
        let (tx, mut cmds) = mpsc::unbounded_channel();
        let listener = bind(&path).await.unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "permissões do socket");
        tokio::spawn(serve(listener, hub.clone(), tx));

        let mut a = UnixStream::connect(&path).await.unwrap();
        let mut b = UnixStream::connect(&path).await.unwrap();
        for s in [&mut a, &mut b] {
            match read_msg(s).await {
                ServerMsg::Hello { proto_version, .. } => assert_eq!(proto_version, PROTO_VERSION),
                other => panic!("primeira mensagem devia ser Hello: {other:?}"),
            }
            // Estado completo, por ordem de chave.
            assert!(matches!(read_event(s).await, Event::SpeakerMode(true)));
            assert!(matches!(read_event(s).await, Event::Rssi(-60)));
        }

        // Evento ao vivo chega aos dois.
        hub.publish(Event::Notice(Notice::Failed {
            op: Op::Mirror,
            error: hyprlink_proto::link::ErrorKind::NotImplemented,
        }));
        for s in [&mut a, &mut b] {
            assert!(matches!(
                read_event(s).await,
                Event::Notice(Notice::Failed { op: Op::Mirror, .. })
            ));
        }

        // Comando de um cliente chega ao executor.
        a.write_all(&ipc::encode(&ClientMsg::Command(Command::SetMic(true))))
            .await
            .unwrap();
        let got = tokio::time::timeout(Duration::from_secs(2), cmds.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(got, Command::SetMic(true)));

        // Um cliente que sai não afeta o outro.
        drop(a);
        hub.publish(Event::Rssi(-61));
        assert!(matches!(read_event(&mut b).await, Event::Rssi(-61)));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn stale_socket_replaced_live_one_refused() {
        let path = temp_socket("stale");
        // Socket morto: um ficheiro de socket sem ninguém a ouvir.
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        let live = bind(&path)
            .await
            .expect("socket morto devia ser substituído");
        // Agora há alguém vivo: um segundo daemon não lhe toca.
        assert!(matches!(bind(&path).await, Err(ServeError::InUse(_))));
        drop(live);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pending_coalesces_state_keeps_happenings() {
        let mut p = Pending::default();
        p.push(Event::Rssi(-50));
        for i in 0..1000 {
            p.push(Event::Levels {
                mic: None,
                tap: Some(i as f32 / 1000.0),
            });
        }
        p.push(Event::Notice(Notice::MirrorStarted));
        p.push(Event::Rssi(-55));
        let out = p.drain();
        assert_eq!(out.len(), 3, "{out:?}");
        // A posição é a da primeira ocorrência; o valor é o último.
        assert!(matches!(out[0], Event::Rssi(-55)));
        assert!(matches!(out[1], Event::Levels { tap: Some(t), .. } if t > 0.99));
        assert!(matches!(out[2], Event::Notice(_)));
    }

    #[test]
    fn pending_caps_happenings_not_state() {
        let mut p = Pending::default();
        p.push(Event::Rssi(-50));
        for _ in 0..(MAX_PENDING * 2) {
            p.push(Event::Notice(Notice::MirrorStarted));
        }
        assert_eq!(p.len(), MAX_PENDING);
        let out = p.drain();
        assert!(matches!(out[0], Event::Rssi(-50)), "o estado não se larga");
    }

    /// Um cliente que não lê não trava o hub nem os outros.
    #[tokio::test]
    async fn slow_client_does_not_block_others() {
        let path = temp_socket("slow");
        let hub = Hub::new();
        let (tx, _cmds) = mpsc::unbounded_channel();
        tokio::spawn(serve(bind(&path).await.unwrap(), hub.clone(), tx));
        let _stuck = UnixStream::connect(&path).await.unwrap();
        let mut ok = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(read_msg(&mut ok).await, ServerMsg::Hello { .. }));
        for i in 0..20_000 {
            hub.publish(Event::Levels {
                mic: Some(0.5),
                tap: Some((i % 1000) as f32 / 1000.0),
            });
        }
        hub.publish(Event::Rssi(-42));
        // O cliente que lê chega ao último estado depressa.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            assert!(tokio::time::Instant::now() < deadline, "não chegou ao Rssi");
            if let Event::Rssi(-42) = read_event(&mut ok).await {
                break;
            }
        }
        let _ = std::fs::remove_file(&path);
    }
}
