use super::*;
use iced::widget::column;

pub fn config_screen(hud: &Hud) -> Element<'static, Message> {
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
            background: Some(Background::Color(GREEN_BG)),
            border: Border { color: GREEN_BRD, width: 1.0, radius: 14.0.into() },
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
        .style(ghost_button(TEXT_2, 8.0))
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

