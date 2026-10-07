//! Estado interno do daemon. Quem está de fora (o hyprlink-gui, o
//! hyprlinkctl) vê-o pela bridge (`bridge.rs`), nunca diretamente.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use hyprlink_proto::link::{LinkPhase, NotifAction};

const MAX_LOG_LINES: usize = 200;
/// Histórico de CLIP/NOTIF fica só em memória por ora (reseta ao reiniciar
/// o daemon) — persistência em disco com retenção configurável é Fase C.
const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone)]
pub struct ClipEntry {
    pub id: u64,
    pub pinned: bool,
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
}

#[derive(Debug, Clone)]
pub struct TransferRecord {
    pub id: u64,
    pub wire_id: Option<u64>,
    pub at_unix: u64,
    pub total: u64,
    pub cancelled: bool,
    pub name: String,
    pub direction: &'static str,
    pub bytes: u64,
    pub ok: bool,
    pub error: Option<String>,
}

/// Amostra periódica (`sample_battery_history`, a cada ~5 min) — 144
/// amostras cobrem 12h, o bastante pro gráfico de barras do BATT.
#[derive(Debug, Clone)]
pub struct BatterySample {
    pub at_unix: u64,
    pub phone: Option<i64>,
}
const MAX_BATTERY_SAMPLES: usize = 144;

#[derive(Debug, Clone)]
pub struct NotifEntry {
    /// Chave do Android (`StatusBarNotification.key`).
    pub key: String,
    pub at_unix: u64,
    pub app: String,
    pub title: String,
    pub text: String,
    /// Botões da notificação, com a marca de «pede texto».
    pub actions: Vec<NotifAction>,
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
    /// Codec efetivo do stream de vídeo atual, pelo byte que o telemóvel
    /// envia: `"H.264"` / `"H.265"`. `None` = ainda não chegou.
    pub webcam_codec: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct HudState {
    pub conn: ConnState,
    pub pairing_token_hex: String,
    pub logs: Vec<String>,
    pub modules: ModuleStatus,
}

impl HudState {
    pub fn new(pairing_token_hex: String) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            conn: ConnState::Pairing,
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
    publish_link(LinkPhase::Connecting);
}

pub fn set_connected(state: &Arc<Mutex<HudState>>, device_name: String, fingerprint_hex: String) {
    state.lock().unwrap().conn = ConnState::Connected {
        device_name,
        fingerprint_hex,
    };
    publish_link(LinkPhase::Connected);
}

pub fn set_pairing(state: &Arc<Mutex<HudState>>) {
    state.lock().unwrap().conn = ConnState::Pairing;
    publish_link(LinkPhase::Disconnected);
}

/// A fase do link como os clientes do socket a veem (a cor do LINK na GUI,
/// o campo `link` do JSON v1). O hub só difunde quando muda.
pub fn link_phase(conn: &ConnState) -> LinkPhase {
    match conn {
        ConnState::Connected { .. } => LinkPhase::Connected,
        ConnState::Connecting => LinkPhase::Connecting,
        ConnState::Pairing => LinkPhase::Disconnected,
    }
}

fn publish_link(phase: LinkPhase) {
    crate::hub::global().publish(hyprlink_proto::link::Event::Link(phase));
}

pub const DIR_PHONE_TO_PC: &str = "telemóvel → PC";
pub const DIR_PC_TO_PHONE: &str = "PC → telemóvel";

pub fn push_clip_entry(state: &Arc<Mutex<HudState>>, direction: &'static str, text: String) {
    let bytes = text.len() as u64;
    push_clip(state, direction, "text/plain", Some(text), bytes);
}

/// Imagem PNG no clipboard: só o tamanho fica (os bytes não se guardam).
pub fn push_clip_image(state: &Arc<Mutex<HudState>>, direction: &'static str, bytes: u64) {
    push_clip(state, direction, "image/png", None, bytes);
}

fn push_clip(
    state: &Arc<Mutex<HudState>>,
    direction: &'static str,
    mime: &'static str,
    content: Option<String>,
    bytes: u64,
) {
    let mut s = state.lock().unwrap();
    let id = s.modules.clip_next_id;
    s.modules.clip_next_id += 1;
    s.modules.clip_history.push_front(ClipEntry {
        id,
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

pub fn push_notif_entry(
    state: &Arc<Mutex<HudState>>,
    key: String,
    app: String,
    title: String,
    text: String,
    actions: Vec<NotifAction>,
) {
    let mut s = state.lock().unwrap();
    s.modules.notif_count += 1;
    let entry = NotifEntry {
        key,
        at_unix: now_unix(),
        app,
        title,
        text,
        actions,
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

/// Notificação que **já estava** na barra do telemóvel quando ele ligou
/// (`notification.post` com `replay: true`): entra na lista de ativas, mas não
/// conta como nova nem vai para o histórico — não é um acontecimento.
pub fn register_active_notif(
    state: &Arc<Mutex<HudState>>,
    key: String,
    app: String,
    title: String,
    text: String,
    actions: Vec<NotifAction>,
) {
    let mut s = state.lock().unwrap();
    let entry = NotifEntry {
        key,
        at_unix: now_unix(),
        app,
        title,
        text,
        actions,
    };
    if !entry.key.is_empty() {
        s.modules.notif_active.retain(|n| n.key != entry.key);
    }
    s.modules.notif_active.insert(0, entry);
    s.modules.notif_active.truncate(MAX_HISTORY);
}

/// A lista de ativas é a do telemóvel: quando ele desliga, esvazia-se (o
/// telemóvel volta a enviá-la inteira quando ligar).
pub fn clear_active_notifs(state: &Arc<Mutex<HudState>>) {
    state.lock().unwrap().modules.notif_active.clear();
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
        s.modules.file_history.push_front(TransferRecord {
            id: t.id,
            wire_id: t.wire_id,
            at_unix: t.at_unix,
            total: t.total,
            cancelled,
            name: t.name,
            direction: t.direction,
            bytes: t.bytes,
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
        s.modules.webcam_codec = None;
    }
}

pub fn set_webcam_codec(state: &Arc<Mutex<HudState>>, label: &'static str) {
    state.lock().unwrap().modules.webcam_codec = Some(label);
}

/// Esvazia o histórico de ficheiros (a transferência em curso não está aqui:
/// vive em `file_transfer` e só passa para o histórico quando acaba).
pub fn clear_file_history(state: &Arc<Mutex<HudState>>) {
    state.lock().unwrap().modules.file_history.clear();
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
        HudState::new("00".into())
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
    fn clear_file_history_keeps_the_transfer_in_progress() {
        let h = hud();
        for n in ["a.bin", "b.bin"] {
            start_file_transfer(&h, n.into(), "enviando", 10);
            finish_file_transfer(&h, true, None);
        }
        // Um terceiro ainda a decorrer.
        start_file_transfer(&h, "c.bin".into(), "enviando", 10);
        assert_eq!(h.lock().unwrap().modules.file_history.len(), 2);

        clear_file_history(&h);

        {
            let s = h.lock().unwrap();
            let m = &s.modules;
            assert!(m.file_history.is_empty());
            assert_eq!(
                m.file_transfer.as_ref().map(|t| t.name.as_str()),
                Some("c.bin")
            );
            assert!(m.file_cancel.is_some(), "a flag de cancelamento fica");
        } // o guard cai aqui; `finish_file_transfer` volta a bloquear o Mutex
        // Depois de acabar, o que estava em curso entra no histórico.
        finish_file_transfer(&h, true, None);
        assert_eq!(h.lock().unwrap().modules.file_history.len(), 1);
        // Limpar o que já está vazio não faz mal.
        clear_file_history(&h);
        clear_file_history(&h);
        assert!(h.lock().unwrap().modules.file_history.is_empty());
    }

    #[test]
    fn webcam_codec_is_forgotten_when_the_stream_ends() {
        let h = hud();
        set_webcam_active(&h, true);
        set_webcam_codec(&h, "H.265");
        assert_eq!(h.lock().unwrap().modules.webcam_codec, Some("H.265"));
        set_webcam_active(&h, false);
        assert_eq!(h.lock().unwrap().modules.webcam_codec, None);
    }

    #[test]
    fn active_notifications_follow_the_phone() {
        let h = hud();
        push_notif_entry(&h, "k1".into(), "app".into(), "a".into(), "".into(), vec![]);
        push_notif_entry(&h, "k2".into(), "app".into(), "b".into(), "".into(), vec![]);
        // Atualização da mesma chave substitui, não duplica.
        push_notif_entry(
            &h,
            "k1".into(),
            "app".into(),
            "a2".into(),
            "".into(),
            vec![],
        );
        assert_eq!(active_notif_keys(&h), vec!["k1", "k2"]);
        assert!(remove_active_notif(&h, "k2"));
        assert!(!remove_active_notif(&h, "k2"));
        assert_eq!(active_notif_keys(&h), vec!["k1"]);
        // O histórico guarda tudo.
        assert_eq!(h.lock().unwrap().modules.notif_history.len(), 3);
    }

    #[test]
    fn replayed_notifications_are_active_but_not_events() {
        let h = hud();
        register_active_notif(&h, "r1".into(), "app".into(), "a".into(), "".into(), vec![]);
        register_active_notif(&h, "r2".into(), "app".into(), "b".into(), "".into(), vec![]);
        // O mesmo `replay` duas vezes não duplica.
        register_active_notif(&h, "r1".into(), "app".into(), "a".into(), "".into(), vec![]);
        assert_eq!(active_notif_keys(&h), vec!["r1", "r2"]);
        {
            let s = h.lock().unwrap();
            assert_eq!(s.modules.notif_count, 0);
            assert!(s.modules.notif_history.is_empty());
        } // o guard tem de cair antes de `clear_active_notifs` voltar a bloquear
        clear_active_notifs(&h);
        assert!(active_notif_keys(&h).is_empty());
    }
}

#[cfg(test)]
mod link_phase_tests {
    use super::*;

    #[test]
    fn conn_state_maps_to_link_phase() {
        assert_eq!(link_phase(&ConnState::Pairing), LinkPhase::Disconnected);
        assert_eq!(link_phase(&ConnState::Connecting), LinkPhase::Connecting);
        let up = ConnState::Connected {
            device_name: "Poco F4".into(),
            fingerprint_hex: "9f2c".into(),
        };
        assert_eq!(link_phase(&up), LinkPhase::Connected);
    }
}
