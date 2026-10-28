//! `hyprlink-daemon --help/--version/<desconhecido>` e o segundo daemon: saem
//! logo, sem arrancar. Tudo num `XDG_RUNTIME_DIR`/`HOME` temporários; se o
//! binário não sair a tempo, é morto e o teste falha.

use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hyprlink-daemon");

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hyprlinkd-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("run")).unwrap();
    std::fs::create_dir_all(d.join("home")).unwrap();
    d
}

/// Corre o daemon e espera no máximo `limit`; devolve a saída e o tempo.
fn run(dir: &Path, args: &[&str], limit: Duration) -> (Output, Duration) {
    let t0 = Instant::now();
    let mut child = Command::new(BIN)
        .args(args)
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("home/.config"))
        .env("XDG_STATE_HOME", dir.join("home/.local/state"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if t0.elapsed() > limit {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!(
                "{args:?} não saiu em {limit:?} (arrancou?)\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let elapsed = t0.elapsed();
    (child.wait_with_output().unwrap(), elapsed)
}

/// Nada criado no runtime dir (sockets, status.json) nem na config.
fn untouched(dir: &Path) {
    let run: Vec<_> = std::fs::read_dir(dir.join("run")).unwrap().collect();
    assert!(run.is_empty(), "criou coisas no XDG_RUNTIME_DIR: {run:?}");
    assert!(
        !dir.join("home/.config/hyprlink").exists(),
        "gerou identidade/config"
    );
}

#[test]
fn help_sai_com_0_sem_arrancar() {
    let d = tmp("help");
    for arg in ["--help", "-h"] {
        let (out, t) = run(&d, &[arg], Duration::from_secs(3));
        assert!(out.status.success(), "{arg}: {:?}", out.status);
        assert!(t < Duration::from_secs(1), "{arg}: {t:?}");
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(
            s.contains("systemctl --user enable --now hyprlink-bridge"),
            "{s}"
        );
    }
    untouched(&d);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn version_sai_com_0_sem_arrancar() {
    let d = tmp("version");
    for arg in ["--version", "-V"] {
        let (out, t) = run(&d, &[arg], Duration::from_secs(3));
        assert!(out.status.success());
        assert!(t < Duration::from_secs(1), "{arg}: {t:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("hyprlink-daemon {}", env!("CARGO_PKG_VERSION"))
        );
    }
    untouched(&d);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn argumento_desconhecido_sai_com_2() {
    let d = tmp("bad");
    let (out, _) = run(&d, &["--hidden"], Duration::from_secs(3));
    assert_eq!(out.status.code(), Some(2));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("argumento desconhecido: --hidden"), "{e}");
    untouched(&d);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn segundo_daemon_recusa_arrancar() {
    let d = tmp("dois");
    std::fs::create_dir_all(d.join("run/hyprlink")).unwrap();
    // «Outro daemon»: o cmd.sock existe e aceita ligações.
    let sock = d.join("run/hyprlink/cmd.sock");
    let _listener = UnixListener::bind(&sock).unwrap();
    let (out, _) = run(&d, &[], Duration::from_secs(3));
    assert_eq!(out.status.code(), Some(1));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("já há um daemon a correr"), "{e}");
    assert!(e.contains("cmd.sock"), "{e}");
    assert!(
        !d.join("home/.config/hyprlink").exists(),
        "chegou a carregar a identidade"
    );
    std::fs::remove_dir_all(&d).ok();
}
