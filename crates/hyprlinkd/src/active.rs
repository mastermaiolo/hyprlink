//! Handle da conexão QUIC atualmente pareada (design "exclusivo-par": um
//! telemóvel por vez). Tarefas de fundo (clipboard, hypr.event, bateria,
//! mídia) usam isso pra empurrar pacotes `D→P` a qualquer momento, sem
//! depender de estarem "dentro" do handler de uma stream.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ciborium::Value;

use crate::protocol::{Packet, read_frame, write_frame};

pub type ActiveConn = Arc<Mutex<Option<quinn::Connection>>>;

pub fn new_registry() -> ActiveConn {
    Arc::new(Mutex::new(None))
}

pub fn set(active: &ActiveConn, connection: quinn::Connection) {
    *active.lock().unwrap() = Some(connection);
}

/// Limpa o registo **só se** ainda lá estiver esta ligação; devolve se limpou.
/// Quando o telemóvel volta a ligar, a ligação nova fica registada e a velha
/// só morre depois (até 60 s, pelo `max_idle_timeout`): um `clear` cego
/// apagava a nova e o daemon ficava sem como empurrar nada ao telemóvel
/// (clipboard, notificações) até à próxima reconexão.
pub fn clear_if(active: &ActiveConn, connection: &quinn::Connection) -> bool {
    let id = connection.stable_id();
    take_if(&mut active.lock().unwrap(), |c| c.stable_id() == id)
}

fn take_if<T>(slot: &mut Option<T>, is_it: impl Fn(&T) -> bool) -> bool {
    if slot.as_ref().is_some_and(is_it) {
        *slot = None;
        true
    } else {
        false
    }
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
    let frame = packet.encode();
    crate::bridge::log_packet_tx(kind, frame.len(), id);
    let _ = write_frame(&mut send, &frame).await;
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
    let frame = packet.encode();
    crate::bridge::log_packet_tx(kind, frame.len(), id);
    let _ = write_frame(&mut send, &frame).await;
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
    let frame = packet.encode();
    crate::bridge::log_packet_tx(kind, frame.len(), id);
    write_frame(&mut send, &frame).await.ok()?;
    send.finish().ok()?;
    let raw = tokio::time::timeout(std::time::Duration::from_secs(5), read_frame(&mut recv))
        .await
        .ok()?
        .ok()?;
    Packet::decode(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::take_if;

    #[test]
    fn take_if_so_limpa_a_ligacao_registada() {
        // A velha (1) morre depois de a nova (2) ficar registada.
        let mut slot = Some(2u32);
        assert!(!take_if(&mut slot, |c| *c == 1));
        assert_eq!(slot, Some(2));
        assert!(take_if(&mut slot, |c| *c == 2));
        assert_eq!(slot, None);
        assert!(!take_if(&mut slot, |c| *c == 2));
    }
}
