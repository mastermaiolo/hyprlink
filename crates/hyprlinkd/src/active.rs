//! Handle da conexão QUIC atualmente pareada (design "exclusivo-par": um
//! telemóvel por vez). Tarefas de fundo (clipboard, hypr.event, bateria,
//! mídia) usam isso pra empurrar pacotes `D→P` a qualquer momento, sem
//! depender de estarem "dentro" do handler de uma stream.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ciborium::Value;

use crate::protocol::{read_frame, write_frame, Packet};

pub type ActiveConn = Arc<Mutex<Option<quinn::Connection>>>;

pub fn new_registry() -> ActiveConn {
    Arc::new(Mutex::new(None))
}

pub fn set(active: &ActiveConn, connection: quinn::Connection) {
    *active.lock().unwrap() = Some(connection);
}

pub fn clear(active: &ActiveConn) {
    *active.lock().unwrap() = None;
}

fn next_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Abre um stream bidirecional novo e empurra um pacote `D→P` (push
/// espontâneo, sem esperar resposta) — mesma semântica que o app usa pros
/// pushes que ele nos manda. Devolve o `id` usado (ex: webcam precisa dele
/// pra pré-registrar qual uni-stream esperar de volta) — a maioria dos
/// chamadores ignora, o que é seguro (não é `#[must_use]`).
pub async fn push(active: &ActiveConn, kind: &str, body: Option<Value>) -> Option<u64> {
    let connection = { active.lock().unwrap().clone() };
    let connection = connection?;
    let (mut send, _recv) = connection.open_bi().await.ok()?;
    let id = next_id();
    let packet = Packet::new(id, kind, body, false);
    let _ = write_frame(&mut send, &packet.encode()).await;
    let _ = send.finish();
    Some(id)
}

/// Igual a `push`, mas com `has_payload=true` — anuncia que um uni-stream
/// separado vai seguir, correlacionado pelo `id` devolvido (mesmo padrão que
/// o telemóvel usa pra `share.file`/`webcam.mic_start`, agora do lado do
/// daemon pra `share.file` D→P). Fecha a stream de controlo prontamente —
/// o telemóvel bloqueia lendo até 10s antes de aceitar o uni-stream.
pub async fn announce(active: &ActiveConn, kind: &str, body: Option<Value>) -> Option<u64> {
    let connection = { active.lock().unwrap().clone() };
    let connection = connection?;
    let (mut send, _recv) = connection.open_bi().await.ok()?;
    let id = next_id();
    let packet = Packet::new(id, kind, body, true);
    let _ = write_frame(&mut send, &packet.encode()).await;
    let _ = send.finish();
    Some(id)
}

/// Igual a `push`, mas **espera a resposta** na mesma stream (padrão
/// inverso do que o telemóvel já faz pra `battery.request`/`audio.state` —
/// aqui é o daemon quem pergunta e o telemóvel quem responde). Timeout de
/// 5s, mesma folga usada pelos `req/ack` do telemóvel pro lado do PC.
pub async fn request(active: &ActiveConn, kind: &str, body: Option<Value>) -> Option<Packet> {
    let connection = { active.lock().unwrap().clone() };
    let connection = connection?;
    let (mut send, mut recv) = connection.open_bi().await.ok()?;
    let id = next_id();
    let packet = Packet::new(id, kind, body, false);
    write_frame(&mut send, &packet.encode()).await.ok()?;
    send.finish().ok()?;
    let raw = tokio::time::timeout(std::time::Duration::from_secs(5), read_frame(&mut recv)).await.ok()?.ok()?;
    Packet::decode(&raw).ok()
}
