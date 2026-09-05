use super::*;
use iced::widget::column;

pub fn battery_card(label: &'static str, pct: Option<i64>, charging: Option<bool>) -> Element<'static, Message> {
    let pct_text = pct.map(|p| format!("{p}%")).unwrap_or_else(|| "--%".to_string());
    let sub = match (pct, charging) {
        (Some(_), Some(true)) => "a carregar".to_string(),
        (Some(_), Some(false)) => "na bateria".to_string(),
        (Some(_), None) => "nível conhecido, carregando desconhecido".to_string(),
        (None, _) => "aguardando dado real".to_string(),
    };
    container(
        column![
            text(label).size(9).color(TEXT_2),
            text(pct_text).size(40).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(GREEN),
            text(sub).size(10).color(TEXT_4),
        ]
        .spacing(6),
    )
    .padding(18)
    .width(Length::Fill)
    .style(|_| container::Style { background: Some(Background::Color(GREEN_BG)), border: Border { color: GREEN_BRD, width: 1.0, radius: 14.0.into() }, ..Default::default() })
    .into()
}


/// Barras das últimas 12h — `history` já vem em ordem cronológica; reduz a
/// no máximo 12 amostras espaçadas igualmente (uma por "hora" aproximada,
/// já que a amostragem real é a cada ~5 min).
pub fn battery_chart(history: &[crate::state::BatterySample]) -> Element<'static, Message> {
    if history.is_empty() {
        return text("Sem histórico ainda nesta sessão — volte daqui a pouco.").size(10).color(TEXT_5).into();
    }
    let n = history.len();
    let buckets = 12.min(n);
    let bars = (0..buckets).map(|i| {
        let idx = i * (n - 1) / buckets.max(1).saturating_sub(1).max(1);
        let sample = &history[idx.min(n - 1)];
        let pct = sample.phone.or(sample.pc).unwrap_or(0).clamp(0, 100) as f32;
        Element::from(container(text("")).width(Length::Fill).height(Length::Fixed((pct / 100.0 * 90.0).max(2.0))).style(move |_| container::Style {
            background: Some(Background::Color(if pct <= 20.0 { AMBER_BRD } else { GREEN_BRD })),
            border: Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }))
    });
    row(bars).spacing(6).height(90).align_y(Alignment::End).into()
}


pub fn alert_toggle(label: &'static str, enabled: bool, on_toggle: Message) -> Element<'static, Message> {
    row![
        text(label).size(11).color(TEXT_1).width(Length::Fill),
        button(text(if enabled { "ativo" } else { "desligado" }).size(10).color(if enabled { GREEN } else { TEXT_5 }))
            .padding([5, 10])
            .style(move |_, _| button::Style {
                background: Some(Background::Color(if enabled { Color { a: 0.10, ..GREEN } } else { Color::TRANSPARENT })),
                border: Border { color: if enabled { Color { a: 0.35, ..GREEN } } else { BRD_1 }, width: 1.0, radius: 7.0.into() },
                text_color: if enabled { GREEN } else { TEXT_5 },
                ..Default::default()
            })
            .on_press(on_toggle),
    ]
    .align_y(Alignment::Center)
    .into()
}


pub fn batt_screen(hud: &Hud) -> Element<'_, Message> {
    let modules = &hud.snapshot.modules;
    let cards = row![
        battery_card("TELEMÓVEL", modules.phone_battery_pct, None),
        battery_card("PC", modules.pc_battery_pct, Some(modules.pc_battery_charging)),
    ]
    .spacing(14);

    let chart_card = container(
        column![
            row![text("ÚLTIMAS 12H").size(9).color(TEXT_2), Space::new().width(Length::Fill), text("telemóvel, quando disponível").size(9).color(TEXT_5)].align_y(Alignment::Center),
            battery_chart(&modules.battery_history),
        ]
        .spacing(14),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| glass(14.0));

    let alerts = config::battery_alerts(&hud.config);
    let alerts_card = container(
        column![
            text("ALERTAS NO DESKTOP").size(9).color(TEXT_2),
            alert_toggle("avisar abaixo de 20% (telemóvel)", alerts.low, Message::BattAlertToggle(config::BatteryAlertKind::Low, !alerts.low)),
            alert_toggle("avisar quando carregada a 100%", alerts.full, Message::BattAlertToggle(config::BatteryAlertKind::Full, !alerts.full)),
        ]
        .spacing(12),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| glass(14.0));

    let note = text("Temperatura, saúde e ciclos não existem no protocolo — só nível e carregamento chegam do telemóvel hoje.").size(10).color(TEXT_5);

    column![module_header("BATERIA", "Telemetria do telemóvel e do PC".to_string(), TEXT_2), cards, chart_card, alerts_card, note]
        .spacing(14)
        .into()
}

