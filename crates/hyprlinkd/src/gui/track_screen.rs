use super::*;
use iced::widget::column;

/// Posição do cursor + resolução do monitor focado — só pro espelho do
/// TRACK, buscados sob demanda (abrir a tela / a cada tick de `TrackPoll`).
pub fn fetch_cursor_pos() -> Task<Message> {
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


/// TRACK (`09`): rato/teclado virtual via `/dev/uinput` — já real e validado
/// (Fase 3), só faltava esta tela. Espelho do cursor e sliders de
/// sensibilidade/scroll aplicados de verdade entram no Estágio 4.
/// Grelha simples (16×9) marcando a célula onde o cursor está agora —
/// aproximação leve do "espelho do cursor" do mockup sem precisar de
/// `canvas` (posicionamento livre de pixel não é trivial em iced 0.14 fora
/// dele). `size` é a resolução do monitor focado.
pub fn cursor_grid(cursor: Option<(i64, i64)>, size: (i64, i64)) -> Element<'static, Message> {
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
                text(t("ESPELHO DO CURSOR")).size(9).color(TEXT_2),
                Space::new().width(Length::Fill),
                text(format!("{}×{}", size.0, size.1)).size(9).color(TEXT_5),
            ]
            .align_y(Alignment::Center),
            grid,
            text(cursor.map(|(x, y)| format!("x {x} · y {y}")).unwrap_or_else(|| t("aguardando…").to_string())).size(9).color(TEXT_5),
        ]
        .spacing(10),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| container::Style { background: Some(Background::Color(TERMINAL)), border: Border { radius: 14.0.into(), ..Default::default() }, ..Default::default() })
    .into()
}


pub fn track_slider(label: &'static str, value: f32, display: String, range: std::ops::RangeInclusive<f32>, on_change: impl Fn(f32) -> Message + 'static) -> Element<'static, Message> {
    column![
        row![text(t(label)).size(11).color(TEXT_1), Space::new().width(Length::Fill), text(display).size(10).color(TEXT_3)].align_y(Alignment::Center),
        slider(range, value, on_change).step(0.1_f32),
    ]
    .spacing(9)
    .into()
}


pub fn track_toggle(label: &'static str, enabled: bool, on_toggle: Message) -> Element<'static, Message> {
    button(text(format!("{} {}", t(label), if enabled { "✓" } else { "" })).size(11).color(if enabled { TEXT_1 } else { TEXT_4 }))
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


pub fn track_screen(hud: &Hud) -> Element<'_, Message> {
    let ts = config::track_settings(&hud.config);
    let mirror = cursor_grid(hud.cursor_pos, hud.screen_size);

    let sliders = container(
        column![
            track_slider("SENSIBILIDADE", ts.sensitivity, format!("{:.1}×", ts.sensitivity), 0.2..=3.0, Message::TrackSensitivity),
            track_slider("VELOCIDADE DE SCROLL", ts.scroll_speed, format!("{:.1}×", ts.scroll_speed), 0.2..=3.0, Message::TrackScrollSpeed),
        ]
        .spacing(16),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| glass(14.0));

    let toggles = row![
        track_toggle("aceleração", ts.acceleration, Message::TrackAcceleration(!ts.acceleration)),
        track_toggle("inverter scroll", ts.invert_scroll, Message::TrackInvertScroll(!ts.invert_scroll)),
        track_toggle("teclado virtual", ts.virtual_keyboard, Message::TrackVirtualKeyboard(!ts.virtual_keyboard)),
    ]
    .spacing(10);

    let note = text(t("Movimento, cliques e scroll chegam do telemóvel via /dev/uinput — os sliders acima já se aplicam de verdade.")).size(10).color(TEXT_5);

    column![module_header("TRACK", t("Rato e teclado virtuais").to_string(), TEXT_2), mirror, sliders, toggles, note]
        .spacing(16)
        .into()
}

