//! Configuração persistente do daemon (por ora só a pasta de destino dos
//! ficheiros recebidos) — `~/.config/hyprlink/config.json`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub type SharedConfig = Arc<Mutex<AppConfig>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shortcut {
    pub name: String,
    /// Mesmo formato aceito pelo campo livre do CONTROL: "workspace 2",
    /// "focuswindow address:0x..", etc — vai direto pra `hypr::dispatch`.
    pub command: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TrackSettings {
    #[serde(default = "default_sensitivity")]
    pub sensitivity: f32,
    #[serde(default = "default_scroll_speed")]
    pub scroll_speed: f32,
    /// ponytail: multiplicador linear fixo quando ativo, não uma curva de
    /// aceleração de verdade — suficiente pra "sente mais rápido puxando
    /// forte", revisitar se um dia precisar de algo mais fino.
    #[serde(default)]
    pub acceleration: bool,
    #[serde(default)]
    pub invert_scroll: bool,
    #[serde(default = "default_true")]
    pub virtual_keyboard: bool,
}

fn default_sensitivity() -> f32 {
    1.0
}
fn default_scroll_speed() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}

impl Default for TrackSettings {
    fn default() -> Self {
        Self { sensitivity: 1.0, scroll_speed: 1.0, acceleration: false, invert_scroll: false, virtual_keyboard: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BatteryAlertKind {
    Low,
    Full,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct BatteryAlerts {
    #[serde(default)]
    pub low: bool,
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub download_dir: PathBuf,
    /// Minimizar pra bandeja usa uma workspace especial do Hyprland (ver
    /// `hypr.rs::tray_hide/tray_show`) — não é Wayland genérico, então dá
    /// pra desligar se causar problema noutro compositor/config.
    #[serde(default = "default_tray_special_workspace")]
    pub tray_special_workspace: bool,
    /// Atalhos do módulo CONTROL (nome → comando `hyprctl dispatch`).
    #[serde(default)]
    pub shortcuts: Vec<Shortcut>,
    #[serde(default)]
    pub battery_alerts: BatteryAlerts,
    #[serde(default)]
    pub track: TrackSettings,
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
            .unwrap_or_else(|| Self {
                download_dir: default_download_dir(),
                tray_special_workspace: default_tray_special_workspace(),
                shortcuts: Vec::new(),
                battery_alerts: BatteryAlerts::default(),
                track: TrackSettings::default(),
            })
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

pub fn shortcuts(config: &SharedConfig) -> Vec<Shortcut> {
    config.lock().unwrap().shortcuts.clone()
}

pub fn add_shortcut(config: &SharedConfig, name: String, command: String) {
    let mut c = config.lock().unwrap();
    c.shortcuts.push(Shortcut { name, command });
    c.save();
}

pub fn remove_shortcut(config: &SharedConfig, index: usize) {
    let mut c = config.lock().unwrap();
    if index < c.shortcuts.len() {
        c.shortcuts.remove(index);
        c.save();
    }
}

pub fn battery_alerts(config: &SharedConfig) -> BatteryAlerts {
    config.lock().unwrap().battery_alerts
}

pub fn set_battery_alert_low(config: &SharedConfig, enabled: bool) {
    let mut c = config.lock().unwrap();
    c.battery_alerts.low = enabled;
    c.save();
}

pub fn set_battery_alert_full(config: &SharedConfig, enabled: bool) {
    let mut c = config.lock().unwrap();
    c.battery_alerts.full = enabled;
    c.save();
}

pub fn track_settings(config: &SharedConfig) -> TrackSettings {
    config.lock().unwrap().track
}

pub fn set_track_sensitivity(config: &SharedConfig, v: f32) {
    let mut c = config.lock().unwrap();
    c.track.sensitivity = v;
    c.save();
}

pub fn set_track_scroll_speed(config: &SharedConfig, v: f32) {
    let mut c = config.lock().unwrap();
    c.track.scroll_speed = v;
    c.save();
}

pub fn set_track_acceleration(config: &SharedConfig, enabled: bool) {
    let mut c = config.lock().unwrap();
    c.track.acceleration = enabled;
    c.save();
}

pub fn set_track_invert_scroll(config: &SharedConfig, enabled: bool) {
    let mut c = config.lock().unwrap();
    c.track.invert_scroll = enabled;
    c.save();
}

pub fn set_track_virtual_keyboard(config: &SharedConfig, enabled: bool) {
    let mut c = config.lock().unwrap();
    c.track.virtual_keyboard = enabled;
    c.save();
}
