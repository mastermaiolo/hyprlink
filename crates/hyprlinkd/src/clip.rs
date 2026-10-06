//! Clipboard bidirecional via `wl-copy`/`wl-paste` (wl-clipboard).
//!
//! Duas cargas: texto (watcher `wl-paste --watch`, instantâneo) e imagens
//! PNG (poll de 1,5s — o `--watch` só serve UMA flag `--type` e não existe
//! evento "tipos mudaram"; ver `watch_images`).

use std::sync::{Arc, Mutex};

use ciborium::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::active::{ActiveConn, announce, push};
use crate::state::{self, HudState, push_log};

/// Teto pra aceitar uma imagem do clipboard (32 MiB) — maior que isso é
/// recusado com log; imagem de clipboard tem de ser "uma foto/screenshot",
/// não "um PSD de 2 GB".
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

/// Último texto que o próprio daemon escreveu no clipboard local (via pedido
/// do telemóvel) — evita reenviar o mesmo conteúdo de volta assim que o
/// watcher do wl-paste perceber a mudança que nós mesmos causamos.
pub type LastLocalSet = Arc<Mutex<Option<String>>>;

pub fn new_guard() -> LastLocalSet {
    Arc::new(Mutex::new(None))
}

/// Hash SHA-256 (hex) da última imagem que o próprio daemon escreveu no
/// clipboard local — mesma função do `LastLocalSet`, pro poll de imagens
/// não ecoar de volta o que acabou de chegar do telemóvel.
pub type LastLocalImage = Arc<Mutex<Option<String>>>;

pub fn new_image_guard() -> LastLocalImage {
    Arc::new(Mutex::new(None))
}

/// Id do `clipboard.set` (imagem) esperado no próximo uni-stream — mesmo
/// padrão do `pending_mic`.
pub type PendingClipImage = Arc<Mutex<Option<u64>>>;

pub fn new_pending_clip() -> PendingClipImage {
    Arc::new(Mutex::new(None))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `clipboard.set` recebido do telemóvel: escreve no clipboard do Wayland.
pub async fn set_from_remote(text: &str, guard: &LastLocalSet) {
    *guard.lock().unwrap() = Some(text.to_string());
    let mut child = match Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(text.as_bytes()).await;
    }
    let _ = child.wait().await;
}

/// Observa o clipboard local (`wl-paste --watch`) e empurra `clipboard.set`
/// pro telemóvel a cada mudança que não veio dele mesmo.
pub async fn watch(active: ActiveConn, guard: LastLocalSet, hud: Arc<Mutex<HudState>>) {
    loop {
        let mut child = match Command::new("wl-paste")
            .args(["--type", "text", "--no-newline", "--watch", "cat"])
            .stdout(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                push_log(&hud, format!("[!] wl-paste indisponível: {e}"));
                return;
            }
        };
        let Some(mut stdout) = child.stdout.take() else {
            return;
        };
        let mut buf = vec![0u8; 64 * 1024];
        // Um clipboard maior que 64 KiB chega em mais de um `read()` — sem
        // isso, cada pedaço virava um `clipboard.set` próprio e corrompia o
        // conteúdo do lado do telemóvel. Acumula enquanto os bytes ainda
        // estiverem chegando; só manda quando a stream fica ~150ms quieta
        // (fim natural de uma cópia — o `wl-paste --watch` só volta a
        // escrever de novo quando o usuário copiar outra coisa).
        const IDLE_FLUSH: std::time::Duration = std::time::Duration::from_millis(150);
        let mut acc: Vec<u8> = Vec::new();
        loop {
            match tokio::time::timeout(IDLE_FLUSH, stdout.read(&mut buf)).await {
                Ok(Ok(0)) | Ok(Err(_)) => break, // wl-paste morreu, tenta reiniciar
                Ok(Ok(n)) => {
                    acc.extend_from_slice(&buf[..n]);
                    continue;
                }
                Err(_) => {} // timeout: nada novo há IDLE_FLUSH, cai pro flush abaixo
            }
            if acc.is_empty() {
                continue;
            }
            let text = String::from_utf8_lossy(&acc).to_string();
            acc.clear();
            let was_local = guard.lock().unwrap().as_deref() == Some(text.as_str());
            if was_local {
                *guard.lock().unwrap() = None;
                continue;
            }
            state::push_clip_entry(&hud, state::DIR_PC_TO_PHONE, text.clone());
            let body = Value::Map(vec![(Value::Text("text".into()), Value::Text(text))]);
            push(&active, "clipboard.set", Some(body)).await;
        }
        let _ = child.wait().await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// `clipboard.set` de IMAGEM recebido do telemóvel: escreve PNG cru no
/// clipboard do Wayland oferecendo `image/png`. O hash fica no guard pra o
/// poll não devolver a mesma imagem ao remetente.
pub async fn set_image_from_remote(
    bytes: Vec<u8>,
    guard: &LastLocalImage,
    hud: &Arc<Mutex<HudState>>,
) {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        push_log(
            hud,
            format!("[!] clipboard: imagem recusada ({} bytes)", bytes.len()),
        );
        return;
    }
    *guard.lock().unwrap() = Some(sha256_hex(&bytes));
    let mut child = match Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            push_log(hud, format!("[!] clipboard: wl-copy falhou: {e}"));
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        if stdin.write_all(&bytes).await.is_err() {
            let _ = child.kill().await;
            return;
        }
    }
    let _ = child.wait().await;
    state::push_clip_image(hud, state::DIR_PHONE_TO_PC, bytes.len() as u64);
    push_log(
        hud,
        format!(
            "[i] clipboard.set · telemóvel → PC · imagem {} KB",
            bytes.len() / 1024
        ),
    );
}

/// Uni-stream de imagem anunciada por `clipboard.set` (has_payload=true):
/// lê os bytes até EOF e entrega ao `wl-copy`.
pub async fn receive_image_stream(
    mut recv: quinn::RecvStream,
    guard: LastLocalImage,
    hud: Arc<Mutex<HudState>>,
) {
    let mut bytes: Vec<u8> = Vec::with_capacity(256 * 1024);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match recv.read(&mut buf).await {
            Ok(Some(n)) => n,
            Ok(None) | Err(_) => break,
        };
        if bytes.len() + n > MAX_IMAGE_BYTES {
            push_log(
                &hud,
                "[!] clipboard: imagem do telemóvel excede 32 MiB, descartada".to_string(),
            );
            let _ = recv.stop(0u32.into());
            return;
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    set_image_from_remote(bytes, &guard, &hud).await;
}

/// Envia uma imagem do clipboard local pro telemóvel — announce + uni-stream,
/// o mesmo par `share.file`/`webcam.mic_start` já usa (PROTOCOL.md §4).
async fn send_image_to_phone(active: &ActiveConn, hud: &Arc<Mutex<HudState>>, bytes: Vec<u8>) {
    let body = Value::Map(vec![
        (Value::Text("mime".into()), Value::Text("image/png".into())),
        (
            Value::Text("size".into()),
            Value::Integer((bytes.len() as i64).into()),
        ),
    ]);
    let Some(id) = announce(active, "clipboard.set", Some(body)).await else {
        return; // sem conexão — o poll tenta de novo quando houver? Não: o hash
        // já foi marcado como visto; a próxima imagem nova é que sai.
    };
    let Some(connection) = active.lock().unwrap().clone() else {
        return;
    };
    let Ok(mut send) = connection.open_uni().await else {
        return;
    };
    if send.write_all(&id.to_be_bytes()).await.is_err() {
        return;
    }
    if send.write_all(&bytes).await.is_err() {
        return;
    }
    let _ = send.finish();
    state::push_clip_image(hud, state::DIR_PC_TO_PHONE, bytes.len() as u64);
    push_log(
        hud,
        format!(
            "[i] clipboard.set · PC → telemóvel · imagem {} KB",
            bytes.len() / 1024
        ),
    );
}

/// Poll de imagens do clipboard local → telemóvel. Por que poll e não
/// `--watch` como o texto: o `wl-paste --watch` serve exatamente UM `--type`
/// e não tem evento de "a lista de tipos mudou" — um watcher `--type text`
/// nem sequer vê cópias só-imagem direito. Um poll de 1,5s lendo `wl-paste
/// -l` é barato (roundtrip wayland pequeno) e o custo real (ler os bytes)
/// só acontece quando há `image/png` na lista e o hash mudou.
pub async fn watch_images(active: ActiveConn, guard: LastLocalImage, hud: Arc<Mutex<HudState>>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(1500));
    // Baseline: NÃO enviar o que já está no clipboard quando o daemon sobe —
    // só cópias novas contam.
    let mut last_seen: Option<String> = None;
    loop {
        interval.tick().await;

        let types = match Command::new("wl-paste")
            .args(["--list-types"])
            .output()
            .await
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(_) => continue,
        };
        if !types.split_whitespace().any(|t| t == "image/png") {
            continue;
        }

        let bytes = match Command::new("wl-paste")
            .args(["--type", "image/png"])
            .output()
            .await
        {
            Ok(o) => o.stdout,
            Err(_) => continue,
        };
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
            continue;
        }
        let hash = sha256_hex(&bytes);

        // Mesma imagem da última vista → nada novo.
        if last_seen.as_deref() == Some(hash.as_str()) {
            continue;
        }
        let is_first = last_seen.is_none();
        last_seen = Some(hash.clone());

        // Echo do que o próprio daemon escreveu (veio do telemóvel agora
        // mesmo) — consome o guard e segue.
        if guard.lock().unwrap().as_deref() == Some(hash.as_str()) {
            *guard.lock().unwrap() = None;
            continue;
        }
        if is_first {
            continue; // baseline do arranque, sem surpresas
        }
        send_image_to_phone(&active, &hud, bytes).await;
    }
}
