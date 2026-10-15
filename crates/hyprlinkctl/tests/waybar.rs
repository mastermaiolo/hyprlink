//! `hyprlinkctl status --waybar` e o `contrib/waybar-hyprlink.sh` dão o
//! mesmo JSON (uma linha, saída 0) — pelo binário, pela versão jq e com um
//! hyprlinkctl antigo (sem `--waybar`) no PATH.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_hyprlinkctl");

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contrib/waybar-hyprlink.sh")
}

struct Rt {
    root: PathBuf,
}

impl Rt {
    fn new(name: &str, status: Option<&Value>) -> Self {
        let root =
            std::env::temp_dir().join(format!("hyprlinkctl-waybar-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("run/hyprlink")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        if let Some(s) = status {
            std::fs::write(root.join("run/hyprlink/status.json"), s.to_string()).unwrap();
        }
        // Só o que o script precisa; o hyprlinkctl entra (ou não) por teste.
        for tool in ["bash", "jq", "cat", "id"] {
            std::os::unix::fs::symlink(format!("/usr/bin/{tool}"), root.join("bin").join(tool))
                .unwrap();
        }
        Rt { root }
    }

    fn with_ctl(&self, ctl: &str) {
        let p = self.root.join("bin/hyprlinkctl");
        let _ = std::fs::remove_file(&p);
        match ctl {
            "novo" => std::os::unix::fs::symlink(BIN, &p).unwrap(),
            // O hyprlinkctl 0.1.0: `status …` vai pelo protocolo de texto.
            "antigo" => {
                use std::os::unix::fs::PermissionsExt;
                std::fs::write(&p, "#!/usr/bin/bash\necho 'ok {\"connected\":true}'\n").unwrap();
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            _ => {}
        }
    }

    fn run(&self, mut c: Command) -> Value {
        let out = c
            .env_clear()
            .env("PATH", self.root.join("bin"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .output()
            .unwrap();
        assert!(out.status.success());
        let text = String::from_utf8(out.stdout).unwrap();
        assert_eq!(text.lines().count(), 1, "uma só linha: {text:?}");
        serde_json::from_str(text.trim()).unwrap()
    }

    fn binary(&self) -> Value {
        let mut c = Command::new(BIN);
        c.args(["status", "--waybar"]);
        self.run(c)
    }

    fn script(&self) -> Value {
        let mut c = Command::new("/usr/bin/bash");
        c.arg(script());
        self.run(c)
    }
}

impl Drop for Rt {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn casos() -> Vec<(&'static str, Option<Value>)> {
    vec![
        ("ausente", None),
        (
            "desligado",
            Some(json!({"connected": false, "device": null})),
        ),
        (
            "ligado",
            Some(json!({
                "connected": true,
                "device": "Poco \"F4\"\n2.ª linha 🚀 <b>&co",
                "phone_battery_pct": 49,
                "media": "Ana & Rui — <ao vivo>",
                "audio_tap": true, "mic": false, "speaker": true,
            })),
        ),
        (
            "minimo",
            Some(
                json!({"connected": true, "device": null, "phone_battery_pct": null, "media": ""}),
            ),
        ),
    ]
}

#[test]
fn binario_e_script_dao_o_mesmo() {
    for (name, status) in casos() {
        let rt = Rt::new(name, status.as_ref());
        let bin = rt.binary();
        // Versão jq (sem hyprlinkctl no PATH).
        assert_eq!(rt.script(), bin, "{name}: jq");
        // Com o hyprlinkctl novo: o script delega.
        rt.with_ctl("novo");
        assert_eq!(rt.script(), bin, "{name}: binário");
        // Com um hyprlinkctl antigo: «ok …» não é JSON → volta ao jq.
        rt.with_ctl("antigo");
        assert_eq!(rt.script(), bin, "{name}: antigo");
    }
}

#[test]
fn classes_e_textos() {
    let rt = Rt::new("classes", None);
    assert_eq!(rt.binary()["class"], "disconnected");
    let st = json!({"connected": true, "device": "A <b>", "phone_battery_pct": 7});
    let rt = Rt::new("classes2", Some(&st));
    let v = rt.binary();
    assert_eq!(v["class"], "connected");
    assert_eq!(v["text"], "📱 7%");
    assert_eq!(v["tooltip"], "A &lt;b&gt;");
}
