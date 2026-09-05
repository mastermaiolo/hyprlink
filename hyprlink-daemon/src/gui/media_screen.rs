use super::*;
use iced::widget::column;

pub fn media_screen(modules: &crate::state::ModuleStatus) -> Element<'static, Message> {
    let now_playing = modules.media.clone().unwrap_or_else(|| t("Nenhum leitor MPRIS ativo").to_string());
    let card = container(text(now_playing).size(14).color(TEXT))
        .padding(20)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(GREEN_BG)),
            border: Border { color: GREEN_BRD, width: 1.0, radius: 14.0.into() },
            ..Default::default()
        });

    let transport_btn = |label: &'static str, cmd: &'static str| {
        button(text(label).size(12).color(TEXT))
            .padding([10, 18])
            .style(ghost_button(TEXT, 10.0))
            .on_press(Message::MediaCommand(cmd))
    };
    let transport = row![
        transport_btn("⏮", "previous"),
        transport_btn("⏯", "playpause"),
        transport_btn("⏭", "next"),
    ]
    .spacing(10);

    let note = text(t("Volume, shuffle/repeat e lista de players ainda não existem no protocolo — só o essencial (título/artista/álbum + transporte) é real hoje."))
        .size(10)
        .color(TEXT_3);

    column![module_header("MEDIA", "MPRIS".to_string(), GREEN), card, transport, note].spacing(16).into()
}

