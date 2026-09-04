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

use iced::widget::{button, column, container, row, scrollable, slider, text, text_editor, text_input, Space};
use iced::window;
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Shadow, Task, Theme, Vector};

use crate::active::ActiveConn;
use crate::clip;
use crate::config::{self, SharedConfig};
use crate::state::{ConnState, HudState};

// Tokens de cor alinhados com o app Android (code-spec-iced.md §1).
const GREEN: Color = Color::from_rgb(0x3D as f32 / 255.0, 0xFF as f32 / 255.0, 0x9E as f32 / 255.0);
const AMBER: Color = Color::from_rgb(0xFF as f32 / 255.0, 0xB0 as f32 / 255.0, 0x20 as f32 / 255.0);
const RED: Color = Color::from_rgb(0xFF as f32 / 255.0, 0x47 as f32 / 255.0, 0x57 as f32 / 255.0);
const MUTED: Color = Color::from_rgb(0x5E as f32 / 255.0, 0x5E as f32 / 255.0, 0x5E as f32 / 255.0);
const TEXT: Color = Color::from_rgb(0.929, 0.929, 0.937);
const TEXT_2: Color = Color::from_rgb(0.604, 0.604, 0.643);
const TEXT_3: Color = Color::from_rgb(0x8A as f32 / 255.0, 0x8A as f32 / 255.0, 0x8A as f32 / 255.0);

const PANEL_W: u32 = 900;
const PANEL_H: u32 = 800;

/// Só os módulos com backend 100% real ganham tela própria por ora.
/// CLIP/NOTIF têm histórico em memória (reseta ao reiniciar — persistência
/// em disco é Fase C); AUDIO precisa do VU meter e lista de apps do mixer
/// (mais plumbing); WEBCAM/TRACK não têm backend; FILES já tem sua própria
/// ação direta (seletor de pasta) em vez de tela.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ModuleId {
    Clip,
    Notif,
    Batt,
    Control,
    Media,
    Audio,
    Webcam,
    Config,
}

#[derive(Debug, Clone, PartialEq)]
enum Screen {
    Dashboard,
    Module(ModuleId),
}

struct Hud {
    shared: Arc<Mutex<HudState>>,
    snapshot: HudState,
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
    webcam_resolution: &'static str,
    webcam_fps: i64,
    webcam_codec: &'static str,
    webcam_test: WebcamTest,
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
    Quit,
    MinimizeToTray,
    CopyText(String),
    ConsoleAction(text_editor::Action),
    PickDownloadDir,
    DownloadDirPicked(Option<PathBuf>),
    PickFileToSend,
    FileToSendPicked(Option<PathBuf>),
    OpenModule(ModuleId),
    Back,
    MediaCommand(&'static str),
    HyprInputChanged(String),
    HyprDispatch,
    HyprDispatchResult(String),
    HyprWorkspacesLoaded(Vec<i64>),
    WorkspaceClicked(i64),
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
}

impl Hud {
    fn new(shared: Arc<Mutex<HudState>>, config: SharedConfig, active: ActiveConn, pending_webcam: crate::webcam::PendingWebcam, tray_show: crate::tray::ShowRequested) -> Self {
        let snapshot = shared.lock().unwrap().clone();
        let console_len = snapshot.logs.len();
        let console = text_editor::Content::with_text(&snapshot.logs.join("\n"));
        let download_dir = config::download_dir(&config);
        Self {
            shared,
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
            webcam_resolution: "1280x720",
            webcam_fps: 24,
            webcam_codec: "h264",
            webcam_test: WebcamTest::Idle,
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
        Message::MinimizeToTray => {
            crate::state::push_log(&hud.shared, "[i] tray: minimizando".to_string());
            Task::perform(async { tokio::task::spawn_blocking(crate::hypr::tray_hide).await.unwrap_or(false) }, |_| Message::Tick)
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
                return fetch_workspaces();
            }
            if id == ModuleId::Audio {
                let active = hud.active.clone();
                return Task::batch([fetch_audio(), fetch_phone_audio(active)]);
            }
            Task::none()
        }
        Message::Back => {
            hud.screen = Screen::Dashboard;
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

fn subscription(_hud: &Hud) -> iced::Subscription<Message> {
    iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick)
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

fn status_pill(accent: Color, label: &str) -> Element<'static, Message> {
    container(
        row![
            container(text("")).width(6).height(6).style(move |_| container::Style {
                background: Some(Background::Color(accent)),
                border: Border { radius: 999.0.into(), ..Default::default() },
                ..Default::default()
            }),
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
/// (code-tarefas.md, decisão 2). `NotImplemented` e `Idle` são visualmente
/// iguais (cinza), só o texto do subtítulo muda.
enum ModuleState {
    /// Módulo com atividade real recente — ponto verde, subtítulo é o dado.
    Active(String),
    /// Implementado, mas sem dados ainda (ex: nenhuma notificação chegou).
    Idle(&'static str),
    /// Ainda não implementado no daemon (webcam, track).
    NotImplemented,
}

/// `open` = `Some(id)` pros 4 módulos que já têm tela própria (Fase B) —
/// vira um botão que abre o ecrã do módulo. Os demais continuam só exibindo
/// o estado, sem clique (telas próprias ficam pra Fase C/D, ver `ModuleId`).
fn module_row(num: &str, title: &str, state: ModuleState, open: Option<ModuleId>) -> Element<'static, Message> {
    let (dot, sub, sub_color): (Color, String, Color) = match state {
        ModuleState::Active(sub) => (GREEN, sub, TEXT_2),
        ModuleState::Idle(sub) => (MUTED, sub.to_string(), TEXT_3),
        ModuleState::NotImplemented => (MUTED, "Não implementado".to_string(), TEXT_3),
    };
    let content = row![
        text(num.to_string()).size(9).color(TEXT_2).width(16),
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
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    match open {
        Some(id) => button(content)
            .padding([7, 10])
            .width(Length::Fill)
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 10.0.into() },
                text_color: TEXT,
                ..Default::default()
            })
            .on_press(Message::OpenModule(id))
            .into(),
        None => container(content).padding([7, 10]).style(|_| glass(10.0)).into(),
    }
}

/// Linha de FILES com duas ações diretas (sem tela própria, ver
/// code-tarefas.md): "enviar" escolhe um ficheiro e manda pro telemóvel,
/// "alterar" troca a pasta onde os ficheiros recebidos são salvos.
fn files_row(download_dir: &std::path::Path) -> Element<'static, Message> {
    let sub = format!("Recebe em {}", download_dir.display());
    let action_btn = |label: &'static str, msg: Message| {
        button(text(label).size(9).color(TEXT_2))
            .padding([5, 9])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))),
                border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.1), width: 1.0, radius: 7.0.into() },
                text_color: TEXT_2,
                ..Default::default()
            })
            .on_press(msg)
    };
    container(
        row![
            text("02").size(9).color(TEXT_2).width(16),
            column![
                text("FILES").size(12).color(TEXT).font(Font::MONOSPACE),
                text(sub).size(9).color(TEXT_2),
            ]
            .spacing(2)
            .width(Length::Fill),
            action_btn("enviar", Message::PickFileToSend),
            action_btn("alterar", Message::PickDownloadDir),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([7, 10])
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
        border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 10.0.into() },
        ..Default::default()
    })
    .into()
}

fn module_list(download_dir: &std::path::Path, modules: &crate::state::ModuleStatus) -> Element<'static, Message> {
    let clip_state = match modules.clip_history.first() {
        Some(entry) => ModuleState::Active(format!("{}: {}", entry.direction, clip::preview(&entry.text))),
        None => ModuleState::Idle("Nada sincronizado ainda"),
    };
    let notif_state = if modules.notif_count > 0 {
        ModuleState::Active(format!("{} espelhada(s) nesta sessão", modules.notif_count))
    } else {
        ModuleState::Idle("Nenhuma notificação ainda")
    };
    let media_state = match &modules.media {
        Some(status) => ModuleState::Active(status.clone()),
        None => ModuleState::Idle("Nenhum leitor ativo"),
    };
    let batt_state = match modules.phone_battery_pct {
        Some(pct) => ModuleState::Active(format!("Telemóvel em {pct}%")),
        None => ModuleState::Idle("Aguardando bateria do telemóvel"),
    };
    let control_state = match &modules.workspace {
        Some(ws) => ModuleState::Active(format!("Workspace {ws}")),
        None => ModuleState::Idle("Hyprland IPC"),
    };
    let audio_state = if modules.audio_tap_active {
        ModuleState::Active("Audio tap ativo".to_string())
    } else {
        ModuleState::Idle("Mixer e audio tap")
    };
    let webcam_state = if modules.webcam_active {
        ModuleState::Active("Stream ativo · /dev/video42".to_string())
    } else {
        ModuleState::Idle("Câmara remota do PC")
    };

    let rows = column![
        module_row("01", "CLIP", clip_state, Some(ModuleId::Clip)),
        files_row(download_dir),
        module_row("03", "NOTIF", notif_state, Some(ModuleId::Notif)),
        module_row("04", "MEDIA", media_state, Some(ModuleId::Media)),
        module_row("05", "BATT", batt_state, Some(ModuleId::Batt)),
        module_row("06", "CONTROL", control_state, Some(ModuleId::Control)),
        module_row("07", "AUDIO", audio_state, Some(ModuleId::Audio)),
        module_row("08", "WEBCAM", webcam_state, Some(ModuleId::Webcam)),
        module_row("09", "TRACK", ModuleState::NotImplemented, None),
        module_row("10", "CONFIG", ModuleState::Idle("Permissões e dispositivos"), Some(ModuleId::Config)),
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
fn module_ruler(active: ModuleId) -> Element<'static, Message> {
    let entry = |num: &'static str, is_active: bool| -> Element<'static, Message> {
        container(text(num).size(10).color(if is_active { GREEN } else { TEXT_3 }))
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
            })
            .into()
    };
    column![
        entry("01", active == ModuleId::Clip),
        entry("02", false),
        entry("03", active == ModuleId::Notif),
        entry("04", active == ModuleId::Media),
        entry("05", active == ModuleId::Batt),
        entry("06", active == ModuleId::Control),
        entry("07", active == ModuleId::Audio),
        entry("08", active == ModuleId::Webcam),
        entry("09", false),
        entry("10", active == ModuleId::Config),
    ]
    .spacing(8)
    .width(44)
    .align_x(Alignment::Center)
    .into()
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
        ModuleId::Clip => clip_screen(&hud.snapshot.modules),
        ModuleId::Notif => notif_screen(&hud.snapshot.modules),
        ModuleId::Batt => batt_screen(&hud.snapshot.modules),
        ModuleId::Control => control_screen(hud),
        ModuleId::Media => media_screen(&hud.snapshot.modules),
        ModuleId::Audio => audio_screen(hud),
        ModuleId::Webcam => webcam_screen(hud),
        ModuleId::Config => config_screen(&hud.snapshot),
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
                    background: Some(Background::Color(Color::from_rgba(0x14 as f32 / 255.0, 0x14 as f32 / 255.0, 0x14 as f32 / 255.0, 1.0))),
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

fn clip_screen(modules: &crate::state::ModuleStatus) -> Element<'static, Message> {
    let rows = modules
        .clip_history
        .iter()
        .map(|entry| {
            container(
                column![
                    row![
                        text(entry.direction).size(9).color(GREEN),
                        Space::new().width(Length::Fill),
                        text(entry.at.clone()).size(9).color(TEXT_3),
                    ],
                    text(entry.text.clone()).size(11).color(TEXT).line_height(text::LineHeight::Relative(1.4)),
                ]
                .spacing(4),
            )
            .padding([10, 12])
            .width(Length::Fill)
            .into()
        })
        .collect();
    column![
        module_header("CLIP", format!("{} no histórico desta sessão", modules.clip_history.len()), TEXT_2),
        history_list(rows, "Nada sincronizado ainda nesta sessão."),
    ]
    .spacing(16)
    .height(Length::Fill)
    .into()
}

fn notif_screen(modules: &crate::state::ModuleStatus) -> Element<'static, Message> {
    let rows = modules
        .notif_history
        .iter()
        .map(|entry| {
            container(
                row![
                    text(entry.app.clone()).size(9).color(MUTED).width(64),
                    column![
                        row![
                            text(entry.title.clone()).size(11).color(TEXT).width(Length::Fill),
                            text(entry.at.clone()).size(9).color(TEXT_3),
                        ]
                        .align_y(Alignment::Start),
                        text(entry.text.clone()).size(11).color(TEXT_3).line_height(text::LineHeight::Relative(1.4)),
                    ]
                    .spacing(3)
                    .width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Start),
            )
            .padding([10, 12])
            .width(Length::Fill)
            .into()
        })
        .collect();
    column![
        module_header("NOTIF", format!("{} espelhada(s) nesta sessão", modules.notif_count), TEXT_2),
        history_list(rows, "Nenhuma notificação espelhada ainda nesta sessão."),
    ]
    .spacing(16)
    .height(Length::Fill)
    .into()
}

fn batt_screen(modules: &crate::state::ModuleStatus) -> Element<'static, Message> {
    let (pct_text, sub) = match modules.phone_battery_pct {
        Some(pct) => (format!("{pct}%"), "Bateria do telemóvel (só nível e carregamento — o protocolo não envia mais telemetria hoje)".to_string()),
        None => ("--%".to_string(), "Aguardando o telemóvel enviar o primeiro battery.state".to_string()),
    };
    let card = container(
        column![
            text(pct_text).size(56).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(GREEN),
            text(sub).size(11).color(TEXT_2),
        ]
        .spacing(8),
    )
    .padding(20)
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x15 as f32 / 255.0, 0x10 as f32 / 255.0, 1.0))),
        border: Border { color: Color::from_rgba(0x1E as f32 / 255.0, 0x3E as f32 / 255.0, 0x30 as f32 / 255.0, 1.0), width: 1.0, radius: 14.0.into() },
        ..Default::default()
    });
    column![module_header("BATT", "Telemetria do telemóvel".to_string(), TEXT_2), card].spacing(16).into()
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

    scrollable(
        column![
            module_header("AUDIO", "Mixer do PC e do telemóvel".to_string(), TEXT_3),
            text("SAÍDAS (PC)").size(9).color(TEXT_2),
            sinks,
            text("APPS (PC)").size(9).color(TEXT_2),
            apps,
            phone_audio_section(hud),
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

    column![module_header("WEBCAM", "Câmara remota do PC".to_string(), status_color), preview, config_row, action_row, test_panel, note]
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

    column![module_header("CONTROL", "Hyprland IPC".to_string(), GREEN), card, dispatch_row, result].spacing(16).into()
}

fn config_screen(snapshot: &HudState) -> Element<'static, Message> {
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

    let note = text("Lista de dispositivos já pareados (com revogar), nível de log e retenção de histórico ainda não têm UI — dá pra reparear via QR se precisar trocar de telemóvel.")
        .size(10)
        .color(TEXT_3);

    column![module_header("CONFIG", "Rede e dispositivo ligado".to_string(), TEXT_2), device_card, list, note]
        .spacing(16)
        .into()
}

fn view(hud: &Hud) -> Element<'_, Message> {
    let accent = hud.accent();

    let logo = row![
        text("HYPR").size(21).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(TEXT),
        text("LINK").size(21).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(accent),
    ];

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

    let header_right: Element<'_, Message> = if hud.screen != Screen::Dashboard {
        let back = button(text("‹ VOLTAR").size(11).color(TEXT_2))
            .padding([6, 12])
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
                border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 8.0.into() },
                text_color: TEXT_2,
                ..Default::default()
            })
            .on_press(Message::Back);
        row![back, minimize_btn, close_btn].spacing(8).align_y(Alignment::Center).into()
    } else {
        row![status_pill(status_color, status_label), minimize_btn, close_btn].spacing(8).align_y(Alignment::Center).into()
    };

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
            background: Some(Background::Color(Color::from_rgba(0.031, 0.035, 0.047, 0.62))),
            border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.10), width: 1.0, radius: 28.0.into() },
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

pub fn run(shared: Arc<Mutex<HudState>>, config: SharedConfig, active: ActiveConn, pending_webcam: crate::webcam::PendingWebcam, tray_show: crate::tray::ShowRequested) -> iced::Result {
    iced::application(move || Hud::new(shared.clone(), config.clone(), active.clone(), pending_webcam.clone(), tray_show.clone()), update, view)
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
