//! O ambiente detetado ao arrancar — o mesmo código que o `hyprlinkctl
//! doctor` (`hyprlink-env`), por isso o relatório mostra o que o daemon faz.
//! Lido uma vez no arranque; as opções do `config.json` também só se lêem aí.

use std::sync::OnceLock;

use hyprlink_env::hyprland::DispatchMode;
use hyprlink_env::overrides::{DispatchChoice, Overrides, Resolved};
use hyprlink_env::report::{self, Depth, ModeProbe, Report};
use hyprlink_env::shell::Shell;
use hyprlink_env::{Env, RealRunner};
use hyprlink_proto::link::EnvValue;

use crate::config::{self, SharedConfig};

struct Info {
    report: Report,
    problems: Vec<String>,
}

static INFO: OnceLock<Info> = OnceLock::new();

/// O `Env` real, com as variáveis de sessão que o daemon descobre (para o
/// `hyprctl` funcionar mesmo arrancado fora do Hyprland).
pub fn daemon_env() -> Env {
    let mut env = Env::real();
    env.runner = Box::new(RealRunner {
        envs: crate::session::real_session_env(),
        ..RealRunner::default()
    });
    env
}

/// Deteta o ambiente (bloqueante, rápido) e devolve as linhas para o Diário.
/// **Nunca faz dispatch:** o modo do `hyprctl dispatch` é forçado
/// (`hypr_dispatch_mode`) ou inferido; se a inferência for segura fica já em
/// cache, senão confirma-se no primeiro dispatch a sério (`hypr.rs`).
pub fn init(config: &SharedConfig) -> Vec<String> {
    let ov = config::env_overrides(config);
    let (rep, log) = detect(&daemon_env(), &ov);
    if rep.options.hypr_dispatch_mode == DispatchChoice::Auto && rep.dispatch_mode_certain {
        crate::hypr::seed_mode(rep.dispatch_mode);
    }
    let problems = rep.option_problems.clone();
    let _ = INFO.set(Info {
        report: rep,
        problems,
    });
    log
}

/// «Lua (inferido)» / «Lua (override)» / «clássico (a confirmar no primeiro uso)».
fn mode_line(rep: &Report) -> String {
    let mode = match rep.dispatch_mode {
        DispatchMode::Lua => "Lua",
        DispatchMode::Classic => "clássico",
    };
    let why = if rep.options.hypr_dispatch_mode != DispatchChoice::Auto {
        "override"
    } else if rep.dispatch_mode_certain {
        "inferido"
    } else {
        "a confirmar no primeiro uso"
    };
    format!("{mode} ({why})")
}

/// A deteção do arranque, sem tocar no estado global (testável com um `Env`
/// falso).
fn detect(env: &Env, ov: &Overrides) -> (Report, Vec<String>) {
    let rep = report::collect_with(env, ov, Depth::Quick, ModeProbe::Infer);
    let mut log = vec![
        format!(
            "[i] ambiente · shell: {} · dispatch: {} · GPU: {}",
            rep.shell.shell.label(),
            mode_line(&rep),
            rep.gpu_source.why
        ),
        format!(
            "[i] ambiente · áudio: {} · bateria do sistema: {} · temperatura: {}",
            rep.audio_server.label(),
            if rep.battery_present { "sim" } else { "não" },
            rep.temp_sensor
                .as_ref()
                .map_or("sem sensor".to_string(), |t| t.why.clone())
        ),
    ];
    log.extend(
        rep.option_problems
            .iter()
            .map(|p| format!("[!] config.json · {p}")),
    );
    (rep, log)
}

/// O grafo do PipeWire e o estado do EasyEffects agora (bloqueante: corre o
/// `pw-dump`; chamar em `spawn_blocking`). `None` = sem `pw-dump`.
pub fn pw_snapshot() -> Option<(
    hyprlink_env::pipewire::Graph,
    hyprlink_env::pipewire::EeState,
)> {
    hyprlink_env::pipewire::snapshot(&daemon_env())
}

/// Opções validadas (por omissão, tudo automático, antes do `init`).
pub fn resolved() -> Resolved {
    INFO.get()
        .map(|i| i.report.options.clone())
        .unwrap_or_default()
}

pub fn shell() -> Shell {
    INFO.get().map_or(Shell::None, |i| i.report.shell.shell)
}

/// Linhas «chave → valor em uso» para a GUI (Definições).
pub fn values() -> Vec<EnvValue> {
    let Some(i) = INFO.get() else {
        return Vec::new();
    };
    let r = &i.report;
    let o = &r.options;
    let v = |key: &str, value: String| EnvValue {
        key: key.to_string(),
        value,
    };
    let mut out = vec![
        v("shell", r.shell.shell.label().to_string()),
        v(
            "hypr_dispatch_mode",
            format!(
                "{} ({})",
                o_label(&format!("{:?}", o.hypr_dispatch_mode)),
                match r.dispatch_mode {
                    hyprlink_env::hyprland::DispatchMode::Lua => "Lua",
                    hyprlink_env::hyprland::DispatchMode::Classic => "clássico",
                }
            ),
        ),
        v(
            "lock_command",
            r.lock_plan
                .first()
                .cloned()
                .unwrap_or_else(|| "nenhum".into()),
        ),
        v(
            "screenshot_tool",
            match r.screenshot_plan {
                Some(hyprlink_env::plan::ShotTool::Grimblast) => "grimblast".into(),
                Some(hyprlink_env::plan::ShotTool::Grim) => "grim".into(),
                None => "nenhuma".into(),
            },
        ),
        v(
            "temp_sensor",
            r.temp_sensor
                .as_ref()
                .map_or("sem sensor".into(), |t| t.why.clone()),
        ),
        v("gpu_source", r.gpu_source.why.clone()),
        v(
            "audio_backend",
            match r.audio_backend {
                Some(hyprlink_env::plan::AudioBackend::Wpctl) => "wpctl".into(),
                Some(hyprlink_env::plan::AudioBackend::Pactl) => "pactl".into(),
                None => "nenhum".into(),
            },
        ),
        v("v4l2_device_nr", o.v4l2_device_nr.to_string()),
        v(
            "tap_source",
            match (&r.tap_target, &r.tap_error) {
                (Some(t), _) => {
                    format!("{:?} → {} ({})", o.tap_source, t.sink, t.why).to_lowercase()
                }
                (_, Some(e)) => format!("{:?}: {e}", o.tap_source).to_lowercase(),
                _ => format!("{:?}", o.tap_source).to_lowercase(),
            },
        ),
    ];
    out.extend(i.problems.iter().map(|p| v("aviso", p.clone())));
    out
}

fn o_label(debug: &str) -> String {
    debug.to_ascii_lowercase()
}

/// O retorno de áudio e o microfone usam `pipewiresrc`/`pw-*`. Só se recusam
/// com **prova** de que o servidor é o PulseAudio (processo `pulseaudio` e
/// nenhum `pipewire`); se a deteção não souber, deixa tentar.
pub fn pipewire_required(feature: &str) -> Result<(), String> {
    let server = INFO.get().map(|i| i.report.audio_server);
    gate(server, feature)
}

fn gate(server: Option<hyprlink_env::plan::AudioServer>, feature: &str) -> Result<(), String> {
    match server {
        Some(hyprlink_env::plan::AudioServer::PulseAudio) => Err(format!(
            "{feature} precisa de PipeWire (pipewiresrc), mas o servidor de áudio é o PulseAudio: desativado; o mixer, o volume e o modo coluna continuam"
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprlink_env::plan::AudioServer;

    #[test]
    fn so_recusa_com_prova_de_pulseaudio() {
        assert!(gate(Some(AudioServer::PipeWire), "retorno").is_ok());
        assert!(gate(Some(AudioServer::Other), "retorno").is_ok());
        assert!(gate(Some(AudioServer::None), "retorno").is_ok());
        assert!(gate(None, "retorno").is_ok());
        let e = gate(Some(AudioServer::PulseAudio), "o retorno de áudio").unwrap_err();
        assert!(e.contains("PulseAudio") && e.contains("retorno"));
    }

    /// Regista todas as chamadas; o `hyprctl` responde como um Hyprland 0.56
    /// em Lua (aceita `hl.*`).
    struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl hyprlink_env::Runner for Recorder {
        fn run(&self, prog: &str, args: &[&str]) -> Option<hyprlink_env::RunOut> {
            self.0
                .lock()
                .unwrap()
                .push(format!("{prog} {}", args.join(" ")));
            let stdout = match args {
                ["version"] => "Hyprland 0.56.2 built from branch v0.56.2\n".to_string(),
                _ => String::new(),
            };
            Some(hyprlink_env::RunOut {
                ok: prog == "hyprctl",
                stdout,
            })
        }
    }

    fn fake_env(config: Option<&str>) -> (Env, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let root = std::env::temp_dir().join(format!(
            "hyprlink-envinfo-{}-{}",
            std::process::id(),
            config.unwrap_or("nenhum")
        ));
        let hypr = root.join("config/hypr");
        std::fs::create_dir_all(&hypr).unwrap();
        if let Some(f) = config {
            std::fs::write(hypr.join(f), "").unwrap();
        }
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let env = Env {
            sys: root.join("sys"),
            proc_dir: root.join("proc"),
            etc: root.join("etc"),
            dev: root.join("dev"),
            vars: [(
                "XDG_CONFIG_HOME".to_string(),
                root.join("config").display().to_string(),
            )]
            .into_iter()
            .collect(),
            have: Box::new(|t| t == "hyprctl"),
            runner: Box::new(Recorder(calls.clone())),
            sock_query: Box::new(|_, _| None),
        };
        (env, calls)
    }

    #[test]
    fn arranque_nunca_faz_dispatch() {
        for config in [Some("hyprland.lua"), Some("hyprland.conf"), None] {
            let (env, calls) = fake_env(config);
            let (rep, log) = detect(&env, &Overrides::default());
            let calls = calls.lock().unwrap().clone();
            assert!(
                !calls.iter().any(|c| c.contains("dispatch")),
                "{config:?}: houve dispatch: {calls:?}"
            );
            assert!(calls.iter().any(|c| c == "hyprctl version"), "{calls:?}");
            let line = &log[0];
            match config {
                Some("hyprland.lua") => {
                    assert_eq!(rep.dispatch_mode, DispatchMode::Lua);
                    assert!(rep.dispatch_mode_certain);
                    assert!(line.contains("dispatch: Lua (inferido)"), "{line}");
                }
                Some(_) => {
                    assert_eq!(rep.dispatch_mode, DispatchMode::Classic);
                    assert!(line.contains("dispatch: clássico (inferido)"), "{line}");
                }
                None => {
                    // 0.56 sem ficheiro: palpite Lua, confirmado no primeiro uso.
                    assert_eq!(rep.dispatch_mode, DispatchMode::Lua);
                    assert!(!rep.dispatch_mode_certain);
                    assert!(
                        line.contains("dispatch: Lua (a confirmar no primeiro uso)"),
                        "{line}"
                    );
                }
            }
        }
    }

    #[test]
    fn override_aparece_como_override() {
        let (env, calls) = fake_env(None);
        let ov: Overrides = serde_json::from_str(r#"{"hypr_dispatch_mode": "classic"}"#).unwrap();
        let (rep, log) = detect(&env, &ov);
        assert_eq!(rep.dispatch_mode, DispatchMode::Classic);
        assert!(
            log[0].contains("dispatch: clássico (override)"),
            "{}",
            log[0]
        );
        assert!(!calls.lock().unwrap().iter().any(|c| c.contains("dispatch")));
    }
}
