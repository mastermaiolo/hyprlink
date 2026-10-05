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
    // ── dados para o socket local (a GUI antiga usa os de cima) ──
    pub from_phone: bool,
    pub mime: &'static str,
    /// Conteúdo de texto; `None` para imagens (os bytes não ficam guardados).
    pub content: Option<String>,
    pub bytes: u64,
    pub at_unix: u64,
}

/// Transferência em andamento — só uma por vez (o protocolo não faz fila).
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub id: u64,
    /// Id do pacote `share.file` no QUIC (só nos envios PC → telemóvel):
    /// é o que o telemóvel cita no `share.done`.
    pub wire_id: Option<u64>,
    pub at_unix: u64,
    pub name: String,
    pub direction: &'static str, // "recebendo" · "enviando"
    pub bytes: u64,
    pub total: u64,
    pub started_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct TransferRecord {
    pub id: u64,
    pub wire_id: Option<u64>,
    pub at_unix: u64,
    pub total: u64,
    pub cancelled: bool,
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
    pub at_unix: u64,
    pub pc: Option<i64>,
    pub phone: Option<i64>,
}
const MAX_BATTERY_SAMPLES: usize = 144;

#[derive(Debug, Clone)]
pub struct NotifEntry {
    /// Chave do Android (`StatusBarNotification.key`).
    pub key: String,
    pub at_unix: u64,
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
    Connected {
        device_name: String,
        fingerprint_hex: String,
    },
}

/// Estado real de cada módulo pra sidebar — `None`/`false` = cinza (sem
/// dados ainda ou desligado por opção), nunca vermelho: vermelho é só erro
/// de verdade ou ausência de ligação (ver `code-tarefas.md`, decisão 2).
#[derive(Debug, Clone, Default)]
pub struct ModuleStatus {
    /// Mais recente primeiro (exceto os fixados, que a GUI ordena no topo).
    /// `VecDeque` porque toda entrada nova entra no início — `Vec::insert(0,
    /// ..)` desloca todo o histórico na memória a cada evento, `push_front`
    /// é O(1).
    pub clip_history: std::collections::VecDeque<ClipEntry>,
    clip_next_id: u64,
    pub notif_count: u32,
    /// Mais recente primeiro.
    pub notif_history: std::collections::VecDeque<NotifEntry>,
    /// As que ainda estão no telemóvel (sai com `notification.dismissed`
    /// ou quando o PC as dispensa). Mais recente primeiro.
    pub notif_active: Vec<NotifEntry>,
    pub media: Option<String>,
    pub phone_battery_pct: Option<i64>,
    pub phone_battery_charging: bool,
    /// Último `phone.status` do telemóvel (rede, armazenamento, RAM, ecrã,
    /// notificações, now-playing) — a bridge polia e publica como
    /// `Event::Phone` pro hub (GUI/hyprlinkctl). Default = tudo `None`.
    /// Fica vazio (default) ao desconectar, via `set_pairing`.
    pub phone_status: hyprlink_proto::link::PhoneStatus,
    pub pc_battery_pct: Option<i64>,
    pub pc_battery_charging: bool,
    /// Mais antiga primeiro (ordem de gráfico).
    pub battery_history: Vec<BatterySample>,
    pub workspace: Option<String>,
    /// `None` = nenhuma transferência em andamento agora.
    pub file_transfer: Option<TransferProgress>,
    file_cancel: Option<Arc<AtomicBool>>,
    transfer_next_id: u64,
    /// Mais recente primeiro.
    pub file_history: std::collections::VecDeque<TransferRecord>,
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
    /// Pico linear 0–1 do último bloco de PCM do microfone e quando chegou.
    pub mic_level: Option<(f32, std::time::Instant)>,
    /// Modo coluna ativo (`speaker.rs`): o sink virtual `hyprlink-speaker`
    /// é o padrão e o tap aponta pra ele — o som do PC sai no telemóvel.
    pub speaker_active: bool,
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
    pub fn new(
        local_addr: String,
        server_fingerprint_hex: String,
        pairing_token_hex: String,
    ) -> Arc<Mutex<Self>> {
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

/// Segundos Unix agora.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn push_log(state: &Arc<Mutex<HudState>>, line: impl Into<String>) {
    let line = format!(
        "{} {}",
        chrono::Local::now().format("%H:%M:%S"),
        line.into()
    );
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
    state.lock().unwrap().conn = ConnState::Connected {
        device_name,
        fingerprint_hex,
    };
}

pub fn set_pairing(state: &Arc<Mutex<HudState>>) {
    state.lock().unwrap().conn = ConnState::Pairing;
}

pub const DIR_PHONE_TO_PC: &str = "telemóvel → PC";
pub const DIR_PC_TO_PHONE: &str = "PC → telemóvel";

pub fn push_clip_entry(state: &Arc<Mutex<HudState>>, direction: &'static str, text: String) {
    let bytes = text.len() as u64;
    push_clip(
        state,
        direction,
        text.clone(),
        "text/plain",
        Some(text),
        bytes,
    );
}

/// Imagem PNG no clipboard: só o tamanho fica (os bytes não se guardam).
pub fn push_clip_image(state: &Arc<Mutex<HudState>>, direction: &'static str, bytes: u64) {
    let label = format!("🖼️ PNG ({} KB)", bytes / 1024);
    push_clip(state, direction, label, "image/png", None, bytes);
}

fn push_clip(
    state: &Arc<Mutex<HudState>>,
    direction: &'static str,
    text: String,
    mime: &'static str,
    content: Option<String>,
    bytes: u64,
) {
    let mut s = state.lock().unwrap();
    let at = chrono::Local::now().format("%H:%M:%S").to_string();
    let id = s.modules.clip_next_id;
    s.modules.clip_next_id += 1;
    s.modules.clip_history.push_front(ClipEntry {
        id,
        at,
        direction,
        text,
        pinned: false,
        from_phone: direction == DIR_PHONE_TO_PC,
        mime,
        content,
        bytes,
        at_unix: now_unix(),
    });
    s.modules.clip_history.truncate(MAX_HISTORY);
}

pub fn set_clip_pin(state: &Arc<Mutex<HudState>>, id: u64, pinned: bool) -> bool {
    let mut s = state.lock().unwrap();
    match s.modules.clip_history.iter_mut().find(|e| e.id == id) {
        Some(e) => {
            e.pinned = pinned;
            true
        }
        None => false,
    }
}

pub fn delete_clip(state: &Arc<Mutex<HudState>>, id: u64) -> bool {
    let mut s = state.lock().unwrap();
    let before = s.modules.clip_history.len();
    s.modules.clip_history.retain(|e| e.id != id);
    s.modules.clip_history.len() != before
}

pub fn clip_entry(state: &Arc<Mutex<HudState>>, id: u64) -> Option<ClipEntry> {
    let s = state.lock().unwrap();
    s.modules.clip_history.iter().find(|e| e.id == id).cloned()
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

pub fn push_notif_entry(
    state: &Arc<Mutex<HudState>>,
    key: String,
    app: String,
    title: String,
    text: String,
) {
    let mut s = state.lock().unwrap();
    s.modules.notif_count += 1;
    let entry = NotifEntry {
        key,
        at_unix: now_unix(),
        at: chrono::Local::now().format("%H:%M:%S").to_string(),
        app,
        title,
        text,
    };
    s.modules.notif_history.push_front(entry.clone());
    s.modules.notif_history.truncate(MAX_HISTORY);
    // Uma atualização da mesma notificação substitui a anterior.
    if !entry.key.is_empty() {
        s.modules.notif_active.retain(|n| n.key != entry.key);
    }
    s.modules.notif_active.insert(0, entry);
    s.modules.notif_active.truncate(MAX_HISTORY);
}

/// Saiu do telemóvel (ou o PC dispensou-a). `true` se estava na lista.
pub fn remove_active_notif(state: &Arc<Mutex<HudState>>, key: &str) -> bool {
    let mut s = state.lock().unwrap();
    let before = s.modules.notif_active.len();
    s.modules.notif_active.retain(|n| n.key != key);
    s.modules.notif_active.len() != before
}

pub fn active_notif_keys(state: &Arc<Mutex<HudState>>) -> Vec<String> {
    let s = state.lock().unwrap();
    s.modules
        .notif_active
        .iter()
        .map(|n| n.key.clone())
        .collect()
}

/// Marca o início de uma transferência e devolve a flag de cancelamento —
/// `share.rs` checa essa flag a cada bloco lido/escrito.
pub fn start_file_transfer(
    state: &Arc<Mutex<HudState>>,
    name: String,
    direction: &'static str,
    total: u64,
) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut s = state.lock().unwrap();
    s.modules.transfer_next_id += 1;
    let id = s.modules.transfer_next_id;
    s.modules.file_transfer = Some(TransferProgress {
        id,
        wire_id: None,
        at_unix: now_unix(),
        name,
        direction,
        bytes: 0,
        total,
        started_at: std::time::Instant::now(),
    });
    s.modules.file_cancel = Some(cancel.clone());
    cancel
}

pub fn update_file_transfer_progress(state: &Arc<Mutex<HudState>>, bytes: u64) {
    if let Some(t) = state.lock().unwrap().modules.file_transfer.as_mut() {
        t.bytes = bytes;
    }
}

pub fn set_file_transfer_wire_id(state: &Arc<Mutex<HudState>>, wire_id: u64) {
    if let Some(t) = state.lock().unwrap().modules.file_transfer.as_mut() {
        t.wire_id = Some(wire_id);
    }
}

/// O recetor (telemóvel) rejeitou um envio que do nosso lado acabou bem
/// (`share.done` com `ok: false`, ex. SHA-256 diferente). `true` se o
/// encontrou no histórico.
pub fn mark_transfer_rejected(
    state: &Arc<Mutex<HudState>>,
    wire_id: u64,
    error: Option<String>,
) -> bool {
    let mut s = state.lock().unwrap();
    match s
        .modules
        .file_history
        .iter_mut()
        .find(|r| r.wire_id == Some(wire_id))
    {
        Some(r) => {
            r.ok = false;
            r.error = error;
            true
        }
        None => false,
    }
}

/// Cancela a transferência `id` se for a que está a decorrer.
pub fn cancel_file_transfer_id(state: &Arc<Mutex<HudState>>, id: u64) -> bool {
    let current = state
        .lock()
        .unwrap()
        .modules
        .file_transfer
        .as_ref()
        .map(|t| t.id);
    if current == Some(id) {
        cancel_file_transfer(state);
        true
    } else {
        false
    }
}

pub fn cancel_file_transfer(state: &Arc<Mutex<HudState>>) {
    if let Some(flag) = &state.lock().unwrap().modules.file_cancel {
        flag.store(true, Ordering::Relaxed);
    }
}

pub fn finish_file_transfer(state: &Arc<Mutex<HudState>>, ok: bool, error: Option<String>) {
    let mut s = state.lock().unwrap();
    let cancelled = s
        .modules
        .file_cancel
        .as_ref()
        .is_some_and(|f| f.load(Ordering::Relaxed));
    if let Some(t) = s.modules.file_transfer.take() {
        let duration_secs = t.started_at.elapsed().as_secs();
        s.modules.file_history.push_front(TransferRecord {
            id: t.id,
            wire_id: t.wire_id,
            at_unix: t.at_unix,
            total: t.total,
            cancelled,
            at: chrono::Local::now().format("%H:%M").to_string(),
            name: t.name,
            direction: t.direction,
            bytes: t.bytes,
            duration_secs,
            ok,
            error,
        });
        s.modules.file_history.truncate(MAX_HISTORY);
    }
    s.modules.file_cancel = None;
}

pub fn set_media_status(state: &Arc<Mutex<HudState>>, status: Option<String>) {
    state.lock().unwrap().modules.media = status;
}

pub fn set_phone_battery(state: &Arc<Mutex<HudState>>, pct: i64, charging: bool) {
    let mut s = state.lock().unwrap();
    s.modules.phone_battery_pct = Some(pct);
    s.modules.phone_battery_charging = charging;
}

pub fn phone_battery_pct(state: &Arc<Mutex<HudState>>) -> Option<i64> {
    state.lock().unwrap().modules.phone_battery_pct
}

/// Guarda o último `phone.status` (chega a cada 30 s e em cada mudança —
/// a app faz o debounce; aqui é só "o mais recente ganha"). A bridge lê
/// `modules.phone_status` diretamente (mesmo padrão dos outros campos).
pub fn set_phone_status(state: &Arc<Mutex<HudState>>, status: hyprlink_proto::link::PhoneStatus) {
    state.lock().unwrap().modules.phone_status = status;
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
    let sample = BatterySample {
        at_unix: now_unix(),
        pc: s.modules.pc_battery_pct,
        phone: s.modules.phone_battery_pct,
    };
    s.modules.battery_history.push(sample);
    let overflow = s
        .modules
        .battery_history
        .len()
        .saturating_sub(MAX_BATTERY_SAMPLES);
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

pub fn set_speaker_active(state: &Arc<Mutex<HudState>>, active: bool) {
    state.lock().unwrap().modules.speaker_active = active;
}

pub fn set_audio_tap_bytes(state: &Arc<Mutex<HudState>>, bytes: u64) {
    state.lock().unwrap().modules.audio_tap_bytes = bytes;
}

/// Segundos desde que o tap começou nesta sessão — `None` se estiver parado.
pub fn audio_tap_elapsed_secs(state: &Arc<Mutex<HudState>>) -> Option<u64> {
    state
        .lock()
        .unwrap()
        .modules
        .audio_tap_started_at
        .map(|t| t.elapsed().as_secs())
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
        let dur_str = if mins > 0 {
            format!("{mins} min {secs} s")
        } else {
            format!("{secs} s")
        };
        s.modules.webcam_last_used =
            Some((chrono::Local::now().format("%H:%M").to_string(), dur_str));
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
    let mut s = state.lock().unwrap();
    s.modules.mic_active = active;
    if !active {
        s.modules.mic_level = None;
    }
}

pub fn set_mic_level(state: &Arc<Mutex<HudState>>, peak: f32) {
    state.lock().unwrap().modules.mic_level = Some((peak, std::time::Instant::now()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hud() -> Arc<Mutex<HudState>> {
        HudState::new("127.0.0.1:7443".into(), "AA".into(), "00".into())
    }

    #[test]
    fn phone_rejection_marks_the_sent_file_failed() {
        let h = hud();
        start_file_transfer(&h, "foto.jpg".into(), "enviando", 10);
        set_file_transfer_wire_id(&h, 42);
        update_file_transfer_progress(&h, 10);
        finish_file_transfer(&h, true, None);
        assert!(h.lock().unwrap().modules.file_history[0].ok);
        assert!(mark_transfer_rejected(&h, 42, Some("sha256".into())));
        let r = h.lock().unwrap().modules.file_history[0].clone();
        assert!(!r.ok && r.error.as_deref() == Some("sha256"));
        assert!(!mark_transfer_rejected(&h, 7, None), "id desconhecido");
    }

    #[test]
    fn cancelled_transfer_is_recorded_as_cancelled() {
        let h = hud();
        start_file_transfer(&h, "a.bin".into(), "enviando", 10);
        let id = h.lock().unwrap().modules.file_transfer.as_ref().unwrap().id;
        assert!(!cancel_file_transfer_id(&h, id + 1));
        assert!(cancel_file_transfer_id(&h, id));
        finish_file_transfer(&h, false, Some("cancelado pelo usuário".into()));
        let r = h.lock().unwrap().modules.file_history[0].clone();
        assert!(r.cancelled && !r.ok && r.id == id);
    }

    #[test]
    fn active_notifications_follow_the_phone() {
        let h = hud();
        push_notif_entry(&h, "k1".into(), "app".into(), "a".into(), "".into());
        push_notif_entry(&h, "k2".into(), "app".into(), "b".into(), "".into());
        // Atualização da mesma chave substitui, não duplica.
        push_notif_entry(&h, "k1".into(), "app".into(), "a2".into(), "".into());
        assert_eq!(active_notif_keys(&h), vec!["k1", "k2"]);
        assert!(remove_active_notif(&h, "k2"));
        assert!(!remove_active_notif(&h, "k2"));
        assert_eq!(active_notif_keys(&h), vec!["k1"]);
        // O histórico (GUI antiga) guarda tudo.
        assert_eq!(h.lock().unwrap().modules.notif_history.len(), 3);
    }
}
