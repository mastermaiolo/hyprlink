//! HUD flutuante (vidro fosco, cantos arredondados) via iced puro — janela
//! normal (não layer-shell), pra poder ser movida e ficar presa a uma única
//! workspace, com blur "de graça" pelo `decoration:blur` do Hyprland (que já
//! se aplica a qualquer janela com transparência, sem precisar de layerrule).
//! Acento colorido pelo estado da ligação (verde=conectado,
//! amarelo=conectando/pareando, vermelho=indisponível).
//!
//! ponytail: ícones SVG por módulo ficaram de fora desta primeira versão —
//! número + título já comunica bem. Adicionar quando fizer sentido.
//!
//! Segregado em `gui/*.rs` (uma tela por arquivo, `widgets.rs` com os
//! helpers reusados) e com os botões repetidos (`ghost_button()`,
//! `accent_button()` em `widgets.rs`) consolidados em 2026-09-05 — este
//! arquivo (`mod.rs`) fica só com o que é genuinamente compartilhado:
//! `Hud`/`Message`/`update`/`view`/`run`. Se `update()` (a maior função,
//! ~380 linhas) voltar a incomodar, o próximo corte natural é mover a
//! lógica de cada variante de `Message` específica de uma tela pro arquivo
//! daquela tela (`pub fn handle(hud, msg) -> Option<Task<Message>>`).

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
    AMBER, AMBER_BG, AMBER_BRD, BRD_1, DIVIDER, GLASS, GLASS_BRD, GREEN, GREEN_BG, GREEN_BRD, MUTED, RED, RED_BRD, TERMINAL, TEXT, TEXT_1, TEXT_2, TEXT_3,
    TEXT_4, TEXT_5, TEXT_6, WINDOW_BG, WINDOW_BRD,
};


mod i18n;
mod widgets;
mod clip_screen;
mod files_screen;
mod notif_screen;
mod media_screen;
mod batt_screen;
mod control_screen;
mod audio_screen;
mod webcam_screen;
mod track_screen;
mod settings_screen;

use i18n::*;
use widgets::*;
use clip_screen::*;
use files_screen::*;
use notif_screen::*;
use media_screen::*;
use batt_screen::*;
use control_screen::*;
use audio_screen::*;
use webcam_screen::*;
use track_screen::*;
use settings_screen::*;

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
    /// Idioma usado na última reconstrução do console — junto com
    /// `console_len`, força reconstrução também ao trocar de idioma (não só
    /// quando chega linha nova), senão o log já escrito ficaria "preso" no
    /// idioma antigo até o próximo `push_log`.
    console_lang: config::Lang,
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
    ///
    /// ponytail: só 3 chaves possíveis ("ring"/"media"/"alarm"), conhecidas
    /// em tempo de compilação — um HashMap aqui é mais flexível do que
    /// precisa (e uma string errada falha calada). Upgrade: struct com os
    /// 3 campos `Option<i64>` se algum dia doer.
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
    SetLang(config::Lang),
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
        let console_lang = config::lang(&config);
        i18n::set_current(console_lang);
        let console = text_editor::Content::with_text(&snapshot.logs.iter().map(|l| i18n::tr_log(l)).collect::<Vec<_>>().join("\n"));
        let download_dir = config::download_dir(&config);
        Self {
            shared,
            pairing,
            start_time: std::time::Instant::now(),
            snapshot,
            console,
            console_lang,
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
            let current_lang = config::lang(&hud.config);
            if hud.snapshot.logs.len() != hud.console_len || current_lang != hud.console_lang {
                hud.console_len = hud.snapshot.logs.len();
                hud.console_lang = current_lang;
                i18n::set_current(current_lang);
                hud.console = text_editor::Content::with_text(&hud.snapshot.logs.iter().map(|l| i18n::tr_log(l)).collect::<Vec<_>>().join("\n"));
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
        Message::SetLang(lang) => {
            config::set_lang(&hud.config, lang);
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


fn view(hud: &Hud) -> Element<'_, Message> {
    i18n::set_current(config::lang(&hud.config));
    let accent = hud.accent();
    let elapsed = hud.start_time.elapsed().as_secs_f32();

    // "HyprLink" é uma palavra só — HYPR e LINK não podem ter o mesmo
    // espaçamento do ponto de estado, senão lê como duas palavras.
    let wordmark = row![
        text("HYPR").size(21).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(TEXT),
        text("LINK").size(21).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(accent),
    ]
    .spacing(0);
    let logo = row![
        container(breathing_dot(accent, 8.0, elapsed)).width(14).height(14).align_x(Alignment::Center).align_y(Alignment::Center),
        wordmark,
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
        .style(ghost_button(TEXT_2, 8.0))
        .on_press(Message::Quit);

    let minimize_btn = button(text("–").size(16).color(TEXT_2))
        .padding([2, 9])
        .style(ghost_button(TEXT_2, 8.0))
        .on_press(Message::MinimizeToTray);

    let mut header_right_row = row![].spacing(8).align_y(Alignment::Center);
    if hud.screen != Screen::Dashboard {
        let back = button(text(t("‹ VOLTAR")).size(11).color(TEXT_2))
            .padding([6, 12])
            .style(ghost_button(TEXT_2, 8.0))
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
                text(t("console de diagnóstico")).size(9).color(TEXT_2),
                Space::new().width(Length::Fill),
                button(text(t("copiar log")).size(9).color(TEXT_2))
                    .padding([4, 10])
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))),
                        border: Border { color: GLASS_BRD, width: 1.0, radius: 8.0.into() },
                        text_color: TEXT_2,
                        ..Default::default()
                    })
                    .on_press(Message::CopyText(hud.snapshot.logs.join("\n"))),
            ]
            .align_y(Alignment::Center);

            row![
                column![connbar, module_list(&hud.download_dir, &hud.snapshot.modules, true)].spacing(14),
                container(column![console_header, console(&hud.console)].spacing(8))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(18)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(GLASS)),
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
                    text(t("APONTE A CÂMARA DO TELEMÓVEL")).size(11).color(TEXT_2),
                    pairing_qr(&payload),
                    text(t("Abra o HyprLink no Android e escaneie o código")).size(10).color(TEXT_2),
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
                background: Some(Background::Color(GLASS)),
                border: Border { color: Color { a: 0.35, ..AMBER }, width: 1.0, radius: 14.0.into() },
                ..Default::default()
            });

            row![module_list(&hud.download_dir, &hud.snapshot.modules, false), pairing_card].spacing(14).height(Length::Fill).into()
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


const PANEL_W: u32 = 900;


const PANEL_H: u32 = 800;

