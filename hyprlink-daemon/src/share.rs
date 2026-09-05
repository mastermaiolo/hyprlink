//! Transferência de ficheiros (Fase 5): telemóvel→PC e PC→telemóvel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::active::{announce, push, ActiveConn};
use crate::config::{self, SharedConfig};
use crate::state::{push_log, HudState};

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
    registry.lock().unwrap().insert(id, PendingIncoming { name, size });
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
        push_log(&hud, format!("[!] share.file: stream sem anúncio correspondente (id={id})"));
        return;
    };

    let dir = config::download_dir(&config);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        push_log(&hud, format!("[!] share.file: não foi possível criar {}: {e}", dir.display()));
        return;
    }
    let filename = sanitize_filename(&pending.name);
    let path = dir.join(&filename);

    let mut file = match tokio::fs::File::create(&path).await {
        Ok(f) => f,
        Err(e) => {
            push_log(&hud, format!("[!] share.file: não foi possível criar {}: {e}", path.display()));
            return;
        }
    };

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 32 * 1024];
    let mut total: u64 = 0;
    let mut last_progress = 0u64;
    let cancel = crate::state::start_file_transfer(&hud, filename.clone(), "recebendo", pending.size);

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
                (Value::Text("total".into()), Value::Integer(pending.size.into())),
            ]);
            push(&active, "share.progress", Some(body)).await;
        }
    }
    let _ = file.flush().await;
    let cancelled = cancel.load(std::sync::atomic::Ordering::Relaxed);
    crate::state::finish_file_transfer(&hud, !cancelled, cancelled.then(|| "cancelado pelo usuário".to_string()));
    if cancelled {
        push_log(&hud, format!("[!] ficheiro recebido cancelado: {filename} ({total} de {} bytes)", pending.size));
        return;
    }

    let sha_hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    push_log(&hud, format!("[+] ficheiro recebido: {filename} ({total} bytes) · sha256 {sha_hex}"));

    let body = Value::Map(vec![
        (Value::Text("id".into()), Value::Integer(id.into())),
        (Value::Text("ok".into()), Value::Bool(true)),
        (Value::Text("sha256".into()), Value::Text(sha_hex)),
        (Value::Text("bytes".into()), Value::Integer(total.into())),
        (Value::Text("error".into()), Value::Null),
    ]);
    push(&active, "share.done", Some(body)).await;
}

/// Envia um ficheiro do PC pro telemóvel: anuncia (`share.file`, D→P) pra
/// ganhar um `id`, depois abre um uni-stream próprio (8 bytes de id + bytes
/// crus, mesmo formato que o telemóvel usa na direção inversa) e transmite
/// em chunks de 32 KiB. Não espera pelo `share.done` de volta — isso chega
/// como qualquer outro pacote em `server.rs` e só é logado.
pub async fn send_file(active: &ActiveConn, hud: &Arc<Mutex<HudState>>, path: &std::path::Path) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("arquivo").to_string();
    let size = match tokio::fs::metadata(path).await {
        Ok(m) => m.len(),
        Err(e) => {
            push_log(hud, format!("[!] share.file: não foi possível ler {}: {e}", path.display()));
            return;
        }
    };

    let body = Value::Map(vec![
        (Value::Text("name".into()), Value::Text(name.clone())),
        (Value::Text("size".into()), Value::Integer(size.into())),
    ]);
    let Some(id) = announce(active, "share.file", Some(body)).await else {
        push_log(hud, "[!] share.file: sem conexão ativa pra enviar".to_string());
        return;
    };

    let connection = { active.lock().unwrap().clone() };
    let Some(connection) = connection else { return };
    let mut send = match connection.open_uni().await {
        Ok(s) => s,
        Err(e) => {
            push_log(hud, format!("[!] share.file: não foi possível abrir o uni-stream: {e}"));
            return;
        }
    };
    if send.write_all(&id.to_be_bytes()).await.is_err() {
        return;
    }

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            push_log(hud, format!("[!] share.file: não foi possível abrir {}: {e}", path.display()));
            return;
        }
    };

    push_log(hud, format!("[i] share.file · enviando {name} ({size} bytes)"));
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 32 * 1024];
    let mut total: u64 = 0;
    let mut last_progress = 0u64;
    let cancel = crate::state::start_file_transfer(hud, name.clone(), "enviando", size);
    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = send.reset(0u32.into());
            crate::state::finish_file_transfer(hud, false, Some("cancelado pelo usuário".to_string()));
            push_log(hud, format!("[!] envio cancelado: {name} ({total} de {size} bytes)"));
            return;
        }
        let n = match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if send.write_all(&buf[..n]).await.is_err() {
            crate::state::finish_file_transfer(hud, false, Some("falha de rede".to_string()));
            push_log(hud, format!("[!] share.file: falha ao enviar {name} em {total} bytes"));
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
    let sha_hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let done_body = Value::Map(vec![
        (Value::Text("id".into()), Value::Integer(id.into())),
        (Value::Text("ok".into()), Value::Bool(true)),
        (Value::Text("sha256".into()), Value::Text(sha_hex.clone())),
        (Value::Text("bytes".into()), Value::Integer(total.into())),
        (Value::Text("error".into()), Value::Null),
    ]);
    push(active, "share.done", Some(done_body)).await;
    push_log(hud, format!("[+] ficheiro enviado: {name} ({total} bytes) · sha256 {sha_hex}"));
}
