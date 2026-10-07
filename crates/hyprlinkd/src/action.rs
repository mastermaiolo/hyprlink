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
pub fn plan(
    name: &str,
    have: &dyn Fn(&str) -> bool,
    shots: &Path,
    stamp: &str,
) -> Result<Plan, String> {
    let file = shots.join(format!("Screenshot_{stamp}.png"));
    let file = file.to_string_lossy().into_owned();
    let none = |what: &str| Err(format!("no {what} tool found"));
    match name {
        "lock" => {
            let mut c = Vec::new();
            if have("noctalia") {
                c.push(argv(&["noctalia", "msg", "session", "lock"]));
            }
            if have("hyprlock") {
                c.push(argv(&["hyprlock"]));
            }
            if have("loginctl") {
                c.push(argv(&["loginctl", "lock-session"]));
            }
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
            if have("grimblast") {
                Ok(Plan::Exec(vec![argv(&[
                    "grimblast",
                    "copysave",
                    "screen",
                    &file,
                ])]))
            } else if have("grim") {
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
            if have("grimblast") {
                Ok(Plan::Exec(vec![argv(&[
                    "grimblast",
                    "copysave",
                    "area",
                    &file,
                ])]))
            } else if have("grim") && have("slurp") {
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
            if !have("wpctl") {
                return none("volume");
            }
            let a = match name {
                "volume_up" => argv(&[
                    "wpctl",
                    "set-volume",
                    "-l",
                    "1",
                    "@DEFAULT_AUDIO_SINK@",
                    "5%+",
                ]),
                "volume_down" => argv(&["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"]),
                _ => argv(&["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]),
            };
            Ok(Plan::Exec(vec![a]))
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
    let plan = plan(name, &on_path, &dir, &stamp)?;
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
