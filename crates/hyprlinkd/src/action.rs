//! `pc.action {name}` (P→D, pedido/resposta): ações rápidas do início da app —
//! bloquear, suspender, captura de ecrã, volume, media. O daemon é que escolhe
//! o comando (a app só diz **o quê**), porque o que existe muda de PC para PC:
//! aqui não há `grimblast` nem `hyprlock`, e o `hyprctl dispatch exec …`
//! clássico falha no Hyprland com configuração Lua.
//!
//! Resposta: `{ok: true}` ou `{ok: false, error: "<texto curto em inglês>"}`
//! (a app traduz). O mapeamento nome → comando (`plan`) é puro e testado sem
//! executar nada.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hyprlink_env::overrides::Resolved;
use hyprlink_env::plan::{ShotTool, audio_backend, lock_candidates, screenshot_tool, volume_argv};
use hyprlink_env::shell::Shell;

/// Nomes suportados, pela ordem em que a app os deve mostrar.
#[cfg_attr(not(test), allow(dead_code))]
pub const NAMES: &[&str] = &[
    "lock",
    "suspend",
    "screenshot",
    "screenshot_area",
    "volume_up",
    "volume_down",
    "volume_mute",
    "media_play_pause",
    "media_next",
    "media_previous",
];

/// O que fazer para uma ação.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Comandos alternativos, por ordem: o primeiro que terminar bem (ou ainda
    /// estiver a correr passado o tempo limite) conta como sucesso.
    Exec(Vec<Vec<String>>),
    /// `media_*`: vai pelo MPRIS (`media::handle_command`).
    Media(&'static str),
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Mapeia `name` para um plano, dados os executáveis disponíveis (`have`), a
/// pasta das capturas e um carimbo de data para o nome do ficheiro.
/// Erro = texto curto em inglês para a app.
#[cfg_attr(not(test), allow(dead_code))]
pub fn plan(
    name: &str,
    have: &dyn Fn(&str) -> bool,
    shots: &Path,
    stamp: &str,
) -> Result<Plan, String> {
    plan_with(name, have, shots, stamp, Shell::None, &Resolved::default())
}

/// Como `plan`, com a shell detetada e as opções do `config.json` (o que o
/// daemon usa de verdade; o `hyprlinkctl doctor` mostra o mesmo).
pub fn plan_with(
    name: &str,
    have: &dyn Fn(&str) -> bool,
    shots: &Path,
    stamp: &str,
    shell: Shell,
    opts: &Resolved,
) -> Result<Plan, String> {
    let file = shots.join(format!("Screenshot_{stamp}.png"));
    let file = file.to_string_lossy().into_owned();
    let none = |what: &str| Err(format!("no {what} tool found"));
    match name {
        "lock" => {
            let c = lock_candidates(shell, have, opts.lock_command.as_deref());
            if c.is_empty() {
                none("lock")
            } else {
                Ok(Plan::Exec(c))
            }
        }
        "suspend" => {
            let mut c = Vec::new();
            if have("systemctl") {
                c.push(argv(&["systemctl", "suspend"]));
            }
            if have("loginctl") {
                c.push(argv(&["loginctl", "suspend"]));
            }
            if c.is_empty() {
                none("suspend")
            } else {
                Ok(Plan::Exec(c))
            }
        }
        "screenshot" => {
            let tool = screenshot_tool(opts.screenshot_tool, have);
            if tool == Some(ShotTool::Grimblast) {
                Ok(Plan::Exec(vec![argv(&[
                    "grimblast",
                    "copysave",
                    "screen",
                    &file,
                ])]))
            } else if tool == Some(ShotTool::Grim) {
                let copy = if have("wl-copy") {
                    "&& wl-copy < \"$1\""
                } else {
                    ""
                };
                Ok(Plan::Exec(vec![argv(&[
                    "sh",
                    "-c",
                    &format!("grim \"$1\" {copy}"),
                    "sh",
                    &file,
                ])]))
            } else {
                none("screenshot")
            }
        }
        "screenshot_area" => {
            let tool = screenshot_tool(opts.screenshot_tool, have);
            if tool == Some(ShotTool::Grimblast) {
                Ok(Plan::Exec(vec![argv(&[
                    "grimblast",
                    "copysave",
                    "area",
                    &file,
                ])]))
            } else if tool == Some(ShotTool::Grim) && have("slurp") {
                let copy = if have("wl-copy") {
                    "&& wl-copy < \"$1\""
                } else {
                    ""
                };
                Ok(Plan::Exec(vec![argv(&[
                    "sh",
                    "-c",
                    &format!("g=$(slurp) || exit 1; grim -g \"$g\" \"$1\" {copy}"),
                    "sh",
                    &file,
                ])]))
            } else {
                none("area screenshot")
            }
        }
        "volume_up" | "volume_down" | "volume_mute" => {
            let Some(backend) = audio_backend(opts.audio_backend, have) else {
                return none("volume");
            };
            let what = match name {
                "volume_up" => "up",
                "volume_down" => "down",
                _ => "mute",
            };
            Ok(Plan::Exec(vec![
                volume_argv(backend, what).expect("up/down/mute existem"),
            ]))
        }
        "media_play_pause" => Ok(Plan::Media("play_pause")),
        "media_next" => Ok(Plan::Media("next")),
        "media_previous" => Ok(Plan::Media("previous")),
        other => Err(format!("unknown action: {other}")),
    }
}

/// Quanto esperar antes de dar o comando por «a correr» (e logo bem-sucedido):
/// o bloqueio e a suspensão não terminam enquanto durarem, e a área espera
/// que a pessoa a desenhe.
fn patience(name: &str) -> Duration {
    match name {
        "screenshot_area" => Duration::from_secs(60),
        "screenshot" => Duration::from_secs(10),
        _ => Duration::from_secs(3),
    }
}

/// Variáveis de sessão em falta no ambiente do daemon (arrancado pelo
/// `systemd --user` ou por um `exec-once`, pode não ter `WAYLAND_DISPLAY` nem
/// `HYPRLAND_INSTANCE_SIGNATURE`), descobertas pelos sockets em
/// `$XDG_RUNTIME_DIR`. Só devolve o que falta.
pub fn session_env(
    runtime_dir: &Path,
    get: &dyn Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if get("WAYLAND_DISPLAY").is_none() {
        let mut socks: Vec<String> = std::fs::read_dir(runtime_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("wayland-") && !n.ends_with(".lock"))
            .collect();
        socks.sort();
        if let Some(s) = socks.into_iter().next() {
            out.push(("WAYLAND_DISPLAY".to_string(), s));
        }
    }
    if get("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        // A instância mais recente (as assinaturas começam por um hash e
        // acabam em `_<timestamp>_<n>`).
        let mut inst: Vec<(std::time::SystemTime, String)> =
            std::fs::read_dir(runtime_dir.join("hypr"))
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().join(".socket.sock").exists())
                .filter_map(|e| {
                    let t = e.metadata().ok()?.modified().ok()?;
                    Some((t, e.file_name().to_string_lossy().into_owned()))
                })
                .collect();
        inst.sort();
        if let Some((_, sig)) = inst.pop() {
            out.push(("HYPRLAND_INSTANCE_SIGNATURE".to_string(), sig));
        }
    }
    out
}

/// `session_env` com o ambiente real do processo.
pub fn real_session_env() -> Vec<(String, String)> {
    let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        return Vec::new();
    };
    session_env(&rt, &|k| std::env::var(k).ok())
}

fn on_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(exe).is_file()))
}

fn shots_dir() -> PathBuf {
    dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Screenshots")
}

/// Corre os candidatos por ordem; devolve o primeiro sucesso.
/// «Sucesso» = saiu com 0, ou ainda corre passado `patience`.
fn run_candidates(cands: &[Vec<String>], wait: Duration, area: bool) -> Result<(), String> {
    let env = real_session_env();
    let mut last = "failed".to_string();
    for c in cands {
        let mut cmd = std::process::Command::new(&c[0]);
        cmd.args(&c[1..])
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = match cmd.spawn() {
            Ok(ch) => ch,
            Err(e) => {
                last = format!("failed to start {}: {e}", c[0]);
                continue;
            }
        };
        // Uma thread espera pelo filho até ao fim (sem zombies mesmo que a
        // paciência acabe antes) e entrega o estado por um canal.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait());
        });
        match rx.recv_timeout(wait) {
            Ok(Ok(st)) if st.success() => return Ok(()),
            Ok(Ok(st)) => {
                last = if area && st.code() == Some(1) {
                    "cancelled".to_string()
                } else {
                    format!("{} failed ({st})", c[0])
                };
                if area && st.code() == Some(1) {
                    return Err(last);
                }
            }
            Ok(Err(e)) => last = format!("{} failed: {e}", c[0]),
            // Ainda a correr: bloqueio ativo, a suspender, área a ser desenhada…
            Err(_) => return Ok(()),
        }
    }
    Err(last)
}

/// Executa a ação `name`. Bloqueia (usa `spawn_blocking` à volta).
pub async fn run(name: &str) -> Result<(), String> {
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let dir = shots_dir();
    let plan = plan_with(
        name,
        &on_path,
        &dir,
        &stamp,
        crate::envinfo::shell(),
        &crate::envinfo::resolved(),
    )?;
    match plan {
        Plan::Media(cmd) => {
            crate::media::handle_command(cmd).await;
            Ok(())
        }
        Plan::Exec(cands) => {
            if name.starts_with("screenshot") {
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            }
            let (wait, area) = (patience(name), name == "screenshot_area");
            tokio::task::spawn_blocking(move || run_candidates(&cands, wait, area))
                .await
                .map_err(|_| "action task failed".to_string())?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[&str] = &[
        "noctalia",
        "hyprlock",
        "loginctl",
        "systemctl",
        "grimblast",
        "grim",
        "slurp",
        "wl-copy",
        "wpctl",
    ];

    fn with(set: &[&str]) -> impl Fn(&str) -> bool {
        let set: Vec<String> = set.iter().map(|s| s.to_string()).collect();
        move |e: &str| set.iter().any(|s| s == e)
    }

    fn p(name: &str, tools: &[&str]) -> Result<Plan, String> {
        plan(name, &with(tools), Path::new("/shots"), "T")
    }

    fn cands(r: Result<Plan, String>) -> Vec<Vec<String>> {
        match r.unwrap() {
            Plan::Exec(c) => c,
            other => panic!("esperava Exec: {other:?}"),
        }
    }

    #[test]
    fn todos_os_nomes_tem_plano_e_so_eles() {
        for n in NAMES {
            assert!(p(n, ALL).is_ok(), "{n}");
        }
        assert_eq!(p("rm_rf", ALL), Err("unknown action: rm_rf".into()));
        assert_eq!(p("", ALL), Err("unknown action: ".into()));
        // O nome nunca entra num comando: nada de injeção por `name`.
        assert!(p("lock; reboot", ALL).is_err());
    }

    #[test]
    fn bloquear_prefere_noctalia_e_cai_nos_outros() {
        let c = cands(p("lock", ALL));
        assert_eq!(c[0], argv(&["noctalia", "msg", "session", "lock"]));
        assert_eq!(c.last().unwrap(), &argv(&["loginctl", "lock-session"]));
        // Como nesta máquina: só noctalia + loginctl (sem hyprlock).
        let c = cands(p("lock", &["noctalia", "loginctl"]));
        assert_eq!(c.len(), 2);
        assert_eq!(p("lock", &[]), Err("no lock tool found".into()));
    }

    #[test]
    fn suspender() {
        let c = cands(p("suspend", ALL));
        assert_eq!(c[0], argv(&["systemctl", "suspend"]));
        assert_eq!(p("suspend", &[]), Err("no suspend tool found".into()));
    }

    #[test]
    fn captura_com_grimblast_ou_com_grim() {
        let c = cands(p("screenshot", ALL));
        assert_eq!(
            c[0],
            argv(&["grimblast", "copysave", "screen", "/shots/Screenshot_T.png"])
        );
        // Sem grimblast (como aqui): grim + wl-copy, o ficheiro vai em $1.
        let c = cands(p("screenshot", &["grim", "wl-copy"]));
        assert_eq!(c[0][0..2], argv(&["sh", "-c"]));
        assert!(
            c[0][2].contains("grim \"$1\"") && c[0][2].contains("wl-copy"),
            "{:?}",
            c[0]
        );
        assert_eq!(c[0].last().unwrap(), "/shots/Screenshot_T.png");
        // Sem wl-copy: só grava.
        let c = cands(p("screenshot", &["grim"]));
        assert!(!c[0][2].contains("wl-copy"));
        assert_eq!(
            p("screenshot", &["wl-copy"]),
            Err("no screenshot tool found".into())
        );
    }

    #[test]
    fn captura_de_area() {
        let c = cands(p("screenshot_area", ALL));
        assert_eq!(c[0][..3], argv(&["grimblast", "copysave", "area"]));
        let c = cands(p("screenshot_area", &["grim", "slurp", "wl-copy"]));
        assert!(
            c[0][2].contains("slurp") && c[0][2].contains("grim -g"),
            "{:?}",
            c[0]
        );
        // Sem slurp não há área.
        assert_eq!(
            p("screenshot_area", &["grim"]),
            Err("no area screenshot tool found".into())
        );
    }

    #[test]
    fn volume_e_media() {
        assert_eq!(
            cands(p("volume_up", ALL))[0],
            argv(&[
                "wpctl",
                "set-volume",
                "-l",
                "1",
                "@DEFAULT_AUDIO_SINK@",
                "5%+"
            ])
        );
        assert_eq!(
            cands(p("volume_down", ALL))[0],
            argv(&["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"])
        );
        assert_eq!(
            cands(p("volume_mute", ALL))[0],
            argv(&["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
        );
        assert_eq!(p("volume_up", &[]), Err("no volume tool found".into()));
        assert_eq!(p("media_play_pause", &[]), Ok(Plan::Media("play_pause")));
        assert_eq!(p("media_next", &[]), Ok(Plan::Media("next")));
        assert_eq!(p("media_previous", &[]), Ok(Plan::Media("previous")));
    }

    #[test]
    fn execucao_real_de_comandos_inofensivos() {
        let ok = vec![argv(&["true"])];
        assert_eq!(run_candidates(&ok, Duration::from_secs(5), false), Ok(()));
        // Falha com `false` → erro; o 2.º candidato é que salva.
        let seg = vec![argv(&["false"]), argv(&["true"])];
        assert_eq!(run_candidates(&seg, Duration::from_secs(5), false), Ok(()));
        let mau = vec![argv(&["false"])];
        assert!(run_candidates(&mau, Duration::from_secs(5), false).is_err());
        // Executável inexistente.
        let nada = vec![argv(&["hyprlink-nao-existe"])];
        let e = run_candidates(&nada, Duration::from_secs(1), false).unwrap_err();
        assert!(e.contains("failed to start"), "{e}");
        // Ainda a correr passada a paciência = sucesso (bloqueio/suspensão/área).
        let lento = vec![argv(&["sleep", "2"])];
        assert_eq!(
            run_candidates(&lento, Duration::from_millis(100), false),
            Ok(())
        );
        // Área cancelada (exit 1) → "cancelled", sem tentar outros.
        let canc = vec![argv(&["sh", "-c", "exit 1"]), argv(&["true"])];
        assert_eq!(
            run_candidates(&canc, Duration::from_secs(5), true),
            Err("cancelled".into())
        );
    }

    #[test]
    fn ambiente_de_sessao_so_o_que_falta() {
        let rt = std::env::temp_dir().join(format!("hyprlink-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&rt);
        std::fs::create_dir_all(rt.join("hypr/abc_1_2")).unwrap();
        std::fs::write(rt.join("hypr/abc_1_2/.socket.sock"), b"").unwrap();
        std::fs::create_dir_all(rt.join("hypr/vazia_0_0")).unwrap(); // sem socket
        std::fs::write(rt.join("wayland-1"), b"").unwrap();
        std::fs::write(rt.join("wayland-1.lock"), b"").unwrap();

        let nada = |_: &str| None;
        let v = session_env(&rt, &nada);
        assert!(
            v.contains(&("WAYLAND_DISPLAY".into(), "wayland-1".into())),
            "{v:?}"
        );
        assert!(
            v.contains(&("HYPRLAND_INSTANCE_SIGNATURE".into(), "abc_1_2".into())),
            "{v:?}"
        );
        // Já definidas: não se mexe.
        let tudo = |_: &str| Some("x".to_string());
        assert!(session_env(&rt, &tudo).is_empty());
        std::fs::remove_dir_all(&rt).ok();
    }

    /// À mão, na máquina real (NÃO bloqueia nem suspende):
    /// `cargo test -p hyprlinkd -- --ignored manual_acoes --nocapture`.
    #[test]
    #[ignore]
    fn manual_acoes() {
        let dir = std::env::temp_dir().join(format!("hyprlink-shots-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let Plan::Exec(c) = plan("screenshot", &on_path, &dir, "manual").unwrap() else {
            panic!()
        };
        println!("captura: {c:?}");
        run_candidates(&c, Duration::from_secs(10), false).expect("captura");
        let f = dir.join("Screenshot_manual.png");
        let n = std::fs::metadata(&f).expect("ficheiro").len();
        println!("{} = {n} bytes", f.display());
        assert!(n > 1000);
        // Mute duas vezes: volta ao estado inicial.
        for _ in 0..2 {
            let Plan::Exec(c) = plan("volume_mute", &on_path, &dir, "x").unwrap() else {
                panic!()
            };
            run_candidates(&c, Duration::from_secs(5), false).expect("mute");
        }
        println!("lock plan: {:?}", plan("lock", &on_path, &dir, "x"));
        println!("env: {:?}", real_session_env());
    }
}

#[cfg(test)]
mod env_tests {
    use super::*;
    use hyprlink_env::overrides::{AudioChoice, ScreenshotChoice};

    fn has(set: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |t| set.contains(&t)
    }

    fn exec(r: Result<Plan, String>) -> Vec<String> {
        match r.unwrap() {
            Plan::Exec(c) => c.iter().map(|a| a.join(" ")).collect(),
            Plan::Media(_) => panic!("era Exec"),
        }
    }

    fn go(
        name: &str,
        tools: &'static [&'static str],
        shell: Shell,
        o: &Resolved,
    ) -> Result<Plan, String> {
        plan_with(name, &has(tools), Path::new("/shots"), "T", shell, o)
    }

    #[test]
    fn bloqueio_pela_shell_vem_antes_do_hyprlock() {
        let t = &["noctalia", "hyprlock", "loginctl", "ryoku-shell"];
        let o = Resolved::default();
        assert_eq!(exec(go("lock", t, Shell::Ryoku, &o))[0], "ryoku-shell lock");
        assert_eq!(
            exec(go("lock", t, Shell::NoctaliaV5, &o))[0],
            "noctalia msg session lock"
        );
        assert_eq!(
            exec(go("lock", t, Shell::Caelestia, &o))[0],
            "loginctl lock-session"
        );
        assert_eq!(exec(go("lock", t, Shell::NoctaliaV4, &o))[0], "hyprlock");
    }

    #[test]
    fn bloqueio_forcado_pela_opcao() {
        let o = Resolved {
            lock_command: Some(vec!["meu-lock".into(), "--agora".into()]),
            ..Resolved::default()
        };
        assert_eq!(
            exec(go("lock", &["hyprlock"], Shell::None, &o)),
            vec!["meu-lock --agora"]
        );
    }

    #[test]
    fn captura_forcada_e_em_falta_nao_cai_noutra() {
        let t = &["grimblast", "grim", "slurp", "wl-copy"];
        let grim = Resolved {
            screenshot_tool: ScreenshotChoice::Grim,
            ..Resolved::default()
        };
        assert!(exec(go("screenshot", t, Shell::None, &grim))[0].starts_with("sh -c grim"));
        let blast = Resolved {
            screenshot_tool: ScreenshotChoice::Grimblast,
            ..Resolved::default()
        };
        assert!(go("screenshot", &["grim"], Shell::None, &blast).is_err());
        assert!(
            go(
                "screenshot_area",
                &["grim"],
                Shell::None,
                &Resolved::default()
            )
            .is_err(),
            "sem slurp"
        );
    }

    #[test]
    fn volume_cai_no_pactl_sem_wpctl() {
        let o = Resolved::default();
        let up = exec(go("volume_up", &["pactl"], Shell::None, &o));
        assert_eq!(up, vec!["pactl set-sink-volume @DEFAULT_SINK@ +5%"]);
        let mute = exec(go("volume_mute", &["pactl"], Shell::None, &o));
        assert_eq!(mute, vec!["pactl set-sink-mute @DEFAULT_SINK@ toggle"]);
        assert!(go("volume_down", &[], Shell::None, &o).is_err());
        let forced = Resolved {
            audio_backend: AudioChoice::Pactl,
            ..Resolved::default()
        };
        assert!(
            exec(go("volume_down", &["wpctl", "pactl"], Shell::None, &forced))[0]
                .starts_with("pactl")
        );
    }
}
