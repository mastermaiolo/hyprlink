//! Estado partilhado entre o daemon (tokio, thread própria) e a GUI (iced,
//! thread principal). A GUI só lê uma cópia (via `snapshot()`); o daemon é
//! quem escreve.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MAX_LOG_LINES: usize = 200;
/// Histórico de CLIP/NOTIF fica só em memória por ora (reseta ao reiniciar
/// o daemon) — persistência em disco com retenção configurável é Fase C.
const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone)]
pub struct ClipEntry {
    pub id: u64,
    pub at: String,
    pub direction: &'static str,
    pub text: String,
    pub pinned: bool,
}

/// Transferência em andamento — só uma por vez (o protocolo não faz fila).
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub name: String,
    pub direction: &'static str, // "recebendo" · "enviando"
    pub bytes: u64,
    pub total: u64,
    pub started_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct TransferRecord {
    pub at: String,
    pub name: String,
    pub direction: &'static str,
    pub bytes: u64,
    pub duration_secs: u64,
    pub ok: bool,
    pub error: Option<String>,
}

/// Amostra periódica (`sample_battery_history`, a cada ~5 min) — 144
/// amostras cobrem 12h, o bastante pro gráfico de barras do BATT.
#[derive(Debug, Clone)]
pub struct BatterySample {
    pub pc: Option<i64>,
    pub phone: Option<i64>,
}
const MAX_BATTERY_SAMPLES: usize = 144;

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
    /// Mais recente primeiro (exceto os fixados, que a GUI ordena no topo).
    pub clip_history: Vec<ClipEntry>,
    clip_next_id: u64,
    pub notif_count: u32,
    /// Mais recente primeiro.
    pub notif_history: Vec<NotifEntry>,
    pub media: Option<String>,
    pub phone_battery_pct: Option<i64>,
    pub pc_battery_pct: Option<i64>,
    pub pc_battery_charging: bool,
    /// Mais antiga primeiro (ordem de gráfico).
    pub battery_history: Vec<BatterySample>,
    pub workspace: Option<String>,
    /// `None` = nenhuma transferência em andamento agora.
    pub file_transfer: Option<TransferProgress>,
    file_cancel: Option<Arc<AtomicBool>>,
    /// Mais recente primeiro.
    pub file_history: Vec<TransferRecord>,
    /// Picos recentes (0-100%) do audio tap — vira o VU meter do AUDIO.
    /// Mais recente no fim, capado em `MAX_VU_SAMPLES`.
    pub audio_vu: Vec<u8>,
    pub audio_tap_active: bool,
    pub audio_tap_bytes: u64,
    audio_tap_started_at: Option<std::time::Instant>,
    pub webcam_active: bool,
    webcam_started_at: Option<std::time::Instant>,
    /// (hora, duração) do último stream que terminou nesta sessão do daemon.
    pub webcam_last_used: Option<(String, String)>,
    pub mic_active: bool,
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
    let id = s.modules.clip_next_id;
    s.modules.clip_next_id += 1;
    s.modules.clip_history.insert(0, ClipEntry { id, at, direction, text, pinned: false });
    s.modules.clip_history.truncate(MAX_HISTORY);
}

/// Fixados não escapam do teto de `MAX_HISTORY` (sem persistência em disco
/// ainda, ver comentário na constante) — só ficam ordenados no topo da lista
/// enquanto estiverem dentro da janela das últimas 50 entradas.
pub fn toggle_clip_pin(state: &Arc<Mutex<HudState>>, id: u64) {
    let mut s = state.lock().unwrap();
    if let Some(entry) = s.modules.clip_history.iter_mut().find(|e| e.id == id) {
        entry.pinned = !entry.pinned;
    }
}

pub fn push_notif_entry(state: &Arc<Mutex<HudState>>, app: String, title: String, text: String) {
    let mut s = state.lock().unwrap();
    s.modules.notif_count += 1;
    let at = chrono::Local::now().format("%H:%M:%S").to_string();
    s.modules.notif_history.insert(0, NotifEntry { at, app, title, text });
    s.modules.notif_history.truncate(MAX_HISTORY);
}

/// Marca o início de uma transferência e devolve a flag de cancelamento —
/// `share.rs` checa essa flag a cada bloco lido/escrito.
pub fn start_file_transfer(state: &Arc<Mutex<HudState>>, name: String, direction: &'static str, total: u64) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut s = state.lock().unwrap();
    s.modules.file_transfer = Some(TransferProgress { name, direction, bytes: 0, total, started_at: std::time::Instant::now() });
    s.modules.file_cancel = Some(cancel.clone());
    cancel
}

pub fn update_file_transfer_progress(state: &Arc<Mutex<HudState>>, bytes: u64) {
    if let Some(t) = state.lock().unwrap().modules.file_transfer.as_mut() {
        t.bytes = bytes;
    }
}

pub fn cancel_file_transfer(state: &Arc<Mutex<HudState>>) {
    if let Some(flag) = &state.lock().unwrap().modules.file_cancel {
        flag.store(true, Ordering::Relaxed);
    }
}

pub fn finish_file_transfer(state: &Arc<Mutex<HudState>>, ok: bool, error: Option<String>) {
    let mut s = state.lock().unwrap();
    if let Some(t) = s.modules.file_transfer.take() {
        let duration_secs = t.started_at.elapsed().as_secs();
        s.modules.file_history.insert(0, TransferRecord { at: chrono::Local::now().format("%H:%M").to_string(), name: t.name, direction: t.direction, bytes: t.bytes, duration_secs, ok, error });
        s.modules.file_history.truncate(MAX_HISTORY);
    }
    s.modules.file_cancel = None;
}

pub fn set_media_status(state: &Arc<Mutex<HudState>>, status: Option<String>) {
    state.lock().unwrap().modules.media = status;
}

pub fn set_phone_battery(state: &Arc<Mutex<HudState>>, pct: i64) {
    state.lock().unwrap().modules.phone_battery_pct = Some(pct);
}

pub fn phone_battery_pct(state: &Arc<Mutex<HudState>>) -> Option<i64> {
    state.lock().unwrap().modules.phone_battery_pct
}

pub fn set_pc_battery(state: &Arc<Mutex<HudState>>, level: i64, charging: bool) {
    let mut s = state.lock().unwrap();
    s.modules.pc_battery_pct = Some(level);
    s.modules.pc_battery_charging = charging;
}

/// Chamado periodicamente (`battery::poll_and_push`) — grava o valor mais
/// recente conhecido de cada bateria, não força uma leitura nova.
pub fn sample_battery_history(state: &Arc<Mutex<HudState>>) {
    let mut s = state.lock().unwrap();
    let sample = BatterySample { pc: s.modules.pc_battery_pct, phone: s.modules.phone_battery_pct };
    s.modules.battery_history.push(sample);
    let overflow = s.modules.battery_history.len().saturating_sub(MAX_BATTERY_SAMPLES);
    if overflow > 0 {
        s.modules.battery_history.drain(0..overflow);
    }
}

pub fn set_workspace(state: &Arc<Mutex<HudState>>, workspace: String) {
    state.lock().unwrap().modules.workspace = Some(workspace);
}

pub fn set_audio_tap_active(state: &Arc<Mutex<HudState>>, active: bool) {
    let mut s = state.lock().unwrap();
    s.modules.audio_tap_active = active;
    if active {
        s.modules.audio_tap_bytes = 0;
        s.modules.audio_tap_started_at = Some(std::time::Instant::now());
    } else {
        s.modules.audio_vu.clear();
        s.modules.audio_tap_started_at = None;
    }
}

pub fn set_audio_tap_bytes(state: &Arc<Mutex<HudState>>, bytes: u64) {
    state.lock().unwrap().modules.audio_tap_bytes = bytes;
}

/// Segundos desde que o tap começou nesta sessão — `None` se estiver parado.
pub fn audio_tap_elapsed_secs(state: &Arc<Mutex<HudState>>) -> Option<u64> {
    state.lock().unwrap().modules.audio_tap_started_at.map(|t| t.elapsed().as_secs())
}

const MAX_VU_SAMPLES: usize = 16;

pub fn push_audio_vu(state: &Arc<Mutex<HudState>>, peak_pct: u8) {
    let mut s = state.lock().unwrap();
    s.modules.audio_vu.push(peak_pct);
    let overflow = s.modules.audio_vu.len().saturating_sub(MAX_VU_SAMPLES);
    if overflow > 0 {
        s.modules.audio_vu.drain(0..overflow);
    }
}

pub fn set_webcam_active(state: &Arc<Mutex<HudState>>, active: bool) {
    let mut s = state.lock().unwrap();
    if active {
        s.modules.webcam_started_at = Some(std::time::Instant::now());
    } else if let Some(start) = s.modules.webcam_started_at.take() {
        let dur = start.elapsed();
        let (mins, secs) = (dur.as_secs() / 60, dur.as_secs() % 60);
        let dur_str = if mins > 0 { format!("{mins} min {secs} s") } else { format!("{secs} s") };
        s.modules.webcam_last_used = Some((chrono::Local::now().format("%H:%M").to_string(), dur_str));
    }
    s.modules.webcam_active = active;
    if !active {
        s.modules.webcam_mbps = None;
    }
}

pub fn set_webcam_mbps(state: &Arc<Mutex<HudState>>, mbps: f64) {
    state.lock().unwrap().modules.webcam_mbps = Some(mbps);
}

pub fn set_mic_active(state: &Arc<Mutex<HudState>>, active: bool) {
    state.lock().unwrap().modules.mic_active = active;
}
