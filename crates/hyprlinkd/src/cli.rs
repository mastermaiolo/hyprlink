//! Argumentos do `hyprlink-daemon`. Sem argumentos arranca (é o que o
//! `hyprlink-bridge.service` faz); `--help`/`--version` só informam; qualquer
//! outro argumento é recusado **sem arrancar**. Antes, `--help` arrancava um
//! daemon a sério ao lado do serviço.

use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Cli {
    Run,
    Help,
    Version,
    /// Argumento desconhecido (o primeiro).
    Bad(String),
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Cli {
    let mut it = args.into_iter();
    let Some(first) = it.next() else {
        return Cli::Run;
    };
    match first.as_str() {
        "-h" | "--help" => Cli::Help,
        "-V" | "--version" => Cli::Version,
        _ => Cli::Bad(first),
    }
}

pub fn version() -> String {
    format!("hyprlink-daemon {}", env!("CARGO_PKG_VERSION"))
}

pub fn usage() -> String {
    format!(
        "{}: daemon do HyprLink (telemóvel Android ⇄ PC com Hyprland, QUIC + mTLS na porta 7443/UDP)

Uso:
  hyprlink-daemon              arranca o daemon (normalmente pelo serviço)
  hyprlink-daemon --help       esta ajuda
  hyprlink-daemon --version    a versão

Como serviço:
  systemctl --user enable --now hyprlink-bridge
  systemctl --user restart hyprlink-bridge
  journalctl --user -u hyprlink-bridge -f      registo do serviço

Ficheiros:
  ~/.config/hyprlink/              certificado, chave, aparelhos emparelhados
  ~/.config/hyprlink/config.json   opções (ver README, «Compatibilidade»)
  ~/.local/state/hyprlink/daemon.log   registo quando arrancado por scripts/hyprlink-start.sh
  $XDG_RUNTIME_DIR/hyprlink.sock, hyprlink/cmd.sock, hyprlink/status.json

Variáveis de ambiente que lê:
  XDG_RUNTIME_DIR, XDG_CONFIG_HOME, XDG_STATE_HOME, HOME   onde ficam sockets e ficheiros
  HYPRLAND_INSTANCE_SIGNATURE, WAYLAND_DISPLAY, XDG_CURRENT_DESKTOP, XDG_SESSION_TYPE
                                   a sessão do Hyprland (descobertas sozinhas se faltarem)

A interface é o hyprlink-gui e o hyprlinkctl. Só pode correr um daemon de cada vez.",
        version()
    )
}

/// Sockets que um daemon a correr serve.
fn sockets() -> Vec<PathBuf> {
    [
        hyprlink_proto::ipc::socket_path(),
        crate::ctl::socket_path(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Já há um daemon a correr? Devolve o socket que respondeu. Um socket que
/// existe mas não responde é o resto de um daemon que morreu: não conta.
pub fn running_daemon() -> Option<PathBuf> {
    sockets()
        .into_iter()
        .find(|p| std::os::unix::net::UnixStream::connect(p).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Cli {
        parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn argumentos() {
        assert_eq!(p(&[]), Cli::Run);
        assert_eq!(p(&["--help"]), Cli::Help);
        assert_eq!(p(&["-h"]), Cli::Help);
        assert_eq!(p(&["--version"]), Cli::Version);
        assert_eq!(p(&["-V"]), Cli::Version);
        assert_eq!(p(&["--hidden"]), Cli::Bad("--hidden".into()));
        assert_eq!(p(&["start"]), Cli::Bad("start".into()));
    }

    #[test]
    fn ajuda_tem_o_essencial() {
        let u = usage();
        for s in [
            "systemctl --user enable --now hyprlink-bridge",
            "config.json",
            "journalctl",
            "XDG_RUNTIME_DIR",
            env!("CARGO_PKG_VERSION"),
        ] {
            assert!(u.contains(s), "falta {s:?}");
        }
    }
}
