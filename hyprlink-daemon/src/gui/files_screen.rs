use super::*;
use iced::widget::column;

/// FILES (`02`): pasta de destino + envio de ficheiro (real, `rfd`) hoje;
/// progresso ao vivo e histórico de transferências entram no Estágio 3
/// (precisam de `share.rs` passar a escrever em `HudState`, hoje só loga).
pub fn files_screen(hud: &Hud) -> Element<'_, Message> {
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
                .style(ghost_button(TEXT_2, 8.0))
                .on_press(Message::PickDownloadDir),
        ]
        .align_y(Alignment::Center),
    )
    .padding([13, 16])
    .width(Length::Fill)
    .style(|_| glass(12.0));

    let send_btn = button(text("enviar ficheiro…").size(12).color(GREEN))
        .padding([10, 18])
        .style(accent_button(GREEN, 10.0))
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

