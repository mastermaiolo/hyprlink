use super::*;
use iced::widget::column;

pub fn fetch_audio() -> Task<Message> {
    Task::perform(async { tokio::task::spawn_blocking(crate::audio::snapshot).await.unwrap_or_default() }, Message::AudioLoaded)
}


pub fn fetch_phone_audio(active: ActiveConn) -> Task<Message> {
    Task::perform(async move { crate::phone_audio::get_state(&active).await.unwrap_or_default() }, Message::PhoneAudioLoaded)
}


pub fn sink_row(s: &crate::audio::SinkInfo) -> Element<'static, Message> {
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
                .style(accent_button(GREEN, 8.0))
                .on_press(Message::AudioSetDefaultSink(name)),
        );
    }

    container(controls).padding(10).width(Length::Fill).style(|_| glass(10.0)).into()
}


pub fn app_row(a: &crate::audio::AppInfo) -> Element<'static, Message> {
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
pub fn phone_volume_row(label: &'static str, stream: &'static str, percent: i64) -> Element<'static, Message> {
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


pub fn ringer_mode_button(label: &'static str, mode: &'static str, active_mode: &str) -> Element<'static, Message> {
    let is_active = active_mode == mode;
    button(text(label).size(11).color(if is_active { GREEN } else { TEXT_2 }))
        .padding([8, 14])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if is_active { Color { a: 0.10, ..GREEN } } else { GLASS })),
            border: Border { color: if is_active { Color { a: 0.35, ..GREEN } } else { GLASS_BRD }, width: 1.0, radius: 8.0.into() },
            text_color: if is_active { GREEN } else { TEXT_2 },
            ..Default::default()
        })
        .on_press(Message::PhoneAudioSetRingerMode(mode))
        .into()
}


pub fn dnd_button(enabled: bool) -> Element<'static, Message> {
    let color = if enabled { AMBER } else { TEXT_2 };
    button(text(if enabled { "não perturbe: ligado" } else { "não perturbe: desligado" }).size(11).color(color))
        .padding([8, 14])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if enabled { Color { a: 0.10, ..AMBER } } else { GLASS })),
            border: Border { color: if enabled { Color { a: 0.35, ..AMBER } } else { GLASS_BRD }, width: 1.0, radius: 8.0.into() },
            text_color: color,
            ..Default::default()
        })
        .on_press(Message::PhoneAudioSetDnd(!enabled))
        .into()
}


pub fn phone_audio_section(hud: &Hud) -> Element<'_, Message> {
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


pub fn audio_screen(hud: &Hud) -> Element<'_, Message> {
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

    // Microfone do telemóvel: função independente da webcam — pode ligar
    // por aqui (pede pro telemóvel) ou por lá (botão no telemóvel), os dois
    // convergem no mesmo estado real (`mic_active`).
    let mic_active = hud.snapshot.modules.mic_active;
    let mic_status = container(
        column![
            checkbox(mic_active)
                .label(if mic_active { "🎙️ microfone do telemóvel: ativo" } else { "🎙️ microfone do telemóvel: desligado" })
                .on_toggle(Message::PhoneMicToggle)
                .size(16)
                .text_size(11),
            if mic_active {
                Element::from(text("Selecione \"HyprLink-Mic\" como entrada de áudio em qualquer app.").size(10).color(TEXT_2))
            } else {
                Element::from(iced::widget::Space::new())
            },
        ]
        .spacing(6),
    )
    .padding(10)
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(if mic_active { Color { a: 0.10, ..GREEN } } else { GLASS })),
        border: Border { color: if mic_active { Color { a: 0.35, ..GREEN } } else { GLASS_BRD }, width: 1.0, radius: 10.0.into() },
        ..Default::default()
    });

    let tap_active = hud.snapshot.modules.audio_tap_active;
    let vu: Element<'_, Message> = if tap_active {
        let bars = hud.snapshot.modules.audio_vu.iter().map(|&pct| {
            let above = pct as f32 >= 60.0;
            Element::from(
                container(text("")).width(Length::Fill).height(Length::Fixed((pct as f32 / 100.0 * 56.0).max(2.0))).style(move |_| container::Style {
                    background: Some(Background::Color(if above { GREEN } else { GREEN_BRD })),
                    ..Default::default()
                }),
            )
        });
        row(bars).spacing(3).height(56).align_y(Alignment::End).into()
    } else {
        Space::new().height(56).into()
    };
    let tap_meta = match (tap_active, crate::state::audio_tap_elapsed_secs(&hud.shared)) {
        (true, Some(secs)) => format!("{} KB enviados · {secs}s", hud.snapshot.modules.audio_tap_bytes / 1024),
        _ => "Tap parado".to_string(),
    };
    let tap_card = container(
        column![row![text("SAÍDA ENCAMINHADA").size(9).color(if tap_active { GREEN } else { TEXT_2 }), Space::new().width(Length::Fill), text(tap_meta).size(9).color(TEXT_4)].align_y(Alignment::Center), vu]
            .spacing(14),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(if tap_active { GREEN_BG } else { Color::from_rgba(1.0, 1.0, 1.0, 0.025) })),
        border: Border { color: if tap_active { GREEN_BRD } else { BRD_1 }, width: 1.0, radius: 14.0.into() },
        ..Default::default()
    });

    scrollable(
        column![
            module_header("AUDIO", "Mixer do PC e do telemóvel".to_string(), TEXT_3),
            tap_card,
            text("SAÍDAS (PC)").size(9).color(TEXT_2),
            sinks,
            text("APPS (PC)").size(9).color(TEXT_2),
            apps,
            phone_audio_section(hud),
            mic_status,
        ]
        .spacing(12),
    )
    .into()
}

