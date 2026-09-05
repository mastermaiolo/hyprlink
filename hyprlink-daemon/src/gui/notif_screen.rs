use super::*;
use iced::widget::column;

pub fn notif_screen(hud: &Hud) -> Element<'_, Message> {
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

    let mut chips = row![filter_chip(t("todas").to_string(), modules.notif_history.len(), hud.notif_app_filter.is_none(), Message::NotifAppFilter(None))].spacing(7);
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
                    button(text(t("copiar")).size(9).color(TEXT_4))
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
        text(t1("HISTÓRICO · {} NESTA SESSÃO", modules.notif_history.len())).size(9).color(TEXT_5),
        Space::new().width(Length::Fill),
        search_row(query, Message::NotifSearch, Message::NotifSearch(String::new())),
    ]
    .align_y(Alignment::Center);

    column![
        module_header("NOTIFICAÇÕES", t1("{} espelhada(s) nesta sessão", modules.notif_count), TEXT_2),
        scrollable(chips).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(2).scroller_width(2))),
        header_row,
        history_list(rows, "Nenhuma notificação espelhada ainda nesta sessão."),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}

