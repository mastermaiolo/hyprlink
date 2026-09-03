//! Estado partilhado entre o daemon (tokio, thread própria) e a GUI (iced,
//! thread principal). A GUI só lê uma cópia (via `snapshot()`); o daemon é
//! quem escreve.

use std::sync::{Arc, Mutex};

const MAX_LOG_LINES: usize = 200;
/// Histórico de CLIP/NOTIF fica só em memória por ora (reseta ao reiniciar
/// o daemon) — persistência em disco com retenção configurável é Fase C.
const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone)]
pub struct ClipEntry {
    pub at: String,
    pub direction: &'static str,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct NotifEntry {
    pub at: String,
    pub app: String,
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnState {
    /// Nenhum dispositivo pareado, aguardando QR/entrada manual.
    Pairing,
    /// Handshake em andamento (QUIC/mTLS negociando).
    Connecting,
    /// core.hello trocado e autorizado.
    Connected { device_name: String, fingerprint_hex: String },
}

/// Estado real de cada módulo pra sidebar — `None`/`false` = cinza (sem
/// dados ainda ou desligado por opção), nunca vermelho: vermelho é só erro
/// de verdade ou ausência de ligação (ver `code-tarefas.md`, decisão 2).
#[derive(Debug, Clone, Default)]
pub struct ModuleStatus {
    /// Mais recente primeiro.
    pub clip_history: Vec<ClipEntry>,
    pub notif_count: u32,
    /// Mais recente primeiro.
    pub notif_history: Vec<NotifEntry>,
    pub media: Option<String>,
    pub phone_battery_pct: Option<i64>,
    pub workspace: Option<String>,
    pub audio_tap_active: bool,
    pub webcam_active: bool,
    /// Vazão medida da stream de vídeo atual (Mbps, janela de ~1s) — usado
    /// tanto pro teste de rede quanto exibido ao vivo durante um stream normal.
    pub webcam_mbps: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct HudState {
    pub conn: ConnState,
    pub local_addr: String,
    pub server_fingerprint_hex: String,
    pub pairing_token_hex: String,
    pub logs: Vec<String>,
    pub modules: ModuleStatus,
}

impl HudState {
    pub fn new(local_addr: String, server_fingerprint_hex: String, pairing_token_hex: String) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            conn: ConnState::Pairing,
            local_addr,
            server_fingerprint_hex,
            pairing_token_hex,
            logs: Vec::new(),
            modules: ModuleStatus::default(),
        }))
    }
}

pub fn push_log(state: &Arc<Mutex<HudState>>, line: impl Into<String>) {
    let line = format!("{} {}", chrono::Local::now().format("%H:%M:%S"), line.into());
    println!("{line}");
    let mut s = state.lock().unwrap();
    s.logs.push(line);
    let overflow = s.logs.len().saturating_sub(MAX_LOG_LINES);
    if overflow > 0 {
        s.logs.drain(0..overflow);
    }
}

pub fn set_connecting(state: &Arc<Mutex<HudState>>) {
    state.lock().unwrap().conn = ConnState::Connecting;
}

pub fn set_connected(state: &Arc<Mutex<HudState>>, device_name: String, fingerprint_hex: String) {
    state.lock().unwrap().conn = ConnState::Connected { device_name, fingerprint_hex };
}

pub fn set_pairing(state: &Arc<Mutex<HudState>>) {
    state.lock().unwrap().conn = ConnState::Pairing;
}

pub fn push_clip_entry(state: &Arc<Mutex<HudState>>, direction: &'static str, text: String) {
    let mut s = state.lock().unwrap();
    let at = chrono::Local::now().format("%H:%M:%S").to_string();
    s.modules.clip_history.insert(0, ClipEntry { at, direction, text });
    s.modules.clip_history.truncate(MAX_HISTORY);
}

pub fn push_notif_entry(state: &Arc<Mutex<HudState>>, app: String, title: String, text: String) {
    let mut s = state.lock().unwrap();
    s.modules.notif_count += 1;
    let at = chrono::Local::now().format("%H:%M:%S").to_string();
    s.modules.notif_history.insert(0, NotifEntry { at, app, title, text });
    s.modules.notif_history.truncate(MAX_HISTORY);
}

pub fn set_media_status(state: &Arc<Mutex<HudState>>, status: Option<String>) {
    state.lock().unwrap().modules.media = status;
}

pub fn set_phone_battery(state: &Arc<Mutex<HudState>>, pct: i64) {
    state.lock().unwrap().modules.phone_battery_pct = Some(pct);
}

pub fn set_workspace(state: &Arc<Mutex<HudState>>, workspace: String) {
    state.lock().unwrap().modules.workspace = Some(workspace);
}

pub fn set_audio_tap_active(state: &Arc<Mutex<HudState>>, active: bool) {
    state.lock().unwrap().modules.audio_tap_active = active;
}

pub fn set_webcam_active(state: &Arc<Mutex<HudState>>, active: bool) {
    let mut s = state.lock().unwrap();
    s.modules.webcam_active = active;
    if !active {
        s.modules.webcam_mbps = None;
    }
}

pub fn set_webcam_mbps(state: &Arc<Mutex<HudState>>, mbps: f64) {
    state.lock().unwrap().modules.webcam_mbps = Some(mbps);
}
