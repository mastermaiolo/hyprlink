//! Clipboard bidirecional via `wl-copy`/`wl-paste` (wl-clipboard).

use std::sync::{Arc, Mutex};

use ciborium::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::active::{push, ActiveConn};
use crate::state::{push_log, HudState};

/// Último texto que o próprio daemon escreveu no clipboard local (via pedido
/// do telemóvel) — evita reenviar o mesmo conteúdo de volta assim que o
/// watcher do wl-paste perceber a mudança que nós mesmos causamos.
pub type LastLocalSet = Arc<Mutex<Option<String>>>;

pub fn new_guard() -> LastLocalSet {
    Arc::new(Mutex::new(None))
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
        let Some(mut stdout) = child.stdout.take() else { return };
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match stdout.read(&mut buf).await {
                Ok(0) | Err(_) => break, // wl-paste morreu, tenta reiniciar
                Ok(n) => n,
            };
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            let was_local = guard.lock().unwrap().as_deref() == Some(text.as_str());
            if was_local {
                *guard.lock().unwrap() = None;
                continue;
            }
            let body = Value::Map(vec![(Value::Text("text".into()), Value::Text(text))]);
            push(&active, "clipboard.set", Some(body)).await;
        }
        let _ = child.wait().await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
