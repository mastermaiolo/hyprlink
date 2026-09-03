//! Handle da conexão QUIC atualmente pareada (design "exclusivo-par": um
//! telemóvel por vez). Tarefas de fundo (clipboard, hypr.event, bateria,
//! mídia) usam isso pra empurrar pacotes `D→P` a qualquer momento, sem
//! depender de estarem "dentro" do handler de uma stream.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ciborium::Value;

use crate::protocol::{write_frame, Packet};

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
