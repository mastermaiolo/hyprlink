//! HUD flutuante (vidro fosco, cantos arredondados) via iced puro — janela
//! normal (não layer-shell), pra poder ser movida e ficar presa a uma única
//! workspace, com blur "de graça" pelo `decoration:blur` do Hyprland (que já
//! se aplica a qualquer janela com transparência, sem precisar de layerrule).
//! Acento colorido pelo estado da ligação (verde=conectado,
//! amarelo=conectando/pareando, vermelho=indisponível).
//!
//! ponytail: ícones SVG por módulo ficaram de fora desta primeira versão —
//! número + título já comunica bem. Adicionar quando fizer sentido.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::widget::{button, checkbox, column, container, row, scrollable, slider, text, text_editor, text_input, Space};
use iced::window;
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Shadow, Task, Theme, Vector};

use crate::active::ActiveConn;
use crate::clip;
use crate::config::{self, SharedConfig};
use crate::state::{ConnState, HudState};
use crate::theme::{
    AMBER, AMBER_BG, AMBER_BRD, BRD_1, DIVIDER, GLASS_BRD, GREEN, GREEN_BG, GREEN_BRD, MUTED, RED, RED_BRD, TERMINAL, TEXT, TEXT_1, TEXT_2, TEXT_3, TEXT_4,
    TEXT_5, TEXT_6, WINDOW_BG, WINDOW_BRD,
};

const PANEL_W: u32 = 900;
const PANEL_H: u32 = 800;

/// Os 10 módulos do design (code-spec-iced.md §4) — todos têm tela própria
/// agora (Ronda 6). Ordem = ordem na sidebar/régua, não mexer sem atualizar
/// `module_glyph`/`module_ruler`/`module_list` juntos.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ModuleId {
    Clip,
    Files,
    Notif,
    Media,
    Batt,
    Control,
    Audio,
    Webcam,
    Track,
    Config,
}

/// Glifo por módulo (code-spec-iced.md §4). ⚠ alguns (`≋ ◔ ◈ ⧉`) podem faltar
/// em fontes monoespaçadas sem símbolos Unicode extras — se aparecerem como
/// caixa vazia na prática, é candidato a virar ícone embutido/`canvas` depois.
fn module_glyph(id: ModuleId) -> &'static str {
    match id {
        ModuleId::Clip => "⧉",
        ModuleId::Files => "⇄",
        ModuleId::Notif => "◔",
        ModuleId::Media => "▷",
        ModuleId::Batt => "▮",
        ModuleId::Control => "⌘",
        ModuleId::Audio => "≋",
        ModuleId::Webcam => "◎",
        ModuleId::Track => "◈",
        ModuleId::Config => "⚙",
    }
}

fn module_number(id: ModuleId) -> &'static str {
    match id {
        ModuleId::Clip => "01",
        ModuleId::Files => "02",
        ModuleId::Notif => "03",
        ModuleId::Media => "04",
        ModuleId::Batt => "05",
        ModuleId::Control => "06",
        ModuleId::Audio => "07",
        ModuleId::Webcam => "08",
        ModuleId::Track => "09",
        ModuleId::Config => "10",
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Screen {
    Dashboard,
    Module(ModuleId),
}

struct Hud {
    shared: Arc<Mutex<HudState>>,
    pairing: Arc<Mutex<crate::pairing::PairingStore>>,
    snapshot: HudState,
    /// Referência de tempo pra animação do ponto de estado (code-spec-iced.md
    /// §7) — período depende do acento atual, calculado no `view()`.
    start_time: std::time::Instant,
    /// Conteúdo do console — só reconstruído quando o número de linhas muda
    /// de verdade (não a cada Tick), pra não perder seleção/scroll do
    /// usuário enquanto ele está lendo o histórico.
    console: text_editor::Content,
    console_len: usize,
    config: SharedConfig,
    download_dir: PathBuf,
    active: ActiveConn,
    pending_webcam: crate::webcam::PendingWebcam,
    audio: crate::audio::AudioSnapshot,
    phone_audio: crate::phone_audio::PhoneAudioState,
    /// Valor "ao vivo" enquanto o slider do telemóvel está sendo arrastado —
    /// só visual, o comando de rede só sai no soltar (ver nota em
    /// `phone_volume_row`, mesma classe de bug do audio tap: abrir um
    /// stream QUIC por pixel arrastado afoga o telemóvel).
    phone_volume_drag: std::collections::HashMap<&'static str, i64>,
    tray_show: crate::tray::ShowRequested,
    screen: Screen,
    hypr_input: String,
    hypr_workspaces: Vec<i64>,
    hypr_last_result: Option<String>,
    hypr_window_count: usize,
    hypr_focused: Option<(String, String)>, // (classe, título)
    shortcut_name: String,
    shortcut_cmd: String,
    cursor_pos: Option<(i64, i64)>,
    screen_size: (i64, i64),
    webcam_resolution: &'static str,
    webcam_fps: i64,
    webcam_codec: &'static str,
    webcam_test: WebcamTest,
    clip_search: String,
    notif_search: String,
    notif_app_filter: Option<String>,
}

// 2K aqui = 2560x1440 (QHD, o "2K" comum de consumo, não o 2048x1080 de
// cinema). Suporte real a 2K/4K/60fps depende do hardware de câmara do
// telemóvel (CameraX/MediaCodec) — o daemon aceita qualquer valor, só não
// tem como garantir que o telemóvel consiga entregar.
const WEBCAM_RESOLUTIONS: &[&str] = &["640x480", "1280x720", "1920x1080", "2560x1440", "3840x2160"];
const WEBCAM_FPS_OPTIONS: &[i64] = &[15, 24, 30, 60];
const WEBCAM_CODECS: &[&str] = &["h264", "h265"];
const WEBCAM_TEST_DURATION: Duration = Duration::from_secs(60);

/// Estado do teste de rede/hardware da webcam — transmite um stream de
/// verdade em 1080p30 (ver `suggest_webcam_config` sobre por que não testa
/// direto em 4K) por 60s (mesmo protocolo do stream normal, nada novo no
/// app) e mede a vazão real; combina com núcleos de CPU disponíveis
/// (decodificação hoje é só por software) pra sugerir a melhor configuração.
#[derive(Debug, Clone)]
enum WebcamTest {
    Idle,
    Running { started: std::time::Instant },
    Done { mbps: f64, suggested: (&'static str, i64) },
}

/// Combina o que a rede aguenta com o que a CPU aguenta decodificar (por
/// software — `avdec_h264`/`avdec_h265`, sem aceleração de hardware) e pega
/// o mais conservador dos dois.
///
/// ponytail: a sugestão automática nunca passa de 1080p60, mesmo com rede
/// sobrando — decodificar 2K/4K por software é pesado o bastante (sobretudo
/// num Ryzen 5500U móvel) que só medir Mbps de rede não garante fluidez;
/// mediria isso de verdade só com uso real de CPU durante o teste, que não
/// fazemos ainda. 2K/4K continuam selecionáveis à mão pra quem quiser testar
/// na prática — só não são sugeridos sozinhos.
fn suggest_webcam_config(mbps: f64) -> (&'static str, i64) {
    fn rank(v: (&'static str, i64)) -> u8 {
        match v {
            ("1920x1080", 60) => 4,
            ("1920x1080", 30) => 3,
            ("1280x720", 30) => 2,
            ("1280x720", 24) => 1,
            _ => 0,
        }
    }
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    let cpu_cap: (&'static str, i64) = if cores >= 8 {
        ("1920x1080", 60)
    } else if cores >= 6 {
        ("1920x1080", 30)
    } else if cores >= 4 {
        ("1280x720", 30)
    } else {
        ("1280x720", 24)
    };
    let net_cap: (&'static str, i64) = if mbps >= 16.0 {
        ("1920x1080", 60)
    } else if mbps >= 8.0 {
        ("1920x1080", 30)
    } else if mbps >= 4.0 {
        ("1280x720", 30)
    } else if mbps >= 2.0 {
        ("1280x720", 24)
    } else {
        ("640x480", 15)
    };
    if rank(cpu_cap) <= rank(net_cap) { cpu_cap } else { net_cap }
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    /// Só força um redesenho (sem buscar estado novo) pra animar o ponto de
    /// estado — code-spec-iced.md §7, "não animar mais nada".
    Tock,
    Quit,
    MinimizeToTray,
    ToggleTraySpecialWorkspace(bool),
    PhoneMicToggle(bool),
    PhoneMicRequestResult(bool, bool),
    CopyText(String),
    ConsoleAction(text_editor::Action),
    PickDownloadDir,
    DownloadDirPicked(Option<PathBuf>),
    PickFileToSend,
    FileToSendPicked(Option<PathBuf>),
    OpenModule(ModuleId),
    Back,
    ClipSearch(String),
    ClipPin(u64),
    NotifSearch(String),
    NotifAppFilter(Option<String>),
    MediaCommand(&'static str),
    HyprInputChanged(String),
    HyprDispatch,
    HyprDispatchResult(String),
    HyprWorkspacesLoaded(Vec<i64>),
    WorkspaceClicked(i64),
    ControlContextLoaded(usize, Option<(String, String)>),
    ShortcutNameChanged(String),
    ShortcutCmdChanged(String),
    ShortcutAdd,
    ShortcutRemove(usize),
    ShortcutRun(String),
    WebcamStart,
    WebcamStop,
    WebcamStartResult(bool),
    WebcamResolutionChanged(&'static str),
    WebcamFpsChanged(i64),
    WebcamCodecChanged(&'static str),
    WebcamTestStart,
    WebcamTestStartResult(bool),
    WebcamTestApply,
    AudioLoaded(crate::audio::AudioSnapshot),
    AudioSetVolume(&'static str, i64, i64),
    AudioSetMute(&'static str, i64, bool),
    AudioSetDefaultSink(String),
    PhoneAudioLoaded(crate::phone_audio::PhoneAudioState),
    PhoneAudioVolumeDragged(&'static str, i64),
    PhoneAudioVolumeRelease(&'static str),
    PhoneAudioSetRingerMode(&'static str),
    PhoneAudioSetDnd(bool),
    ConfigRevoke(String),
    ConfigRestartDaemon,
    FileTransferCancel,
    BattAlertToggle(config::BatteryAlertKind, bool),
    TrackPoll,
    CursorPosLoaded(Option<(i64, i64)>, (i64, i64)),
    TrackSensitivity(f32),
    TrackScrollSpeed(f32),
    TrackAcceleration(bool),
    TrackInvertScroll(bool),
    TrackVirtualKeyboard(bool),
}

impl Hud {
    fn new(
        shared: Arc<Mutex<HudState>>,
        config: SharedConfig,
        active: ActiveConn,
        pending_webcam: crate::webcam::PendingWebcam,
        tray_show: crate::tray::ShowRequested,
        pairing: Arc<Mutex<crate::pairing::PairingStore>>,
    ) -> Self {
        let snapshot = shared.lock().unwrap().clone();
        let console_len = snapshot.logs.len();
        let console = text_editor::Content::with_text(&snapshot.logs.join("\n"));
        let download_dir = config::download_dir(&config);
        Self {
            shared,
            pairing,
            start_time: std::time::Instant::now(),
            snapshot,
            console,
            console_len,
            config,
            download_dir,
            active,
            pending_webcam,
            audio: crate::audio::AudioSnapshot::default(),
            phone_audio: crate::phone_audio::PhoneAudioState::default(),
            phone_volume_drag: std::collections::HashMap::new(),
            tray_show,
            screen: Screen::Dashboard,
            hypr_input: String::new(),
            hypr_workspaces: Vec::new(),
            hypr_last_result: None,
            hypr_window_count: 0,
            hypr_focused: None,
            shortcut_name: String::new(),
            shortcut_cmd: String::new(),
            cursor_pos: None,
            screen_size: (1920, 1080),
            webcam_resolution: "1280x720",
            webcam_fps: 24,
            webcam_codec: "h264",
            webcam_test: WebcamTest::Idle,
            clip_search: String::new(),
            notif_search: String::new(),
            notif_app_filter: None,
        }
    }

    fn accent(&self) -> Color {
        match self.snapshot.conn {
            ConnState::Connected { .. } => GREEN,
            ConnState::Connecting => AMBER,
            ConnState::Pairing => AMBER,
        }
    }
}

fn update(hud: &mut Hud, message: Message) -> Task<Message> {
    match message {
        Message::Tock => Task::none(),
        Message::Tick => {
            hud.snapshot = hud.shared.lock().unwrap().clone();
            if hud.snapshot.logs.len() != hud.console_len {
                hud.console_len = hud.snapshot.logs.len();
                hud.console = text_editor::Content::with_text(&hud.snapshot.logs.join("\n"));
            }
            if crate::tray::take_show_requested(&hud.tray_show) {
                crate::state::push_log(&hud.shared, "[i] tray: restaurando janela".to_string());
                return Task::perform(async { tokio::task::spawn_blocking(crate::hypr::tray_show).await.unwrap_or(false) }, |_| Message::Tick);
            }
            if let WebcamTest::Running { started } = hud.webcam_test {
                if started.elapsed() >= WEBCAM_TEST_DURATION {
                    let mbps = hud.snapshot.modules.webcam_mbps.unwrap_or(0.0);
                    hud.webcam_test = WebcamTest::Done { mbps, suggested: suggest_webcam_config(mbps) };
                    *hud.pending_webcam.lock().unwrap() = None;
                    let active = hud.active.clone();
                    return Task::perform(async move { crate::active::push(&active, "webcam.stop", None).await }, |_| Message::Tick);
                }
            }
            Task::none()
        }
        Message::Quit => {
            if let Some(connection) = hud.active.lock().unwrap().take() {
                connection.close(0u32.into(), b"HyprLink: GUI encerrada");
                // O close() só enfileira o frame CONNECTION_CLOSE — precisa
                // dar tempo da task do QUIC (noutra thread) chegar a
                // transmiti-lo antes do processo morrer de vez.
                std::thread::sleep(Duration::from_millis(150));
            }
            kill_other_instances();
            std::process::exit(0)
        }
        Message::TrackPoll => fetch_cursor_pos(),
        Message::CursorPosLoaded(pos, size) => {
            hud.cursor_pos = pos;
            hud.screen_size = size;
            Task::none()
        }
        Message::TrackSensitivity(v) => {
            config::set_track_sensitivity(&hud.config, v);
            Task::none()
        }
        Message::TrackScrollSpeed(v) => {
            config::set_track_scroll_speed(&hud.config, v);
            Task::none()
        }
        Message::TrackAcceleration(enabled) => {
            config::set_track_acceleration(&hud.config, enabled);
            Task::none()
        }
        Message::TrackInvertScroll(enabled) => {
            config::set_track_invert_scroll(&hud.config, enabled);
            Task::none()
        }
        Message::TrackVirtualKeyboard(enabled) => {
            config::set_track_virtual_keyboard(&hud.config, enabled);
            Task::none()
        }
        Message::BattAlertToggle(kind, enabled) => {
            match kind {
                config::BatteryAlertKind::Low => config::set_battery_alert_low(&hud.config, enabled),
                config::BatteryAlertKind::Full => config::set_battery_alert_full(&hud.config, enabled),
            }
            Task::none()
        }
        Message::FileTransferCancel => {
            crate::state::cancel_file_transfer(&hud.shared);
            Task::none()
        }
        Message::ConfigRevoke(fp) => {
            let mut store = hud.pairing.lock().unwrap();
            let _ = store.revoke(&fp);
            Task::none()
        }
        Message::ConfigRestartDaemon => {
            if let Some(connection) = hud.active.lock().unwrap().take() {
                connection.close(0u32.into(), b"HyprLink: reiniciando");
                std::thread::sleep(Duration::from_millis(150));
            }
            kill_other_instances();
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
            std::process::exit(0)
        }
        Message::MinimizeToTray => {
            crate::state::push_log(&hud.shared, "[i] tray: minimizando".to_string());
            Task::perform(async { tokio::task::spawn_blocking(crate::hypr::tray_hide).await.unwrap_or(false) }, |_| Message::Tick)
        }
        Message::ToggleTraySpecialWorkspace(enabled) => {
            config::set_tray_special_workspace(&hud.config, enabled);
            Task::none()
        }
        Message::PhoneMicToggle(enabled) => {
            let active = hud.active.clone();
            Task::perform(
                async move {
                    let ok = if enabled {
                        crate::mic::request_start(&active).await
                    } else {
                        crate::mic::request_stop(&active).await
                    };
                    (enabled, ok)
                },
                |(enabled, ok)| Message::PhoneMicRequestResult(enabled, ok),
            )
        }
        Message::PhoneMicRequestResult(enabled, ok) => {
            if ok {
                crate::state::push_log(&hud.shared, format!("[i] microfone: pedido de {} enviado ao telemóvel", if enabled { "ligar" } else { "desligar" }));
            } else {
                crate::state::push_log(&hud.shared, "[!] microfone: não foi possível enviar o pedido (sem conexão ativa?)".to_string());
            }
            Task::none()
        }
        Message::CopyText(value) => iced::clipboard::write(value),
        Message::ConsoleAction(action) => {
            if !action.is_edit() {
                hud.console.perform(action);
            }
            Task::none()
        }
        Message::PickDownloadDir => {
            let starting = hud.download_dir.clone();
            Task::perform(
                async move {
                    rfd::AsyncFileDialog::new()
                        .set_directory(&starting)
                        .set_title("Pasta de destino dos ficheiros recebidos")
                        .pick_folder()
                        .await
                        .map(|handle| handle.path().to_path_buf())
                },
                Message::DownloadDirPicked,
            )
        }
        Message::DownloadDirPicked(Some(dir)) => {
            config::set_download_dir(&hud.config, &dir);
            hud.download_dir = dir;
            Task::none()
        }
        Message::DownloadDirPicked(None) => Task::none(),
        Message::PickFileToSend => Task::perform(
            async { rfd::AsyncFileDialog::new().set_title("Ficheiro pra enviar pro telemóvel").pick_file().await.map(|handle| handle.path().to_path_buf()) },
            Message::FileToSendPicked,
        ),
        Message::FileToSendPicked(Some(path)) => {
            let active = hud.active.clone();
            let shared = hud.shared.clone();
            Task::perform(async move { crate::share::send_file(&active, &shared, &path).await }, |_| Message::Tick)
        }
        Message::FileToSendPicked(None) => Task::none(),
        Message::OpenModule(id) => {
            hud.screen = Screen::Module(id);
            if id == ModuleId::Control {
                return Task::batch([fetch_workspaces(), fetch_control_context()]);
            }
            if id == ModuleId::Audio {
                let active = hud.active.clone();
                return Task::batch([fetch_audio(), fetch_phone_audio(active)]);
            }
            if id == ModuleId::Track {
                return fetch_cursor_pos();
            }
            Task::none()
        }
        Message::Back => {
            hud.screen = Screen::Dashboard;
            Task::none()
        }
        Message::ClipSearch(q) => {
            hud.clip_search = q;
            Task::none()
        }
        Message::ClipPin(id) => {
            crate::state::toggle_clip_pin(&hud.shared, id);
            hud.snapshot = hud.shared.lock().unwrap().clone();
            Task::none()
        }
        Message::NotifSearch(q) => {
            hud.notif_search = q;
            Task::none()
        }
        Message::NotifAppFilter(app) => {
            hud.notif_app_filter = app;
            Task::none()
        }
        Message::MediaCommand(cmd) => {
            Task::perform(async move { crate::media::handle_command(cmd).await }, |_| Message::Tick)
        }
        Message::HyprInputChanged(value) => {
            hud.hypr_input = value;
            Task::none()
        }
        Message::HyprDispatch => {
            let cmd = hud.hypr_input.clone();
            if cmd.trim().is_empty() {
                return Task::none();
            }
            Task::perform(
                async move { tokio::task::spawn_blocking(move || crate::hypr::dispatch(&cmd)).await.unwrap_or_default() },
                Message::HyprDispatchResult,
            )
        }
        Message::HyprDispatchResult(result) => {
            hud.hypr_last_result = Some(result);
            fetch_workspaces()
        }
        Message::HyprWorkspacesLoaded(ids) => {
            hud.hypr_workspaces = ids;
            Task::none()
        }
        Message::ControlContextLoaded(count, focused) => {
            hud.hypr_window_count = count;
            hud.hypr_focused = focused;
            Task::none()
        }
        Message::ShortcutNameChanged(v) => {
            hud.shortcut_name = v;
            Task::none()
        }
        Message::ShortcutCmdChanged(v) => {
            hud.shortcut_cmd = v;
            Task::none()
        }
        Message::ShortcutAdd => {
            if !hud.shortcut_name.trim().is_empty() && !hud.shortcut_cmd.trim().is_empty() {
                config::add_shortcut(&hud.config, hud.shortcut_name.trim().to_string(), hud.shortcut_cmd.trim().to_string());
                hud.shortcut_name.clear();
                hud.shortcut_cmd.clear();
            }
            Task::none()
        }
        Message::ShortcutRemove(index) => {
            config::remove_shortcut(&hud.config, index);
            Task::none()
        }
        Message::ShortcutRun(cmd) => Task::perform(
            async move { tokio::task::spawn_blocking(move || crate::hypr::dispatch(&cmd)).await.unwrap_or_default() },
            Message::HyprDispatchResult,
        ),
        Message::WorkspaceClicked(id) => Task::perform(
            async move { tokio::task::spawn_blocking(move || crate::hypr::dispatch(&format!("workspace {id}"))).await.unwrap_or_default() },
            Message::HyprDispatchResult,
        ),
        Message::WebcamStart => {
            let active = hud.active.clone();
            let pending = hud.pending_webcam.clone();
            let (width, height) = hud.webcam_resolution.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).unwrap_or((1280, 720));
            let fps = hud.webcam_fps;
            let codec = hud.webcam_codec;
            Task::perform(
                async move { crate::webcam::request_start(&active, &pending, width, height, fps, codec).await },
                Message::WebcamStartResult,
            )
        }
        Message::WebcamStop => {
            *hud.pending_webcam.lock().unwrap() = None;
            let active = hud.active.clone();
            Task::perform(async move { crate::active::push(&active, "webcam.stop", None).await }, |_| Message::Tick)
        }
        Message::WebcamStartResult(ok) => {
            if !ok {
                crate::state::push_log(&hud.shared, "[!] webcam: não foi possível pedir o stream (sem conexão ativa?)".to_string());
            }
            Task::none()
        }
        Message::WebcamResolutionChanged(res) => {
            hud.webcam_resolution = res;
            Task::none()
        }
        Message::WebcamFpsChanged(fps) => {
            hud.webcam_fps = fps;
            Task::none()
        }
        Message::WebcamCodecChanged(codec) => {
            hud.webcam_codec = codec;
            Task::none()
        }
        Message::WebcamTestStart => {
            hud.webcam_test = WebcamTest::Running { started: std::time::Instant::now() };
            let active = hud.active.clone();
            let pending = hud.pending_webcam.clone();
            Task::perform(
                async move { crate::webcam::request_start(&active, &pending, 1920, 1080, 30, "h264").await },
                Message::WebcamTestStartResult,
            )
        }
        Message::WebcamTestStartResult(ok) => {
            if !ok {
                hud.webcam_test = WebcamTest::Idle;
                crate::state::push_log(&hud.shared, "[!] webcam: não foi possível iniciar o teste (sem conexão ativa?)".to_string());
            }
            Task::none()
        }
        Message::WebcamTestApply => {
            if let WebcamTest::Done { suggested, .. } = hud.webcam_test {
                hud.webcam_resolution = suggested.0;
                hud.webcam_fps = suggested.1;
            }
            hud.webcam_test = WebcamTest::Idle;
            Task::none()
        }
        Message::AudioLoaded(snapshot) => {
            hud.audio = snapshot;
            Task::none()
        }
        Message::AudioSetVolume(kind, id, volume) => {
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || crate::audio::set_volume(kind, id, volume)).await.ok();
                    tokio::task::spawn_blocking(crate::audio::snapshot).await.unwrap_or_default()
                },
                Message::AudioLoaded,
            )
        }
        Message::AudioSetMute(kind, id, muted) => {
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || crate::audio::set_mute(kind, id, muted)).await.ok();
                    tokio::task::spawn_blocking(crate::audio::snapshot).await.unwrap_or_default()
                },
                Message::AudioLoaded,
            )
        }
        Message::AudioSetDefaultSink(name) => Task::perform(
            async move {
                tokio::task::spawn_blocking(move || crate::audio::set_default_sink(&name)).await.ok();
                tokio::task::spawn_blocking(crate::audio::snapshot).await.unwrap_or_default()
            },
            Message::AudioLoaded,
        ),
        Message::PhoneAudioLoaded(state) => {
            hud.phone_audio = state;
            Task::none()
        }
        Message::PhoneAudioVolumeDragged(stream, percent) => {
            hud.phone_volume_drag.insert(stream, percent);
            Task::none()
        }
        Message::PhoneAudioVolumeRelease(stream) => {
            let fallback = match stream {
                "ring" => hud.phone_audio.ring_percent,
                "media" => hud.phone_audio.media_percent,
                "alarm" => hud.phone_audio.alarm_percent,
                _ => 50,
            };
            let percent = hud.phone_volume_drag.remove(stream).unwrap_or(fallback);
            let active = hud.active.clone();
            Task::perform(
                async move {
                    crate::phone_audio::set_volume(&active, stream, percent).await;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    crate::phone_audio::get_state(&active).await.unwrap_or_default()
                },
                Message::PhoneAudioLoaded,
            )
        }
        Message::PhoneAudioSetRingerMode(mode) => {
            let active = hud.active.clone();
            Task::perform(
                async move {
                    crate::phone_audio::set_ringer_mode(&active, mode).await;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    crate::phone_audio::get_state(&active).await.unwrap_or_default()
                },
                Message::PhoneAudioLoaded,
            )
        }
        Message::PhoneAudioSetDnd(enabled) => {
            let active = hud.active.clone();
            Task::perform(
                async move {
                    crate::phone_audio::set_dnd(&active, enabled).await;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    crate::phone_audio::get_state(&active).await.unwrap_or_default()
                },
                Message::PhoneAudioLoaded,
            )
        }
    }
}

fn fetch_audio() -> Task<Message> {
    Task::perform(async { tokio::task::spawn_blocking(crate::audio::snapshot).await.unwrap_or_default() }, Message::AudioLoaded)
}

fn fetch_phone_audio(active: ActiveConn) -> Task<Message> {
    Task::perform(async move { crate::phone_audio::get_state(&active).await.unwrap_or_default() }, Message::PhoneAudioLoaded)
}

/// Só a lista de ids de workspace (não `id`/`name` completo) — o suficiente
/// pra desenhar a grelha e destacar a ativa via `modules.workspace`.
fn fetch_workspaces() -> Task<Message> {
    Task::perform(
        async {
            let raw = tokio::task::spawn_blocking(crate::hypr::workspaces_json).await.unwrap_or_default();
            let parsed: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
            let mut ids: Vec<i64> = parsed.iter().filter_map(|w| w.get("id")?.as_i64()).collect();
            ids.sort_unstable();
            ids
        },
        Message::HyprWorkspacesLoaded,
    )
}

/// Linha de contexto do CONTROL: nº de janelas na workspace ativa + a janela
/// focada agora (classe · título) — `clients -j` + `activewindow -j`.
fn fetch_control_context() -> Task<Message> {
    Task::perform(
        async {
            let (clients_raw, active_raw) = tokio::join!(
                tokio::task::spawn_blocking(crate::hypr::clients_json),
                tokio::task::spawn_blocking(crate::hypr::active_window_json),
            );
            let active: serde_json::Value = serde_json::from_str(&active_raw.unwrap_or_default()).unwrap_or_default();
            let focused_ws = active.get("workspace").and_then(|w| w.get("id")).and_then(|v| v.as_i64());
            let focused = match (active.get("class").and_then(|v| v.as_str()), active.get("title").and_then(|v| v.as_str())) {
                (Some(c), Some(t)) if !c.is_empty() => Some((c.to_string(), t.to_string())),
                _ => None,
            };
            let clients: Vec<serde_json::Value> = serde_json::from_str(&clients_raw.unwrap_or_default()).unwrap_or_default();
            let count = match focused_ws {
                Some(ws) => clients.iter().filter(|c| c.get("workspace").and_then(|w| w.get("id")).and_then(|v| v.as_i64()) == Some(ws)).count(),
                None => 0,
            };
            (count, focused)
        },
        |(count, focused)| Message::ControlContextLoaded(count, focused),
    )
}

/// Posição do cursor + resolução do monitor focado — só pro espelho do
/// TRACK, buscados sob demanda (abrir a tela / a cada tick de `TrackPoll`).
fn fetch_cursor_pos() -> Task<Message> {
    Task::perform(
        async {
            let (pos_raw, mon_raw) = tokio::join!(
                tokio::task::spawn_blocking(crate::hypr::cursor_pos_json),
                tokio::task::spawn_blocking(crate::hypr::monitors_json),
            );
            let pos: serde_json::Value = serde_json::from_str(&pos_raw.unwrap_or_default()).unwrap_or_default();
            let cursor = match (pos.get("x").and_then(|v| v.as_i64()), pos.get("y").and_then(|v| v.as_i64())) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            };
            let monitors: Vec<serde_json::Value> = serde_json::from_str(&mon_raw.unwrap_or_default()).unwrap_or_default();
            let focused = monitors.iter().find(|m| m.get("focused").and_then(|v| v.as_bool()) == Some(true)).or_else(|| monitors.first());
            let size = match focused.and_then(|m| Some((m.get("width")?.as_i64()?, m.get("height")?.as_i64()?))) {
                Some(s) => s,
                None => (1920, 1080),
            };
            (cursor, size)
        },
        |(cursor, size)| Message::CursorPosLoaded(cursor, size),
    )
}

fn subscription(hud: &Hud) -> iced::Subscription<Message> {
    let mut subs = vec![
        iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick),
        iced::time::every(Duration::from_millis(60)).map(|_| Message::Tock),
    ];
    if hud.screen == Screen::Module(ModuleId::Track) {
        subs.push(iced::time::every(Duration::from_millis(1000)).map(|_| Message::TrackPoll));
    }
    iced::Subscription::batch(subs)
}

fn glass(radius: f32) -> container::Style {
    container::Style {
        text_color: None,
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
        border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: radius.into() },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// Período do "respirar" por acento (code-spec-iced.md §7) — mais rápido
/// quanto mais urgente o estado.
fn breathe_period(accent: Color) -> f32 {
    if accent == GREEN {
        2.8
    } else if accent == AMBER {
        1.5
    } else {
        1.2
    }
}

/// Opacidade 0.45→1.0 e escala 1.0→1.5 num ciclo suave (meio período de ida,
/// meio de volta) — mesmo efeito do ponto de estado do app Android.
fn breathe_phase(accent: Color, elapsed: f32) -> (f32, f32) {
    let period = breathe_period(accent);
    let t = (elapsed % period) / period;
    let wave = (1.0 - (t * 2.0 - 1.0).abs()).clamp(0.0, 1.0); // triângulo 0→1→0
    (0.45 + 0.55 * wave, 1.0 + 0.5 * wave)
}

/// O único ponto animado da UI — usado no cabeçalho (junto ao wordmark) e no
/// pill de estado do dashboard. `base_size` em px antes de aplicar a escala.
fn breathing_dot(accent: Color, base_size: f32, elapsed: f32) -> Element<'static, Message> {
    let (alpha, scale) = breathe_phase(accent, elapsed);
    let size = base_size * scale;
    container(text(""))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: alpha, ..accent })),
            border: Border { radius: 999.0.into(), ..Default::default() },
            shadow: Shadow { color: Color { a: alpha * 0.8, ..accent }, offset: Vector::default(), blur_radius: size },
            ..Default::default()
        })
        .into()
}

fn status_pill(accent: Color, label: &str, elapsed: f32) -> Element<'static, Message> {
    container(
        row![
            container(breathing_dot(accent, 7.0, elapsed)).width(13).height(13).align_x(Alignment::Center).align_y(Alignment::Center),
            text(label.to_string()).size(11).color(accent),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([5, 10])
    .style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.10, ..accent })),
        border: Border { color: Color { a: 0.35, ..accent }, width: 1.0, radius: 999.0.into() },
        ..Default::default()
    })
    .into()
}

/// Estado real de um módulo — nunca vermelho: vermelho é só erro/sem ligação
/// (code-tarefas.md, decisão 2).
enum ModuleState {
    /// Módulo com atividade real recente — ponto verde, subtítulo é o dado.
    Active(String),
    /// Implementado, mas sem dados ainda (ex: nenhuma notificação chegou).
    Idle(String),
}

impl ModuleState {
    fn idle(s: impl Into<String>) -> Self {
        ModuleState::Idle(s.into())
    }
}

/// Linha da sidebar (code-spec-iced.md §4): nº · glifo · título · subtítulo
/// real · ponto de estado · chevron. Todos os 10 módulos abrem tela própria
/// agora (Ronda 6) — clicar chama `Message::OpenModule`.
fn module_row(id: ModuleId, title: &str, state: ModuleState) -> Element<'static, Message> {
    let is_active = matches!(state, ModuleState::Active(_));
    let (dot, sub, sub_color): (Color, String, Color) = match state {
        ModuleState::Active(sub) => (GREEN, sub, TEXT_2),
        ModuleState::Idle(sub) => (MUTED, sub, TEXT_4),
    };
    let glyph_color = if is_active { GREEN } else { TEXT_3 };
    let content = row![
        text(module_number(id)).size(10).color(TEXT_6).width(16),
        text(module_glyph(id)).size(13).color(glyph_color),
        column![
            text(title.to_string()).size(12).color(TEXT).font(Font::MONOSPACE),
            text(sub).size(9).color(sub_color),
        ]
        .spacing(2)
        .width(Length::Fill),
        container(text("")).width(6).height(6).style(move |_| container::Style {
            background: Some(Background::Color(dot)),
            border: Border { radius: 999.0.into(), ..Default::default() },
            ..Default::default()
        }),
        text("›").size(13).color(TEXT_5),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    button(content)
        .padding([7, 10])
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
            border: Border { color: GLASS_BRD, width: 1.0, radius: 10.0.into() },
            text_color: TEXT,
            ..Default::default()
        })
        .on_press(Message::OpenModule(id))
        .into()
}

fn module_list(download_dir: &std::path::Path, modules: &crate::state::ModuleStatus) -> Element<'static, Message> {
    let clip_state = match modules.clip_history.first() {
        Some(entry) => ModuleState::Active(format!("{}: {}", entry.direction, clip::preview(&entry.text))),
        None => ModuleState::idle("Nada sincronizado ainda"),
    };
    let files_state = ModuleState::idle(format!("Recebe em {}", download_dir.display()));
    let notif_state = if modules.notif_count > 0 {
        ModuleState::Active(format!("{} espelhada(s) nesta sessão", modules.notif_count))
    } else {
        ModuleState::idle("Nenhuma notificação ainda")
    };
    let media_state = match &modules.media {
        Some(status) => ModuleState::Active(status.clone()),
        None => ModuleState::idle("Nenhum leitor ativo"),
    };
    let batt_state = match modules.phone_battery_pct {
        Some(pct) => ModuleState::Active(format!("Telemóvel em {pct}%")),
        None => ModuleState::idle("Aguardando bateria do telemóvel"),
    };
    let control_state = match &modules.workspace {
        Some(ws) => ModuleState::Active(format!("Workspace {ws}")),
        None => ModuleState::idle("Hyprland IPC"),
    };
    let audio_state = if modules.audio_tap_active {
        ModuleState::Active("Audio tap ativo".to_string())
    } else {
        ModuleState::idle("Mixer e audio tap")
    };
    let webcam_state = if modules.webcam_active {
        ModuleState::Active("Stream ativo · /dev/video42".to_string())
    } else {
        ModuleState::idle("Câmara remota do PC")
    };

    let rows = column![
        module_row(ModuleId::Clip, "CLIP", clip_state),
        module_row(ModuleId::Files, "FILES", files_state),
        module_row(ModuleId::Notif, "NOTIF", notif_state),
        module_row(ModuleId::Media, "MEDIA", media_state),
        module_row(ModuleId::Batt, "BATT", batt_state),
        module_row(ModuleId::Control, "CONTROL", control_state),
        module_row(ModuleId::Audio, "AUDIO", audio_state),
        module_row(ModuleId::Webcam, "WEBCAM", webcam_state),
        module_row(ModuleId::Track, "TRACK", ModuleState::idle("Rato/teclado virtual pronto (uinput)")),
        module_row(ModuleId::Config, "CONFIG", ModuleState::idle("Permissões e dispositivos")),
    ]
    .spacing(7);
    scrollable(rows).width(296).height(Length::Fill).into()
}

/// Colore cada linha do console pela tag de severidade (`[+]`/`[i]`/`[!]`),
/// igual ao console do app Android — hoje o desktop mostrava tudo no mesmo
/// cinza. Um `Highlighter` de linha simples, sem estado entre linhas.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SeveritySettings;

struct SeverityHighlighter {
    current_line: usize,
}

impl iced::advanced::text::Highlighter for SeverityHighlighter {
    type Settings = SeveritySettings;
    type Highlight = Color;
    type Iterator<'a> = std::vec::IntoIter<(std::ops::Range<usize>, Color)>;

    fn new(_settings: &Self::Settings) -> Self {
        Self { current_line: 0 }
    }

    fn update(&mut self, _new_settings: &Self::Settings) {}

    fn change_line(&mut self, line: usize) {
        self.current_line = line;
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.current_line += 1;
        // Só a tag (`[+]`/`[i]`/`[!]`/`[x]`) fica colorida — o resto da
        // linha (timestamp + mensagem) segue no TEXT_2 padrão do editor.
        for (tag, color) in [("[!]", AMBER), ("[x]", RED), ("[+]", GREEN), ("[i]", TEXT_3)] {
            if let Some(pos) = line.find(tag) {
                return vec![(pos..pos + tag.len(), color)].into_iter();
            }
        }
        Vec::new().into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

fn severity_format(color: &Color, _theme: &Theme) -> iced::advanced::text::highlighter::Format<Font> {
    iced::advanced::text::highlighter::Format { color: Some(*color), font: None }
}

/// Console de diagnóstico real: `text_editor` em modo "só leitura" (ignora
/// `Action::Edit`, aceita mover/selecionar/scroll/copiar) — dá scroll,
/// seleção de texto e Ctrl+A/Ctrl+C de verdade, ao contrário de uma pilha de
/// `Text` estáticos.
fn console(content: &text_editor::Content) -> Element<'_, Message> {
    text_editor(content)
        .on_action(Message::ConsoleAction)
        .font(Font::MONOSPACE)
        .size(10)
        .padding(0)
        .highlight_with::<SeverityHighlighter>(SeveritySettings, severity_format)
        .style(|_theme, _status| text_editor::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            placeholder: TEXT_2,
            value: TEXT_2,
            selection: Color { a: 0.35, ..GREEN },
        })
        .into()
}

fn pairing_qr(payload: &str) -> Element<'static, Message> {
    let code = qrcode::QrCode::new(payload.as_bytes()).expect("payload de pareamento sempre cabe num QR");
    let rendered = code.render::<image::Luma<u8>>().max_dimensions(210, 210).build();
    let rgba = image::DynamicImage::ImageLuma8(rendered).to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let handle = iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw());
    container(iced::widget::image(handle).width(210).height(210))
        .padding(14)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::WHITE)),
            border: Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// Linha chave/valor copiável — mostra `display_value` mas copia `copy_value`
/// por inteiro (o fingerprint é abreviado na tela, o token não pode ser).
fn kv_row(label: &str, display_value: String, copy_value: String) -> Element<'static, Message> {
    button(
        row![
            column![
                text(label.to_string()).size(9).color(TEXT_2),
                text(display_value).size(11).color(TEXT).font(Font::MONOSPACE),
            ]
            .spacing(2)
            .width(Length::Fill),
            text("copiar").size(9).color(TEXT_2),
        ]
        .align_y(Alignment::Center),
    )
    .padding([9, 12])
    .width(Length::Fill)
    .style(|_, _| button::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))),
        border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 10.0.into() },
        text_color: TEXT,
        ..Default::default()
    })
    .on_press(Message::CopyText(copy_value))
    .into()
}

/// Régua de 44px que substitui a sidebar de 296px quando um módulo está
/// aberto (code-spec-iced.md §5) — só o módulo ativo fica destacado.
const ALL_MODULES: [ModuleId; 10] = [
    ModuleId::Clip,
    ModuleId::Files,
    ModuleId::Notif,
    ModuleId::Media,
    ModuleId::Batt,
    ModuleId::Control,
    ModuleId::Audio,
    ModuleId::Webcam,
    ModuleId::Track,
    ModuleId::Config,
];

/// Régua de 44px que substitui a sidebar quando um módulo está aberto
/// (code-spec-iced.md §5) — o botão ativo mostra o glifo do módulo; os
/// outros, só o número em `TEXT_6`.
fn module_ruler(active: ModuleId) -> Element<'static, Message> {
    let entry = |id: ModuleId| -> Element<'static, Message> {
        let is_active = id == active;
        let label = if is_active { module_glyph(id) } else { module_number(id) };
        let color = if is_active { GREEN } else { TEXT_6 };
        let el = container(text(label).size(if is_active { 13 } else { 10 }).color(color))
            .width(34)
            .height(34)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_: &Theme| {
                if is_active {
                    container::Style {
                        background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
                        border: Border { color: GREEN, width: 1.0, radius: 9.0.into() },
                        ..Default::default()
                    }
                } else {
                    container::Style::default()
                }
            });
        if is_active {
            el.into()
        } else {
            button(el)
                .padding(0)
                .style(|_, _| button::Style { background: None, ..Default::default() })
                .on_press(Message::OpenModule(id))
                .into()
        }
    };
    column(ALL_MODULES.into_iter().map(entry)).spacing(8).width(44).align_x(Alignment::Center).into()
}

fn module_header(title: &str, subtitle: String, subtitle_color: Color) -> Element<'static, Message> {
    column![
        text(title.to_string()).size(20).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(TEXT),
        text(subtitle).size(10).color(subtitle_color),
    ]
    .spacing(4)
    .into()
}

fn module_screen(hud: &Hud, id: ModuleId) -> Element<'_, Message> {
    let screen: Element<'_, Message> = match id {
        ModuleId::Clip => clip_screen(hud),
        ModuleId::Files => files_screen(hud),
        ModuleId::Notif => notif_screen(hud),
        ModuleId::Batt => batt_screen(hud),
        ModuleId::Control => control_screen(hud),
        ModuleId::Media => media_screen(&hud.snapshot.modules),
        ModuleId::Audio => audio_screen(hud),
        ModuleId::Webcam => webcam_screen(hud),
        ModuleId::Track => track_screen(hud),
        ModuleId::Config => config_screen(hud),
    };
    column![screen].width(Length::Fill).height(Length::Fill).into()
}

/// Lista de histórico compartilhada por CLIP/NOTIF: fundo escuro, separador
/// fino entre linhas, `Scrollable` (sem paginação — 50 entradas cabem bem).
fn history_list(rows: Vec<Element<'static, Message>>, empty_label: &'static str) -> Element<'static, Message> {
    let inner: Element<'static, Message> = if rows.is_empty() {
        text(empty_label).size(11).color(TEXT_3).into()
    } else {
        let mut col = column![].spacing(0);
        for (i, row_el) in rows.into_iter().enumerate() {
            if i > 0 {
                col = col.push(container(text("")).height(1).width(Length::Fill).style(|_| container::Style {
                    background: Some(Background::Color(DIVIDER)),
                    ..Default::default()
                }));
            }
            col = col.push(row_el);
        }
        col.into()
    };
    container(scrollable(inner).height(Length::Fill))
        .padding(4)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 1.0))),
            border: Border { radius: 14.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// Cabeçalho de busca reusado por CLIP/NOTIF (code-spec-iced.md: "pesquisar…"
/// + "limpar"). `on_clear` some quando `value` já está vazio.
fn search_row(value: &str, on_input: impl Fn(String) -> Message + 'static, on_clear: Message) -> Element<'static, Message> {
    let mut r = row![text_input("pesquisar…", value)
        .on_input(on_input)
        .size(10)
        .padding([6, 10])
        .width(160)
        .style(|_theme, _status| text_input::Style {
            background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35)),
            border: Border { color: GLASS_BRD, width: 1.0, radius: 6.0.into() },
            icon: TEXT_3,
            placeholder: TEXT_4,
            value: TEXT,
            selection: Color { a: 0.35, ..GREEN },
        })]
    .spacing(8)
    .align_y(Alignment::Center);
    if !value.is_empty() {
        r = r.push(
            button(text("limpar").size(9).color(TEXT_4))
                .padding([6, 9])
                .style(|_, _| button::Style { background: None, border: Border { color: BRD_1, width: 1.0, radius: 6.0.into() }, text_color: TEXT_4, ..Default::default() })
                .on_press(on_clear),
        );
    }
    r.into()
}

fn matches_search(haystack: &str, query: &str) -> bool {
    query.is_empty() || haystack.to_lowercase().contains(&query.to_lowercase())
}

fn clip_screen(hud: &Hud) -> Element<'_, Message> {
    let modules = &hud.snapshot.modules;
    let query = &hud.clip_search;
    let mut visible: Vec<&crate::state::ClipEntry> = modules.clip_history.iter().filter(|e| matches_search(&e.text, query)).collect();
    visible.sort_by_key(|e| !e.pinned); // fixados primeiro, mantém ordem relativa (stable sort)

    let action_btn = |label: &'static str, msg: Message| {
        button(text(label).size(10).color(TEXT_2))
            .padding([6, 11])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                border: Border { color: GLASS_BRD, width: 1.0, radius: 7.0.into() },
                text_color: TEXT_2,
                ..Default::default()
            })
            .on_press(msg)
    };

    let rows = visible
        .into_iter()
        .map(|entry| {
            let pinned = entry.pinned;
            let pin_label = if pinned { "fixado" } else { "fixar" };
            let id = entry.id;
            container(
                column![
                    row![
                        text(entry.direction).size(9).color(if pinned { GREEN } else { TEXT_3 }),
                        Space::new().width(Length::Fill),
                        text(entry.at.clone()).size(9).color(TEXT_5),
                    ],
                    text(entry.text.clone()).size(11).color(TEXT).line_height(text::LineHeight::Relative(1.4)),
                    row![
                        action_btn("copiar", Message::CopyText(entry.text.clone())),
                        action_btn(pin_label, Message::ClipPin(id)),
                    ]
                    .spacing(8),
                ]
                .spacing(8),
            )
            .padding([10, 12])
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: if pinned { Some(Background::Color(Color { a: 0.03, ..GREEN })) } else { None },
                ..Default::default()
            })
            .into()
        })
        .collect();

    let header_row = row![
        text(format!("HISTÓRICO · {} NESTA SESSÃO", modules.clip_history.len())).size(9).color(TEXT_5),
        Space::new().width(Length::Fill),
        search_row(query, Message::ClipSearch, Message::ClipSearch(String::new())),
    ]
    .align_y(Alignment::Center);

    column![
        module_header("CLIP", format!("{} no histórico desta sessão", modules.clip_history.len()), TEXT_2),
        header_row,
        history_list(rows, "Nada sincronizado ainda nesta sessão."),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}

fn filter_chip(label: String, count: usize, active: bool, on_press: Message) -> Element<'static, Message> {
    let color = if active { GREEN } else { TEXT_3 };
    button(text(format!("{label} · {count}")).size(10).color(color))
        .padding([6, 12])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if active { Color { a: 0.10, ..GREEN } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.03) })),
            border: Border { color: if active { Color { a: 0.35, ..GREEN } } else { BRD_1 }, width: 1.0, radius: 999.0.into() },
            text_color: color,
            ..Default::default()
        })
        .on_press(on_press)
        .into()
}

fn notif_screen(hud: &Hud) -> Element<'_, Message> {
    let modules = &hud.snapshot.modules;
    let query = &hud.notif_search;

    // Contagem por app, ordem de primeira aparição — vira os chips de filtro.
    let mut app_counts: Vec<(String, usize)> = Vec::new();
    for e in &modules.notif_history {
        match app_counts.iter_mut().find(|(a, _)| a == &e.app) {
            Some((_, c)) => *c += 1,
            None => app_counts.push((e.app.clone(), 1)),
        }
    }

    let mut chips = row![filter_chip("todas".to_string(), modules.notif_history.len(), hud.notif_app_filter.is_none(), Message::NotifAppFilter(None))].spacing(7);
    for (app, count) in &app_counts {
        chips = chips.push(filter_chip(app.clone(), *count, hud.notif_app_filter.as_deref() == Some(app.as_str()), Message::NotifAppFilter(Some(app.clone()))));
    }

    let rows = modules
        .notif_history
        .iter()
        .filter(|e| hud.notif_app_filter.as_deref().is_none_or(|f| f == e.app))
        .filter(|e| matches_search(&e.title, query) || matches_search(&e.text, query))
        .map(|entry| {
            container(
                row![
                    text(entry.app.clone()).size(9).color(MUTED).width(64),
                    column![
                        row![
                            text(entry.title.clone()).size(11).color(TEXT).width(Length::Fill),
                            text(entry.at.clone()).size(9).color(TEXT_5),
                        ]
                        .align_y(Alignment::Start),
                        text(entry.text.clone()).size(11).color(TEXT_3).line_height(text::LineHeight::Relative(1.4)),
                    ]
                    .spacing(3)
                    .width(Length::Fill),
                    button(text("copiar").size(9).color(TEXT_4))
                        .padding([4, 8])
                        .style(|_, _| button::Style { background: None, text_color: TEXT_4, ..Default::default() })
                        .on_press(Message::CopyText(format!("{}\n{}", entry.title, entry.text))),
                ]
                .spacing(10)
                .align_y(Alignment::Start),
            )
            .padding([10, 12])
            .width(Length::Fill)
            .into()
        })
        .collect();

    let header_row = row![
        text(format!("HISTÓRICO · {} NESTA SESSÃO", modules.notif_history.len())).size(9).color(TEXT_5),
        Space::new().width(Length::Fill),
        search_row(query, Message::NotifSearch, Message::NotifSearch(String::new())),
    ]
    .align_y(Alignment::Center);

    column![
        module_header("NOTIF", format!("{} espelhada(s) nesta sessão", modules.notif_count), TEXT_2),
        scrollable(chips).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(2).scroller_width(2))),
        header_row,
        history_list(rows, "Nenhuma notificação espelhada ainda nesta sessão."),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}

fn battery_card(label: &'static str, pct: Option<i64>, charging: Option<bool>) -> Element<'static, Message> {
    let pct_text = pct.map(|p| format!("{p}%")).unwrap_or_else(|| "--%".to_string());
    let sub = match (pct, charging) {
        (Some(_), Some(true)) => "a carregar".to_string(),
        (Some(_), Some(false)) => "na bateria".to_string(),
        (Some(_), None) => "nível conhecido, carregando desconhecido".to_string(),
        (None, _) => "aguardando dado real".to_string(),
    };
    container(
        column![
            text(label).size(9).color(TEXT_2),
            text(pct_text).size(40).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(GREEN),
            text(sub).size(10).color(TEXT_4),
        ]
        .spacing(6),
    )
    .padding(18)
    .width(Length::Fill)
    .style(|_| container::Style { background: Some(Background::Color(GREEN_BG)), border: Border { color: GREEN_BRD, width: 1.0, radius: 14.0.into() }, ..Default::default() })
    .into()
}

/// Barras das últimas 12h — `history` já vem em ordem cronológica; reduz a
/// no máximo 12 amostras espaçadas igualmente (uma por "hora" aproximada,
/// já que a amostragem real é a cada ~5 min).
fn battery_chart(history: &[crate::state::BatterySample]) -> Element<'static, Message> {
    if history.is_empty() {
        return text("Sem histórico ainda nesta sessão — volte daqui a pouco.").size(10).color(TEXT_5).into();
    }
    let n = history.len();
    let buckets = 12.min(n);
    let bars = (0..buckets).map(|i| {
        let idx = i * (n - 1) / buckets.max(1).saturating_sub(1).max(1);
        let sample = &history[idx.min(n - 1)];
        let pct = sample.phone.or(sample.pc).unwrap_or(0).clamp(0, 100) as f32;
        Element::from(container(text("")).width(Length::Fill).height(Length::Fixed((pct / 100.0 * 90.0).max(2.0))).style(move |_| container::Style {
            background: Some(Background::Color(if pct <= 20.0 { AMBER_BRD } else { GREEN_BRD })),
            border: Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }))
    });
    row(bars).spacing(6).height(90).align_y(Alignment::End).into()
}

fn alert_toggle(label: &'static str, enabled: bool, on_toggle: Message) -> Element<'static, Message> {
    row![
        text(label).size(11).color(TEXT_1).width(Length::Fill),
        button(text(if enabled { "ativo" } else { "desligado" }).size(10).color(if enabled { GREEN } else { TEXT_5 }))
            .padding([5, 10])
            .style(move |_, _| button::Style {
                background: Some(Background::Color(if enabled { Color { a: 0.10, ..GREEN } } else { Color::TRANSPARENT })),
                border: Border { color: if enabled { Color { a: 0.35, ..GREEN } } else { BRD_1 }, width: 1.0, radius: 7.0.into() },
                text_color: if enabled { GREEN } else { TEXT_5 },
                ..Default::default()
            })
            .on_press(on_toggle),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn batt_screen(hud: &Hud) -> Element<'_, Message> {
    let modules = &hud.snapshot.modules;
    let cards = row![
        battery_card("TELEMÓVEL", modules.phone_battery_pct, None),
        battery_card("PC", modules.pc_battery_pct, Some(modules.pc_battery_charging)),
    ]
    .spacing(14);

    let chart_card = container(
        column![
            row![text("ÚLTIMAS 12H").size(9).color(TEXT_2), Space::new().width(Length::Fill), text("telemóvel, quando disponível").size(9).color(TEXT_5)].align_y(Alignment::Center),
            battery_chart(&modules.battery_history),
        ]
        .spacing(14),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| glass(14.0));

    let alerts = config::battery_alerts(&hud.config);
    let alerts_card = container(
        column![
            text("ALERTAS NO DESKTOP").size(9).color(TEXT_2),
            alert_toggle("avisar abaixo de 20% (telemóvel)", alerts.low, Message::BattAlertToggle(config::BatteryAlertKind::Low, !alerts.low)),
            alert_toggle("avisar quando carregada a 100%", alerts.full, Message::BattAlertToggle(config::BatteryAlertKind::Full, !alerts.full)),
        ]
        .spacing(12),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| glass(14.0));

    let note = text("Temperatura, saúde e ciclos não existem no protocolo — só nível e carregamento chegam do telemóvel hoje.").size(10).color(TEXT_5);

    column![module_header("BATT", "Telemetria do telemóvel e do PC".to_string(), TEXT_2), cards, chart_card, alerts_card, note]
        .spacing(14)
        .into()
}

fn media_screen(modules: &crate::state::ModuleStatus) -> Element<'static, Message> {
    let now_playing = modules.media.clone().unwrap_or_else(|| "Nenhum leitor MPRIS ativo".to_string());
    let card = container(text(now_playing).size(14).color(TEXT))
        .padding(20)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x15 as f32 / 255.0, 0x10 as f32 / 255.0, 1.0))),
            border: Border { color: Color::from_rgba(0x1E as f32 / 255.0, 0x3E as f32 / 255.0, 0x30 as f32 / 255.0, 1.0), width: 1.0, radius: 14.0.into() },
            ..Default::default()
        });

    let transport_btn = |label: &'static str, cmd: &'static str| {
        button(text(label).size(12).color(TEXT))
            .padding([10, 18])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 10.0.into() },
                text_color: TEXT,
                ..Default::default()
            })
            .on_press(Message::MediaCommand(cmd))
    };
    let transport = row![
        transport_btn("⏮", "previous"),
        transport_btn("⏯", "playpause"),
        transport_btn("⏭", "next"),
    ]
    .spacing(10);

    let note = text("Volume, shuffle/repeat e lista de players ainda não existem no protocolo — só o essencial (título/artista/álbum + transporte) é real hoje.")
        .size(10)
        .color(TEXT_3);

    column![module_header("MEDIA", "MPRIS".to_string(), GREEN), card, transport, note].spacing(16).into()
}

fn mute_button(muted: bool, on_press: Message) -> Element<'static, Message> {
    let label = if muted { "🔇" } else { "🔊" };
    button(text(label).size(13))
        .padding([6, 10])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if muted { Color { a: 0.10, ..RED } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.045) })),
            border: Border { color: if muted { Color { a: 0.35, ..RED } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.08) }, width: 1.0, radius: 8.0.into() },
            text_color: if muted { RED } else { TEXT_2 },
            ..Default::default()
        })
        .on_press(on_press)
        .into()
}

fn sink_row(s: &crate::audio::SinkInfo) -> Element<'static, Message> {
    let id = s.id;
    let muted = s.muted;
    let name_col = column![
        text(s.description.clone()).size(11).color(TEXT),
        if s.is_default { text("saída padrão").size(9).color(GREEN) } else { text("").size(9) },
    ]
    .spacing(2)
    .width(Length::FillPortion(3));

    let mut controls = row![
        name_col,
        slider(0.0..=150.0, s.volume as f64, move |v| Message::AudioSetVolume("sink", id, v.round() as i64)).width(Length::FillPortion(4)),
        text(format!("{}%", s.volume)).size(10).color(TEXT_2).width(32),
        mute_button(muted, Message::AudioSetMute("sink", id, !muted)),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    if !s.is_default {
        let name = s.name.clone();
        controls = controls.push(
            button(text("usar").size(10).color(GREEN))
                .padding([6, 10])
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
                    border: Border { color: Color { a: 0.35, ..GREEN }, width: 1.0, radius: 8.0.into() },
                    text_color: GREEN,
                    ..Default::default()
                })
                .on_press(Message::AudioSetDefaultSink(name)),
        );
    }

    container(controls).padding(10).width(Length::Fill).style(|_| glass(10.0)).into()
}

fn app_row(a: &crate::audio::AppInfo) -> Element<'static, Message> {
    let id = a.id;
    let muted = a.muted;
    let label = a.media.clone().unwrap_or_else(|| a.name.clone());
    let name_col = column![text(a.name.clone()).size(11).color(TEXT), text(label).size(9).color(TEXT_2)].spacing(2).width(Length::FillPortion(3));

    container(
        row![
            name_col,
            slider(0.0..=150.0, a.volume as f64, move |v| Message::AudioSetVolume("app", id, v.round() as i64)).width(Length::FillPortion(4)),
            text(format!("{}%", a.volume)).size(10).color(TEXT_2).width(32),
            mute_button(muted, Message::AudioSetMute("app", id, !muted)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(10)
    .width(Length::Fill)
    .style(|_| glass(10.0))
    .into()
}

/// `percent` já vem resolvido pelo chamador (valor real ou o que está sendo
/// arrastado agora) — o comando de rede só sai no `on_release`, arrastar é
/// só visual (ver `Hud::phone_volume_drag`).
fn phone_volume_row(label: &'static str, stream: &'static str, percent: i64) -> Element<'static, Message> {
    row![
        text(label).size(11).color(TEXT).width(Length::FillPortion(2)),
        slider(0.0..=100.0, percent as f64, move |v| Message::PhoneAudioVolumeDragged(stream, v.round() as i64))
            .on_release(Message::PhoneAudioVolumeRelease(stream))
            .width(Length::FillPortion(4)),
        text(format!("{percent}%")).size(10).color(TEXT_2).width(32),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn ringer_mode_button(label: &'static str, mode: &'static str, active_mode: &str) -> Element<'static, Message> {
    let is_active = active_mode == mode;
    button(text(label).size(11).color(if is_active { GREEN } else { TEXT_2 }))
        .padding([8, 14])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if is_active { Color { a: 0.10, ..GREEN } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.045) })),
            border: Border { color: if is_active { Color { a: 0.35, ..GREEN } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.08) }, width: 1.0, radius: 8.0.into() },
            text_color: if is_active { GREEN } else { TEXT_2 },
            ..Default::default()
        })
        .on_press(Message::PhoneAudioSetRingerMode(mode))
        .into()
}

fn dnd_button(enabled: bool) -> Element<'static, Message> {
    let color = if enabled { AMBER } else { TEXT_2 };
    button(text(if enabled { "não perturbe: ligado" } else { "não perturbe: desligado" }).size(11).color(color))
        .padding([8, 14])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if enabled { Color { a: 0.10, ..AMBER } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.045) })),
            border: Border { color: if enabled { Color { a: 0.35, ..AMBER } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.08) }, width: 1.0, radius: 8.0.into() },
            text_color: color,
            ..Default::default()
        })
        .on_press(Message::PhoneAudioSetDnd(!enabled))
        .into()
}

fn phone_audio_section(hud: &Hud) -> Element<'_, Message> {
    let state = &hud.phone_audio;
    let displayed = |stream: &'static str, real: i64| hud.phone_volume_drag.get(stream).copied().unwrap_or(real);
    let mut section = column![
        text("TELEMÓVEL").size(9).color(TEXT_2),
        container(
            column![
                phone_volume_row("toque", "ring", displayed("ring", state.ring_percent)),
                phone_volume_row("mídia", "media", displayed("media", state.media_percent)),
                phone_volume_row("alarme", "alarm", displayed("alarm", state.alarm_percent)),
                row![
                    ringer_mode_button("som", "normal", &state.ringer_mode),
                    ringer_mode_button("vibrar", "vibrate", &state.ringer_mode),
                    ringer_mode_button("silencioso", "silent", &state.ringer_mode),
                ]
                .spacing(8),
                dnd_button(state.dnd_enabled),
            ]
            .spacing(10)
        )
        .padding(10)
        .width(Length::Fill)
        .style(|_| glass(10.0)),
    ]
    .spacing(8);

    if !state.dnd_access {
        section = section.push(
            text("Sem acesso a \"Não Perturbe\" no telemóvel — vibrar/silencioso não têm efeito até conceder essa permissão nas configurações dele.")
                .size(10)
                .color(AMBER),
        );
    }
    section.into()
}

fn audio_screen(hud: &Hud) -> Element<'_, Message> {
    let sinks: Element<'_, Message> = if hud.audio.sinks.is_empty() {
        text("A carregar saídas de som…").size(11).color(TEXT_3).into()
    } else {
        column(hud.audio.sinks.iter().map(sink_row)).spacing(8).into()
    };
    let apps: Element<'_, Message> = if hud.audio.apps.is_empty() {
        text("Nenhuma app tocando som agora.").size(11).color(TEXT_3).into()
    } else {
        column(hud.audio.apps.iter().map(app_row)).spacing(8).into()
    };

    // Microfone do telemóvel: função independente da webcam — pode ligar
    // por aqui (pede pro telemóvel) ou por lá (botão no telemóvel), os dois
    // convergem no mesmo estado real (`mic_active`).
    let mic_active = hud.snapshot.modules.mic_active;
    let mic_status = container(
        column![
            checkbox(mic_active)
                .label(if mic_active { "🎙️ microfone do telemóvel: ativo" } else { "🎙️ microfone do telemóvel: desligado" })
                .on_toggle(Message::PhoneMicToggle)
                .size(16)
                .text_size(11),
            if mic_active {
                Element::from(text("Selecione \"HyprLink-Mic\" como entrada de áudio em qualquer app.").size(10).color(TEXT_2))
            } else {
                Element::from(iced::widget::Space::new())
            },
        ]
        .spacing(6),
    )
    .padding(10)
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(if mic_active { Color { a: 0.10, ..GREEN } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.045) })),
        border: Border { color: if mic_active { Color { a: 0.35, ..GREEN } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.08) }, width: 1.0, radius: 10.0.into() },
        ..Default::default()
    });

    let tap_active = hud.snapshot.modules.audio_tap_active;
    let vu: Element<'_, Message> = if tap_active {
        let bars = hud.snapshot.modules.audio_vu.iter().map(|&pct| {
            let above = pct as f32 >= 60.0;
            Element::from(
                container(text("")).width(Length::Fill).height(Length::Fixed((pct as f32 / 100.0 * 56.0).max(2.0))).style(move |_| container::Style {
                    background: Some(Background::Color(if above { GREEN } else { GREEN_BRD })),
                    ..Default::default()
                }),
            )
        });
        row(bars).spacing(3).height(56).align_y(Alignment::End).into()
    } else {
        Space::new().height(56).into()
    };
    let tap_meta = match (tap_active, crate::state::audio_tap_elapsed_secs(&hud.shared)) {
        (true, Some(secs)) => format!("{} KB enviados · {secs}s", hud.snapshot.modules.audio_tap_bytes / 1024),
        _ => "Tap parado".to_string(),
    };
    let tap_card = container(
        column![row![text("SAÍDA ENCAMINHADA").size(9).color(if tap_active { GREEN } else { TEXT_2 }), Space::new().width(Length::Fill), text(tap_meta).size(9).color(TEXT_4)].align_y(Alignment::Center), vu]
            .spacing(14),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(if tap_active { GREEN_BG } else { Color::from_rgba(1.0, 1.0, 1.0, 0.025) })),
        border: Border { color: if tap_active { GREEN_BRD } else { BRD_1 }, width: 1.0, radius: 14.0.into() },
        ..Default::default()
    });

    scrollable(
        column![
            module_header("AUDIO", "Mixer do PC e do telemóvel".to_string(), TEXT_3),
            tap_card,
            text("SAÍDAS (PC)").size(9).color(TEXT_2),
            sinks,
            text("APPS (PC)").size(9).color(TEXT_2),
            apps,
            phone_audio_section(hud),
            mic_status,
        ]
        .spacing(12),
    )
    .into()
}

fn webcam_screen(hud: &Hud) -> Element<'_, Message> {
    let modules = &hud.snapshot.modules;
    let (status_text, status_color) = if modules.webcam_active {
        ("Stream ativo — a escrever em /dev/video42, use como webcam em qualquer app (Chrome, OBS, etc.)".to_string(), GREEN)
    } else {
        (
            format!("Parado. Ao iniciar, o telemóvel é trazido pro primeiro plano e passa a filmar em {}@{}fps ({}).", hud.webcam_resolution, hud.webcam_fps, hud.webcam_codec.to_uppercase()),
            TEXT_3,
        )
    };
    let preview = container(
        column![text(if modules.webcam_active { "●" } else { "◎" }).size(26).color(status_color), text(status_text.clone()).size(11).color(TEXT_2)]
            .spacing(10)
            .align_x(Alignment::Center),
    )
    .padding(30)
    .width(Length::Fill)
    .align_x(Alignment::Center)
    .style(move |_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 1.0))),
        border: Border { radius: 14.0.into(), ..Default::default() },
        ..Default::default()
    });


    let picker_style = |_theme: &Theme, _status: iced::widget::pick_list::Status| iced::widget::pick_list::Style {
        text_color: TEXT,
        placeholder_color: TEXT_3,
        handle_color: TEXT_3,
        background: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045)),
        border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 10.0.into() },
    };
    let config_row = row![
        column![
            text("resolução").size(9).color(TEXT_2),
            iced::widget::pick_list(WEBCAM_RESOLUTIONS, Some(hud.webcam_resolution), Message::WebcamResolutionChanged)
                .text_size(11)
                .padding([8, 12])
                .style(picker_style),
        ]
        .spacing(4)
        .width(Length::Fill),
        column![
            text("fps").size(9).color(TEXT_2),
            iced::widget::pick_list(WEBCAM_FPS_OPTIONS, Some(hud.webcam_fps), Message::WebcamFpsChanged)
                .text_size(11)
                .padding([8, 12])
                .style(picker_style),
        ]
        .spacing(4)
        .width(Length::Fixed(80.0)),
        column![
            text("codec").size(9).color(TEXT_2),
            iced::widget::pick_list(WEBCAM_CODECS, Some(hud.webcam_codec), Message::WebcamCodecChanged)
                .text_size(11)
                .padding([8, 12])
                .style(picker_style),
        ]
        .spacing(4)
        .width(Length::Fixed(100.0)),
    ]
    .spacing(12);

    let is_testing = matches!(hud.webcam_test, WebcamTest::Running { .. });
    let action = if modules.webcam_active {
        button(text("parar stream").size(12).color(RED))
            .padding([10, 18])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color { a: 0.10, ..RED })),
                border: Border { color: Color { a: 0.35, ..RED }, width: 1.0, radius: 10.0.into() },
                text_color: RED,
                ..Default::default()
            })
            .on_press(Message::WebcamStop)
    } else {
        button(text("iniciar stream").size(12).color(GREEN))
            .padding([10, 18])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
                border: Border { color: Color { a: 0.35, ..GREEN }, width: 1.0, radius: 10.0.into() },
                text_color: GREEN,
                ..Default::default()
            })
            .on_press(Message::WebcamStart)
    };

    let mut action_row = row![action].spacing(10);
    if !modules.webcam_active && !is_testing {
        let test_btn = button(text("testar rede/hardware").size(12).color(TEXT_2))
            .padding([10, 18])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 10.0.into() },
                text_color: TEXT_2,
                ..Default::default()
            })
            .on_press(Message::WebcamTestStart);
        action_row = action_row.push(test_btn);
    }

    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    let test_panel: Element<'_, Message> = match hud.webcam_test {
        WebcamTest::Idle => Space::new().height(0).into(),
        WebcamTest::Running { started } => {
            let remaining = WEBCAM_TEST_DURATION.saturating_sub(started.elapsed()).as_secs() + 1;
            let mbps_text = modules.webcam_mbps.map(|m| format!("{m:.1} Mbps")).unwrap_or_else(|| "medindo…".to_string());
            container(text(format!("Testando em 1920×1080@30 (baseline — não testa 2K/4K, ver nota abaixo) · {mbps_text} · {remaining}s restantes")).size(11).color(AMBER))
                .padding(12)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color { a: 0.10, ..AMBER })),
                    border: Border { color: Color { a: 0.35, ..AMBER }, width: 1.0, radius: 10.0.into() },
                    ..Default::default()
                })
                .into()
        }
        WebcamTest::Done { mbps, suggested } => container(
            row![
                text(format!("Sugestão: {} @ {}fps  ·  medido: {mbps:.1} Mbps na rede, {cores} núcleos de CPU", suggested.0, suggested.1))
                    .size(11)
                    .color(GREEN)
                    .width(Length::Fill),
                button(text("aplicar").size(11).color(GREEN))
                    .padding([6, 14])
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
                        border: Border { color: Color { a: 0.35, ..GREEN }, width: 1.0, radius: 8.0.into() },
                        text_color: GREEN,
                        ..Default::default()
                    })
                    .on_press(Message::WebcamTestApply),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(12)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
            border: Border { color: Color { a: 0.35, ..GREEN }, width: 1.0, radius: 10.0.into() },
            ..Default::default()
        })
        .into(),
    };

    let note = text("Sem pré-visualização aqui na GUI (custo de CPU extra por só cosmético) — só o /dev/video42 recebe o vídeo. Rotação/espelho seguem o que for ajustado no telemóvel. O teste mede vazão real da rede transmitindo por alguns segundos — não estima CPU de decodificação além da contagem de núcleos.")
        .size(10)
        .color(TEXT_3);

    let footer: Element<'_, Message> = match &modules.webcam_last_used {
        Some((at, dur)) => container(text(format!("último uso · {at} · {dur}")).size(10).color(TEXT_4)).padding([10, 12]).width(Length::Fill).style(|_| glass(10.0)).into(),
        None => Space::new().height(0).into(),
    };

    column![module_header("WEBCAM", "Câmara remota do PC".to_string(), status_color), preview, config_row, action_row, test_panel, footer, note]
        .spacing(16)
        .into()
}

fn control_screen(hud: &Hud) -> Element<'_, Message> {
    let current = hud.snapshot.modules.workspace.clone();
    let buttons: Element<'_, Message> = if hud.hypr_workspaces.is_empty() {
        text("A carregar workspaces…").size(11).color(TEXT_3).into()
    } else {
        row(hud.hypr_workspaces.iter().map(|&id| {
            let is_active = current.as_deref() == Some(&id.to_string());
            button(text(id.to_string()).size(13).color(if is_active { Color::from_rgb(0.016, 0.082, 0.051) } else { TEXT_3 }))
                .width(40)
                .height(40)
                .style(move |_, _| {
                    if is_active {
                        button::Style {
                            background: Some(Background::Color(GREEN)),
                            border: Border { radius: 9.0.into(), ..Default::default() },
                            ..Default::default()
                        }
                    } else {
                        button::Style {
                            background: None,
                            border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 9.0.into() },
                            text_color: TEXT_3,
                            ..Default::default()
                        }
                    }
                })
                .on_press(Message::WorkspaceClicked(id))
                .into()
        }))
        .spacing(8)
        .into()
    };

    let card = container(column![text("WORKSPACES").size(9).color(TEXT_2), buttons].spacing(12))
        .padding(20)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x15 as f32 / 255.0, 0x10 as f32 / 255.0, 1.0))),
            border: Border { color: Color::from_rgba(0x1E as f32 / 255.0, 0x3E as f32 / 255.0, 0x30 as f32 / 255.0, 1.0), width: 1.0, radius: 14.0.into() },
            ..Default::default()
        });

    let dispatch_row = row![
        text_input("hyprctl dispatch …", &hud.hypr_input)
            .on_input(Message::HyprInputChanged)
            .on_submit(Message::HyprDispatch)
            .size(11)
            .padding(10)
            .style(|_theme, _status| text_input::Style {
                background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35)),
                border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 10.0.into() },
                icon: TEXT_2,
                placeholder: TEXT_3,
                value: TEXT,
                selection: Color { a: 0.35, ..GREEN },
            }),
        button(text("executar").size(11).color(GREEN))
            .padding([10, 16])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
                border: Border { color: Color { a: 0.35, ..GREEN }, width: 1.0, radius: 10.0.into() },
                text_color: GREEN,
                ..Default::default()
            })
            .on_press(Message::HyprDispatch),
    ]
    .spacing(10);

    let result: Element<'_, Message> = match &hud.hypr_last_result {
        Some(r) if !r.is_empty() => text(r.clone()).size(10).color(TEXT_3).into(),
        _ => Space::new().height(0).into(),
    };

    let context_line = match (&current, &hud.hypr_focused) {
        (Some(ws), Some((class, title))) => {
            text(format!("{ws} · {} janela(s) · foco: {} — {}", hud.hypr_window_count, class, title)).size(10).color(TEXT_4)
        }
        (Some(ws), None) => text(format!("{ws} · {} janela(s)", hud.hypr_window_count)).size(10).color(TEXT_4),
        _ => text("Aguardando o primeiro hypr.event…").size(10).color(TEXT_5),
    };

    let shortcuts = config::shortcuts(&hud.config);
    let shortcuts_list: Element<'_, Message> = if shortcuts.is_empty() {
        text("Nenhum atalho configurado ainda.").size(10).color(TEXT_5).into()
    } else {
        column(shortcuts.iter().enumerate().map(|(i, s)| {
            row![
                column![text(s.name.clone()).size(11).color(TEXT), text(s.command.clone()).size(9).color(TEXT_4)].spacing(2).width(Length::Fill),
                button(text("executar").size(9).color(GREEN))
                    .padding([5, 10])
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
                        border: Border { color: Color { a: 0.35, ..GREEN }, width: 1.0, radius: 7.0.into() },
                        text_color: GREEN,
                        ..Default::default()
                    })
                    .on_press(Message::ShortcutRun(s.command.clone())),
                button(text("remover").size(9).color(RED))
                    .padding([5, 10])
                    .style(|_, _| button::Style { background: None, border: Border { color: RED_BRD, width: 1.0, radius: 7.0.into() }, text_color: RED, ..Default::default() })
                    .on_press(Message::ShortcutRemove(i)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }))
        .spacing(8)
        .into()
    };
    let add_shortcut_row = row![
        text_input("nome", &hud.shortcut_name)
            .on_input(Message::ShortcutNameChanged)
            .size(10)
            .padding(8)
            .width(Length::FillPortion(2))
            .style(|_theme, _status| text_input::Style {
                background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35)),
                border: Border { color: GLASS_BRD, width: 1.0, radius: 8.0.into() },
                icon: TEXT_3,
                placeholder: TEXT_4,
                value: TEXT,
                selection: Color { a: 0.35, ..GREEN },
            }),
        text_input("comando (ex: workspace 3)", &hud.shortcut_cmd)
            .on_input(Message::ShortcutCmdChanged)
            .on_submit(Message::ShortcutAdd)
            .size(10)
            .padding(8)
            .width(Length::FillPortion(3))
            .style(|_theme, _status| text_input::Style {
                background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35)),
                border: Border { color: GLASS_BRD, width: 1.0, radius: 8.0.into() },
                icon: TEXT_3,
                placeholder: TEXT_4,
                value: TEXT,
                selection: Color { a: 0.35, ..GREEN },
            }),
        button(text("adicionar").size(10).color(TEXT_2))
            .padding([8, 12])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                border: Border { color: GLASS_BRD, width: 1.0, radius: 8.0.into() },
                text_color: TEXT_2,
                ..Default::default()
            })
            .on_press(Message::ShortcutAdd),
    ]
    .spacing(8);

    let shortcuts_card = container(column![text("ATALHOS").size(9).color(TEXT_2), shortcuts_list, add_shortcut_row].spacing(12))
        .padding([13, 16])
        .width(Length::Fill)
        .style(|_| glass(12.0));

    column![module_header("CONTROL", "Hyprland IPC".to_string(), GREEN), card, context_line, shortcuts_card, dispatch_row, result]
        .spacing(14)
        .into()
}

fn config_screen(hud: &Hud) -> Element<'static, Message> {
    let snapshot = &hud.snapshot;
    let row_kv = |label: &'static str, value: String| {
        row![
            text(label).size(11).color(TEXT_2),
            Space::new().width(Length::Fill),
            text(value).size(11).color(TEXT).font(Font::MONOSPACE),
        ]
        .align_y(Alignment::Center)
    };
    let list = container(
        column![
            text("REDE").size(9).color(TEXT_2),
            row_kv("host : porta", snapshot.local_addr.clone()),
            row_kv("fingerprint deste PC", short_fp(&snapshot.server_fingerprint_hex)),
        ]
        .spacing(10),
    )
    .padding(16)
    .width(Length::Fill)
    .style(|_| container::Style { background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 1.0))), ..Default::default() });

    let device_card: Element<'static, Message> = match &snapshot.conn {
        ConnState::Connected { device_name, fingerprint_hex } => container(
            column![
                text("TELEMÓVEL LIGADO AGORA").size(9).color(TEXT_2),
                row_kv("nome", device_name.clone()),
                row_kv("fingerprint", short_fp(fingerprint_hex)),
            ]
            .spacing(10),
        )
        .padding(16)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x15 as f32 / 255.0, 0x10 as f32 / 255.0, 1.0))),
            border: Border { color: Color::from_rgba(0x1E as f32 / 255.0, 0x3E as f32 / 255.0, 0x30 as f32 / 255.0, 1.0), width: 1.0, radius: 14.0.into() },
            ..Default::default()
        })
        .into(),
        _ => container(text("Nenhum telemóvel ligado agora.").size(11).color(TEXT_3)).padding(16).width(Length::Fill).into(),
    };

    let tray_toggle = container(
        checkbox(config::tray_special_workspace(&hud.config))
            .label("Minimizar pra bandeja usando workspace especial do Hyprland")
            .on_toggle(Message::ToggleTraySpecialWorkspace)
            .size(16)
            .text_size(11),
    )
    .padding(16)
    .width(Length::Fill)
    .style(|_| container::Style { background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 1.0))), ..Default::default() });

    let tray_note = text("Se desligado, o botão de minimizar some do cabeçalho — esse mecanismo é específico do Hyprland (move a janela pra uma workspace especial), pode não fazer sentido noutro compositor.")
        .size(10)
        .color(TEXT_3);

    let connected_fp = match &snapshot.conn {
        ConnState::Connected { fingerprint_hex, .. } => Some(fingerprint_hex.clone()),
        _ => None,
    };
    let fingerprints = hud.pairing.lock().unwrap().list_fingerprints();
    let paired_rows: Element<'_, Message> = if fingerprints.is_empty() {
        text("Nenhum dispositivo pareado.").size(11).color(TEXT_3).into()
    } else {
        column(fingerprints.into_iter().map(|fp| {
            let is_connected = connected_fp.as_deref() == Some(fp.as_str());
            let revoke_color = if is_connected { RED } else { TEXT_5 };
            row![
                column![
                    text(short_fp(&fp)).size(11).color(TEXT_1),
                    text(if is_connected { "ligado agora" } else { "pareado" }).size(9).color(if is_connected { GREEN } else { TEXT_5 }),
                ]
                .spacing(3)
                .width(Length::Fill),
                button(text("revogar").size(9).color(revoke_color))
                    .padding([5, 10])
                    .style(move |_, _| button::Style { background: None, border: Border { color: revoke_color, width: 1.0, radius: 7.0.into() }, text_color: revoke_color, ..Default::default() })
                    .on_press(Message::ConfigRevoke(fp.clone())),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        }))
        .spacing(10)
        .into()
    };
    let paired_card = container(column![text("DISPOSITIVOS EMPARELHADOS").size(9).color(TEXT_2), paired_rows].spacing(12))
        .padding(16)
        .width(Length::Fill)
        .style(|_| container::Style { background: Some(Background::Color(TERMINAL)), ..Default::default() });

    let restart_btn = button(text("reiniciar daemon").size(10).color(TEXT_2))
        .padding([8, 14])
        .style(|_, _| button::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
            border: Border { color: GLASS_BRD, width: 1.0, radius: 8.0.into() },
            text_color: TEXT_2,
            ..Default::default()
        })
        .on_press(Message::ConfigRestartDaemon);

    let note = text("Nível de log e retenção de histórico ainda não têm UI — dá pra reparear via QR se precisar trocar de telemóvel.")
        .size(10)
        .color(TEXT_3);

    column![
        module_header("CONFIG", "Rede e dispositivo ligado".to_string(), TEXT_2),
        device_card,
        list,
        paired_card,
        tray_toggle,
        tray_note,
        restart_btn,
        note,
    ]
    .spacing(16)
    .into()
}

/// FILES (`02`): pasta de destino + envio de ficheiro (real, `rfd`) hoje;
/// progresso ao vivo e histórico de transferências entram no Estágio 3
/// (precisam de `share.rs` passar a escrever em `HudState`, hoje só loga).
fn files_screen(hud: &Hud) -> Element<'_, Message> {
    let dest_card = container(
        row![
            column![
                text("PASTA DE DESTINO").size(9).color(TEXT_2),
                text(hud.download_dir.display().to_string()).size(12).color(TEXT_1),
            ]
            .spacing(7)
            .width(Length::Fill),
            button(text("escolher pasta").size(11).color(TEXT_2))
                .padding([8, 14])
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                    border: Border { color: GLASS_BRD, width: 1.0, radius: 8.0.into() },
                    text_color: TEXT_2,
                    ..Default::default()
                })
                .on_press(Message::PickDownloadDir),
        ]
        .align_y(Alignment::Center),
    )
    .padding([13, 16])
    .width(Length::Fill)
    .style(|_| glass(12.0));

    let send_btn = button(text("enviar ficheiro…").size(12).color(GREEN))
        .padding([10, 18])
        .style(|_, _| button::Style {
            background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
            border: Border { color: Color { a: 0.35, ..GREEN }, width: 1.0, radius: 10.0.into() },
            text_color: GREEN,
            ..Default::default()
        })
        .on_press(Message::PickFileToSend);

    let progress_card: Element<'_, Message> = match &hud.snapshot.modules.file_transfer {
        Some(t) => {
            let pct = if t.total > 0 { (t.bytes as f64 / t.total as f64 * 100.0).clamp(0.0, 100.0) } else { 0.0 };
            let elapsed = t.started_at.elapsed().as_secs_f64().max(0.1);
            let rate_mbps = (t.bytes as f64 / elapsed) / 1_000_000.0;
            container(
                column![
                    row![
                        text(t.name.clone()).size(12).color(TEXT_1).width(Length::Fill),
                        text(format!("{} · {rate_mbps:.1} MB/s", t.direction)).size(10).color(AMBER),
                    ]
                    .align_y(Alignment::Center),
                    row![
                        container(text("")).height(4).width(Length::FillPortion((pct.round() as u16).max(1))).style(|_| container::Style {
                            background: Some(Background::Color(AMBER)),
                            border: Border { radius: 2.0.into(), ..Default::default() },
                            ..Default::default()
                        }),
                        container(text("")).height(4).width(Length::FillPortion((100 - pct.round() as u16).max(1))).style(|_| container::Style {
                            background: Some(Background::Color(AMBER_BRD)),
                            border: Border { radius: 2.0.into(), ..Default::default() },
                            ..Default::default()
                        }),
                    ],
                    row![
                        text(format!("{:.1} / {:.1} MB", t.bytes as f64 / 1_000_000.0, t.total as f64 / 1_000_000.0)).size(10).color(TEXT_4).width(Length::Fill),
                        button(text("cancelar").size(9).color(RED))
                            .padding([5, 10])
                            .style(|_, _| button::Style { background: None, border: Border { color: RED_BRD, width: 1.0, radius: 7.0.into() }, text_color: RED, ..Default::default() })
                            .on_press(Message::FileTransferCancel),
                    ]
                    .align_y(Alignment::Center),
                ]
                .spacing(10),
            )
            .padding([13, 16])
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(AMBER_BG)),
                border: Border { color: AMBER_BRD, width: 1.0, radius: 14.0.into() },
                ..Default::default()
            })
            .into()
        }
        None => Space::new().height(0).into(),
    };

    let history = &hud.snapshot.modules.file_history;
    let history_rows = history
        .iter()
        .map(|r| {
            let (arrow, arrow_color) = match (r.direction, r.ok) {
                (_, false) => ("✕", RED),
                ("recebendo", true) => ("↓", GREEN),
                _ => ("↑", TEXT_3),
            };
            let meta = if r.ok {
                format!("{} · {:.1} MB · {}s · {}", if r.direction == "recebendo" { "recebido" } else { "enviado" }, r.bytes as f64 / 1_000_000.0, r.duration_secs, r.at)
            } else {
                format!("falhou · {}", r.error.clone().unwrap_or_default())
            };
            container(
                row![
                    text(arrow).size(11).color(arrow_color).width(14),
                    column![text(r.name.clone()).size(11).color(TEXT_1), text(meta).size(9).color(if r.ok { TEXT_4 } else { RED })].spacing(3).width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Start),
            )
            .padding([10, 12])
            .width(Length::Fill)
            .into()
        })
        .collect();

    let note = text("Sem \"pausar\" — o protocolo só permite continuar ou cancelar uma transferência em andamento.").size(10).color(TEXT_5);

    column![
        module_header("FILES", format!("Transferência de ficheiros sobre QUIC{}", if hud.snapshot.modules.file_transfer.is_some() { " · 1 a transferir" } else { "" }), TEXT_2),
        dest_card,
        send_btn,
        progress_card,
        text(format!("HISTÓRICO · {} TRANSFERÊNCIA(S)", history.len())).size(9).color(TEXT_5),
        history_list(history_rows, "Nenhuma transferência ainda nesta sessão."),
        note,
    ]
    .spacing(14)
    .height(Length::Fill)
    .into()
}

/// TRACK (`09`): rato/teclado virtual via `/dev/uinput` — já real e validado
/// (Fase 3), só faltava esta tela. Espelho do cursor e sliders de
/// sensibilidade/scroll aplicados de verdade entram no Estágio 4.
/// Grelha simples (16×9) marcando a célula onde o cursor está agora —
/// aproximação leve do "espelho do cursor" do mockup sem precisar de
/// `canvas` (posicionamento livre de pixel não é trivial em iced 0.14 fora
/// dele). `size` é a resolução do monitor focado.
fn cursor_grid(cursor: Option<(i64, i64)>, size: (i64, i64)) -> Element<'static, Message> {
    const COLS: i64 = 16;
    const ROWS: i64 = 9;
    let cell = cursor.map(|(x, y)| ((x * COLS / size.0.max(1)).clamp(0, COLS - 1), (y * ROWS / size.1.max(1)).clamp(0, ROWS - 1)));

    let mut grid = column![].spacing(3).width(Length::Fill).height(Length::Fixed(220.0));
    for ry in 0..ROWS {
        let mut r = row![].spacing(3).height(Length::Fill);
        for rx in 0..COLS {
            let is_cursor = cell == Some((rx, ry));
            r = r.push(container(text("")).width(Length::Fill).height(Length::Fill).style(move |_| container::Style {
                background: Some(Background::Color(if is_cursor { GREEN } else { Color::from_rgba(1.0, 1.0, 1.0, 0.025) })),
                border: Border { radius: 2.0.into(), ..Default::default() },
                shadow: if is_cursor { Shadow { color: Color { a: 0.6, ..GREEN }, offset: Vector::default(), blur_radius: 10.0 } } else { Shadow::default() },
                ..Default::default()
            }));
        }
        grid = grid.push(r);
    }

    container(
        column![
            row![
                text("ESPELHO DO CURSOR").size(9).color(TEXT_2),
                Space::new().width(Length::Fill),
                text(format!("{}×{}", size.0, size.1)).size(9).color(TEXT_5),
            ]
            .align_y(Alignment::Center),
            grid,
            text(cursor.map(|(x, y)| format!("x {x} · y {y}")).unwrap_or_else(|| "aguardando…".to_string())).size(9).color(TEXT_5),
        ]
        .spacing(10),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| container::Style { background: Some(Background::Color(TERMINAL)), border: Border { radius: 14.0.into(), ..Default::default() }, ..Default::default() })
    .into()
}

fn track_slider(label: &'static str, value: f32, display: String, range: std::ops::RangeInclusive<f32>, on_change: impl Fn(f32) -> Message + 'static) -> Element<'static, Message> {
    column![
        row![text(label).size(11).color(TEXT_1), Space::new().width(Length::Fill), text(display).size(10).color(TEXT_3)].align_y(Alignment::Center),
        slider(range, value, on_change).step(0.1_f32),
    ]
    .spacing(9)
    .into()
}

fn track_toggle(label: &'static str, enabled: bool, on_toggle: Message) -> Element<'static, Message> {
    button(text(format!("{label} {}", if enabled { "✓" } else { "" })).size(11).color(if enabled { TEXT_1 } else { TEXT_4 }))
        .padding([12, 0])
        .width(Length::Fill)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if enabled { Color { a: 0.06, ..GREEN } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.03) })),
            border: Border { color: if enabled { Color { a: 0.3, ..GREEN } } else { BRD_1 }, width: 1.0, radius: 10.0.into() },
            text_color: if enabled { TEXT_1 } else { TEXT_4 },
            ..Default::default()
        })
        .on_press(on_toggle)
        .into()
}

fn track_screen(hud: &Hud) -> Element<'_, Message> {
    let t = config::track_settings(&hud.config);
    let mirror = cursor_grid(hud.cursor_pos, hud.screen_size);

    let sliders = container(
        column![
            track_slider("SENSIBILIDADE", t.sensitivity, format!("{:.1}×", t.sensitivity), 0.2..=3.0, Message::TrackSensitivity),
            track_slider("VELOCIDADE DE SCROLL", t.scroll_speed, format!("{:.1}×", t.scroll_speed), 0.2..=3.0, Message::TrackScrollSpeed),
        ]
        .spacing(16),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| glass(14.0));

    let toggles = row![
        track_toggle("aceleração", t.acceleration, Message::TrackAcceleration(!t.acceleration)),
        track_toggle("inverter scroll", t.invert_scroll, Message::TrackInvertScroll(!t.invert_scroll)),
        track_toggle("teclado virtual", t.virtual_keyboard, Message::TrackVirtualKeyboard(!t.virtual_keyboard)),
    ]
    .spacing(10);

    let note = text("Movimento, cliques e scroll chegam do telemóvel via /dev/uinput — os sliders acima já se aplicam de verdade.").size(10).color(TEXT_5);

    column![module_header("TRACK", "Rato e teclado virtuais".to_string(), TEXT_2), mirror, sliders, toggles, note]
        .spacing(16)
        .into()
}

fn view(hud: &Hud) -> Element<'_, Message> {
    let accent = hud.accent();
    let elapsed = hud.start_time.elapsed().as_secs_f32();

    let logo = row![
        container(breathing_dot(accent, 8.0, elapsed)).width(14).height(14).align_x(Alignment::Center).align_y(Alignment::Center),
        text("HYPR").size(21).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(TEXT),
        text("LINK").size(21).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(accent),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let (status_label, status_color) = match &hud.snapshot.conn {
        ConnState::Connected { .. } => ("LINK ATIVO", GREEN),
        ConnState::Connecting => ("CONECTANDO", AMBER),
        ConnState::Pairing => ("AGUARDANDO PAREAMENTO", AMBER),
    };

    let close_btn = button(text("×").size(16).color(TEXT_2))
        .padding([2, 9])
        .style(|_, _| button::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
            border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 8.0.into() },
            text_color: TEXT_2,
            ..Default::default()
        })
        .on_press(Message::Quit);

    let minimize_btn = button(text("–").size(16).color(TEXT_2))
        .padding([2, 9])
        .style(|_, _| button::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
            border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 8.0.into() },
            text_color: TEXT_2,
            ..Default::default()
        })
        .on_press(Message::MinimizeToTray);

    let mut header_right_row = row![].spacing(8).align_y(Alignment::Center);
    if hud.screen != Screen::Dashboard {
        let back = button(text("‹ VOLTAR").size(11).color(TEXT_2))
            .padding([6, 12])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 8.0.into() },
                text_color: TEXT_2,
                ..Default::default()
            })
            .on_press(Message::Back);
        header_right_row = header_right_row.push(back);
    } else {
        header_right_row = header_right_row.push(status_pill(status_color, status_label, elapsed));
    }
    if config::tray_special_workspace(&hud.config) {
        header_right_row = header_right_row.push(minimize_btn);
    }
    let header_right: Element<'_, Message> = header_right_row.push(close_btn).into();

    let header = row![logo, Space::new().width(Length::Fill), header_right].spacing(8).align_y(Alignment::Center);

    let body: Element<'_, Message> = match (&hud.snapshot.conn, &hud.screen) {
        (ConnState::Connected { .. }, Screen::Module(id)) => row![module_ruler(*id), module_screen(hud, *id)]
            .spacing(16)
            .height(Length::Fill)
            .into(),
        (ConnState::Connected { device_name, fingerprint_hex }, Screen::Dashboard) => {
            let connbar = container(
                column![
                    text(device_name.clone()).size(14).color(TEXT).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }),
                    text(format!("TLS emparelhado · fp {}", short_fp(fingerprint_hex))).size(10).color(TEXT_2),
                ]
                .spacing(2),
            )
            .padding([12, 16])
            .style(|_| glass(14.0));

            let console_header = row![
                text("console de diagnóstico").size(9).color(TEXT_2),
                Space::new().width(Length::Fill),
                button(text("copiar log").size(9).color(TEXT_2))
                    .padding([4, 10])
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))),
                        border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 8.0.into() },
                        text_color: TEXT_2,
                        ..Default::default()
                    })
                    .on_press(Message::CopyText(hud.snapshot.logs.join("\n"))),
            ]
            .align_y(Alignment::Center);

            row![
                column![connbar, module_list(&hud.download_dir, &hud.snapshot.modules)].spacing(14),
                container(column![console_header, console(&hud.console)].spacing(8))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(18)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                        border: Border { color: Color { a: 0.35, ..accent }, width: 1.0, radius: 14.0.into() },
                        ..Default::default()
                    }),
            ]
            .spacing(14)
            .height(Length::Fill)
            .into()
        }
        _ => {
            let payload = format!(
                "{}|{}|{}",
                hud.snapshot.server_fingerprint_hex, hud.snapshot.local_addr, hud.snapshot.pairing_token_hex
            );
            let pairing_card = container(
                column![
                    text("APONTE A CÂMARA DO TELEMÓVEL").size(11).color(TEXT_2),
                    pairing_qr(&payload),
                    text("Abra o HyprLink no Android e escaneie o código").size(10).color(TEXT_2),
                    kv_row(
                        "FINGERPRINT",
                        short_fp(&hud.snapshot.server_fingerprint_hex),
                        hud.snapshot.server_fingerprint_hex.clone(),
                    ),
                    kv_row("HOST : PORTA", hud.snapshot.local_addr.clone(), hud.snapshot.local_addr.clone()),
                    kv_row("TOKEN", hud.snapshot.pairing_token_hex.clone(), hud.snapshot.pairing_token_hex.clone()),
                ]
                .spacing(10)
                .align_x(Alignment::Center),
            )
            .padding(22)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                border: Border { color: Color { a: 0.35, ..AMBER }, width: 1.0, radius: 14.0.into() },
                ..Default::default()
            });

            row![module_list(&hud.download_dir, &hud.snapshot.modules), pairing_card].spacing(14).height(Length::Fill).into()
        }
    };

    let content = column![header, body].spacing(14).padding(22).height(Length::Fill);

    container(content)
        .width(PANEL_W as f32)
        .height(PANEL_H as f32)
        .style(|_| container::Style {
            // alpha mais baixo que o WINDOW_BG do spec (0.82) — valor calibrado
            // ao vivo com o usuário contra o blur real do Hyprland (Fase 2).
            background: Some(Background::Color(Color { a: 0.62, ..WINDOW_BG })),
            border: Border { color: WINDOW_BRD, width: 1.0, radius: 28.0.into() },
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.45), offset: Vector::new(0.0, 12.0), blur_radius: 40.0 },
            ..Default::default()
        })
        .into()
}

/// Botão "×": mata qualquer outra instância órfã do daemon (comum durante
/// desenvolvimento, quando um `cargo run` anterior fica preso na porta 7443)
/// antes de encerrar este processo — `exit(0)` já derruba a thread do daemon
/// que roda dentro deste mesmo processo.
fn kill_other_instances() {
    let my_pid = std::process::id().to_string();
    let Ok(output) = std::process::Command::new("pgrep").args(["-x", "hyprlink-daemon"]).output() else {
        return;
    };
    let Ok(text) = String::from_utf8(output.stdout) else {
        return;
    };
    for pid in text.lines().filter(|p| !p.is_empty() && *p != my_pid) {
        let _ = std::process::Command::new("kill").args(["-9", pid]).status();
    }
}

fn short_fp(fp: &str) -> String {
    let clean: String = fp.chars().filter(|c| *c != ':').collect();
    if clean.len() < 12 {
        return fp.to_string();
    }
    format!("{}…{}", &clean[..8], &clean[clean.len() - 8..])
}

fn style(_hud: &Hud, theme: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    }
}

pub fn run(
    shared: Arc<Mutex<HudState>>,
    config: SharedConfig,
    active: ActiveConn,
    pending_webcam: crate::webcam::PendingWebcam,
    tray_show: crate::tray::ShowRequested,
    pairing: Arc<Mutex<crate::pairing::PairingStore>>,
) -> iced::Result {
    iced::application(
        move || Hud::new(shared.clone(), config.clone(), active.clone(), pending_webcam.clone(), tray_show.clone(), pairing.clone()),
        update,
        view,
    )
        .title("HyprLink")
        .style(style)
        .subscription(subscription)
        .window(window::Settings {
            size: iced::Size::new(PANEL_W as f32, PANEL_H as f32),
            // ponytail: canto superior direito em vez de centralizada — a
            // cada restart do daemon (frequente durante desenvolvimento) ela
            // reabre do zero e cobria o terminal. Fixo pra uma tela 1920x1080;
            // se um dia rodar noutra resolução, mover manualmente (SUPER +
            // clique esquerdo arrasta, sem barra de título) resolve.
            position: window::Position::Specific(iced::Point::new(1920.0 - PANEL_W as f32 - 20.0, 20.0)),
            resizable: false,
            decorations: false,
            transparent: true,
            blur: true,
            platform_specific: window::settings::PlatformSpecific {
                application_id: "hyprlink-hud".to_string(),
                ..Default::default()
            },
            ..Default::default()
        })
        .run()
}
