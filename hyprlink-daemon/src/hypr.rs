//! Controlo do Hyprland: `hyprctl` para consultas/comandos, socket2 do IPC
//! nativo para push de eventos em tempo real.

use std::sync::{Arc, Mutex};

use ciborium::Value;
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixStream;

use crate::active::{push, ActiveConn};
use crate::state::{push_log, HudState};

fn run_hyprctl(args: &[&str]) -> String {
    std::process::Command::new("hyprctl")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

pub fn workspaces_json() -> String {
    let data = run_hyprctl(&["workspaces", "-j"]);
    if data.trim().is_empty() { "[]".to_string() } else { data }
}

pub fn clients_json() -> String {
    let data = run_hyprctl(&["clients", "-j"]);
    if data.trim().is_empty() { "[]".to_string() } else { data }
}

/// `cmd` vem como "workspace 2", "focuswindow address:0x..", etc — o primeiro
/// espaço separa o dispatcher do argumento, igual ao app manda.
pub fn dispatch(cmd: &str) -> String {
    let (disp, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
    let out = if arg.is_empty() {
        run_hyprctl(&["dispatch", disp])
    } else {
        run_hyprctl(&["dispatch", disp, arg])
    };
    let out = out.trim();
    if out.is_empty() { "ok".to_string() } else { out.to_string() }
}

fn socket2_path() -> Option<String> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let candidate = format!("{runtime_dir}/hypr/{sig}/.socket2.sock");
    if std::path::Path::new(&candidate).exists() {
        return Some(candidate);
    }
    let legacy = format!("/tmp/hypr/{sig}/.socket2.sock");
    Some(legacy)
}

/// Escuta o IPC nativo do Hyprland e empurra `hypr.event` a cada linha —
/// reconecta sozinho se o socket cair (ex: Hyprland reiniciou).
pub async fn watch_events(active: ActiveConn, hud: Arc<Mutex<HudState>>) {
    let Some(path) = socket2_path() else {
        push_log(&hud, "[!] HYPRLAND_INSTANCE_SIGNATURE ausente — hypr.event desativado".to_string());
        return;
    };
    loop {
        match UnixStream::connect(&path).await {
            Ok(stream) => {
                let mut lines = tokio::io::BufReader::new(stream).lines();
                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            let body = Value::Map(vec![(Value::Text("event".into()), Value::Text(line))]);
                            push(&active, "hypr.event", Some(body)).await;
                        }
                        _ => break, // socket fechou, reconecta
                    }
                }
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
}
