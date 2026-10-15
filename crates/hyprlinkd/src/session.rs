//! Comandos que precisam da sessão gráfica (Wayland/Hyprland): `wl-copy`,
//! `wl-paste`, `xdg-open`, `hyprctl`, as ações (bloquear, capturas,
//! `notify-send`…) e o `pkexec` (o agente do polkit é uma janela).
//!
//! O daemon pode arrancar pelo `systemd --user` sem `WAYLAND_DISPLAY` nem
//! `HYPRLAND_INSTANCE_SIGNATURE`, ou com uma assinatura velha: estes
//! construtores juntam as variáveis que faltam ou estão mortas
//! (`hyprlink_env::instance`, a mesma descoberta do socket2 e da GUI).
//! O áudio (`pactl`, `wpctl`) só precisa do `XDG_RUNTIME_DIR`, que o
//! `systemd --user` já dá, e fica de fora.

use std::ffi::OsStr;

pub use hyprlink_env::instance::real_session_env;

/// `std::process::Command::new(prog)` com as variáveis de sessão.
pub fn command(prog: impl AsRef<OsStr>) -> std::process::Command {
    let mut c = std::process::Command::new(prog);
    c.envs(real_session_env());
    c
}

/// Como [`command`], para o tokio.
pub fn tokio_command(prog: impl AsRef<OsStr>) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(prog);
    c.envs(real_session_env());
    c
}

#[cfg(test)]
mod tests {
    //! Um processo filho (este mesmo binário de testes) corre os caminhos
    //! reais do daemon com um `PATH` só com binários falsos e **sem**
    //! `WAYLAND_DISPLAY`/`HYPRLAND_INSTANCE_SIGNATURE`, como o daemon do
    //! `systemd --user`. Cada falso grava o ambiente que recebeu. Nada chega
    //! ao clipboard, ao browser nem ao Hyprland reais.

    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    const FAKES: &[&str] = &["wl-copy", "wl-paste", "xdg-open", "hyprctl"];

    fn fake_bin(dir: &Path, name: &str) {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        // Grava o ambiente; só o wl-copy lê o stdin (o texto). Os outros não
        // lhe tocam: o xdg-open herda o stdin do teste e ficaria à espera.
        let drain = if name == "wl-copy" {
            "/usr/bin/cat > /dev/null\n"
        } else {
            ""
        };
        let script =
            format!("#!/bin/sh\n/usr/bin/env > \"$HYPRLINK_FAKE_LOG/{name}.env\"\n{drain}");
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn wait_for(path: &Path) -> String {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(5) {
            if let Ok(s) = std::fs::read_to_string(path)
                && !s.is_empty()
            {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{} não foi chamado", path.display());
    }

    #[test]
    fn comandos_graficos_recebem_a_sessao_descoberta() {
        let root: PathBuf =
            std::env::temp_dir().join(format!("hyprlinkd-sessao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (bin, log, rt) = (root.join("bin"), root.join("log"), root.join("run"));
        for d in [&bin, &log] {
            std::fs::create_dir_all(d).unwrap();
        }
        for f in FAKES {
            fake_bin(&bin, f);
        }
        // Uma instância «viva» (o PID do lock é este processo) com o seu
        // WAYLAND_DISPLAY.
        let inst = rt.join("hypr/teste_1_1");
        std::fs::create_dir_all(&inst).unwrap();
        std::fs::write(inst.join(".socket.sock"), b"").unwrap();
        std::fs::write(
            inst.join("hyprland.lock"),
            format!("{}\nwayland-7\n", std::process::id()),
        )
        .unwrap();
        std::fs::write(rt.join("wayland-7"), b"").unwrap();

        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "session::tests::filho",
                "--exact",
                "--ignored",
                "--test-threads=1",
            ])
            .env_clear()
            .env("PATH", &bin)
            .env("XDG_RUNTIME_DIR", &rt)
            .env("HOME", &root)
            .env("HYPRLINK_FAKE_LOG", &log)
            .env("HYPRLINK_TEST_CHILD", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());

        for f in FAKES {
            let env = wait_for(&log.join(format!("{f}.env")));
            assert!(
                env.lines().any(|l| l == "WAYLAND_DISPLAY=wayland-7"),
                "{f} sem WAYLAND_DISPLAY:\n{env}"
            );
            assert!(
                env.lines()
                    .any(|l| l == "HYPRLAND_INSTANCE_SIGNATURE=teste_1_1"),
                "{f} sem HYPRLAND_INSTANCE_SIGNATURE:\n{env}"
            );
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// Só corre como filho do teste acima (senão usaria o PATH real).
    #[tokio::test]
    #[ignore]
    async fn filho() {
        if std::env::var_os("HYPRLINK_TEST_CHILD").is_none() {
            return;
        }
        crate::clip::set_from_remote("texto de teste", &crate::clip::new_guard()).await;
        let out = crate::session::tokio_command("wl-paste")
            .arg("--list-types")
            .output()
            .await;
        assert!(out.is_ok());
        assert!(crate::open_url::safe_open_url("https://exemplo.pt").is_ok());
        // Consulta (não é dispatch), e o hyprctl é o falso.
        let _ = crate::hypr::workspaces_json();
        // O xdg-open foi lançado sem esperar: dá-lhe tempo de gravar.
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
