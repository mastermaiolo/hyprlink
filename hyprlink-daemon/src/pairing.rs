//! Lista de dispositivos pareados (por fingerprint SHA-256 do certificado de
//! cliente) e o token de pareamento de uso único gerado a cada boot do daemon.
//! Fonte de verdade da autenticação: ver PROTOCOL.md §1/§4.

use std::collections::HashSet;
use std::path::PathBuf;

use rand::RngExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct PairedDevicesFile {
    fingerprints: HashSet<String>,
}

pub struct PairingStore {
    path: PathBuf,
    devices: PairedDevicesFile,
    /// Token gerado neste boot; autoriza exatamente um novo pareamento por sessão do daemon.
    pub current_token_hex: String,
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .expect("sem diretório de config do usuário (XDG_CONFIG_HOME/HOME)")
        .join("hyprlink")
}

fn random_token_hex() -> String {
    let mut buf = [0u8; 16];
    rand::rng().fill(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

impl PairingStore {
    pub fn load() -> std::io::Result<Self> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("paired_devices.json");
        let devices = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ok(Self {
            path,
            devices,
            current_token_hex: random_token_hex(),
        })
    }

    pub fn is_paired(&self, fingerprint: &str) -> bool {
        self.devices.fingerprints.contains(fingerprint)
    }

    /// Só os fingerprints — a GUI (CONFIG) não tem nome/last-seen por
    /// dispositivo hoje (exigiria persistir isso no `core.hello`, fora de
    /// escopo por ora), mostra o que existe de verdade.
    pub fn list_fingerprints(&self) -> Vec<String> {
        let mut v: Vec<String> = self.devices.fingerprints.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn revoke(&mut self, fingerprint: &str) -> std::io::Result<()> {
        self.devices.fingerprints.remove(fingerprint);
        self.save()
    }

    /// Autoriza um novo dispositivo se o token bater com o desta sessão do daemon.
    pub fn try_pair_with_token(&mut self, fingerprint: &str, token_hex: &str) -> std::io::Result<bool> {
        if token_hex.eq_ignore_ascii_case(&self.current_token_hex) {
            self.devices.fingerprints.insert(fingerprint.to_string());
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.devices).expect("serialização infalível");
        std::fs::write(&self.path, json)
    }
}
