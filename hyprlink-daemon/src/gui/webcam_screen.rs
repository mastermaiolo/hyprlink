use super::*;
use iced::widget::column;

// 2K aqui = 2560x1440 (QHD, o "2K" comum de consumo, não o 2048x1080 de
// cinema). Suporte real a 2K/4K/60fps depende do hardware de câmara do
// telemóvel (CameraX/MediaCodec) — o daemon aceita qualquer valor, só não
// tem como garantir que o telemóvel consiga entregar.
pub const WEBCAM_RESOLUTIONS: &[&str] = &["640x480", "1280x720", "1920x1080", "2560x1440", "3840x2160"];


pub const WEBCAM_FPS_OPTIONS: &[i64] = &[15, 24, 30, 60];


pub const WEBCAM_CODECS: &[&str] = &["h264", "h265"];


pub const WEBCAM_TEST_DURATION: Duration = Duration::from_secs(60);


/// Estado do teste de rede/hardware da webcam — transmite um stream de
/// verdade em 1080p30 (ver `suggest_webcam_config` sobre por que não testa
/// direto em 4K) por 60s (mesmo protocolo do stream normal, nada novo no
/// app) e mede a vazão real; combina com núcleos de CPU disponíveis
/// (decodificação hoje é só por software) pra sugerir a melhor configuração.
#[derive(Debug, Clone)]
pub enum WebcamTest {
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
pub fn suggest_webcam_config(mbps: f64) -> (&'static str, i64) {
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


pub fn webcam_screen(hud: &Hud) -> Element<'_, Message> {
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
        background: Background::Color(GLASS),
        border: Border { color: GLASS_BRD, width: 1.0, radius: 10.0.into() },
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
            .style(accent_button(RED, 10.0))
            .on_press(Message::WebcamStop)
    } else {
        button(text("iniciar stream").size(12).color(GREEN))
            .padding([10, 18])
            .style(accent_button(GREEN, 10.0))
            .on_press(Message::WebcamStart)
    };

    let mut action_row = row![action].spacing(10);
    if !modules.webcam_active && !is_testing {
        let test_btn = button(text("testar rede/hardware").size(12).color(TEXT_2))
            .padding([10, 18])
            .style(ghost_button(TEXT_2, 10.0))
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
                    .style(accent_button(GREEN, 8.0))
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

