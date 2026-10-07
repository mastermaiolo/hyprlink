//! Controlo do Hyprland: `hyprctl` para consultas/comandos, socket2 do IPC
//! nativo para push de eventos em tempo real.

use std::sync::{Arc, Mutex};

use ciborium::Value;
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixStream;

use crate::active::{ActiveConn, push};
use crate::state::{self, HudState, push_log};

fn run_hyprctl(args: &[&str]) -> String {
    std::process::Command::new("hyprctl")
        .args(args)
        .envs(crate::action::real_session_env())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Como `run_hyprctl`, mas diferencia sucesso de falha (o clássico
/// `hyprctl dispatch <disp> <args>` funciona em qualquer Hyprland padrão;
/// alguns forks — ex. Hyprland-Lua — substituem isso por avaliação de Lua e
/// retornam código de saída != 0 quando recebem a sintaxe clássica).
fn run_hyprctl_checked(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("hyprctl")
        .args(args)
        .envs(crate::action::real_session_env())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn workspaces_json() -> String {
    let data = run_hyprctl(&["workspaces", "-j"]);
    if data.trim().is_empty() {
        "[]".to_string()
    } else {
        data
    }
}

pub fn clients_json() -> String {
    let data = run_hyprctl(&["clients", "-j"]);
    if data.trim().is_empty() {
        "[]".to_string()
    } else {
        data
    }
}

/// O workspace focado (existe mesmo vazio, ao contrário de `activewindow`).
pub fn active_workspace_json() -> String {
    let data = run_hyprctl(&["activeworkspace", "-j"]);
    if data.trim().is_empty() {
        "{}".to_string()
    } else {
        data
    }
}

/// A janela focada agora — usado só pra linha de contexto do CONTROL
/// (`activewindow` do socket2 já cobre o resto em tempo real).
pub fn active_window_json() -> String {
    let data = run_hyprctl(&["activewindow", "-j"]);
    if data.trim().is_empty() {
        "{}".to_string()
    } else {
        data
    }
}

/// `cmd` vem como "workspace 2", "focuswindow address:0x..", etc — o primeiro
/// espaço separa o dispatcher do argumento, igual ao app manda.
pub fn dispatch(cmd: &str) -> String {
    dispatch_checked(cmd).unwrap_or_else(|e| format!("erro: {e}"))
}

/// Como `dispatch`, mas diferencia o sucesso da falha (o texto do erro é
/// curto, em inglês, para a app o poder mostrar).
pub fn dispatch_checked(cmd: &str) -> Result<String, String> {
    let (disp, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
    let classic: Vec<&str> = if arg.is_empty() {
        vec!["dispatch", disp]
    } else {
        vec!["dispatch", disp, arg]
    };
    if let Some(out) = run_hyprctl_checked(&classic) {
        return Ok(finish(out));
    }
    // Fallback: forks tipo Hyprland-Lua ("ryoku") trocam o dispatch clássico
    // por avaliação de `hl.dispatch(...)` — traduz os dispatchers que a app
    // usa (workspace/focuswindow/closewindow/movetoworkspacesilent e, desde
    // 2026-10-07, exec/killactive/togglespecialworkspace/fullscreen). Ver
    // PROTOCOL.md §6.
    if let Some(lua_expr) = lua_fallback(disp, arg) {
        return match run_hyprctl_checked(&["dispatch", &lua_expr]) {
            Some(out) => Ok(finish(out)),
            None => Err(format!(
                "Hyprland refused `{disp}` (classic and Lua syntax)"
            )),
        };
    }
    Err(format!(
        "Hyprland refused the classic syntax and `{disp}` has no Lua mapping"
    ))
}

/// Texto como literal de string Lua (entre aspas duplas).
fn lua_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\0' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn lua_fallback(disp: &str, arg: &str) -> Option<String> {
    match disp {
        // Número → posição; `e-1`, `+1`, `name:x`, `special:x` → texto.
        "workspace" if arg.parse::<i64>().is_ok() => {
            Some(format!("hl.dsp.focus({{workspace = {arg}}})"))
        }
        "workspace" if !arg.is_empty() => {
            Some(format!("hl.dsp.focus({{workspace = {}}})", lua_quote(arg)))
        }
        "focuswindow" => Some(format!("hl.dsp.focus({{window = \"{arg}\"}})")),
        "closewindow" => Some(format!("hl.dsp.window.close({{address = \"{arg}\"}})")),
        "killactive" => Some("hl.dsp.window.close()".to_string()),
        // `exec` clássico: o comando inteiro vai como texto (sem interpretar).
        "exec" if !arg.is_empty() => Some(format!("hl.dsp.exec_cmd({})", lua_quote(arg))),
        "togglespecialworkspace" => Some(format!(
            "hl.dsp.workspace.toggle_special({})",
            lua_quote(if arg.is_empty() { "special" } else { arg })
        )),
        // 0/vazio = ecrã inteiro, 1 = maximizar (como as teclas do utilizador).
        "fullscreen" => match arg.trim() {
            "" | "0" => Some("hl.dsp.window.fullscreen()".to_string()),
            m if m.parse::<u8>().is_ok() => {
                Some(format!("hl.dsp.window.fullscreen({{mode = {m}}})"))
            }
            _ => None,
        },
        // `follow = false` é o que faz o "silent" ser silent de verdade —
        // sem isso, o Hyprland-Lua foca a workspace de destino mesmo sem
        // mostrá-la (confirmado: github.com/hyprwm/Hyprland/issues/681 e
        // discussions/14205), o que trava a janela "seguindo" o usuário por
        // todas as telas e rouba o foco de tudo — exatamente o sintoma
        // visto ao vivo com "minimizar" indo pra `special:hyprlinktray`.
        "movetoworkspacesilent" => Some(format!(
            "hl.dsp.window.move({{workspace = \"{arg}\", follow = false}})"
        )),
        _ => None,
    }
}

fn finish(out: String) -> String {
    let out = out.trim();
    if out.is_empty() {
        "ok".to_string()
    } else {
        out.to_string()
    }
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
        push_log(
            &hud,
            "[!] HYPRLAND_INSTANCE_SIGNATURE ausente — hypr.event desativado".to_string(),
        );
        return;
    };
    loop {
        match UnixStream::connect(&path).await {
            Ok(stream) => {
                let mut lines = tokio::io::BufReader::new(stream).lines();
                // Sai quando o socket fecha (ou dá erro) e reconecta.
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(ws) = line.strip_prefix("workspace>>") {
                        state::set_workspace(&hud, ws.to_string());
                    }
                    let body = Value::Map(vec![(Value::Text("event".into()), Value::Text(line))]);
                    push(&active, "hypr.event", Some(body)).await;
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
    use super::{dispatch_checked, lua_fallback};

    /// Sintaxe confirmada ao vivo contra um Hyprland-Lua real (fork "ryoku")
    /// em 2026-09-03 — ver PROTOCOL.md §6. Se isso quebrar, o fallback
    /// silenciosamente para de funcionar nesses forks.
    #[test]
    fn lua_fallback_matches_verified_syntax() {
        assert_eq!(
            lua_fallback("workspace", "2").as_deref(),
            Some("hl.dsp.focus({workspace = 2})")
        );
        assert_eq!(
            lua_fallback("focuswindow", "address:0x559fbda7b900").as_deref(),
            Some(r#"hl.dsp.focus({window = "address:0x559fbda7b900"})"#)
        );
        assert_eq!(
            lua_fallback("closewindow", "address:0x559fc0665c80").as_deref(),
            Some(r#"hl.dsp.window.close({address = "address:0x559fc0665c80"})"#)
        );
        // Relativo e nomeado: texto (como em /usr/share/hypr/hyprland.lua, `workspace = "e-1"`).
        assert_eq!(
            lua_fallback("workspace", "e-1").as_deref(),
            Some(r#"hl.dsp.focus({workspace = "e-1"})"#)
        );
        assert_eq!(lua_fallback("workspace", ""), None);
        // `exec`: o comando inteiro como literal Lua — aspas e barras escapadas.
        assert_eq!(
            lua_fallback("exec", "hyprlock").as_deref(),
            Some(r#"hl.dsp.exec_cmd("hyprlock")"#)
        );
        assert_eq!(
            lua_fallback("exec", r#"sh -c "echo a\b""#).as_deref(),
            Some(r#"hl.dsp.exec_cmd("sh -c \"echo a\\b\"")"#)
        );
        assert_eq!(lua_fallback("exec", ""), None);
        assert_eq!(
            lua_fallback("exec", "a\nb").as_deref(),
            Some(r#"hl.dsp.exec_cmd("a\nb")"#),
            "quebras de linha não escapam do literal"
        );
        assert_eq!(
            lua_fallback("killactive", "").as_deref(),
            Some("hl.dsp.window.close()")
        );
        assert_eq!(
            lua_fallback("togglespecialworkspace", "magic").as_deref(),
            Some(r#"hl.dsp.workspace.toggle_special("magic")"#)
        );
        assert_eq!(
            lua_fallback("fullscreen", "1").as_deref(),
            Some("hl.dsp.window.fullscreen({mode = 1})")
        );
        assert_eq!(
            lua_fallback("fullscreen", "").as_deref(),
            Some("hl.dsp.window.fullscreen()")
        );
        assert_eq!(lua_fallback("fullscreen", "x"), None);
        assert_eq!(lua_fallback("dispatcher_inventado", "x"), None);
        assert_eq!(
            lua_fallback("movetoworkspacesilent", "special:hyprlinktray").as_deref(),
            Some(r#"hl.dsp.window.move({workspace = "special:hyprlinktray", follow = false})"#)
        );
    }

    /// À mão, no Hyprland real (config Lua): o `exec` clássico tem de funcionar
    /// pelo fallback. `cargo test -p hyprlinkd -- --ignored manual_dispatch_exec`.
    #[test]
    #[ignore]
    fn manual_dispatch_exec() {
        let f = std::env::temp_dir().join(format!("hyprlink-exec-{}", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let out = dispatch_checked(&format!("exec touch {}", f.display()));
        println!("dispatch -> {out:?}");
        std::thread::sleep(std::time::Duration::from_millis(800));
        assert!(f.exists(), "o exec não correu");
        std::fs::remove_file(&f).ok();
        // Dispatcher sem tradução → erro claro.
        assert!(dispatch_checked("grimblast copysave area").is_err());
    }
}
