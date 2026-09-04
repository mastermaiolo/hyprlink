//! Configuração persistente do daemon (por ora só a pasta de destino dos
//! ficheiros recebidos) — `~/.config/hyprlink/config.json`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub type SharedConfig = Arc<Mutex<AppConfig>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub download_dir: PathBuf,
    /// Minimizar pra bandeja usa uma workspace especial do Hyprland (ver
    /// `hypr.rs::tray_hide/tray_show`) — não é Wayland genérico, então dá
    /// pra desligar se causar problema noutro compositor/config.
    #[serde(default = "default_tray_special_workspace")]
    pub tray_special_workspace: bool,
}

fn default_download_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default()).join("HyprLink")
}

fn default_tray_special_workspace() -> bool {
    true
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .expect("sem diretório de config do usuário (XDG_CONFIG_HOME/HOME)")
        .join("hyprlink")
        .join("config.json")
}

impl AppConfig {
    fn load_from_disk() -> Self {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| Self { download_dir: default_download_dir(), tray_special_workspace: default_tray_special_workspace() })
    }

    fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

pub fn load() -> SharedConfig {
    Arc::new(Mutex::new(AppConfig::load_from_disk()))
}

pub fn download_dir(config: &SharedConfig) -> PathBuf {
    config.lock().unwrap().download_dir.clone()
}

pub fn set_download_dir(config: &SharedConfig, dir: &Path) {
    let mut c = config.lock().unwrap();
    c.download_dir = dir.to_path_buf();
    c.save();
}

pub fn tray_special_workspace(config: &SharedConfig) -> bool {
    config.lock().unwrap().tray_special_workspace
}

pub fn set_tray_special_workspace(config: &SharedConfig, enabled: bool) {
    let mut c = config.lock().unwrap();
    c.tray_special_workspace = enabled;
    c.save();
}
