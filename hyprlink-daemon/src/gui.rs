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

use iced::widget::{button, column, container, row, scrollable, text, text_editor, Space};
use iced::window;
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Shadow, Task, Theme, Vector};

use crate::active::ActiveConn;
use crate::config::{self, SharedConfig};
use crate::state::{ConnState, HudState};

const GREEN: Color = Color::from_rgb(0.220, 1.0, 0.612);
const AMBER: Color = Color::from_rgb(1.0, 0.694, 0.231);
const RED: Color = Color::from_rgb(1.0, 0.231, 0.361);
const TEXT: Color = Color::from_rgb(0.929, 0.929, 0.937);
const TEXT_2: Color = Color::from_rgb(0.604, 0.604, 0.643);

const PANEL_W: u32 = 900;
const PANEL_H: u32 = 800;

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
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    Quit,
    CopyText(String),
    ConsoleAction(text_editor::Action),
    PickDownloadDir,
    DownloadDirPicked(Option<PathBuf>),
}

impl Hud {
    fn new(shared: Arc<Mutex<HudState>>, config: SharedConfig, active: ActiveConn) -> Self {
        let snapshot = shared.lock().unwrap().clone();
        let console_len = snapshot.logs.len();
        let console = text_editor::Content::with_text(&snapshot.logs.join("\n"));
        let download_dir = config::download_dir(&config);
        Self { shared, snapshot, console, console_len, config, download_dir, active }
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
    }
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

fn module_row(num: &str, title: &str, sub: &str, status: (&'static str, Color)) -> Element<'static, Message> {
    let (label, color) = status;
    container(
        row![
            text(num.to_string()).size(9).color(TEXT_2).width(16),
            column![
                text(title.to_string()).size(12).color(TEXT).font(Font::MONOSPACE),
                text(sub.to_string()).size(9).color(TEXT_2),
            ]
            .spacing(2)
            .width(Length::Fill),
            text(label.to_string()).size(9).color(color),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([7, 10])
    .style(|_| glass(10.0))
    .into()
}

/// A linha de FILES é clicável: abre o seletor de pasta nativo (portal XDG)
/// pra escolher onde os ficheiros recebidos são salvos.
fn files_row(download_dir: &std::path::Path) -> Element<'static, Message> {
    let sub = format!("Recebe em {}", download_dir.display());
    button(
        row![
            text("02").size(9).color(TEXT_2).width(16),
            column![
                text("FILES").size(12).color(TEXT).font(Font::MONOSPACE),
                text(sub).size(9).color(TEXT_2),
            ]
            .spacing(2)
            .width(Length::Fill),
            text("alterar").size(9).color(TEXT_2),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([7, 10])
    .style(|_, _| button::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
        border: Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.08), width: 1.0, radius: 10.0.into() },
        text_color: TEXT,
        ..Default::default()
    })
    .on_press(Message::PickDownloadDir)
    .into()
}

fn module_list(download_dir: &std::path::Path) -> Element<'static, Message> {
    let off = ("OFF", RED);
    let rows = column![
        module_row("01", "CLIP", "Área de transferência", off),
        files_row(download_dir),
        module_row("03", "NOTIF", "Espelhamento", off),
        module_row("04", "MEDIA", "Nenhum leitor ativo", off),
        module_row("05", "BATT", "Telemetria de energia", off),
        module_row("06", "CONTROL", "Hyprland IPC", off),
        module_row("07", "AUDIO", "Mixer e audio tap", off),
        module_row("08", "WEBCAM", "Câmara remota do PC", off),
        module_row("09", "TRACK", "Rato e teclado virtual", off),
        module_row("10", "CONFIG", "Permissões e dispositivos", off),
    ]
    .spacing(7);
    scrollable(rows).width(296).height(Length::Fill).into()
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

    let header = row![
        logo,
        Space::new().width(Length::Fill),
        status_pill(status_color, status_label),
        close_btn
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let body: Element<'_, Message> = match &hud.snapshot.conn {
        ConnState::Connected { device_name, fingerprint_hex } => {
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
                column![connbar, module_list(&hud.download_dir)].spacing(14),
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

            row![module_list(&hud.download_dir), pairing_card].spacing(14).height(Length::Fill).into()
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

pub fn run(shared: Arc<Mutex<HudState>>, config: SharedConfig, active: ActiveConn) -> iced::Result {
    iced::application(move || Hud::new(shared.clone(), config.clone(), active.clone()), update, view)
        .title("HyprLink")
        .style(style)
        .subscription(subscription)
        .window(window::Settings {
            size: iced::Size::new(PANEL_W as f32, PANEL_H as f32),
            position: window::Position::Centered,
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
