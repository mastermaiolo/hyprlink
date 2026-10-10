//! The pages and blocks added after Fase 0: Câmara & Ecrã, Notificações,
//! Partilha, Multimédia, Definições — plus the new blocks inside Dispositivos
//! (bateria), Secretária (atalhos, trackpad) and Áudio (volumes do telemóvel,
//! misturador do PC). Built only from `ui.rs` components and `theme.rs` tokens.
//!
//! Reading order is always the same: the phone first, this PC after.

use crate::app::{App, CamMode, Message};
use crate::graphics::CameraFrame;
use crate::link::{
    CamCodec, Cap, Command2, Dir, MediaAction, Origin, PhoneNotification, PhoneStream, Ringer,
    TrackpadConfig, TransferState,
};
use crate::theme::{self, *};
use crate::ui::*;
use crate::views::{bar, fill_portion, number_word, spark};
use hyprlink_gui::fmt;
use hyprlink_gui::i18n::{Lang, t};
use hyprlink_gui::tr;

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{Space, button, canvas, column, container, row, slider, text, text_input};
use iced::{Alignment, Length, Padding};

/// Honest marker for features the protocol does not have yet.
pub fn proposed_banner<'a>(why: &'a str) -> El<'a> {
    container(
        row![
            tag_outline(t("PROPOSTO"), HOT),
            hgap(space::L),
            text(why).font(serif_italic()).size(size::DECK_S).color(SUB),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([space::M, space::L]))
    .width(Length::Fill)
    .style(theme::outline(LINE_STRONG))
    .into()
}

fn small_btn<'a>(label: &'a str, msg: Option<Message>) -> El<'a> {
    button(
        text(label)
            .font(MONO_SEMI)
            .size(10.5)
            .wrapping(Wrapping::None),
    )
    .padding(Padding::from([4, 0]))
    .style(theme::bare)
    .on_press_maybe(msg)
    .into()
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

// ═════════════════════════ 02 DISPOSITIVOS · bateria ═════════════════════════

pub fn battery_block(app: &App) -> El<'_> {
    let data: Vec<f32> = app.battery_hist.iter().map(|p| p.level as f32).collect();
    let low = app.alerts.low;
    let axis = row![
        kicker("−12 H"),
        fill_x(),
        kicker("−6 H"),
        fill_x(),
        kicker(t("AGORA"))
    ];
    let mut low_row = column![setting(
        t("Avisar bateria baixa"),
        t("notificação no PC quando o telemóvel desce do limiar"),
        switch(low.is_some(), Message::LowAlert),
    )];
    if let Some(v) = low {
        low_row = low_row.push(
            column![
                row![
                    kicker(t("LIMIAR")),
                    fill_x(),
                    kicker_c(format!("{v}%"), ACID)
                ],
                gap(space::S),
                slider(5.0..=50.0, v as f32, Message::LowLevel)
                    .step(5.0_f32)
                    .style(theme::fader),
            ]
            .padding(Padding::from([space::M, 0.0])),
        );
        low_row = low_row.push(rule());
    }
    column![
        subhead("B", t("Bateria — últimas 12 horas")),
        spark(data, ACID, 0.0, 100.0, 72.0),
        gap(space::S),
        axis,
        gap(space::L),
        low_row,
        setting(
            t("Avisar carga completa"),
            t("quando chega aos 100 % a carregar"),
            switch(app.alerts.full, Message::FullAlert),
        ),
        gap(space::XXL),
    ]
    .into()
}

// ═════════════════════════ 03 SECRETÁRIA · atalhos e trackpad ═════════════════════════

pub fn shortcuts(app: &App) -> El<'_> {
    let mut grid = row![].spacing(space::S);
    for s in &app.shortcuts {
        grid = grid.push(
            button(
                column![
                    text(s.label.as_str()).font(SANS_SEMI).size(14),
                    gap(2.0),
                    text(truncate(&s.dispatch, 26))
                        .font(MONO)
                        .size(10.5)
                        .color(MUTED),
                ]
                .width(170),
            )
            .padding(space::M)
            .style(theme::card(false))
            .on_press(Message::Do(Command2::RunDispatch(s.dispatch.clone()))),
        );
    }
    let editor: El<'_> = if app.sc_edit {
        shortcuts_editor(app)
    } else {
        gap(0.0)
    };
    column![
        subhead("C", t("Atalhos")),
        grid.wrap().vertical_spacing(space::S),
        gap(space::M),
        row![
            fill_x(),
            btn(
                if app.sc_edit {
                    t("CONCLUÍDO")
                } else {
                    t("EDITAR ATALHOS")
                },
                theme::ghost,
                Some(Message::ShortcutsEdit(!app.sc_edit))
            ),
        ],
        editor,
        gap(space::L),
        row![
            text_input(
                t("hyprctl dispatch …  (ex.: workspace 5, exec kitty)"),
                &app.dispatch_input
            )
            .on_input(Message::DispatchInput)
            .on_submit(Message::DispatchRun)
            .font(MONO)
            .size(12)
            .padding(Padding::from([9, 12]))
            .style(theme::input),
            hgap(space::S),
            btn(
                t("EXECUTAR"),
                theme::primary,
                (!app.dispatch_input.trim().is_empty()).then_some(Message::DispatchRun)
            ),
        ]
        .align_y(Alignment::Center),
        gap(space::S),
        mono(t("os atalhos também aparecem no telemóvel"), FAINT),
    ]
    .into()
}

/// Editor de atalhos: uma linha por atalho (nome + comando, GUARDAR e
/// REMOVER) e uma linha para um novo.
fn shortcuts_editor(app: &App) -> El<'_> {
    let field = |placeholder: &'static str,
                 value: &str,
                 max: usize,
                 on_input: Box<dyn Fn(String) -> Message + 'static>,
                 submit: Option<Message>|
     -> El<'static> {
        let over = value.chars().count() > max;
        let mut input = text_input(placeholder, value)
            .on_input(on_input)
            .font(MONO)
            .size(12)
            .padding(Padding::from([8, 12]))
            .style(theme::input);
        if let Some(m) = submit {
            input = input.on_submit(m);
        }
        column![
            input,
            if over {
                El::from(mono(tr!("máx. {} caracteres", max), ACID).size(10.5))
            } else {
                gap(0.0)
            }
        ]
        .into()
    };
    let name_max = crate::link::SHORTCUT_NAME_MAX;
    let cmd_max = crate::link::SHORTCUT_COMMAND_MAX;
    let mut rows = column![].spacing(space::S);
    for i in 0..app.shortcuts.len() {
        let (name, command) = app.shortcut_draft(i);
        let saved = &app.shortcuts[i];
        let changed = name != saved.label || command != saved.dispatch;
        let valid = crate::app::shortcut_valid(&name, &command);
        let save = (changed && valid).then_some(Message::ShortcutSave(i));
        rows = rows.push(
            row![
                container(field(
                    t("nome"),
                    &name,
                    name_max,
                    Box::new(move |s| Message::ShortcutName(i, s)),
                    save.clone(),
                ))
                .width(Length::FillPortion(2)),
                hgap(space::S),
                container(field(
                    t("comando"),
                    &command,
                    cmd_max,
                    Box::new(move |s| Message::ShortcutCommand(i, s)),
                    save.clone(),
                ))
                .width(Length::FillPortion(3)),
                hgap(space::S),
                btn(t("GUARDAR"), theme::primary, save),
                hgap(space::S),
                btn(t("REMOVER"), theme::ghost, Some(Message::ShortcutRemove(i))),
            ]
            .align_y(Alignment::Start),
        );
    }
    let (new_name, new_cmd) = &app.sc_new;
    let can_add = crate::app::shortcut_valid(new_name, new_cmd)
        && app.shortcuts.len() < crate::link::SHORTCUTS_MAX;
    let add = can_add.then_some(Message::ShortcutAdd);
    rows = rows.push(
        row![
            container(field(
                t("novo atalho"),
                new_name,
                name_max,
                Box::new(Message::ShortcutNewName),
                add.clone(),
            ))
            .width(Length::FillPortion(2)),
            hgap(space::S),
            container(field(
                t("comando"),
                new_cmd,
                cmd_max,
                Box::new(Message::ShortcutNewCommand),
                add.clone(),
            ))
            .width(Length::FillPortion(3)),
            hgap(space::S),
            btn(t("ADICIONAR"), theme::primary, add),
        ]
        .align_y(Alignment::Start),
    );
    column![gap(space::M), rule(), gap(space::M), rows].into()
}

pub fn trackpad(app: &App) -> El<'_> {
    let c = app.trackpad;
    let fader = |title: &'static str,
                 v: f32,
                 f: fn(TrackpadConfig, f32) -> TrackpadConfig|
     -> El<'static> {
        column![
            row![
                text(title).font(SANS_MEDIUM).size(size::BODY).color(PAPER),
                fill_x(),
                text(format!("{v:.2}×"))
                    .font(MONO_SEMI)
                    .size(12)
                    .color(ACID),
            ],
            gap(space::M),
            slider(0.25..=3.0, v, move |x| Message::Trackpad(f(c, x)))
                .step(0.05_f32)
                .style(theme::fader),
        ]
        .padding(Padding::from([12, 0]))
        .into()
    };
    column![
        subhead("D", t("Trackpad")),
        fader(t("Sensibilidade"), c.sensitivity, |c, x| TrackpadConfig {
            sensitivity: x,
            ..c
        }),
        rule(),
        fader(t("Velocidade do scroll"), c.scroll, |c, x| TrackpadConfig {
            scroll: x,
            ..c
        }),
        rule(),
        setting(
            t("Aceleração"),
            t("movimentos rápidos vão mais longe"),
            switch(c.acceleration, move |b| Message::Trackpad(TrackpadConfig {
                acceleration: b,
                ..c
            })),
        ),
        setting(
            t("Scroll natural"),
            t("inverte o sentido, como no telemóvel"),
            switch(c.natural_scroll, move |b| Message::Trackpad(
                TrackpadConfig {
                    natural_scroll: b,
                    ..c
                }
            )),
        ),
        setting(
            t("Teclado do telemóvel"),
            t("escrever no PC a partir do teclado Android"),
            switch(c.keyboard, move |b| Message::Trackpad(TrackpadConfig {
                keyboard: b,
                ..c
            })),
        ),
    ]
    .into()
}

// ═════════════════════════ 04 CÂMARA & ECRÃ ═════════════════════════

pub fn camera(app: &App) -> El<'_> {
    let mode = app.cam_mode;
    let modes = row![
        chip(
            t("CÂMARA"),
            mode == CamMode::Camera,
            Message::CamMode(CamMode::Camera)
        ),
        chip(
            t("ECRÃ · PROPOSTO"),
            mode == CamMode::Screen,
            Message::CamMode(CamMode::Screen)
        ),
    ]
    .spacing(space::S);

    let (headline_text, deck_text) = match mode {
        CamMode::Camera => (
            t("A CÂMARA DO BOLSO."),
            t("A câmara do telemóvel chega ao PC como uma webcam — qualquer app a vê."),
        ),
        CamMode::Screen => (
            t("O ECRÃ, NUMA JANELA."),
            t("O telemóvel projetado numa janela flutuante do Hyprland — com as tuas regras."),
        ),
    };

    let body: El = match mode {
        CamMode::Screen => column![
            proposed_banner(
                t("O espelho de ecrã precisa de MediaProjection na app Android e do pacote mirror.start. O desenho está pronto.")
            ),
            gap(space::XXL),
            crate::views::mirror_body(app),
        ]
        .into(),
        CamMode::Camera => camera_body(app),
    };

    column![
        opener("04", t("CÂMARA & ECRÃ"), headline_text, deck_text),
        row![modes, fill_x(), kicker(t("TECLA 4"))].align_y(Alignment::Center),
        gap(space::XL),
        body,
    ]
    .into()
}

fn camera_body(app: &App) -> El<'_> {
    // Chips e rótulos: o formato EFETIVO quando o telemóvel o reporta,
    // senão a preferência (`webcam_cfg`).
    let cfg = app.cam_shown();
    let live = app.webcam.is_some();
    let frame = canvas(CameraFrame {
        t: app.t,
        live,
        // O codec que está mesmo a chegar (o telemóvel pode ter voltado a
        // H.264), não só o pedido.
        label: format!(
            "{}P{} · {}",
            cfg.height,
            cfg.fps,
            fmt::cam_codec(app.webcam.and_then(|w| w.codec).unwrap_or(cfg.codec))
        ),
        elapsed: app.t,
    })
    .width(Length::Fill)
    .height(430);

    let status: El = match app.webcam {
        Some(w) => column![
            row![
                square(
                    if (app.t * 1.5).fract() < 0.6 {
                        HOT
                    } else {
                        INK_2
                    },
                    10.0
                ),
                hgap(space::M),
                headline(t("EM DIRETO"), size::D3).color(HOT)
            ]
            .align_y(Alignment::Center),
            gap(space::XL),
            row![
                container(stat(t("DÉBITO"), format!("{:.1}", w.mbps), "Mb/s", PAPER))
                    .width(Length::Fill),
                container(stat(
                    t("IMAGENS/S"),
                    w.fps
                        .map(|f| format!("{f:.0}"))
                        .unwrap_or_else(|| fmt::DASH.into()),
                    if w.fps.is_some() {
                        "fps"
                    } else {
                        t("não medido")
                    },
                    PAPER
                ))
                .width(Length::Fill),
            ],
        ]
        .into(),
        None => column![
            headline(t("PARADA"), size::D3).color(MUTED),
            gap(space::S),
            deck_s(t(
                "Escolhe o formato e liga. Aparece no PC como /dev/video42."
            )),
        ]
        .into(),
    };

    let res = [(640u32, 480u32), (1280, 720), (1920, 1080)].iter().fold(
        row![].spacing(space::S),
        |r, (w, h)| {
            r.push(chip(
                format!("{h}p"),
                cfg.width == *w && cfg.height == *h,
                Message::CamRes(*w, *h),
            ))
        },
    );
    let fps = [15u32, 30, 60]
        .iter()
        .fold(row![].spacing(space::S), |r, f| {
            r.push(chip(format!("{f}"), cfg.fps == *f, Message::CamFps(*f)))
        });
    let codecs = CamCodec::ALL.iter().fold(row![].spacing(space::S), |r, c| {
        r.push(chip(
            fmt::cam_codec(*c),
            cfg.codec == *c,
            Message::CamCodec(*c),
        ))
    });

    let net: El = match app.nettest {
        Some(n) => column![
            kv_text(t("DÉBITO"), format!("{:.1} Mb/s", n.mbps)),
            kv_text(t("RTT"), format!("{:.1} ms", n.rtt_ms)),
            kv_text(t("PERDAS"), format!("{:.1} %", n.loss_pct)),
        ]
        .into(),
        None => column![deck_s(t("Ainda não testada.")), gap(space::S)].into(),
    };

    let controls = column![
        status,
        gap(space::XXL),
        subhead("A", t("Formato")),
        match app.cam_adjusted_note() {
            Some(n) => column![
                deck_s(format!("{} {n}", t("o telemóvel ajustou:"))),
                gap(space::S)
            ]
            .into(),
            None => El::from(gap(0.0)),
        },
        setting(
            t("Resolução"),
            format!("{}×{}", cfg.width, cfg.height),
            res.into()
        ),
        setting(
            t("Imagens por segundo"),
            t("o telemóvel pode baixar"),
            fps.into()
        ),
        setting(
            t("Codec"),
            t("H.265 poupa débito; o telemóvel volta a H.264 se não tiver encoder HEVC"),
            codecs.into()
        ),
        gap(space::XL),
        subhead("B", t("Rede")),
        net,
        gap(space::M),
        btn(
            t("TESTAR REDE"),
            theme::ghost,
            Some(Message::Do(Command2::TestNetwork))
        ),
        gap(space::XL),
        subhead("C", t("No PC")),
        kv_text(t("DISPOSITIVO"), "/dev/video42"),
        kv_text(t("DRIVER"), "v4l2loopback"),
        kv_text(t("MÓDULO"), "webcam.rs"),
        kv_text(
            t("CODEC EM USO"),
            match app.webcam.and_then(|w| w.codec) {
                Some(c) => fmt::cam_codec(c).to_string(),
                None => fmt::DASH.to_string(),
            }
        ),
        gap(space::XL),
        if live {
            btn(
                t("■  DESLIGAR CÂMARA"),
                theme::danger,
                Some(Message::WebcamStop),
            )
        } else {
            btn(
                t("▶  LIGAR CÂMARA"),
                theme::primary,
                Some(Message::WebcamStart),
            )
        },
    ]
    .width(if app.is_compact() { Length::Fill } else { Length::Fixed(440.0) });

    let preview = column![
        frame,
        gap(space::S),
        row![
            kicker(t("CÂMARA DO TELEMÓVEL")),
            fill_x(),
            kicker(format!("{}×{} · {} FPS", cfg.width, cfg.height, cfg.fps)),
        ],
    ]
    .width(Length::Fill);

    if app.is_compact() {
        column![
            preview,
            gap(space::XXL),
            controls,
        ]
        .into()
    } else {
        row![
            preview,
            hgap(space::GUTTER),
            controls,
        ]
        .into()
    }
}

// ═════════════════════════ 05 ÁUDIO · telemóvel e misturador ═════════════════════════

pub fn phone_audio(app: &App) -> El<'_> {
    let Some(a) = app.phone_audio.as_ref() else {
        return column![
            kicker_c(t("O TELEMÓVEL"), ACID),
            gap(space::S),
            deck_s(t("Sem volumes do telemóvel.")),
        ]
        .into();
    };
    let mut streams = column![];
    for s in PhoneStream::ALL {
        let Some(l) = a.streams.iter().find(|x| x.stream == s).copied() else {
            continue;
        };
        streams = streams.push(column![
            row![
                text(fmt::stream(s))
                    .font(SANS_MEDIUM)
                    .size(size::BODY)
                    .color(PAPER)
                    .width(130),
                slider(0.0..=l.max as f32, l.level as f32, move |v| {
                    Message::Do(Command2::SetPhoneVolume(s, v.round() as u8))
                })
                .step(1.0_f32)
                .style(theme::fader),
                hgap(space::L),
                text(format!("{:>2}/{}", l.level, l.max))
                    .font(MONO_SEMI)
                    .size(12)
                    .color(ACID)
                    .width(44),
            ]
            .align_y(Alignment::Center)
            .padding(Padding::from([10, 0])),
            rule(),
        ]);
    }
    let ringers = [Ringer::Normal, Ringer::Vibrate, Ringer::Silent]
        .iter()
        .fold(row![].spacing(space::S), |r, m| {
            r.push(chip(
                fmt::ringer(*m),
                a.ringer == *m,
                Message::Do(Command2::SetRinger(*m)),
            ))
        });
    container(column![
        row![
            kicker_c(t("O TELEMÓVEL"), ACID),
            hgap(space::M),
            kicker(t("VOLUMES, TOQUE, SILÊNCIO")),
            fill_x(),
            kicker("phone_audio.rs")
        ]
        .align_y(Alignment::Center),
        gap(space::L),
        row![
            container(streams).width(fill_portion(3)),
            hgap(space::GUTTER),
            column![
                kicker(t("MODO DE TOQUE")),
                gap(space::S),
                ringers,
                gap(space::L),
                setting(
                    t("Não incomodar"),
                    t("silencia o telemóvel inteiro"),
                    switch(a.dnd, |b| Message::Do(Command2::SetDnd(b))),
                ),
            ]
            .width(fill_portion(2)),
        ],
    ])
    .padding(space::XL)
    .width(Length::Fill)
    .style(theme::frame)
    .into()
}

pub fn mixer(app: &App) -> El<'_> {
    let vol_row = |title: String,
                   sub: String,
                   vol: u8,
                   muted: bool,
                   right: El<'static>,
                   on_vol: Box<dyn Fn(f32) -> Message>,
                   on_mute: Message|
     -> El<'static> {
        column![
            row![
                column![
                    text(title)
                        .font(SANS_SEMI)
                        .size(14)
                        .color(if muted { MUTED } else { PAPER }),
                    gap(2.0),
                    text(sub)
                        .font(MONO)
                        .size(10.5)
                        .color(FAINT)
                        .wrapping(Wrapping::None),
                ]
                .width(fill_portion(3)),
                container(right).width(110),
                slider(0.0..=150.0, vol as f32, on_vol)
                    .step(1.0_f32)
                    .style(theme::fader)
                    .width(fill_portion(3)),
                hgap(space::L),
                text(format!("{vol}%"))
                    .font(MONO_SEMI)
                    .size(12)
                    .color(if vol > 100 {
                        HOT
                    } else if muted {
                        MUTED
                    } else {
                        ACID
                    })
                    .width(48),
                hgap(space::S),
                container(chip(
                    if muted { t("MUDO") } else { t("SOM") },
                    muted,
                    on_mute
                ))
                .width(64)
                .align_x(Alignment::End),
            ]
            .align_y(Alignment::Center)
            .padding(Padding::from([10, 0])),
            rule(),
        ]
        .into()
    };

    let mut sinks = column![];
    for s in &app.sinks {
        let id = s.id;
        let right: El = if s.default {
            tag(t("PREDEFINIDA"), PAPER, VOID)
        } else {
            small_btn(
                t("USAR ESTA"),
                Some(Message::Do(Command2::SetDefaultSink(id))),
            )
        };
        sinks = sinks.push(vol_row(
            s.description.clone(),
            s.name.clone(),
            s.volume,
            s.muted,
            right,
            Box::new(move |v| Message::Do(Command2::SetSinkVolume(id, v.round() as u8))),
            Message::Do(Command2::SetSinkMute(id, !s.muted)),
        ));
    }
    let mut apps = column![];
    for a in &app.apps {
        let id = a.id;
        let sink = app
            .sinks
            .iter()
            .find(|s| s.id == a.sink)
            .map(|s| s.description.clone())
            .unwrap_or_default();
        apps = apps.push(vol_row(
            a.app.clone(),
            format!("→ {sink}"),
            a.volume,
            a.muted,
            Space::new().into(),
            Box::new(move |v| Message::Do(Command2::SetAppVolume(id, v.round() as u8))),
            Message::Do(Command2::SetAppMute(id, !a.muted)),
        ));
    }

    column![
        row![
            kicker(t("ESTE PC")),
            hgap(space::M),
            text(t("Misturador")).font(SANS_SEMI).size(14).color(PAPER),
            fill_x(),
            kicker("PIPEWIRE · audio.rs"),
        ]
        .align_y(Alignment::Center),
        gap(space::S),
        rule_c(PAPER, 1.0),
        gap(space::L),
        kicker(t("SAÍDAS")),
        sinks,
        gap(space::XL),
        kicker(t("APLICAÇÕES")),
        apps,
    ]
    .into()
}

// ═════════════════════════ 06 NOTIFICAÇÕES ═════════════════════════

/// Campo de resposta + «RESPONDER» (Enter envia) para as notificações que
/// têm uma ação de resposta; as outras não mostram nada.
fn reply_row<'a>(app: &'a App, x: &'a PhoneNotification) -> Option<El<'a>> {
    let idx = x.actions.iter().find(|a| a.is_reply)?.idx;
    let draft = app
        .reply_drafts
        .get(&x.key)
        .map(String::as_str)
        .unwrap_or("");
    let send = Message::ReplySend(x.key.clone(), idx);
    let key = x.key.clone();
    Some(
        row![
            text_input(t("responder…"), draft)
                .on_input(move |s| Message::ReplyInput(key.clone(), s))
                .on_submit(send.clone())
                .font(MONO)
                .size(12)
                .padding(Padding::from([8, 12]))
                .style(theme::input),
            hgap(space::S),
            btn(
                t("RESPONDER"),
                theme::primary,
                (!draft.trim().is_empty()).then_some(send)
            ),
        ]
        .align_y(Alignment::Center)
        .into(),
    )
}

pub fn notifications(app: &App) -> El<'_> {
    let n = app.notifs.len();
    let headline_text = if n == 0 {
        t("NADA POR LER.").to_string()
    } else {
        tr!("{} POR LER.", number_word(n))
    };

    // App index, magazine-style.
    let mut apps: Vec<(String, usize)> = Vec::new();
    for x in &app.notifs {
        match apps.iter_mut().find(|(a, _)| *a == x.app) {
            Some((_, c)) => *c += 1,
            None => apps.push((x.app.clone(), 1)),
        }
    }
    let entry = |label: String, count: usize, selected: bool, msg: Message| -> El<'static> {
        button(
            column![
                row![
                    text(label)
                        .font(if selected { SANS_SEMI } else { SANS_MEDIUM })
                        .size(14)
                        .color(if selected { PAPER } else { SUB }),
                    fill_x(),
                    text(format!("{count}"))
                        .font(MONO_SEMI)
                        .size(11)
                        .color(if selected { ACID } else { FAINT }),
                ]
                .align_y(Alignment::Center),
                gap(space::S),
                rule(),
            ]
            .padding(Padding {
                top: space::S,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            }),
        )
        .padding(0)
        .style(theme::bare)
        .on_press(msg)
        .into()
    };
    let mut index = column![kicker(t("APLICAÇÕES")), gap(space::S), rule_c(PAPER, 1.0)];
    index = index.push(entry(
        t("Todas").into(),
        n,
        app.notif_app.is_none(),
        Message::NotifApp(None),
    ));
    for (a, c) in &apps {
        index = index.push(entry(
            a.clone(),
            *c,
            app.notif_app.as_deref() == Some(a.as_str()),
            Message::NotifApp(Some(a.clone())),
        ));
    }

    let q = app.notif_query.to_lowercase();
    let visible: Vec<_> = app
        .notifs
        .iter()
        .filter(|x| app.notif_app.as_ref().is_none_or(|a| *a == x.app))
        .filter(|x| {
            q.is_empty()
                || x.title.to_lowercase().contains(&q)
                || x.text.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || x.app.to_lowercase().contains(&q)
        })
        .collect();

    let mut list = column![];
    for (i, x) in visible.iter().enumerate() {
        let key = x.key.clone();
        let dismiss = small_btn(
            t("DISPENSAR"),
            Some(Message::Do(Command2::DismissNotification(key))),
        );
        if i == 0 {
            // The lead: the most recent one, set as a headline.
            list = list.push(
                column![
                    row![
                        kicker_c(x.app.to_uppercase(), ACID),
                        hgap(space::M),
                        kicker(fmt::ago(x.at)),
                        fill_x(),
                        dismiss
                    ]
                    .align_y(Alignment::Center),
                    gap(space::M),
                    headline(x.title.to_uppercase(), size::D3),
                    gap(space::S),
                    deck(x.text.clone().unwrap_or_default()),
                    gap(space::M),
                    reply_row(app, x).unwrap_or_else(|| gap(0.0)),
                    gap(space::XL),
                    rule_c(PAPER, 1.0),
                ]
                .padding(Padding {
                    top: 0.0,
                    right: 0.0,
                    bottom: space::M,
                    left: 0.0,
                }),
            );
        } else {
            list = list.push(column![
                row![
                    column![mono(fmt::time(x.at), SUB), kicker(fmt::ago(x.at))].width(84),
                    column![
                        kicker(x.app.to_uppercase()),
                        gap(3.0),
                        text(x.title.as_str()).font(SANS_SEMI).size(16).color(PAPER),
                        gap(2.0),
                        text(x.text.clone().unwrap_or_default())
                            .font(SANS)
                            .size(size::BODY)
                            .color(SUB)
                            .line_height(LineHeight::Relative(1.45)),
                        match reply_row(app, x) {
                            Some(r) => El::from(column![gap(space::S), r]),
                            None => gap(0.0),
                        },
                    ]
                    .width(Length::Fill),
                    dismiss,
                ]
                .align_y(Alignment::Start)
                .padding(Padding::from([14, 0])),
                rule(),
            ]);
        }
    }
    if visible.is_empty() {
        list = list.push(
            column![
                headline(
                    if n == 0 {
                        t("SILÊNCIO.")
                    } else {
                        t("NADA COM ESSE FILTRO.")
                    },
                    size::D3
                ),
                gap(space::S),
                deck_s(if n == 0 {
                    t("O telemóvel está calado.")
                } else {
                    t("Tenta outra aplicação ou outra palavra.")
                }),
            ]
            .padding(Padding::from([space::XL, 0.0])),
        );
    }

    let toolbar = row![
        text_input(
            t("procurar no título, no texto ou na app…"),
            &app.notif_query
        )
        .on_input(Message::NotifQuery)
        .font(MONO)
        .size(12)
        .padding(Padding::from([8, 12]))
        .width(if app.is_compact() { Length::Fill } else { Length::Fixed(360.0) })
        .style(theme::input),
        fill_x(),
        btn(
            t("DISPENSAR TODAS"),
            theme::ghost,
            (n > 0).then_some(Message::Do(Command2::DismissAllNotifications))
        ),
    ]
    .align_y(Alignment::Center);

    let body: El = if app.is_compact() {
        column![
            container(index).width(Length::Fill),
            gap(space::XL),
            column![toolbar, gap(space::XL), list].width(Length::Fill),
        ]
        .into()
    } else {
        row![
            container(index).width(240),
            hgap(space::GUTTER),
            column![toolbar, gap(space::XL), list].width(Length::Fill),
        ]
        .into()
    };

    column![
        opener(
            "06",
            t("NOTIFICAÇÕES"),
            headline_text,
            t("O que o telemóvel recebeu, sem lhe pegar.")
        ),
        body,
    ]
    .into()
}

// ═════════════════════════ 07 PARTILHA ═════════════════════════

/// «Abrir no telemóvel»: URL (só http/https) e/ou app (nome do package).
fn open_on_phone(app: &App) -> El<'_> {
    let linked = app.primary().is_some();
    let url = app.phone_url.trim();
    let url_ok = linked && crate::link::http_url_ok(url);
    let pkg = app.phone_pkg.trim();
    let pkg_ok = linked && crate::link::package_ok(pkg);
    let hint = |on: bool, bad: bool, msg: &'static str| -> El<'static> {
        if on && bad {
            El::from(mono(t(msg), HOT).size(10.5))
        } else {
            gap(0.0)
        }
    };
    column![
        subhead("C", t("Abrir no telemóvel")),
        row![
            text_input(t("https://…"), &app.phone_url)
                .on_input(Message::PhoneUrl)
                .on_submit(Message::PhoneUrlOpen)
                .font(MONO)
                .size(12)
                .padding(Padding::from([9, 12]))
                .style(theme::input),
            hgap(space::S),
            btn(
                t("ABRIR NO TELEMÓVEL"),
                theme::primary,
                url_ok.then_some(Message::PhoneUrlOpen)
            ),
        ]
        .align_y(Alignment::Center),
        hint(
            !url.is_empty(),
            !crate::link::http_url_ok(url),
            "só http:// ou https://"
        ),
        gap(space::M),
        row![
            text_input(t("pacote: com.whatsapp"), &app.phone_pkg)
                .on_input(Message::PhonePkg)
                .on_submit(Message::PhonePkgOpen)
                .font(MONO)
                .size(12)
                .padding(Padding::from([9, 12]))
                .style(theme::input),
            hgap(space::S),
            btn(
                t("ABRIR APP"),
                theme::primary,
                pkg_ok.then_some(Message::PhonePkgOpen)
            ),
        ]
        .align_y(Alignment::Center),
        hint(
            !pkg.is_empty(),
            !crate::link::package_ok(pkg),
            "nome de pacote inválido"
        ),
        gap(space::S),
        if linked {
            mono(
                t("o telemóvel decide se abre já ou mostra uma notificação"),
                FAINT,
            )
        } else {
            mono(t("precisa de um telemóvel ligado"), FAINT)
        },
    ]
    .into()
}

pub fn share(app: &App) -> El<'_> {
    // ── clipboard ──
    let q = app.clip_query.to_lowercase();
    let mut clips: Vec<_> = app
        .clips
        .iter()
        .filter(|c| q.is_empty() || c.text.as_deref().unwrap_or("").to_lowercase().contains(&q))
        .collect();
    clips.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.at.cmp(&a.at)));

    let mut clip_list = column![].spacing(space::S);
    for c in clips {
        let id = c.id;
        let origin_tag = match c.origin {
            Origin::Phone => tag(fmt::origin(c.origin), ACID, VOID),
            Origin::Pc => tag_outline(fmt::origin(c.origin), SUB),
        };
        let body = c
            .text
            .as_deref()
            .map(|t| truncate(t, 140))
            .unwrap_or_else(|| format!("{} · {}", c.mime, fmt::size(c.bytes)));
        clip_list = clip_list.push(
            container(column![
                row![
                    origin_tag,
                    hgap(space::M),
                    kicker(fmt::ago(c.at)),
                    fill_x(),
                    if c.pinned {
                        kicker_c(t("■ FIXADO"), ACID)
                    } else {
                        kicker("")
                    },
                ]
                .align_y(Alignment::Center),
                gap(space::M),
                text(body)
                    .font(MONO)
                    .size(12.5)
                    .color(PAPER)
                    .line_height(LineHeight::Relative(1.5)),
                gap(space::M),
                row![
                    small_btn(t("COPIAR"), Some(Message::Do(Command2::CopyClip(id)))),
                    hgap(space::L),
                    small_btn(
                        t("ENVIAR AO TELEMÓVEL"),
                        Some(Message::Do(Command2::SendClipToPhone(id)))
                    ),
                    hgap(space::L),
                    small_btn(
                        if c.pinned { t("DESAFIXAR") } else { t("FIXAR") },
                        Some(Message::Do(Command2::PinClip(id, !c.pinned)))
                    ),
                    fill_x(),
                    small_btn(t("APAGAR"), Some(Message::Do(Command2::DeleteClip(id)))),
                ],
            ])
            .padding(space::L)
            .width(Length::Fill)
            .style(if c.pinned {
                theme::frame_active
            } else {
                theme::frame
            }),
        );
    }

    let left = column![
        subhead("A", t("Área de transferência")),
        text_input(t("procurar no histórico…"), &app.clip_query)
            .on_input(Message::ClipQuery)
            .font(MONO)
            .size(12)
            .padding(Padding::from([8, 12]))
            .style(theme::input),
        gap(space::L),
        clip_list,
        gap(space::M),
        mono(t("sincroniza sozinha; texto e PNG · clip.rs"), FAINT),
        gap(space::XXL),
        open_on_phone(app),
    ]
    .width(if app.is_compact() { Length::Fill } else { fill_portion(1) });

    // ── files ──
    let linked = app.primary().is_some();
    let drop = container(
        column![
            headline(t("LARGA AQUI."), size::H2 + 14.0),
            gap(space::XS),
            deck_s(t("Um ficheiro largado na janela vai para o telemóvel.")),
            gap(space::L),
            btn(
                t("ESCOLHER FICHEIROS…"),
                theme::primary,
                linked.then_some(Message::PickFiles)
            ),
            gap(space::XS),
            if linked {
                deck_s(t("Escolhe um ou vários ficheiros, de qualquer formato."))
            } else {
                mono(t("precisa de um telemóvel ligado"), FAINT)
            },
            gap(space::M),
            row![
                text_input(
                    t("ou escreve o caminho: ~/Documentos/ficheiro.ext"),
                    &app.send_path
                )
                .on_input(Message::SendPath)
                .on_submit(Message::SendFile)
                .font(MONO)
                .size(12)
                .padding(Padding::from([9, 12]))
                .style(theme::input),
                hgap(space::S),
                btn(
                    t("ENVIAR"),
                    theme::primary,
                    (!app.send_path.trim().is_empty()).then_some(Message::SendFile)
                ),
            ]
            .align_y(Alignment::Center),
        ]
        .align_x(Alignment::Start),
    )
    .padding(space::XL)
    .width(Length::Fill)
    .style(|th| {
        let mut s = theme::frame(th);
        s.border.color = LINE_STRONG;
        s
    });

    let mut active = column![].spacing(space::M);
    let mut history = column![];
    let mut tr: Vec<_> = app.transfers.iter().collect();
    tr.sort_by(|a, b| b.at.cmp(&a.at));
    for xfer in tr {
        let arrow = match xfer.dir {
            Dir::Rx => t("↓ DO TELEMÓVEL"),
            Dir::Tx => t("↑ PARA O TELEMÓVEL"),
        };
        if xfer.state == TransferState::Active {
            let p = if xfer.bytes > 0 {
                xfer.done as f32 / xfer.bytes as f32
            } else {
                0.0
            };
            active = active.push(
                container(column![
                    row![
                        kicker_c(arrow, ACID),
                        fill_x(),
                        kicker(format!("{:.0} %", p * 100.0))
                    ],
                    gap(space::S),
                    text(xfer.name.as_str())
                        .font(SANS_SEMI)
                        .size(16)
                        .color(PAPER),
                    gap(space::M),
                    bar(p, ACID, 3.0),
                    gap(space::S),
                    row![
                        mono(
                            tr!("{} de {}", fmt::size(xfer.done), fmt::size(xfer.bytes)),
                            SUB
                        ),
                        fill_x(),
                        small_btn(
                            t("CANCELAR"),
                            Some(Message::Do(Command2::CancelTransfer(xfer.id)))
                        ),
                    ]
                    .align_y(Alignment::Center),
                ])
                .padding(space::L)
                .width(Length::Fill)
                .style(theme::frame_active),
            );
        } else {
            let c = match xfer.state {
                TransferState::Done => SUB,
                TransferState::Failed => HOT,
                _ => MUTED,
            };
            history = history.push(column![
                row![
                    text(if xfer.dir == Dir::Rx { "↓" } else { "↑" })
                        .font(MONO_SEMI)
                        .size(12)
                        .color(FAINT)
                        .width(20),
                    text(truncate(&xfer.name, 34))
                        .font(SANS_MEDIUM)
                        .size(13.5)
                        .color(PAPER)
                        .width(Length::Fill),
                    mono(fmt::size(xfer.bytes), MUTED).width(80),
                    kicker_c(fmt::transfer_state(xfer.state), c).width(100),
                    kicker(fmt::ago(xfer.at)).width(70),
                ]
                .align_y(Alignment::Center)
                .padding(Padding::from([9, 0])),
                rule(),
            ]);
        }
    }

    let dir = app
        .settings
        .as_ref()
        .map(|s| s.downloads_dir.clone())
        .unwrap_or_else(|| fmt::DASH.into());
    let right = column![
        subhead("B", t("Ficheiros")),
        drop,
        gap(space::L),
        active,
        gap(space::L),
        row![
            kicker(t("HISTÓRICO")),
            fill_x(),
            // Só o histórico (não apaga nada do disco nem cancela envios em
            // curso); sem histórico, desativado.
            small_btn(
                t("LIMPAR HISTÓRICO"),
                app.transfers
                    .iter()
                    .any(|x| x.state != TransferState::Active)
                    .then_some(Message::ClearFileHistory)
            ),
        ]
        .align_y(Alignment::Center),
        gap(space::S),
        rule_c(PAPER, 1.0),
        history,
        gap(space::L),
        row![
            column![kicker(t("PASTA DE DESTINO")), gap(3.0), mono(dir, PAPER)].width(Length::Fill),
            btn(
                t("ABRIR PASTA"),
                theme::ghost,
                Some(Message::Do(Command2::OpenDownloads))
            ),
        ]
        .align_y(Alignment::Center),
    ]
    .width(if app.is_compact() { Length::Fill } else { fill_portion(1) });

    let body: El = if app.is_compact() {
        column![
            left,
            gap(space::XXL),
            right,
        ]
        .into()
    } else {
        row![left, hgap(space::GUTTER), right].into()
    };

    column![
        opener(
            "07",
            t("PARTILHA"),
            t("O QUE PASSA DE MÃO EM MÃO."),
            t("Texto pela área de transferência, ficheiros pelo fio. Larga um ficheiro na janela para o enviar.")
        ),
        body,
    ]
    .into()
}

// ═════════════════════════ 08 MULTIMÉDIA ═════════════════════════

fn transport<'a>(playing: bool, on: impl Fn(MediaAction) -> Option<Message>) -> El<'a> {
    row![
        btn(t("ANTERIOR"), theme::ghost, on(MediaAction::Previous)),
        btn(
            if playing { t("PAUSA") } else { t("TOCAR") },
            theme::primary,
            on(MediaAction::PlayPause)
        ),
        btn(t("SEGUINTE"), theme::ghost, on(MediaAction::Next)),
    ]
    .spacing(space::S)
    .into()
}

fn progress<'a>(pos: Option<u64>, dur: Option<u64>) -> El<'a> {
    let p = match (pos, dur) {
        (Some(p), Some(d)) if d > 0 => p as f32 / d as f32,
        _ => 0.0,
    };
    column![
        bar(p, PAPER, 2.0),
        gap(space::S),
        row![
            mono(pos.map(fmt::clock).unwrap_or_else(|| fmt::DASH.into()), SUB),
            fill_x(),
            mono(
                dur.map(fmt::clock).unwrap_or_else(|| fmt::DASH.into()),
                MUTED
            ),
        ],
    ]
    .into()
}

pub fn media(app: &App) -> El<'_> {
    // Gating pelo contrato: os botões só controlam de verdade quando o
    // telemóvel declarou `media_session` no core.hello (Cap::MediaSession —
    // o daemon mapeia a capability, ver bridge::caps). Sem ela, os botões
    // ficam inertes e a página continua honesta ("proposto").
    let media_cap = app
        .primary()
        .is_some_and(|d| d.caps.contains(&Cap::MediaSession));
    let phone: El = match app.phone.as_ref().and_then(|p| p.now_playing.as_ref()) {
        Some(np) => column![
            row![
                kicker_c(
                    tr!(
                        "{} NO TELEMÓVEL{}",
                        if np.playing {
                            t("A TOCAR")
                        } else {
                            t("EM PAUSA")
                        },
                        np.app
                            .as_ref()
                            .map(|a| format!(" · {}", a.to_uppercase()))
                            .unwrap_or_default()
                    ),
                    ACID
                ),
                fill_x(),
                if media_cap {
                    tag_outline(t("MEDIA SESSION"), ACID)
                } else {
                    tag_outline(t("CONTROLO PROPOSTO"), MUTED)
                },
            ]
            .align_y(Alignment::Center),
            gap(space::L),
            text(np.title.as_str())
                .font(serif_italic())
                .size(72)
                .color(PAPER)
                .line_height(LineHeight::Relative(1.0)),
            gap(space::S),
            text(np.artist.clone().unwrap_or_default())
                .font(SANS_MEDIUM)
                .size(18)
                .color(SUB),
            gap(space::XL),
            progress(np.position_ms, np.duration_ms),
            gap(space::L),
            row![
                transport(np.playing, |a| {
                    media_cap.then(|| Message::Do(Command2::PhoneMedia(a)))
                }),
                fill_x(),
                if media_cap {
                    mono("", FAINT)
                } else {
                    mono(t("precisa de media-session no Android"), FAINT)
                },
            ]
            .align_y(Alignment::Center),
        ]
        .into(),
        None => column![
            kicker_c(t("NO TELEMÓVEL"), ACID),
            gap(space::M),
            deck(t("Nada a tocar no telemóvel.")),
        ]
        .into(),
    };

    let mut players = column![].spacing(space::M);
    for p in &app.players {
        let id = p.id.clone();
        players = players.push(
            container(column![
                row![
                    column![
                        kicker(p.identity.to_uppercase()),
                        gap(space::S),
                        text(p.title.clone().unwrap_or_else(|| fmt::DASH.into()))
                            .font(SANS_SEMI)
                            .size(18)
                            .color(PAPER),
                        gap(2.0),
                        mono(p.artist.clone().unwrap_or_default(), SUB),
                    ]
                    .width(Length::Fill),
                    transport(p.playing, move |a| {
                        Some(Message::Do(Command2::Media {
                            player: id.clone(),
                            action: a,
                        }))
                    }),
                ]
                .align_y(Alignment::Center),
                gap(space::L),
                progress(p.position_ms, p.duration_ms),
            ])
            .padding(space::XL)
            .width(Length::Fill)
            .style(if p.playing {
                theme::frame_active
            } else {
                theme::frame
            }),
        );
    }
    if app.players.is_empty() {
        players = players.push(deck_s(t("Nenhum leitor MPRIS aberto.")));
    }

    column![
        opener(
            "08",
            t("MULTIMÉDIA"),
            t("O QUE ESTÁ A TOCAR."),
            t("Primeiro o telemóvel; depois cada leitor deste PC.")
        ),
        phone,
        gap(space::GUTTER),
        row![
            kicker(t("ESTE PC")),
            hgap(space::M),
            text(t("Leitores")).font(SANS_SEMI).size(14).color(PAPER),
            fill_x(),
            kicker(tr!("MPRIS · {} ABERTOS", app.players.len())),
        ]
        .align_y(Alignment::Center),
        gap(space::S),
        rule_c(PAPER, 1.0),
        gap(space::L),
        players,
    ]
    .into()
}

// ═════════════════════════ ·· DEFINIÇÕES ═════════════════════════

/// O ambiente em uso no daemon (o que detetou e as opções de correção do
/// `config.json`), só para ver — o mesmo que `hyprlinkctl doctor`.
fn env_values(app: &App) -> El<'_> {
    let Some(st) = app.settings.as_ref().filter(|s| !s.env.is_empty()) else {
        return mono(t("sem dados do daemon"), FAINT).into();
    };
    let mut col = column![];
    for v in &st.env {
        let label = match v.key.as_str() {
            "shell" => t("SHELL"),
            "hypr_dispatch_mode" => t("DISPATCH DO HYPRLAND"),
            "lock_command" => t("BLOQUEIO"),
            "screenshot_tool" => t("CAPTURA"),
            "temp_sensor" => t("TEMPERATURA"),
            "gpu_source" => t("CARGA DA GPU"),
            "audio_backend" => t("VOLUME"),
            "v4l2_device_nr" => t("CÂMARA (/dev/videoN)"),
            "tap_source" => t("FONTE DO RETORNO"),
            "aviso" => t("AVISO"),
            other => other,
        };
        col = col.push(kv_text(label, v.value.clone()));
    }
    col.push(mono(
        t("para os forçar: opções no config.json (ver README, «Compatibilidade»)"),
        FAINT,
    ))
    .into()
}

pub fn settings(app: &App) -> El<'_> {
    let s = app.settings.as_ref();
    let current = hyprlink_gui::i18n::get();
    let mut langs = row![].spacing(space::S).align_y(Alignment::Center);
    for l in Lang::ALL {
        langs = langs.push(chip(l.label(), l == current, Message::SetLang(l)));
    }

    let content = column![
        subhead("A", t("Ficheiros")),
        column![
            row![
                column![
                    text(t("Pasta de destino"))
                        .font(SANS_MEDIUM)
                        .size(size::BODY)
                        .color(PAPER),
                    gap(3.0),
                    mono(t("onde chegam os ficheiros do telemóvel"), MUTED).size(11),
                ]
                .width(Length::Fill),
            ],
            gap(space::M),
            row![
                text_input("~/Transferências/HyprLink", &app.downloads_input)
                    .on_input(Message::DownloadsInput)
                    .on_submit(Message::DownloadsSave)
                    .font(MONO)
                    .size(12)
                    .padding(Padding::from([9, 12]))
                    .style(theme::input),
                hgap(space::S),
                btn(
                    t("GUARDAR"),
                    theme::primary,
                    (s.map(|s| s.downloads_dir.as_str()) != Some(app.downloads_input.as_str()))
                        .then_some(Message::DownloadsSave)
                ),
            ]
            .align_y(Alignment::Center),
        ]
        .padding(Padding::from([12, 0])),
        rule(),
        gap(space::XXL),
        subhead("B", t("Daemon")),
        kv(
            t("ESTADO"),
            row![
                square(ACID, 6.0),
                hgap(space::S),
                kicker_c(t("A ESCUTAR"), ACID)
            ]
            .align_y(Alignment::Center)
        ),
        kv_text(
            t("VERSÃO"),
            s.map(|s| format!("hyprlinkd {}", s.daemon_version))
                .unwrap_or_else(|| fmt::DASH.into())
        ),
        kv_text(
            t("SOCKET"),
            s.map(|s| s.socket.clone())
                .unwrap_or_else(|| fmt::DASH.into())
        ),
        kv_text(t("PROTOCOLO"), "hyprlink/1 · QUIC · mTLS · :7443"),
        gap(space::L),
        btn(
            t("REINICIAR DAEMON"),
            theme::danger,
            Some(Message::Do(Command2::RestartDaemon))
        ),
        gap(space::XXL),
        subhead("C", t("Arranque")),
        kv_text(t("COM O HYPRLAND"), "exec-once = hyprlink-gui --hidden"),
        kv_text(t("DAEMON"), "systemctl --user enable --now hyprlinkd"),
        kv_text(t("TRAY"), t("fechar a janela deixa a app no tray")),
        gap(space::XXL),
        subhead("D", t("Ambiente")),
        env_values(app),
        gap(space::XXL),
        subhead("E", t("Idioma")),
        langs,
        gap(space::XXL),
        subhead("F", t("Sobre")),
        kv_text(
            t("GUI"),
            format!("hyprlink-gui {}", env!("CARGO_PKG_VERSION"))
        ),
        kv_text(
            t("TECLAS"),
            t("1–9 e 0 para as secções · Esc fecha o emparelhamento")
        ),
        kv_text(
            t("TIPOGRAFIA"),
            "Anton · Instrument Serif · Inter · IBM Plex Mono — OFL"
        ),
    ];

    column![
        opener(
            "00",
            t("DEFINIÇÕES"),
            t("AS REGRAS DA CASA."),
            t("Poucas, e todas reversíveis.")
        ),
        container(content).max_width(860),
    ]
    .into()
}
