//! Que shell (barra/lançador/bloqueio) está a correr.
//!
//! Fontes das regras: ver `docs/COMPATIBILIDADE.md` §1. Sem ficheiros dos
//! processos nem ferramentas, o resultado é `None`.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    NoctaliaV5,
    NoctaliaV4,
    Caelestia,
    Ryoku,
    /// Há um Quickshell a correr, mas não é nenhuma das conhecidas.
    Other,
    None,
}

impl Shell {
    pub fn label(self) -> &'static str {
        match self {
            Shell::NoctaliaV5 => "Noctalia v5",
            Shell::NoctaliaV4 => "Noctalia v4 (legacy)",
            Shell::Caelestia => "Caelestia",
            Shell::Ryoku => "Ryoku",
            Shell::Other => "outra (Quickshell)",
            Shell::None => "nenhuma detetada",
        }
    }
}

/// Um processo: só o nome e os argumentos (nunca se imprimem).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Proc {
    pub comm: String,
    pub args: Vec<String>,
}

/// Lista os processos de `proc_dir` (`/proc`); os ilegíveis saltam-se.
pub fn list_procs(proc_dir: &Path) -> Vec<Proc> {
    let Ok(rd) = std::fs::read_dir(proc_dir) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
        })
        .filter_map(|e| {
            let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
            let cmdline = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
            let args = cmdline
                .split(|b| *b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            Some(Proc {
                comm: comm.trim().to_string(),
                args,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detection {
    pub shell: Shell,
    /// Porquê (sem caminhos nem argumentos completos).
    pub evidence: String,
}

fn is_qs(p: &Proc) -> bool {
    matches!(p.comm.as_str(), "qs" | "quickshell")
}

/// Deteta a shell: primeiro pelos processos a correr, depois pelos
/// binários instalados.
pub fn detect(procs: &[Proc], have: &dyn Fn(&str) -> bool) -> Detection {
    let det = |shell, evidence: &str| Detection {
        shell,
        evidence: evidence.to_string(),
    };
    if procs.iter().any(|p| p.comm.starts_with("ryoku-shell")) {
        return det(Shell::Ryoku, "processo ryoku-shell a correr");
    }
    if procs.iter().any(|p| p.comm == "noctalia") {
        return det(Shell::NoctaliaV5, "processo noctalia a correr");
    }
    if procs
        .iter()
        .any(|p| is_qs(p) && p.args.iter().any(|a| a == "noctalia-shell"))
    {
        return det(
            Shell::NoctaliaV4,
            "Quickshell com a configuração noctalia-shell",
        );
    }
    if procs.iter().any(|p| {
        p.comm.contains("caelestia") || (is_qs(p) && p.args.iter().any(|a| a.contains("caelestia")))
    }) {
        return det(Shell::Caelestia, "processo da Caelestia a correr");
    }
    // Sem processo reconhecido: o que está instalado.
    if have("ryoku-shell") {
        return det(
            Shell::Ryoku,
            "binário ryoku-shell instalado (não está a correr)",
        );
    }
    if have("caelestia") {
        return det(
            Shell::Caelestia,
            "binário caelestia instalado (não está a correr)",
        );
    }
    if have("noctalia") {
        return det(
            Shell::NoctaliaV5,
            "binário noctalia instalado (não está a correr)",
        );
    }
    if procs.iter().any(is_qs) {
        return det(
            Shell::Other,
            "há um Quickshell a correr, sem configuração conhecida",
        );
    }
    det(Shell::None, "nenhum processo nem binário conhecido")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(comm: &str, args: &[&str]) -> Proc {
        Proc {
            comm: comm.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn none(_: &str) -> bool {
        false
    }

    #[test]
    fn cada_shell_pelo_processo() {
        assert_eq!(detect(&[p("ryoku-shell", &[])], &none).shell, Shell::Ryoku);
        assert_eq!(
            detect(&[p("noctalia", &[])], &none).shell,
            Shell::NoctaliaV5
        );
        assert_eq!(
            detect(&[p("qs", &["qs", "-c", "noctalia-shell"])], &none).shell,
            Shell::NoctaliaV4
        );
        assert_eq!(
            detect(
                &[p("quickshell", &["quickshell", "-c", "caelestia"])],
                &none
            )
            .shell,
            Shell::Caelestia
        );
        assert_eq!(
            detect(&[p("qs", &["qs", "-c", "x"])], &none).shell,
            Shell::Other
        );
        assert_eq!(detect(&[p("bash", &[])], &none).shell, Shell::None);
    }

    #[test]
    fn sem_processo_vale_o_binario_instalado() {
        let d = detect(&[], &|t| t == "caelestia");
        assert_eq!(d.shell, Shell::Caelestia);
        assert!(d.evidence.contains("instalado"));
        assert_eq!(detect(&[], &|t| t == "noctalia").shell, Shell::NoctaliaV5);
    }

    #[test]
    fn processo_vence_binario() {
        let d = detect(&[p("noctalia", &[])], &|t| t == "caelestia");
        assert_eq!(d.shell, Shell::NoctaliaV5);
    }
}
