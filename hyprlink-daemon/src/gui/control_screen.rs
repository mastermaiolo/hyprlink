use super::*;
use iced::widget::column;

/// Só a lista de ids de workspace (não `id`/`name` completo) — o suficiente
/// pra desenhar a grelha e destacar a ativa via `modules.workspace`.
pub fn fetch_workspaces() -> Task<Message> {
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
pub fn fetch_control_context() -> Task<Message> {
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


pub fn control_screen(hud: &Hud) -> Element<'_, Message> {
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
                            border: Border { color: GLASS_BRD, width: 1.0, radius: 9.0.into() },
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
            background: Some(Background::Color(GREEN_BG)),
            border: Border { color: GREEN_BRD, width: 1.0, radius: 14.0.into() },
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
                border: Border { color: GLASS_BRD, width: 1.0, radius: 10.0.into() },
                icon: TEXT_2,
                placeholder: TEXT_3,
                value: TEXT,
                selection: Color { a: 0.35, ..GREEN },
            }),
        button(text("executar").size(11).color(GREEN))
            .padding([10, 16])
            .style(accent_button(GREEN, 10.0))
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
                    .style(accent_button(GREEN, 7.0))
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
            .style(ghost_button(TEXT_2, 8.0))
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

