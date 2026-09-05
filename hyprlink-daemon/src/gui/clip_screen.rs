use super::*;
use iced::widget::column;

pub fn clip_screen(hud: &Hud) -> Element<'_, Message> {
    let modules = &hud.snapshot.modules;
    let query = &hud.clip_search;
    let mut visible: Vec<&crate::state::ClipEntry> = modules.clip_history.iter().filter(|e| matches_search(&e.text, query)).collect();
    visible.sort_by_key(|e| !e.pinned); // fixados primeiro, mantém ordem relativa (stable sort)

    let action_btn = |label: &'static str, msg: Message| {
        button(text(label).size(10).color(TEXT_2))
            .padding([6, 11])
            .style(ghost_button(TEXT_2, 7.0))
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

