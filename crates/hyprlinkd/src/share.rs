//! Transferência de ficheiros (Fase 5): telemóvel→PC e PC→telemóvel.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::active::{ActiveConn, announce, push};
use crate::config::{self, SharedConfig};
use crate::state::{HudState, push_log};

pub(crate) struct PendingIncoming {
    name: String,
    size: u64,
}

/// packet id do anúncio `share.file` -> nome/tamanho esperado, até o
/// uni-stream correspondente chegar.
pub type IncomingRegistry = Arc<Mutex<HashMap<u64, PendingIncoming>>>;

pub fn new_incoming_registry() -> IncomingRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn announce_incoming(id: u64, name: String, size: u64, registry: &IncomingRegistry) {
    registry
        .lock()
        .unwrap()
        .insert(id, PendingIncoming { name, size });
}

/// Só o último componente do caminho, rejeitando `.`/`..`/vazio — mesma
/// cautela que o app aplica do lado dele.
fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    if base.is_empty() || base == "." || base == ".." {
        "arquivo_recebido".to_string()
    } else {
        base.to_string()
    }
}

/// Trata um stream unidirecional recebido: `id` é o id de correlação com um
/// `share.file` já anunciado (já lido pelo chamador — o roteador em
/// `server.rs` precisa decidir entre ficheiro/webcam antes de despachar),
/// com uma pequena espera — o anúncio e o stream de bytes podem chegar em
/// tarefas concorrentes.
pub async fn receive_uni_stream(
    mut recv: quinn::RecvStream,
    id: u64,
    registry: IncomingRegistry,
    active: ActiveConn,
    hud: Arc<Mutex<HudState>>,
    config: SharedConfig,
) {
    let mut pending = None;
    for _ in 0..20 {
        if let Some(p) = registry.lock().unwrap().remove(&id) {
            pending = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let Some(pending) = pending else {
        push_log(
            &hud,
            format!("[!] share.file: stream sem anúncio correspondente (id={id})"),
        );
        return;
    };

    let dir = config::download_dir(&config);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        push_log(
            &hud,
            format!(
                "[!] share.file: não foi possível criar {}: {e}",
                dir.display()
            ),
        );
        return;
    }
    let filename = sanitize_filename(&pending.name);
    let path = dir.join(&filename);

    let mut file = match tokio::fs::File::create(&path).await {
        Ok(f) => f,
        Err(e) => {
            push_log(
                &hud,
                format!(
                    "[!] share.file: não foi possível criar {}: {e}",
                    path.display()
                ),
            );
            return;
        }
    };

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 32 * 1024];
    let mut total: u64 = 0;
    let mut last_progress = 0u64;
    let cancel =
        crate::state::start_file_transfer(&hud, filename.clone(), "recebendo", pending.size);

    let mut size_exceeded = false;
    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = recv.stop(0u32.into());
            break;
        }
        let n = match recv.read(&mut buf).await {
            Ok(Some(n)) => n,
            Ok(None) => break,
            Err(_) => break,
        };
        // O peer anunciou `pending.size` de antemão (share.file) — se mandar
        // mais do que isso, é ou um bug ou alguém tentando lotar o disco;
        // corta na hora em vez de escrever até a stream fechar sozinha.
        if total + n as u64 > pending.size {
            size_exceeded = true;
            let _ = recv.stop(0u32.into());
            break;
        }
        hasher.update(&buf[..n]);
        if file.write_all(&buf[..n]).await.is_err() {
            break;
        }
        total += n as u64;
        if total.saturating_sub(last_progress) >= 512 * 1024 {
            last_progress = total;
            crate::state::update_file_transfer_progress(&hud, total);
            let body = Value::Map(vec![
                (Value::Text("id".into()), Value::Integer(id.into())),
                (Value::Text("bytes".into()), Value::Integer(total.into())),
                (
                    Value::Text("total".into()),
                    Value::Integer(pending.size.into()),
                ),
            ]);
            push(&active, "share.progress", Some(body)).await;
        }
    }
    let _ = file.flush().await;
    let cancelled = cancel.load(std::sync::atomic::Ordering::Relaxed);
    crate::state::finish_file_transfer(
        &hud,
        !cancelled && !size_exceeded,
        match (cancelled, size_exceeded) {
            (true, _) => Some("cancelado pelo usuário".to_string()),
            (_, true) => Some("excedeu o tamanho anunciado".to_string()),
            _ => None,
        },
    );
    if size_exceeded {
        push_log(
            &hud,
            format!(
                "[!] ficheiro recebido excedeu o tamanho anunciado: {filename} ({total} > {} bytes), abortado",
                pending.size
            ),
        );
        return;
    }
    if cancelled {
        push_log(
            &hud,
            format!(
                "[!] ficheiro recebido cancelado: {filename} ({total} de {} bytes)",
                pending.size
            ),
        );
        return;
    }

    let sha_hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    push_log(
        &hud,
        format!("[+] ficheiro recebido: {filename} ({total} bytes) · sha256 {sha_hex}"),
    );

    let body = Value::Map(vec![
        (Value::Text("id".into()), Value::Integer(id.into())),
        (Value::Text("ok".into()), Value::Bool(true)),
        (Value::Text("sha256".into()), Value::Text(sha_hex)),
        (Value::Text("bytes".into()), Value::Integer(total.into())),
        (Value::Text("error".into()), Value::Null),
    ]);
    push(&active, "share.done", Some(body)).await;
}

/// Fila dos envios PC→telemóvel: **um ficheiro de cada vez**. O estado só tem
/// um espaço de transferência ativa (`HudState::file_transfer`/`file_cancel`);
/// com N envios em paralelo o progresso trocava-se, só o último se podia
/// cancelar e o histórico ficava com uma só entrada. A `Semaphore` do tokio é
/// justa (FIFO): os ficheiros saem pela ordem em que chegaram. A **receção**
/// (telemóvel→PC) não passa por aqui — as capturas automáticas do telemóvel não
/// podem esperar por um envio longo (a app dá uma transferência a 0 % > 30 s
/// por falhada).
static SEND_QUEUE: Semaphore = Semaphore::const_new(1);

/// Espera a vez na fila `queue`. `None` se `closed` terminar primeiro (a
/// ligação caiu enquanto esperava): o envio desiste sem erro e **sem ficar
/// com a permissão** — nunca foi adquirida. A permissão devolvida larga-se
/// sozinha (RAII) em qualquer saída do envio: fim, erro de rede, cancelado.
async fn acquire_slot<'a>(
    queue: &'a Semaphore,
    closed: impl Future<Output = ()>,
    on_queued: impl FnOnce(),
) -> Option<SemaphorePermit<'a>> {
    match queue.try_acquire() {
        Ok(p) => Some(p),
        Err(_) => {
            on_queued();
            tokio::select! {
                p = queue.acquire() => p.ok(),
                _ = closed => None,
            }
        }
    }
}

/// Envia um ficheiro do PC pro telemóvel: anuncia (`share.file`, D→P) pra
/// ganhar um `id`, depois abre um uni-stream próprio (8 bytes de id + bytes
/// crus, mesmo formato que o telemóvel usa na direção inversa) e transmite
/// em chunks de 32 KiB. Não espera pelo `share.done` de volta — isso chega
/// como qualquer outro pacote em `server.rs` e só é logado.
pub async fn send_file(active: &ActiveConn, hud: &Arc<Mutex<HudState>>, path: &std::path::Path) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("arquivo")
        .to_string();
    // Um de cada vez, ANTES de anunciar: o telemóvel só espera uns 10 s pelo
    // uni-stream depois do `share.file`, e um anúncio na fila expirava.
    let Some(connection) = active.lock().unwrap().clone() else {
        push_log(
            hud,
            "[!] share.file: sem conexão ativa pra enviar".to_string(),
        );
        return;
    };
    let Some(_slot) = acquire_slot(
        &SEND_QUEUE,
        async {
            let _ = connection.closed().await;
        },
        || push_log(hud, format!("[i] share.file · {name} em fila")),
    )
    .await
    else {
        push_log(
            hud,
            format!("[i] share.file · {name} desistiu da fila (a ligação caiu)"),
        );
        return;
    };
    let size = match tokio::fs::metadata(path).await {
        Ok(m) => m.len(),
        Err(e) => {
            push_log(
                hud,
                format!(
                    "[!] share.file: não foi possível ler {}: {e}",
                    path.display()
                ),
            );
            return;
        }
    };

    let body = Value::Map(vec![
        (Value::Text("name".into()), Value::Text(name.clone())),
        (Value::Text("size".into()), Value::Integer(size.into())),
    ]);
    let Some(id) = announce(active, "share.file", Some(body)).await else {
        push_log(
            hud,
            "[!] share.file: sem conexão ativa pra enviar".to_string(),
        );
        return;
    };

    let connection = { active.lock().unwrap().clone() };
    let Some(connection) = connection else { return };
    let mut send = match connection.open_uni().await {
        Ok(s) => s,
        Err(e) => {
            push_log(
                hud,
                format!("[!] share.file: não foi possível abrir o uni-stream: {e}"),
            );
            return;
        }
    };
    if send.write_all(&id.to_be_bytes()).await.is_err() {
        return;
    }

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            push_log(
                hud,
                format!(
                    "[!] share.file: não foi possível abrir {}: {e}",
                    path.display()
                ),
            );
            return;
        }
    };

    push_log(
        hud,
        format!("[i] share.file · enviando {name} ({size} bytes)"),
    );
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 32 * 1024];
    let mut total: u64 = 0;
    let mut last_progress = 0u64;
    let cancel = crate::state::start_file_transfer(hud, name.clone(), "enviando", size);
    crate::state::set_file_transfer_wire_id(hud, id);
    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = send.reset(0u32.into());
            crate::state::finish_file_transfer(
                hud,
                false,
                Some("cancelado pelo usuário".to_string()),
            );
            push_log(
                hud,
                format!("[!] envio cancelado: {name} ({total} de {size} bytes)"),
            );
            return;
        }
        let n = match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if send.write_all(&buf[..n]).await.is_err() {
            crate::state::finish_file_transfer(hud, false, Some("falha de rede".to_string()));
            push_log(
                hud,
                format!("[!] share.file: falha ao enviar {name} em {total} bytes"),
            );
            return;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
        if total.saturating_sub(last_progress) >= 512 * 1024 {
            last_progress = total;
            crate::state::update_file_transfer_progress(hud, total);
        }
    }
    let _ = send.finish();
    crate::state::finish_file_transfer(hud, true, None);

    // O telemóvel (receptor nessa direção) espera um `share.done` do
    // remetente pra confirmar/verificar — mesmo papel que o daemon já faz
    // pra ficheiros telemóvel→PC, só invertido (ver `receive_uni_stream`).
    let sha_hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let done_body = Value::Map(vec![
        (Value::Text("id".into()), Value::Integer(id.into())),
        (Value::Text("ok".into()), Value::Bool(true)),
        (Value::Text("sha256".into()), Value::Text(sha_hex.clone())),
        (Value::Text("bytes".into()), Value::Integer(total.into())),
        (Value::Text("error".into()), Value::Null),
    ]);
    push(active, "share.done", Some(done_body)).await;
    push_log(
        hud,
        format!("[+] ficheiro enviado: {name} ({total} bytes) · sha256 {sha_hex}"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    type Log = Arc<Mutex<Vec<String>>>;

    /// Um «envio» de teste: espera a vez, regista início e fim, e demora `ms`.
    /// `fail` = sai a meio (como um erro de rede) sem chegar ao fim.
    async fn job(q: Arc<Semaphore>, log: Log, id: usize, ms: u64, fail: bool) {
        let Some(_slot) = acquire_slot(&q, pending::<()>(), || {}).await else {
            return;
        };
        log.lock().unwrap().push(format!("start {id}"));
        tokio::time::sleep(Duration::from_millis(ms)).await;
        if fail {
            log.lock().unwrap().push(format!("fail {id}"));
            return; // a permissão larga-se aqui, como num `return` antecipado
        }
        log.lock().unwrap().push(format!("end {id}"));
    }

    #[tokio::test]
    async fn dois_envios_em_simultaneo_correm_um_depois_do_outro() {
        let q = Arc::new(Semaphore::new(1));
        let log: Log = Arc::default();
        let a = tokio::spawn(job(q.clone(), log.clone(), 0, 40, false));
        let b = tokio::spawn(job(q.clone(), log.clone(), 1, 10, false));
        a.await.unwrap();
        b.await.unwrap();
        // O segundo só começa depois de o primeiro acabar (sem sobreposição),
        // mesmo sendo mais curto.
        assert_eq!(
            *log.lock().unwrap(),
            ["start 0", "end 0", "start 1", "end 1"]
        );
    }

    #[tokio::test]
    async fn a_permissao_volta_quando_o_envio_falha() {
        let q = Arc::new(Semaphore::new(1));
        let log: Log = Arc::default();
        job(q.clone(), log.clone(), 0, 1, true).await;
        assert_eq!(q.available_permits(), 1, "a falha não prende a fila");
        // E o seguinte corre normalmente.
        job(q.clone(), log.clone(), 1, 1, false).await;
        assert_eq!(
            *log.lock().unwrap(),
            ["start 0", "fail 0", "start 1", "end 1"]
        );
    }

    #[tokio::test]
    async fn a_ordem_de_chegada_e_respeitada() {
        let q = Arc::new(Semaphore::new(1));
        let log: Log = Arc::default();
        // O 0 segura a fila enquanto os outros se enfileiram, um a um.
        let mut hs = vec![tokio::spawn(job(q.clone(), log.clone(), 0, 60, false))];
        tokio::time::sleep(Duration::from_millis(10)).await;
        for id in 1..6 {
            hs.push(tokio::spawn(job(q.clone(), log.clone(), id, 1, false)));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        for h in hs {
            h.await.unwrap();
        }
        let starts: Vec<String> = log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("start"))
            .cloned()
            .collect();
        assert_eq!(
            starts,
            [
                "start 0", "start 1", "start 2", "start 3", "start 4", "start 5"
            ]
        );
    }

    #[tokio::test]
    async fn ligacao_a_cair_na_fila_desiste_sem_prender_a_permissao() {
        let q = Arc::new(Semaphore::new(1));
        // Alguém segura a vez.
        let holder = q.clone().acquire_owned().await.unwrap();
        let (drop_conn, closed) = tokio::sync::oneshot::channel::<()>();
        let queued = Arc::new(AtomicUsize::new(0));
        let waiter = {
            let (q, queued) = (q.clone(), queued.clone());
            tokio::spawn(async move {
                acquire_slot(
                    &q,
                    async {
                        let _ = closed.await;
                    },
                    || {
                        queued.fetch_add(1, Ordering::SeqCst);
                    },
                )
                .await
                .is_none()
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(queued.load(Ordering::SeqCst), 1, "avisou que ficou em fila");
        assert!(!waiter.is_finished(), "ainda à espera da vez");
        let _ = drop_conn.send(()); // a ligação caiu
        assert!(waiter.await.unwrap(), "desistiu (None)");
        // O dono larga a vez: está livre, nada ficou preso pelo que desistiu.
        drop(holder);
        assert_eq!(q.available_permits(), 1);
        assert!(q.try_acquire().is_ok());
    }

    #[tokio::test]
    async fn fila_vazia_nao_avisa_nem_espera() {
        let q = Semaphore::new(1);
        let avisou = AtomicUsize::new(0);
        let slot = acquire_slot(&q, pending::<()>(), || {
            avisou.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        assert!(slot.is_some());
        assert_eq!(avisou.load(Ordering::SeqCst), 0);
        assert_eq!(q.available_permits(), 0, "a permissão está em uso");
        drop(slot);
        assert_eq!(q.available_permits(), 1);
    }
}
