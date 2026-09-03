//! Transferência de ficheiros (Fase 5): por ora só a direção telemóvel→PC
//! (o app já tem essa opção pronta em "Partilha"). Enviar do PC pro
//! telemóvel fica pra quando a GUI tiver um seletor de ficheiro — não faz
//! sentido construir isso sem ter como escolher o ficheiro ainda.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::active::{push, ActiveConn};
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

    loop {
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
            let body = Value::Map(vec![
                (Value::Text("id".into()), Value::Integer(id.into())),
                (Value::Text("bytes".into()), Value::Integer(total.into())),
                (Value::Text("total".into()), Value::Integer(pending.size.into())),
            ]);
            push(&active, "share.progress", Some(body)).await;
        }
    }
    let _ = file.flush().await;

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
