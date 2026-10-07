//! `hyprlinkctl doctor` — relatório de compatibilidade do ambiente.
//!
//!   hyprlinkctl doctor                      relatório legível
//!   hyprlinkctl doctor --json               o mesmo, em JSON
//!   hyprlinkctl doctor --report [ficheiro]  grava o relatório (por omissão
//!                                           ~/.local/state/hyprlink/doctor-report.txt)
//!
//! Só lê; nunca altera nada. Usa a mesma deteção que o daemon
//! (`hyprlink-env`), por isso mostra o que o daemon vai mesmo fazer. O
//! relatório não leva nome de utilizador, nome da máquina, MAC, IP, SSID nem o
//! caminho da pasta pessoal.

use std::path::PathBuf;
use std::process::ExitCode;

use hyprlink_env::{Env, overrides, report};

pub fn run(args: &[String]) -> ExitCode {
    let json = args.iter().any(|a| a == "--json");
    let report_at = args.iter().position(|a| a == "--report");
    let env = Env::real();
    let ov = env
        .hyprlink_config_path()
        .map(|p| overrides::load(&p))
        .unwrap_or_default();
    let rep = report::collect(&env, &ov);
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
