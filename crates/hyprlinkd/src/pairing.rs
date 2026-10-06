//! Lista de dispositivos pareados (por fingerprint SHA-256 do certificado de
//! cliente) e o token de pareamento de uso único gerado a cada boot do daemon.
//! Fonte de verdade da autenticação: ver PROTOCOL.md §1/§4.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rand::RngExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct PairedDevicesFile {
    fingerprints: HashSet<String>,
    /// O que o telemóvel disse de si no `core.hello` (por fingerprint).
    /// Ausente em ficheiros antigos — `serde(default)` mantém-nos válidos.
    #[serde(default)]
    meta: HashMap<String, DeviceMeta>,
}

/// Dados em bruto do `core.hello`, guardados para a lista de dispositivos
/// funcionar com o telemóvel desligado.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceMeta {
    pub name: String,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub android: Option<String>,
    #[serde(default)]
    pub app_version: Option<String>,
    /// `capabilities` do hello, tal como vieram.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Segundos Unix do primeiro hello autorizado.
    #[serde(default)]
    pub paired_since: Option<u64>,
    #[serde(default)]
    pub last_seen: Option<u64>,
    /// Nome escolhido no PC. Sobrepõe-se ao do telemóvel e sobrevive a novos hellos.
    #[serde(default)]
    pub alias: Option<String>,
}

/// Nomes que a app põe quando ninguém lhe deu um (valor por omissão antigo).
fn is_generic_name(n: &str) -> bool {
    let n = n.trim();
    n.is_empty() || n == "Android-Phone-Client" || n == "Desconhecido"
}

impl DeviceMeta {
    /// O que se mostra: alias do PC > nome do telemóvel > «fabricante modelo».
    pub fn display_name(&self) -> String {
        if let Some(a) = self.alias.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
            return a.to_string();
        }
        if is_generic_name(&self.name) {
            let fallback = [self.manufacturer.as_deref(), self.model.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            if !fallback.is_empty() {
                return fallback;
            }
        }
        self.name.clone()
    }
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

/// Compara em tempo constante (byte a byte, sem sair mais cedo no primeiro
/// diferente) — o token de pareamento viaja em texto e um atacante na rede
/// local poderia, em tese, medir latência pra adivinhá-lo byte a byte contra
/// uma comparação normal de string.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
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
        self.devices.meta.remove(fingerprint);
        self.save()
    }

    /// Guarda o que veio no `core.hello` de um dispositivo autorizado. O
    /// `paired_since` fica o do primeiro hello; o resto é sempre o último.
    pub fn record_hello(&mut self, fingerprint: &str, mut meta: DeviceMeta, now: u64) {
        let previous = self.devices.meta.get(fingerprint);
        meta.paired_since = previous.and_then(|m| m.paired_since).or(Some(now));
        meta.last_seen = Some(now);
        meta.alias = previous.and_then(|m| m.alias.clone());
        self.devices.meta.insert(fingerprint.to_string(), meta);
        if let Err(e) = self.save() {
            eprintln!("[!] não foi possível guardar paired_devices.json: {e}");
        }
    }

    /// Define (ou limpa, se vazio) o alias de um dispositivo já emparelhado.
    pub fn set_alias(&mut self, fingerprint: &str, alias: &str) -> bool {
        let Some(m) = self.devices.meta.get_mut(fingerprint) else {
            return false;
        };
        let a = alias.trim();
        m.alias = (!a.is_empty()).then(|| a.chars().take(40).collect());
        if let Err(e) = self.save() {
            eprintln!("[!] não foi possível guardar paired_devices.json: {e}");
        }
        true
    }

    pub fn meta(&self, fingerprint: &str) -> Option<&DeviceMeta> {
        self.devices.meta.get(fingerprint)
    }

    /// Autoriza um novo dispositivo se o token bater com o desta sessão do daemon.
    pub fn try_pair_with_token(
        &mut self,
        fingerprint: &str,
        token_hex: &str,
    ) -> std::io::Result<bool> {
        if constant_time_eq(
            token_hex.to_ascii_lowercase().as_bytes(),
            self.current_token_hex.as_bytes(),
        ) {
            self.devices.fingerprints.insert(fingerprint.to_string());
            self.save()?;
            // Uso único por boot: gera um token novo pra este já não valer mais.
            self.current_token_hex = random_token_hex();
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
