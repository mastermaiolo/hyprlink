//! Controlo do Hyprland: `hyprctl` para consultas/comandos, socket2 do IPC
//! nativo para push de eventos em tempo real.

use std::sync::{Arc, Mutex};

use ciborium::Value;
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixStream;

use hyprlink_env::hyprland::{DispatchMode, ModeCache};
use hyprlink_env::overrides::DispatchChoice;
use hyprlink_env::{RunOut, Runner};

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

/// `hyprctl` como `Runner` (com as variáveis de sessão do daemon): permite
/// testar o `dispatch` com um `hyprctl` falso.
struct Hyprctl;

impl Runner for Hyprctl {
    fn run(&self, _prog: &str, args: &[&str]) -> Option<RunOut> {
        let output = std::process::Command::new("hyprctl")
            .args(args)
            .envs(crate::action::real_session_env())
            .output()
            .ok()?;
        Some(RunOut {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        })
    }
}

/// Modo do `hyprctl dispatch` (clássico ou Lua), detetado **uma vez** e
/// guardado; volta a testar-se se um dispatch falhar. Ver
/// `hyprlink_env::hyprland` e `docs/COMPATIBILIDADE.md` §2.
static MODE: ModeCache = ModeCache::new();

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
/// espaço separa o dispatcher do argumento, igual ao app manda. Com o prefixo
/// `lua:` o resto é uma expressão Lua (`lua:hl.dsp.window.kill()`), para o que
/// não tenha mapeamento.
pub fn dispatch(cmd: &str) -> String {
    dispatch_checked(cmd).unwrap_or_else(|e| format!("erro: {e}"))
}

/// Como `dispatch`, mas diferencia o sucesso da falha (o texto do erro é
/// curto, em inglês, para a app o poder mostrar).
pub fn dispatch_checked(cmd: &str) -> Result<String, String> {
    let choice = crate::envinfo::resolved().hypr_dispatch_mode;
    dispatch_with(&Hyprctl, &MODE, choice, cmd)
}

fn hyprctl_ok(rt: &dyn Runner, args: &[&str]) -> Option<String> {
    rt.run("hyprctl", args).filter(|o| o.ok).map(|o| o.stdout)
}

/// O dispatch no modo dado. Clássico: `hyprctl dispatch <disp> <args>`. Lua:
/// a expressão traduzida por `lua_fallback`.
fn try_mode(rt: &dyn Runner, mode: DispatchMode, disp: &str, arg: &str) -> Result<String, String> {
    match mode {
        DispatchMode::Classic => {
            let args: Vec<&str> = if arg.is_empty() {
                vec!["dispatch", disp]
            } else {
                vec!["dispatch", disp, arg]
            };
            hyprctl_ok(rt, &args)
                .map(finish)
                .ok_or_else(|| format!("Hyprland refused `{disp}` (classic syntax)"))
        }
        DispatchMode::Lua => {
            let Some(expr) = lua_fallback(disp, arg) else {
                return Err(format!(
                    "`{disp}` has no Lua mapping (use the `lua:` prefix with a hl.dsp.* expression)"
                ));
            };
            hyprctl_ok(rt, &["dispatch", &expr])
                .map(finish)
                .ok_or_else(|| format!("Hyprland refused `{disp}` (Lua syntax)"))
        }
    }
}

fn dispatch_with(
    rt: &dyn Runner,
    cache: &ModeCache,
    choice: DispatchChoice,
    cmd: &str,
) -> Result<String, String> {
    let cmd = cmd.trim();
    if let Some(expr) = cmd.strip_prefix("lua:") {
        let expr = expr.trim();
        if expr.is_empty() {
            return Err("empty Lua expression".to_string());
        }
        return hyprctl_ok(rt, &["dispatch", expr])
            .map(finish)
            .ok_or_else(|| "Hyprland refused the Lua expression".to_string());
    }
    let (disp, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
    let (disp, arg) = (disp, arg.trim());
    let mode = cache.resolve(choice, rt);
    match try_mode(rt, mode, disp, arg) {
        Ok(out) => Ok(out),
        Err(first) => {
            if choice != DispatchChoice::Auto {
                return Err(first);
            }
            // Falhou: o Hyprland pode ter sido recarregado noutro modo
            // (`hyprctl reload full-reset`). Testa de novo e, se mudou, repete.
            cache.reset();
            let again = cache.resolve(choice, rt);
            if again != mode {
                try_mode(rt, again, disp, arg)
            } else {
                Err(first)
            }
        }
    }
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

/// `l`/`left`… → a palavra que o Hyprland aceita (`left/right/up/down`).
fn direction(arg: &str) -> Option<&'static str> {
    match arg.trim() {
        "l" | "left" => Some("left"),
        "r" | "right" => Some("right"),
        "u" | "up" => Some("up"),
        "d" | "down" => Some("down"),
        _ => None,
    }
}

/// `dx dy` ou `exact x y` (inteiros) → `(x, y, relative)`.
fn xy(arg: &str) -> Option<(i64, i64, bool)> {
    let parts: Vec<&str> = arg.split_whitespace().collect();
    let (nums, relative) = match parts.as_slice() {
        ["exact", a, b] => ([*a, *b], false),
        [a, b] => ([*a, *b], true),
        _ => return None,
    };
    Some((nums[0].parse().ok()?, nums[1].parse().ok()?, relative))
}

/// `3` ou `3,class:foo` → (workspace, janela opcional).
fn ws_and_window(arg: &str) -> Option<(&str, Option<&str>)> {
    let (ws, win) = match arg.split_once(',') {
        Some((w, win)) => (w.trim(), Some(win.trim()).filter(|w| !w.is_empty())),
        None => (arg.trim(), None),
    };
    (!ws.is_empty()).then_some((ws, win))
}

/// Dispatcher clássico → expressão Lua. A tabela e as fontes estão em
/// `docs/COMPATIBILIDADE.md` §2 (wiki do Hyprland: `dispatchers.md`,
/// `selectors.md`); a sintaxe foi testada num Hyprland 0.56.2 real. Sem
/// mapeamento (`dpms` e `forceidle` de propósito: a wiki avisa para não os
/// ligar a atalhos) → `None`.
fn lua_fallback(disp: &str, arg: &str) -> Option<String> {
    let arg = arg.trim();
    let q = lua_quote;
    match disp {
        // Texto sempre, como na wiki: `workspace = "3"`, `"e-1"`, `"special:x"`.
        "workspace" if !arg.is_empty() => {
            Some(format!("hl.dsp.focus({{ workspace = {} }})", q(arg)))
        }
        "focuswindow" if !arg.is_empty() => {
            Some(format!("hl.dsp.focus({{ window = {} }})", q(arg)))
        }
        "movefocus" => Some(format!(
            "hl.dsp.focus({{ direction = \"{}\" }})",
            direction(arg)?
        )),
        "focusmonitor" if !arg.is_empty() => {
            Some(format!("hl.dsp.focus({{ monitor = {} }})", q(arg)))
        }
        // O seletor vai em `window` (a wiki), não numa chave `address`.
        "closewindow" if !arg.is_empty() => {
            Some(format!("hl.dsp.window.close({{ window = {} }})", q(arg)))
        }
        "killactive" => Some("hl.dsp.window.close()".to_string()),
        // `exec` clássico: o comando inteiro vai como texto (sem interpretar).
        "exec" if !arg.is_empty() => Some(format!("hl.dsp.exec_cmd({})", q(arg))),
        "exit" => Some("hl.dsp.exit()".to_string()),
        "togglespecialworkspace" => Some(format!(
            "hl.dsp.workspace.toggle_special({})",
            q(if arg.is_empty() { "special" } else { arg })
        )),
        // Clássico: 0/vazio = ecrã inteiro, 1 = maximizar. Lua: o `mode` é
        // texto (um número é recusado pelo Hyprland).
        "fullscreen" => match arg {
            "" | "0" => Some("hl.dsp.window.fullscreen({ mode = \"fullscreen\" })".to_string()),
            "1" => Some("hl.dsp.window.fullscreen({ mode = \"maximized\" })".to_string()),
            _ => None,
        },
        "togglefloating" | "pin" | "pseudo" | "centerwindow" => {
            let f = match disp {
                "togglefloating" => "float",
                "centerwindow" => "center",
                other => other,
            };
            Some(if arg.is_empty() {
                format!("hl.dsp.window.{f}()")
            } else {
                format!("hl.dsp.window.{f}({{ window = {} }})", q(arg))
            })
        }
        "bringactivetotop" => Some("hl.dsp.window.alter_zorder({ mode = \"top\" })".to_string()),
        // `follow` explícito: o `movetoworkspace` clássico segue a janela; o
        // `silent` não (e é isso que o faz ser silent — hyprwm/Hyprland#681).
        "movetoworkspace" | "movetoworkspacesilent" => {
            let (ws, win) = ws_and_window(arg)?;
            let follow = disp == "movetoworkspace";
            let win = win.map_or(String::new(), |w| format!(", window = {}", q(w)));
            Some(format!(
                "hl.dsp.window.move({{ workspace = {}, follow = {follow}{win} }})",
                q(ws)
            ))
        }
        "movewindow" => Some(format!(
            "hl.dsp.window.move({{ direction = \"{}\" }})",
            direction(arg)?
        )),
        "swapwindow" => Some(format!(
            "hl.dsp.window.swap({{ direction = \"{}\" }})",
            direction(arg)?
        )),
        "cyclenext" => match arg {
            "" => Some("hl.dsp.window.cycle_next()".to_string()),
            "prev" => Some("hl.dsp.window.cycle_next({ next = false })".to_string()),
            _ => None,
        },
        "resizeactive" => {
            let (x, y, rel) = xy(arg)?;
            Some(format!(
                "hl.dsp.window.resize({{ x = {x}, y = {y}, relative = {rel} }})"
            ))
        }
        "moveactive" => {
            let (x, y, rel) = xy(arg)?;
            Some(format!(
                "hl.dsp.window.move({{ x = {x}, y = {y}, relative = {rel} }})"
            ))
        }
        "global" if !arg.is_empty() => Some(format!("hl.dsp.global({})", q(arg))),
        "layoutmsg" if !arg.is_empty() => Some(format!("hl.dsp.layout({})", q(arg))),
        "submap" if !arg.is_empty() => Some(format!("hl.dsp.submap({})", q(arg))),
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
    use super::*;
    use std::sync::Mutex;

    /// Um `hyprctl` falso: `lua` diz se aceita expressões Lua (modo Lua) ou só
    /// a sintaxe clássica; regista cada chamada.
    struct Fake {
        lua: Mutex<bool>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl Fake {
        fn new(lua: bool) -> Self {
            Self {
                lua: Mutex::new(lua),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|c| c.join(" "))
                .collect()
        }
    }

    impl Runner for Fake {
        fn run(&self, _prog: &str, args: &[&str]) -> Option<RunOut> {
            self.calls
                .lock()
                .unwrap()
                .push(args.iter().map(|s| s.to_string()).collect());
            let is_lua_expr = args.get(1).is_some_and(|a| a.starts_with("hl."));
            let lua = *self.lua.lock().unwrap();
            Some(RunOut {
                ok: is_lua_expr == lua,
                stdout: String::new(),
            })
        }
    }

    fn go(f: &Fake, c: &ModeCache, choice: DispatchChoice, cmd: &str) -> Result<String, String> {
        dispatch_with(f, c, choice, cmd)
    }

    #[test]
    fn modo_lua_traduz_e_so_testa_uma_vez() {
        let f = Fake::new(true);
        let c = ModeCache::new();
        assert!(go(&f, &c, DispatchChoice::Auto, "workspace 2").is_ok());
        assert!(go(&f, &c, DispatchChoice::Auto, "killactive").is_ok());
        let calls = f.calls();
        assert_eq!(calls[0], "dispatch hl.dsp.no_op()", "o teste vem primeiro");
        assert_eq!(calls[1], r#"dispatch hl.dsp.focus({ workspace = "2" })"#);
        assert_eq!(calls[2], "dispatch hl.dsp.window.close()");
        assert_eq!(
            calls.len(),
            3,
            "sem tentar o clássico nem repetir o teste: {calls:?}"
        );
    }

    #[test]
    fn modo_classico_envia_a_sintaxe_antiga() {
        let f = Fake::new(false);
        let c = ModeCache::new();
        assert!(go(&f, &c, DispatchChoice::Auto, "workspace 2").is_ok());
        assert!(go(&f, &c, DispatchChoice::Auto, "exec kitty --class x").is_ok());
        let calls = f.calls();
        assert_eq!(calls[0], "dispatch hl.dsp.no_op()");
        assert_eq!(calls[1], "dispatch workspace 2");
        assert_eq!(calls[2], "dispatch exec kitty --class x");
        assert_eq!(calls.len(), 3);
    }

    #[test]
    fn modo_muda_a_meio_e_refaz_o_teste() {
        let f = Fake::new(false);
        let c = ModeCache::new();
        assert!(go(&f, &c, DispatchChoice::Auto, "workspace 2").is_ok());
        // `hyprctl reload full-reset` para Lua: o clássico guardado falha.
        *f.lua.lock().unwrap() = true;
        let r = go(&f, &c, DispatchChoice::Auto, "workspace 3");
        assert!(r.is_ok(), "{r:?}");
        let calls = f.calls();
        assert_eq!(
            calls.last().unwrap(),
            r#"dispatch hl.dsp.focus({ workspace = "3" })"#
        );
        assert_eq!(c.get(), Some(DispatchMode::Lua));
    }

    #[test]
    fn opcao_forca_o_modo_sem_testar() {
        let f = Fake::new(true);
        let c = ModeCache::new();
        assert!(go(&f, &c, DispatchChoice::Lua, "workspace 2").is_ok());
        assert_eq!(f.calls().len(), 1);
        let f = Fake::new(true);
        // Clássico forçado num Hyprland Lua: falha, e não adivinha outro modo.
        assert!(
            go(
                &f,
                &ModeCache::new(),
                DispatchChoice::Classic,
                "workspace 2"
            )
            .is_err()
        );
        assert_eq!(f.calls(), vec!["dispatch workspace 2"]);
    }

    #[test]
    fn sem_mapeamento_da_erro_claro_e_o_prefixo_lua_passa_direto() {
        let f = Fake::new(true);
        let c = ModeCache::new();
        let e = go(&f, &c, DispatchChoice::Auto, "dpms off").unwrap_err();
        assert!(e.contains("no Lua mapping") && e.contains("lua:"), "{e}");
        assert!(go(&f, &c, DispatchChoice::Auto, "lua:hl.dsp.window.kill()").is_ok());
        assert_eq!(f.calls().last().unwrap(), "dispatch hl.dsp.window.kill()");
        assert!(go(&f, &c, DispatchChoice::Auto, "lua:").is_err());
        // O prefixo `lua:` não depende do modo guardado nem o testa.
        let g = Fake::new(false);
        assert!(
            go(
                &g,
                &ModeCache::new(),
                DispatchChoice::Auto,
                "lua:hl.dsp.no_op()"
            )
            .is_err()
        );
        assert_eq!(g.calls().len(), 1);
    }

    /// Sintaxe da tabela de `docs/COMPATIBILIDADE.md` §2, testada à mão num
    /// Hyprland 0.56.2 real em 2026-10-08 (fullscreen só aceita texto; `l/r/u/d`
    /// e `left/right/up/down` são aceites; `focus` recusa chaves desconhecidas).
    #[test]
    fn mapeamento_classico_para_lua() {
        let m = |d: &str, a: &str| lua_fallback(d, a);
        assert_eq!(
            m("workspace", "2").as_deref(),
            Some(r#"hl.dsp.focus({ workspace = "2" })"#)
        );
        assert_eq!(
            m("workspace", "e-1").as_deref(),
            Some(r#"hl.dsp.focus({ workspace = "e-1" })"#)
        );
        assert_eq!(
            m("workspace", "e+1").as_deref(),
            Some(r#"hl.dsp.focus({ workspace = "e+1" })"#)
        );
        assert_eq!(
            m("workspace", "special:hyprlinktray").as_deref(),
            Some(r#"hl.dsp.focus({ workspace = "special:hyprlinktray" })"#)
        );
        assert_eq!(m("workspace", ""), None);
        assert_eq!(
            m("focuswindow", "address:0x559fbda7b900").as_deref(),
            Some(r#"hl.dsp.focus({ window = "address:0x559fbda7b900" })"#)
        );
        // O seletor da janela vai em `window` (antes ia numa chave `address`).
        assert_eq!(
            m("closewindow", "address:0x559fc0665c80").as_deref(),
            Some(r#"hl.dsp.window.close({ window = "address:0x559fc0665c80" })"#)
        );
        assert_eq!(
            m("closewindow", ""),
            None,
            "sem alvo não fecha a janela ativa por engano"
        );
        assert_eq!(
            m("killactive", "").as_deref(),
            Some("hl.dsp.window.close()")
        );
        assert_eq!(
            m("exec", "hyprlock").as_deref(),
            Some(r#"hl.dsp.exec_cmd("hyprlock")"#)
        );
        assert_eq!(
            m("exec", r#"sh -c "echo a\b""#).as_deref(),
            Some(r#"hl.dsp.exec_cmd("sh -c \"echo a\\b\"")"#)
        );
        assert_eq!(m("exec", ""), None);
        assert_eq!(
            m("exec", "a\nb").as_deref(),
            Some(r#"hl.dsp.exec_cmd("a\nb")"#),
            "quebras de linha não escapam do literal"
        );
        assert_eq!(
            m("togglespecialworkspace", "magic").as_deref(),
            Some(r#"hl.dsp.workspace.toggle_special("magic")"#)
        );
        // fullscreen: texto, não número.
        assert_eq!(
            m("fullscreen", "").as_deref(),
            Some(r#"hl.dsp.window.fullscreen({ mode = "fullscreen" })"#)
        );
        assert_eq!(
            m("fullscreen", "0").as_deref(),
            Some(r#"hl.dsp.window.fullscreen({ mode = "fullscreen" })"#)
        );
        assert_eq!(
            m("fullscreen", "1").as_deref(),
            Some(r#"hl.dsp.window.fullscreen({ mode = "maximized" })"#)
        );
        assert_eq!(m("fullscreen", "x"), None);
        assert_eq!(
            m("movefocus", "l").as_deref(),
            Some(r#"hl.dsp.focus({ direction = "left" })"#)
        );
        assert_eq!(
            m("movefocus", "down").as_deref(),
            Some(r#"hl.dsp.focus({ direction = "down" })"#)
        );
        assert_eq!(m("movefocus", "x"), None);
        assert_eq!(
            m("movewindow", "r").as_deref(),
            Some(r#"hl.dsp.window.move({ direction = "right" })"#)
        );
        assert_eq!(
            m("swapwindow", "u").as_deref(),
            Some(r#"hl.dsp.window.swap({ direction = "up" })"#)
        );
        assert_eq!(
            m("focusmonitor", "DP-1").as_deref(),
            Some(r#"hl.dsp.focus({ monitor = "DP-1" })"#)
        );
        assert_eq!(
            m("togglefloating", "").as_deref(),
            Some("hl.dsp.window.float()")
        );
        assert_eq!(
            m("togglefloating", "class:foo").as_deref(),
            Some(r#"hl.dsp.window.float({ window = "class:foo" })"#)
        );
        assert_eq!(m("pin", "").as_deref(), Some("hl.dsp.window.pin()"));
        assert_eq!(m("pseudo", "").as_deref(), Some("hl.dsp.window.pseudo()"));
        assert_eq!(
            m("centerwindow", "").as_deref(),
            Some("hl.dsp.window.center()")
        );
        assert_eq!(
            m("bringactivetotop", "").as_deref(),
            Some(r#"hl.dsp.window.alter_zorder({ mode = "top" })"#)
        );
        assert_eq!(
            m("cyclenext", "").as_deref(),
            Some("hl.dsp.window.cycle_next()")
        );
        assert_eq!(
            m("cyclenext", "prev").as_deref(),
            Some("hl.dsp.window.cycle_next({ next = false })")
        );
        assert_eq!(m("cyclenext", "tiled"), None);
        assert_eq!(
            m("resizeactive", "10 -20").as_deref(),
            Some("hl.dsp.window.resize({ x = 10, y = -20, relative = true })")
        );
        assert_eq!(
            m("resizeactive", "exact 800 600").as_deref(),
            Some("hl.dsp.window.resize({ x = 800, y = 600, relative = false })")
        );
        assert_eq!(
            m("moveactive", "5 5").as_deref(),
            Some("hl.dsp.window.move({ x = 5, y = 5, relative = true })")
        );
        assert_eq!(m("moveactive", "10%  5"), None);
        assert_eq!(
            m("global", "quickshell:lock").as_deref(),
            Some(r#"hl.dsp.global("quickshell:lock")"#)
        );
        assert_eq!(
            m("layoutmsg", "togglesplit").as_deref(),
            Some(r#"hl.dsp.layout("togglesplit")"#)
        );
        assert_eq!(
            m("submap", "reset").as_deref(),
            Some(r#"hl.dsp.submap("reset")"#)
        );
        assert_eq!(m("exit", "").as_deref(), Some("hl.dsp.exit()"));
        // A wiki avisa para não ligar dpms/forceidle a atalhos; nenhum inventado.
        assert_eq!(m("dpms", "off"), None);
        assert_eq!(m("forceidle", "5"), None);
        assert_eq!(m("dispatcher_inventado", "x"), None);
    }

    #[test]
    fn mover_para_workspace_segue_ou_nao() {
        assert_eq!(
            lua_fallback("movetoworkspace", "3").as_deref(),
            Some(r#"hl.dsp.window.move({ workspace = "3", follow = true })"#)
        );
        assert_eq!(
            lua_fallback("movetoworkspacesilent", "special:hyprlinktray").as_deref(),
            Some(r#"hl.dsp.window.move({ workspace = "special:hyprlinktray", follow = false })"#)
        );
        assert_eq!(
            lua_fallback("movetoworkspace", "3,class:foo").as_deref(),
            Some(r#"hl.dsp.window.move({ workspace = "3", follow = true, window = "class:foo" })"#)
        );
        assert_eq!(lua_fallback("movetoworkspace", ""), None);
    }

    /// À mão, no Hyprland real (Lua): o `exec` clássico tem de funcionar pelo
    /// modo Lua. `cargo test -p hyprlinkd -- --ignored manual_dispatch_exec`.
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
        assert!(dispatch_checked("dpms off").is_err());
    }
}
