//! `hyprlinkctl doctor --no-dispatch` com um `hyprctl` falso no `PATH` que
//! responde ao `version` e **falha** (e regista) se receber `dispatch`.
//! Nunca toca no Hyprland real: o `PATH` só tem o falso.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_hyprlinkctl");

struct Fake {
    root: PathBuf,
}

impl Fake {
    fn new(name: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("hyprlinkctl-doc-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["bin", "home/.config/hypr", "run"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("home/.config/hypr/hyprland.lua"), b"-- teste\n").unwrap();
        let hyprctl = root.join("bin/hyprctl");
        std::fs::write(
            &hyprctl,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{log}'\n\
                 case \"$1\" in\n\
                   version) echo 'Hyprland 0.56.2 built from branch teste'; exit 0;;\n\
                   dispatch) echo 'DISPATCH PROIBIDO' >&2; exit 1;;\n\
                 esac\nexit 1\n",
                log = root.join("hyprctl.log").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&hyprctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        Fake { root }
    }

    fn doctor(&self, args: &[&str], env_flag: bool) -> String {
        let mut c = Command::new(BIN);
        c.arg("doctor")
            .args(args)
            .env_clear()
            .env("PATH", self.root.join("bin"))
            .env("HOME", self.root.join("home"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("LANG", "pt_PT.UTF-8");
        if env_flag {
            c.env("HYPRLINK_NO_DISPATCH", "1");
        }
        let out = c.output().unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.root.join("hyprctl.log")).unwrap_or_default()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn no_dispatch_in(calls: &str) -> bool {
    !calls.lines().any(|l| l.starts_with("dispatch"))
}

#[test]
fn flag_no_dispatch_infere_o_modo_sem_dispatch() {
    let f = Fake::new("flag");
    let out = f.doctor(&["--no-dispatch"], false);
    assert!(no_dispatch_in(&f.calls()), "houve dispatch:\n{}", f.calls());
    assert!(f.calls().contains("version"), "devia pedir a versão");
    assert!(out.contains("Hyprland 0.56.2"), "{out}");
    assert!(
        out.contains("dispatch     : Lua — inferido pela versão"),
        "{out}"
    );
}

#[test]
fn variavel_no_dispatch_e_json() {
    let f = Fake::new("var");
    let out = f.doctor(&["--json"], true);
    assert!(no_dispatch_in(&f.calls()), "houve dispatch:\n{}", f.calls());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["dispatch_mode"], "lua");
    assert!(
        v["dispatch_mode_source"]
            .as_str()
            .is_some_and(|s| s.starts_with("inferido pela versão"))
    );
}

/// Controlo: sem a flag, o doctor faz mesmo o teste por dispatch (é isso
/// que o falso apanha) — prova que os testes acima detetariam um dispatch.
#[test]
fn sem_a_flag_o_falso_ve_o_dispatch() {
    let f = Fake::new("controlo");
    let _ = f.doctor(&[], false);
    assert!(!no_dispatch_in(&f.calls()), "{}", f.calls());
}
