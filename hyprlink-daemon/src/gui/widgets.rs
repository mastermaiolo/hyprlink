use super::*;
use iced::widget::column;

pub fn glass(radius: f32) -> container::Style {
    container::Style {
        text_color: None,
        background: Some(Background::Color(GLASS)),
        border: Border { color: GLASS_BRD, width: 1.0, radius: radius.into() },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// Botão "fantasma": fundo/borda translúcidos neutros, texto na cor pedida —
/// o botão mais repetido da GUI (~10 ocorrências idênticas antes desta
/// função existir, uma por tela).
pub fn ghost_button(text_color: Color, radius: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: Some(Background::Color(GLASS)),
        border: Border { color: GLASS_BRD, width: 1.0, radius: radius.into() },
        text_color,
        ..Default::default()
    }
}

/// Botão "pill de acento": fundo do acento a 10%, borda a 35%, texto na cor
/// do acento — a ação primária/destacada de cada tela (~7 ocorrências
/// idênticas antes desta função existir).
pub fn accent_button(accent: Color, radius: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: Some(Background::Color(Color { a: 0.10, ..accent })),
        border: Border { color: Color { a: 0.35, ..accent }, width: 1.0, radius: radius.into() },
        text_color: accent,
        ..Default::default()
    }
}


/// Período do "respirar" por acento (code-spec-iced.md §7) — mais rápido
/// quanto mais urgente o estado.
pub fn breathe_period(accent: Color) -> f32 {
    if accent == GREEN {
        2.8
    } else if accent == AMBER {
        1.5
    } else {
        1.2
    }
}


/// Opacidade 0.45→1.0 e escala 1.0→1.5 num ciclo suave (meio período de ida,
/// meio de volta) — mesmo efeito do ponto de estado do app Android.
pub fn breathe_phase(accent: Color, elapsed: f32) -> (f32, f32) {
    let period = breathe_period(accent);
    let t = (elapsed % period) / period;
    let wave = (1.0 - (t * 2.0 - 1.0).abs()).clamp(0.0, 1.0); // triângulo 0→1→0
    (0.45 + 0.55 * wave, 1.0 + 0.5 * wave)
}


/// O único ponto animado da UI — usado no cabeçalho (junto ao wordmark) e no
/// pill de estado do dashboard. `base_size` em px antes de aplicar a escala.
pub fn breathing_dot(accent: Color, base_size: f32, elapsed: f32) -> Element<'static, Message> {
    let (alpha, scale) = breathe_phase(accent, elapsed);
    let size = base_size * scale;
    container(text(""))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: alpha, ..accent })),
            border: Border { radius: 999.0.into(), ..Default::default() },
            shadow: Shadow { color: Color { a: alpha * 0.8, ..accent }, offset: Vector::default(), blur_radius: size },
            ..Default::default()
        })
        .into()
}


pub fn status_pill(accent: Color, label: &'static str, elapsed: f32) -> Element<'static, Message> {
    container(
        row![
            container(breathing_dot(accent, 7.0, elapsed)).width(13).height(13).align_x(Alignment::Center).align_y(Alignment::Center),
            text(t(label)).size(11).color(accent),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([5, 10])
    .style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.10, ..accent })),
        border: Border { color: Color { a: 0.35, ..accent }, width: 1.0, radius: 999.0.into() },
        ..Default::default()
    })
    .into()
}


/// Estado real de um módulo — nunca vermelho: vermelho é só erro/sem ligação
/// (code-tarefas.md, decisão 2).
pub enum ModuleState {
    /// Módulo com atividade real recente — ponto verde, subtítulo é o dado.
    Active(String),
    /// Implementado, mas sem dados ainda (ex: nenhuma notificação chegou).
    Idle(String),
}


impl ModuleState {
    fn idle(s: impl Into<String>) -> Self {
        ModuleState::Idle(s.into())
    }
}


/// Linha da sidebar (code-spec-iced.md §4): nº · glifo · título · subtítulo
/// real · ponto de estado · chevron. Todos os 10 módulos abrem tela própria
/// agora (Ronda 6) — clicar chama `Message::OpenModule`.
pub fn module_row(id: ModuleId, title: &'static str, state: ModuleState) -> Element<'static, Message> {
    let is_active = matches!(state, ModuleState::Active(_));
    let (dot, sub, sub_color): (Color, String, Color) = match state {
        ModuleState::Active(sub) => (GREEN, sub, TEXT_2),
        ModuleState::Idle(sub) => (MUTED, sub, TEXT_4),
    };
    let glyph_color = if is_active { GREEN } else { TEXT_3 };
    let content = row![
        text(module_number(id)).size(10).color(TEXT_6).width(16),
        text(module_glyph(id)).size(13).color(glyph_color),
        column![
            text(t(title)).size(12).color(TEXT).font(Font::MONOSPACE),
            text(sub).size(9).color(sub_color),
        ]
        .spacing(2)
        .width(Length::Fill),
        container(text("")).width(6).height(6).style(move |_| container::Style {
            background: Some(Background::Color(dot)),
            border: Border { radius: 999.0.into(), ..Default::default() },
            ..Default::default()
        }),
        text("›").size(13).color(TEXT_5),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    button(content)
        .padding([7, 10])
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(GLASS)),
            border: Border { color: GLASS_BRD, width: 1.0, radius: 10.0.into() },
            text_color: TEXT,
            ..Default::default()
        })
        .on_press(Message::OpenModule(id))
        .into()
}


/// Sem ligação ativa, nenhum dado anterior conta — força cinza mesmo que
/// `modules` ainda guarde valores de uma sessão anterior (code-spec-iced.md
/// §4: "sidebar inteira em cinza durante o pareamento").
fn gated(connected: bool, state: ModuleState) -> ModuleState {
    if connected { state } else { ModuleState::idle(t("Aguardando conexão")) }
}

pub fn module_list(download_dir: &std::path::Path, modules: &crate::state::ModuleStatus, connected: bool) -> Element<'static, Message> {
    let clip_state = gated(
        connected,
        match modules.clip_history.first() {
            Some(entry) => ModuleState::Active(format!("{}: {}", t(entry.direction), clip::preview(&entry.text))),
            None => ModuleState::idle(t("Nada sincronizado ainda")),
        },
    );
    let files_state = ModuleState::idle(t1("Recebe em {}", download_dir.display()));
    let notif_state = gated(
        connected,
        if modules.notif_count > 0 {
            ModuleState::Active(t1("{} espelhada(s) nesta sessão", modules.notif_count))
        } else {
            ModuleState::idle(t("Nenhuma notificação ainda"))
        },
    );
    let media_state = gated(
        connected,
        match &modules.media {
            Some(status) => ModuleState::Active(status.clone()),
            None => ModuleState::idle(t("Nenhum leitor ativo")),
        },
    );
    let batt_state = gated(
        connected,
        match modules.phone_battery_pct {
            Some(pct) => ModuleState::Active(t1("Telemóvel em {}%", pct)),
            None => ModuleState::idle(t("Aguardando bateria do telemóvel")),
        },
    );
    let control_state = gated(
        connected,
        match &modules.workspace {
            Some(ws) => ModuleState::Active(t1("Workspace {}", ws)),
            None => ModuleState::idle(t("Hyprland IPC")),
        },
    );
    let audio_state = gated(
        connected,
        if modules.audio_tap_active { ModuleState::Active(t("Audio tap ativo").to_string()) } else { ModuleState::idle(t("Mixer e audio tap")) },
    );
    let webcam_state = gated(
        connected,
        if modules.webcam_active { ModuleState::Active(t("Stream ativo · /dev/video42").to_string()) } else { ModuleState::idle(t("Câmara remota do PC")) },
    );

    let rows = column![
        module_row(ModuleId::Clip, "CLIP", clip_state),
        module_row(ModuleId::Files, "FILES", files_state),
        module_row(ModuleId::Notif, "NOTIFICAÇÕES", notif_state),
        module_row(ModuleId::Media, "MEDIA", media_state),
        module_row(ModuleId::Batt, "BATERIA", batt_state),
        module_row(ModuleId::Control, "CONTROL", control_state),
        module_row(ModuleId::Audio, "AUDIO", audio_state),
        module_row(ModuleId::Webcam, "WEBCAM", webcam_state),
        module_row(ModuleId::Track, "TRACK", ModuleState::idle(t("Rato/teclado virtual pronto (uinput)"))),
        module_row(ModuleId::Config, "CONFIGURAÇÕES", ModuleState::idle(t("Permissões e dispositivos"))),
    ]
    .spacing(7);
    scrollable(rows).width(296).height(Length::Fill).into()
}


/// Colore cada linha do console pela tag de severidade (`[+]`/`[i]`/`[!]`),
/// igual ao console do app Android — hoje o desktop mostrava tudo no mesmo
/// cinza. Um `Highlighter` de linha simples, sem estado entre linhas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeveritySettings;


pub struct SeverityHighlighter {
    current_line: usize,
}


impl iced::advanced::text::Highlighter for SeverityHighlighter {
    type Settings = SeveritySettings;
    type Highlight = Color;
    type Iterator<'a> = std::vec::IntoIter<(std::ops::Range<usize>, Color)>;

    fn new(_settings: &Self::Settings) -> Self {
        Self { current_line: 0 }
    }

    fn update(&mut self, _new_settings: &Self::Settings) {}

    fn change_line(&mut self, line: usize) {
        self.current_line = line;
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.current_line += 1;
        // Só a tag (`[+]`/`[i]`/`[!]`/`[x]`) fica colorida — o resto da
        // linha (timestamp + mensagem) segue no TEXT_2 padrão do editor.
        for (tag, color) in [("[!]", AMBER), ("[x]", RED), ("[+]", GREEN), ("[i]", TEXT_3)] {
            if let Some(pos) = line.find(tag) {
                return vec![(pos..pos + tag.len(), color)].into_iter();
            }
        }
        Vec::new().into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}


pub fn severity_format(color: &Color, _theme: &Theme) -> iced::advanced::text::highlighter::Format<Font> {
    iced::advanced::text::highlighter::Format { color: Some(*color), font: None }
}


/// Console de diagnóstico real: `text_editor` em modo "só leitura" (ignora
/// `Action::Edit`, aceita mover/selecionar/scroll/copiar) — dá scroll,
/// seleção de texto e Ctrl+A/Ctrl+C de verdade, ao contrário de uma pilha de
/// `Text` estáticos.
pub fn console(content: &text_editor::Content) -> Element<'_, Message> {
    text_editor(content)
        .on_action(Message::ConsoleAction)
        .font(Font::MONOSPACE)
        .size(10)
        .padding(0)
        .highlight_with::<SeverityHighlighter>(SeveritySettings, severity_format)
        .style(|_theme, _status| text_editor::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            placeholder: TEXT_2,
            value: TEXT_2,
            selection: Color { a: 0.35, ..GREEN },
        })
        .into()
}


pub fn pairing_qr(payload: &str) -> Element<'static, Message> {
    let code = qrcode::QrCode::new(payload.as_bytes()).expect("payload de pareamento sempre cabe num QR");
    let rendered = code.render::<image::Luma<u8>>().max_dimensions(210, 210).build();
    let rgba = image::DynamicImage::ImageLuma8(rendered).to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let handle = iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw());
    container(iced::widget::image(handle).width(210).height(210))
        .padding(14)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::WHITE)),
            border: Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}


/// Linha chave/valor copiável — mostra `display_value` mas copia `copy_value`
/// por inteiro (o fingerprint é abreviado na tela, o token não pode ser).
pub fn kv_row(label: &'static str, display_value: String, copy_value: String) -> Element<'static, Message> {
    button(
        row![
            column![
                text(t(label)).size(9).color(TEXT_2),
                text(display_value).size(11).color(TEXT).font(Font::MONOSPACE),
            ]
            .spacing(2)
            .width(Length::Fill),
            text(t("copiar")).size(9).color(TEXT_2),
        ]
        .align_y(Alignment::Center),
    )
    .padding([9, 12])
    .width(Length::Fill)
    .style(|_, _| button::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))),
        border: Border { color: GLASS_BRD, width: 1.0, radius: 10.0.into() },
        text_color: TEXT,
        ..Default::default()
    })
    .on_press(Message::CopyText(copy_value))
    .into()
}


/// Régua de 44px que substitui a sidebar de 296px quando um módulo está
/// aberto (code-spec-iced.md §5) — só o módulo ativo fica destacado.
pub const ALL_MODULES: [ModuleId; 10] = [
    ModuleId::Clip,
    ModuleId::Files,
    ModuleId::Notif,
    ModuleId::Media,
    ModuleId::Batt,
    ModuleId::Control,
    ModuleId::Audio,
    ModuleId::Webcam,
    ModuleId::Track,
    ModuleId::Config,
];


/// Régua de 44px que substitui a sidebar quando um módulo está aberto
/// (code-spec-iced.md §5) — o botão ativo mostra o glifo do módulo; os
/// outros, só o número em `TEXT_6`.
pub fn module_ruler(active: ModuleId) -> Element<'static, Message> {
    let entry = |id: ModuleId| -> Element<'static, Message> {
        let is_active = id == active;
        let label = if is_active { module_glyph(id) } else { module_number(id) };
        let color = if is_active { GREEN } else { TEXT_6 };
        let el = container(text(label).size(if is_active { 13 } else { 10 }).color(color))
            .width(34)
            .height(34)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_: &Theme| {
                if is_active {
                    container::Style {
                        background: Some(Background::Color(Color { a: 0.10, ..GREEN })),
                        border: Border { color: GREEN, width: 1.0, radius: 9.0.into() },
                        ..Default::default()
                    }
                } else {
                    container::Style::default()
                }
            });
        if is_active {
            el.into()
        } else {
            button(el)
                .padding(0)
                .style(|_, _| button::Style { background: None, ..Default::default() })
                .on_press(Message::OpenModule(id))
                .into()
        }
    };
    column(ALL_MODULES.into_iter().map(entry)).spacing(8).width(44).align_x(Alignment::Center).into()
}


pub fn module_header(title: &'static str, subtitle: String, subtitle_color: Color) -> Element<'static, Message> {
    column![
        text(t(title)).size(20).font(Font { weight: iced::font::Weight::Bold, ..Font::default() }).color(TEXT),
        text(subtitle).size(10).color(subtitle_color),
    ]
    .spacing(4)
    .into()
}


/// Lista de histórico compartilhada por CLIP/NOTIF: fundo escuro, separador
/// fino entre linhas, `Scrollable` (sem paginação — 50 entradas cabem bem).
pub fn history_list(rows: Vec<Element<'static, Message>>, empty_label: &'static str) -> Element<'static, Message> {
    let inner: Element<'static, Message> = if rows.is_empty() {
        text(t(empty_label)).size(11).color(TEXT_3).into()
    } else {
        let mut col = column![].spacing(0);
        for (i, row_el) in rows.into_iter().enumerate() {
            if i > 0 {
                col = col.push(container(text("")).height(1).width(Length::Fill).style(|_| container::Style {
                    background: Some(Background::Color(DIVIDER)),
                    ..Default::default()
                }));
            }
            col = col.push(row_el);
        }
        col.into()
    };
    container(scrollable(inner).height(Length::Fill))
        .padding(4)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 1.0))),
            border: Border { radius: 14.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}


/// Cabeçalho de busca reusado por CLIP/NOTIF (code-spec-iced.md: "pesquisar…"
/// + "limpar"). `on_clear` some quando `value` já está vazio.
pub fn search_row(value: &str, on_input: impl Fn(String) -> Message + 'static, on_clear: Message) -> Element<'static, Message> {
    let mut r = row![text_input(t("pesquisar…"), value)
        .on_input(on_input)
        .size(10)
        .padding([6, 10])
        .width(160)
        .style(|_theme, _status| text_input::Style {
            background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35)),
            border: Border { color: GLASS_BRD, width: 1.0, radius: 6.0.into() },
            icon: TEXT_3,
            placeholder: TEXT_4,
            value: TEXT,
            selection: Color { a: 0.35, ..GREEN },
        })]
    .spacing(8)
    .align_y(Alignment::Center);
    if !value.is_empty() {
        r = r.push(
            button(text(t("limpar")).size(9).color(TEXT_4))
                .padding([6, 9])
                .style(|_, _| button::Style { background: None, border: Border { color: BRD_1, width: 1.0, radius: 6.0.into() }, text_color: TEXT_4, ..Default::default() })
                .on_press(on_clear),
        );
    }
    r.into()
}


pub fn matches_search(haystack: &str, query: &str) -> bool {
    query.is_empty() || haystack.to_lowercase().contains(&query.to_lowercase())
}


pub fn filter_chip(label: String, count: usize, active: bool, on_press: Message) -> Element<'static, Message> {
    let color = if active { GREEN } else { TEXT_3 };
    button(text(format!("{label} · {count}")).size(10).color(color))
        .padding([6, 12])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if active { Color { a: 0.10, ..GREEN } } else { Color::from_rgba(1.0, 1.0, 1.0, 0.03) })),
            border: Border { color: if active { Color { a: 0.35, ..GREEN } } else { BRD_1 }, width: 1.0, radius: 999.0.into() },
            text_color: color,
            ..Default::default()
        })
        .on_press(on_press)
        .into()
}


pub fn mute_button(muted: bool, on_press: Message) -> Element<'static, Message> {
    let label = if muted { "🔇" } else { "🔊" };
    button(text(label).size(13))
        .padding([6, 10])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if muted { Color { a: 0.10, ..RED } } else { GLASS })),
            border: Border { color: if muted { Color { a: 0.35, ..RED } } else { GLASS_BRD }, width: 1.0, radius: 8.0.into() },
            text_color: if muted { RED } else { TEXT_2 },
            ..Default::default()
        })
        .on_press(on_press)
        .into()
}

