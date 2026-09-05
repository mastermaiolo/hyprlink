//! Controlo do Hyprland: `hyprctl` para consultas/comandos, socket2 do IPC
//! nativo para push de eventos em tempo real.

use std::sync::{Arc, Mutex};

use ciborium::Value;
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixStream;

use crate::active::{push, ActiveConn};
use crate::state::{self, push_log, HudState};

fn run_hyprctl(args: &[&str]) -> String {
    std::process::Command::new("hyprctl")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Como `run_hyprctl`, mas diferencia sucesso de falha (o clássico
/// `hyprctl dispatch <disp> <args>` funciona em qualquer Hyprland padrão;
/// alguns forks — ex. Hyprland-Lua — substituem isso por avaliação de Lua e
/// retornam código de saída != 0 quando recebem a sintaxe clássica).
fn run_hyprctl_checked(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("hyprctl").args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn workspaces_json() -> String {
    let data = run_hyprctl(&["workspaces", "-j"]);
    if data.trim().is_empty() { "[]".to_string() } else { data }
}

pub fn clients_json() -> String {
    let data = run_hyprctl(&["clients", "-j"]);
    if data.trim().is_empty() { "[]".to_string() } else { data }
}

/// A janela focada agora — usado só pra linha de contexto do CONTROL
/// (`activewindow` do socket2 já cobre o resto em tempo real).
pub fn active_window_json() -> String {
    let data = run_hyprctl(&["activewindow", "-j"]);
    if data.trim().is_empty() { "{}".to_string() } else { data }
}

/// Posição do cursor — só pro espelho visual do TRACK, polling leve (1x/seg,
/// só com essa tela aberta).
pub fn cursor_pos_json() -> String {
    let data = run_hyprctl(&["cursorpos", "-j"]);
    if data.trim().is_empty() { "{}".to_string() } else { data }
}

pub fn monitors_json() -> String {
    let data = run_hyprctl(&["monitors", "-j"]);
    if data.trim().is_empty() { "[]".to_string() } else { data }
}

/// `cmd` vem como "workspace 2", "focuswindow address:0x..", etc — o primeiro
/// espaço separa o dispatcher do argumento, igual ao app manda.
pub fn dispatch(cmd: &str) -> String {
    let (disp, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
    let classic: Vec<&str> = if arg.is_empty() { vec!["dispatch", disp] } else { vec!["dispatch", disp, arg] };
    if let Some(out) = run_hyprctl_checked(&classic) {
        return finish(out);
    }
    // Fallback: forks tipo Hyprland-Lua ("ryoku") trocam o dispatch clássico
    // por avaliação de `hl.dispatch(...)` — cobre os 3 dispatchers que o
    // Mission Control do app realmente usa (workspace/focuswindow/closewindow).
    // Ver PROTOCOL.md §6.
    if let Some(lua_expr) = lua_fallback(disp, arg) {
        if let Some(out) = run_hyprctl_checked(&["dispatch", &lua_expr]) {
            return finish(out);
        }
    }
    "erro: dispatch falhou (Hyprland recusou a sintaxe clássica e o fallback Lua)".to_string()
}

fn finish(out: String) -> String {
    let out = out.trim();
    if out.is_empty() { "ok".to_string() } else { out.to_string() }
}

fn lua_fallback(disp: &str, arg: &str) -> Option<String> {
    match disp {
        "workspace" if arg.parse::<i64>().is_ok() => Some(format!("hl.dsp.focus({{workspace = {arg}}})")),
        "focuswindow" => Some(format!("hl.dsp.focus({{window = \"{arg}\"}})")),
        "closewindow" => Some(format!("hl.dsp.window.close({{address = \"{arg}\"}})")),
        "movetoworkspacesilent" => Some(format!("hl.dsp.window.move({{workspace = \"{arg}\"}})")),
        _ => None,
    }
}

/// Nome fixo da workspace especial usada só pra "minimizar" a própria GUI
/// pra bandeja — nunca é alternada (`toggle_special`): sempre um comando
/// determinístico de "esconder" ou "mostrar", nunca "alternar". Um toggle
/// pode dessincronizar do estado real do Hyprland e travar a janela visível
/// em cima de tudo, em toda workspace, sem jeito de fechar — já aconteceu
/// uma vez durante o desenvolvimento (ver conversa), não repetir.
const TRAY_WORKSPACE: &str = "special:hyprlinktray";

fn own_window_address() -> Option<String> {
    let clients: serde_json::Value = serde_json::from_str(&clients_json()).ok()?;
    clients
        .as_array()?
        .iter()
        .find(|c| c.get("class").and_then(|v| v.as_str()) == Some("hyprlink-hud"))?
        .get("address")?
        .as_str()
        .map(String::from)
}

fn active_workspace_id() -> i64 {
    let data = run_hyprctl(&["activeworkspace", "-j"]);
    serde_json::from_str::<serde_json::Value>(&data).ok().and_then(|v| v.get("id")?.as_i64()).unwrap_or(1)
}

/// Move a própria janela da GUI pra uma workspace especial — como ela nunca
/// é mostrada sozinha (ninguém chama `togglespecialworkspace`), fica
/// efetivamente escondida até `tray_show()` a trazer de volta.
pub fn tray_hide() -> bool {
    let Some(addr) = own_window_address() else { return false };
    dispatch(&format!("focuswindow address:{addr}"));
    !dispatch(&format!("movetoworkspacesilent {TRAY_WORKSPACE}")).starts_with("erro")
}

/// Move a janela de volta pra workspace ativa no momento — determinístico
/// (não depende de saber se estava escondida ou não).
pub fn tray_show() -> bool {
    let Some(addr) = own_window_address() else { return false };
    let ws = active_workspace_id();
    dispatch(&format!("focuswindow address:{addr}"));
    !dispatch(&format!("movetoworkspacesilent {ws}")).starts_with("erro")
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
                            if let Some(ws) = line.strip_prefix("workspace>>") {
                                state::set_workspace(&hud, ws.to_string());
                            }
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

#[cfg(test)]
mod tests {
    use super::lua_fallback;

    /// Sintaxe confirmada ao vivo contra um Hyprland-Lua real (fork "ryoku")
    /// em 2026-09-03 — ver PROTOCOL.md §6. Se isso quebrar, o fallback
    /// silenciosamente para de funcionar nesses forks.
    #[test]
    fn lua_fallback_matches_verified_syntax() {
        assert_eq!(lua_fallback("workspace", "2").as_deref(), Some("hl.dsp.focus({workspace = 2})"));
        assert_eq!(
            lua_fallback("focuswindow", "address:0x559fbda7b900").as_deref(),
            Some(r#"hl.dsp.focus({window = "address:0x559fbda7b900"})"#)
        );
        assert_eq!(
            lua_fallback("closewindow", "address:0x559fc0665c80").as_deref(),
            Some(r#"hl.dsp.window.close({address = "address:0x559fc0665c80"})"#)
        );
        assert_eq!(lua_fallback("workspace", "e+1"), None); // não numérico, não arriscamos
        assert_eq!(lua_fallback("exec", "kitty"), None); // dispatcher sem tradução conhecida
    }
}
