//! `hyprlinkctl doctor` — relatório de compatibilidade do ambiente.
//!
//!   hyprlinkctl doctor                      relatório legível
//!   hyprlinkctl doctor --json               o mesmo, em JSON
//!   hyprlinkctl doctor --report [ficheiro]  grava o relatório (por omissão
//!                                           ~/.local/state/hyprlink/doctor-report.txt)
//!   hyprlinkctl doctor --no-dispatch        sem `hyprctl dispatch` (também
//!                                           com HYPRLINK_NO_DISPATCH=1)
//!
//! Por omissão, o modo do `hyprctl dispatch` (clássico/Lua) testa-se com
//! `hyprctl dispatch hl.dsp.no_op()`, que não faz nada mas é um dispatch na
//! sessão. Com `--no-dispatch` o modo é inferido pela versão do Hyprland e
//! pela configuração (`hyprland::infer_mode`): é o que os agentes e scripts
//! automáticos devem usar numa sessão viva.
//!
//! Só lê; nunca altera nada. Usa a mesma deteção que o daemon
//! (`hyprlink-env`), por isso mostra o que o daemon vai mesmo fazer. O
//! relatório não leva nome de utilizador, nome da máquina, MAC, IP, SSID nem o
//! caminho da pasta pessoal.

use std::path::PathBuf;
use std::process::ExitCode;

use hyprlink_env::{Env, RealRunner, overrides, report};

pub fn run(args: &[String]) -> ExitCode {
    let json = args.iter().any(|a| a == "--json");
    let report_at = args.iter().position(|a| a == "--report");
    // O `hyprctl` corre com a instância descoberta (como no daemon): por SSH,
    // cron ou num terminal sem as variáveis, o Hyprland continua visível.
    let mut env = Env::real();
    env.runner = Box::new(RealRunner {
        envs: hyprlink_env::instance::real_session_env(),
        ..RealRunner::default()
    });
    let ov = env
        .hyprlink_config_path()
        .map(|p| overrides::load(&p))
        .unwrap_or_default();
    let rep = if no_dispatch(args, std::env::var("HYPRLINK_NO_DISPATCH").ok().as_deref()) {
        report::collect_no_dispatch(&env, &ov)
    } else {
        report::collect(&env, &ov)
    };
    let text = if json {
        report::json_redacted(&env, &rep)
    } else {
        report::text_redacted(&env, &rep)
    };

    let Some(i) = report_at else {
        println!("{text}");
        return ExitCode::SUCCESS;
    };
    let explicit = args
        .get(i + 1)
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from);
    let Some(path) =
        explicit.or_else(|| Some(env.state_dir()?.join("hyprlink").join("doctor-report.txt")))
    else {
        eprintln!("erro: sem pasta de estado (HOME/XDG_STATE_HOME); indica um ficheiro");
        return ExitCode::from(1);
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(&path, text) {
        Ok(()) => {
            let shown = report::Redactor::new(&env).apply(&path.to_string_lossy());
            println!("Relatório gravado em {shown}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("erro: não consegui gravar o relatório: {e}");
            ExitCode::from(1)
        }
    }
}

/// `--no-dispatch`, ou `HYPRLINK_NO_DISPATCH` com um valor que não seja
/// vazio nem `0`.
fn no_dispatch(args: &[String], env: Option<&str>) -> bool {
    args.iter().any(|a| a == "--no-dispatch") || env.is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sem_dispatch_por_flag_ou_variavel() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(no_dispatch(&a(&["doctor", "--no-dispatch"]), None));
        assert!(no_dispatch(&a(&["doctor"]), Some("1")));
        assert!(!no_dispatch(&a(&["doctor"]), Some("0")));
        assert!(!no_dispatch(&a(&["doctor"]), None));
    }
}
