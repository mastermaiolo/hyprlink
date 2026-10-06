//! The eight pages. Each one opens the same way (kicker, double rule,
//! headline, deck) and then breaks into its own grid.

use crate::app::{App, Filter, Message, Section};
use crate::graphics::{LinkDiagram, Meter, Phone, RssiScale, Spark};
use crate::link::{Codec, Device, Dir, Kind, LinkState, PairingTicket, PhoneStatus, SensorKind};
use crate::theme::{self, *};
use crate::ui::*;
use hyprlink_gui::fmt;
use hyprlink_gui::i18n::t;
use hyprlink_gui::tr;
use hyprlink_gui::link::packets;

use chrono::{Datelike, Local};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
    Space, button, canvas, column, container, qr_code, row, slider, text, text_input,
};
use iced::{Alignment, Color, Length, Padding};

pub fn fill_portion(n: u16) -> Length {
    Length::FillPortion(n)
}

fn state_color(s: LinkState) -> Color {
    match s {
        LinkState::Linked => ACID,
        LinkState::Idle => PAPER,
        LinkState::Offline => MUTED,
    }
}

pub fn number_word(n: usize) -> &'static str {
    [
        t("NENHUM"),
        t("UM"),
        t("DOIS"),
        t("TRÊS"),
        t("QUATRO"),
        t("CINCO"),
        t("SEIS"),
        t("SETE"),
    ]
    .get(n)
    .copied()
    .unwrap_or(t("MUITOS"))
}

fn date_pt() -> String {
    let d = Local::now();
    let wd = [
        t("SEGUNDA"),
        t("TERÇA"),
        t("QUARTA"),
        t("QUINTA"),
        t("SEXTA"),
        t("SÁBADO"),
        t("DOMINGO"),
    ][d.weekday().num_days_from_monday() as usize];
    let m = [
        t("JANEIRO"),
        t("FEVEREIRO"),
        t("MARÇO"),
        t("ABRIL"),
        t("MAIO"),
        t("JUNHO"),
        t("JULHO"),
        t("AGOSTO"),
        t("SETEMBRO"),
        t("OUTUBRO"),
        t("NOVEMBRO"),
        t("DEZEMBRO"),
    ][d.month0() as usize];
    tr!("{0}, {1} DE {2} DE {3}", wd, d.day(), m, d.year())
}

/// A thin proportional bar (battery, countdowns).
pub fn bar<'a>(v: f32, c: Color, h: f32) -> El<'a> {
    let on = ((v.clamp(0.0, 1.0) * 1000.0) as u16).max(1);
    let off = (1000 - on.min(999)).max(1);
    row![
        container(Space::new().height(h))
            .width(fill_portion(on))
            .style(theme::fill(c)),
        container(Space::new().height(h))
            .width(fill_portion(off))
            .style(theme::fill(LINE_STRONG)),
    ]
    .height(h)
    .into()
}

pub fn spark<'a>(data: Vec<f32>, c: Color, min: f32, max: f32, h: f32) -> El<'a> {
    canvas(Spark {
        data,
        color: c,
        min,
        max,
        grid: true,
    })
    .width(Length::Fill)
    .height(h)
    .into()
}

// ═════════════════════════════ 01 CAPA ═════════════════════════════

pub fn cover(app: &App) -> El<'_> {
    let dev = app.primary();
    let name = dev.map(fmt::name).unwrap_or_else(|| t("SEM LIGAÇÃO").into());

    let top = column![
        row![
            kicker_c("§01", ACID),
            hgap(space::M),
            kicker(t("CAPA")),
            fill_x(),
            kicker(date_pt()),
        ]
        .align_y(Alignment::Center),
        gap(space::S),
        rule_c(PAPER, 2.0),
        gap(3.0),
        rule(),
    ];

    // Hero: the device as the cover star, with a table of contents beside it.
    let hero_left = column![
        kicker(t("EM DESTAQUE — O DISPOSITIVO PRINCIPAL")),
        gap(space::M),
        headline(name.clone(), size::MASTHEAD)
            .line_height(LineHeight::Relative(0.92))
            .wrapping(Wrapping::None),
        gap(space::L),
        row![
            match dev {
                Some(d) => tag(
                    format!("■ {}", fmt::state(d.state)),
                    state_color(d.state),
                    VOID
                ),
                None => tag(t("■ DESLIGADO"), HOT, VOID),
            },
            hgap(space::M),
            mono(dev.map(fmt::os).unwrap_or_else(|| fmt::DASH.into()), SUB),
            hgap(space::M),
            mono(
                dev.and_then(|d| d.addr.clone())
                    .unwrap_or_else(|| fmt::DASH.into()),
                MUTED
            ),
        ]
        .align_y(Alignment::Center),
        gap(space::XL),
        deck(t("O telemóvel é a outra metade da tua secretária.")).size(34),
    ]
    .width(fill_portion(5));

    let toc_entry = |s: Section, line: String| -> El<'_> {
        button(
            column![
                row![
                    text(s.num()).font(DISPLAY).size(26).color(FAINT),
                    hgap(space::M),
                    column![
                        text(s.title()).font(SANS_SEMI).size(14).color(PAPER),
                        text(line).font(serif_italic()).size(16).color(SUB),
                    ]
                    .spacing(1),
                ]
                .align_y(Alignment::Center),
                gap(space::M),
                rule(),
            ]
            .padding(Padding {
                top: space::M,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            }),
        )
        .padding(0)
        .style(theme::bare)
        .on_press(Message::Nav(s))
        .into()
    };
    let near = if !app.rssi_known {
        t("sem leitura")
    } else if app.rssi > app.unlock_at {
        t("perto — ao alcance da mão")
    } else if app.rssi > app.lock_at {
        t("na sala, mas longe")
    } else {
        t("longe — sessão a trancar")
    };
    let hero_right = column![
        kicker(t("NESTA EDIÇÃO")),
        gap(space::S),
        rule_c(PAPER, 1.0),
        toc_entry(
            Section::Dispositivos,
            {
                let linked = app
                    .devices
                    .iter()
                    .filter(|d| d.state == LinkState::Linked)
                    .count();
                if app.devices.len() == 1 {
                    tr!("{} aparelho, {} ligado", app.devices.len(), linked)
                } else {
                    tr!("{} aparelhos, {} ligado", app.devices.len(), linked)
                }
            }
        ),
        toc_entry(
            Section::Notificacoes,
            match app.notifs.first() {
                Some(n) => tr!("{} por ler, a última de {}", app.notifs.len(), n.app),
                None => t("nada por ler").into(),
            }
        ),
        toc_entry(
            Section::Partilha,
            match app
                .transfers
                .iter()
                .filter(|t| t.state == crate::link::TransferState::Active)
                .count()
            {
                0 => tr!("{} entradas no clipboard", app.clips.len()),
                1 => t("um ficheiro a caminho").into(),
                n => tr!("{} ficheiros a caminho", n),
            }
        ),
        toc_entry(
            Section::Audio,
            match (app.mic_on, app.tap_on) {
                (true, true) => t("microfone e retorno ativos").into(),
                (true, false) => t("o microfone está aberto").into(),
                (false, true) => t("a música desce para o bolso").into(),
                _ => t("em silêncio").into(),
            }
        ),
        toc_entry(
            Section::Sensores,
            if app.rssi_known {
                let v = format!("{:.0}", app.rssi);
                tr!("{} dBm, {}", v, near)
            } else {
                t("ainda sem leitura de sinal").to_string()
            },
        ),
    ]
    .width(fill_portion(2));

    let hero = row![hero_left, hgap(space::GUTTER), hero_right];

    let ph = app.phone.as_ref();
    let diagram = canvas(LinkDiagram {
        t: app.t,
        left: (
            name.clone(),
            ph.map(|p| fmt::network_line(p).to_uppercase())
                .unwrap_or_else(|| dev.map(fmt::os).unwrap_or_default().to_uppercase()),
        ),
        right: (
            app.host.hostname.to_uppercase(),
            tr!("{} · ESTE PC", app.host.compositor.to_uppercase()),
        ),
        latency: app.latency.last(),
        online: dev.is_some(),
    })
    .width(Length::Fill)
    .height(112);

    // ── O telemóvel: four columns, newspaper-market-page style. ──
    let lat = app.latency.last();
    let lat_max = app.latency.max().max(12.0);
    let bat = dev.and_then(|d| d.battery);
    let charging = dev.map(|d| d.charging).unwrap_or(false);
    let sto_u = ph.and_then(|p| fmt::gb(p.storage_used_b));
    let sto_t = ph.and_then(|p| fmt::gb(p.storage_total_b));
    let (net_main, net_rest) = ph
        .map(fmt::network)
        .unwrap_or_else(|| (fmt::DASH.into(), String::new()));
    let cell = |c: El<'static>| {
        container(c)
            .width(Length::Fill)
            .padding(Padding::from([0.0, space::XL]))
    };
    let phone_stats = row![
        cell(
            column![
                stat(
                    t("BATERIA"),
                    fmt::opt(bat),
                    if charging { t("% · a carregar") } else { "%" },
                    if bat.is_some_and(|b| b < 20) {
                        HOT
                    } else {
                        PAPER
                    }
                ),
                gap(space::M),
                container(bar(bat.unwrap_or(0) as f32 / 100.0, ACID, 6.0))
                    .height(40)
                    .align_y(Alignment::End)
            ]
            .into()
        ),
        vrule(),
        cell(
            column![
                stat(
                    t("REDE MÓVEL"),
                    net_main.to_string(),
                    net_rest.to_string(),
                    PAPER
                ),
                gap(space::M),
                container(
                    row![
                        signal_bars(ph.and_then(|p| p.signal_bars).unwrap_or(0)),
                        hgap(space::M),
                        mono(
                            ph.and_then(|p| p.wifi.as_ref())
                                .and_then(|w| w.ssid.clone())
                                .map(|w| format!("Wi-Fi {w}"))
                                .unwrap_or_default(),
                            MUTED
                        ),
                    ]
                    .align_y(Alignment::End)
                )
                .height(40)
                .align_y(Alignment::End)
            ]
            .into()
        ),
        vrule(),
        cell(
            column![
                stat(
                    t("ARMAZENAMENTO"),
                    sto_u
                        .map(|u| format!("{u:.0}"))
                        .unwrap_or_else(|| fmt::DASH.into()),
                    sto_t.map(|t| format!("/ {t:.0} GB")).unwrap_or_default(),
                    PAPER
                ),
                gap(space::M),
                container(bar(
                    match (sto_u, sto_t) {
                        (Some(u), Some(t)) if t > 0.0 => u / t,
                        _ => 0.0,
                    },
                    PAPER,
                    6.0
                ))
                .height(40)
                .align_y(Alignment::End)
            ]
            .into()
        ),
        vrule(),
        cell(
            column![
                stat(
                    t("LATÊNCIA"),
                    format!("{lat:.1}"),
                    "ms",
                    if lat > 100.0 { HOT } else { PAPER }
                ),
                gap(space::M),
                spark(app.latency.to_vec(), ACID, 0.0, lat_max, 40.0)
            ]
            .into()
        ),
    ]
    .height(130);

    // What the phone is doing right now.
    let now_playing: El = match ph.and_then(|p| p.now_playing.as_ref()) {
        Some(np) => column![
            kicker(tr!(
                "{} NO TELEMÓVEL · {}",
                if np.playing { t("A TOCAR") } else { t("EM PAUSA") },
                np.app.clone().unwrap_or_default().to_uppercase()
            )),
            gap(space::S),
            text(np.title.as_str())
                .font(serif_italic())
                .size(34)
                .color(PAPER),
            mono(np.artist.clone().unwrap_or_default(), SUB),
            gap(space::M),
            bar(np.progress().unwrap_or(0.0), PAPER, 2.0),
        ]
        .into(),
        None => column![
            kicker(t("NO TELEMÓVEL")),
            gap(space::S),
            deck_s(t("Nada a tocar.")),
        ]
        .into(),
    };
    let notif = ph.and_then(|p| p.notifications);
    let phone_now = row![
        container(now_playing).width(fill_portion(3)),
        hgap(space::GUTTER),
        column![
            kicker(t("NOTIFICAÇÕES")),
            gap(space::S),
            text(fmt::opt(notif))
                .font(DISPLAY)
                .size(size::D3)
                .color(if notif.unwrap_or(0) > 0 { ACID } else { FAINT })
                .line_height(LineHeight::Relative(1.0)),
        ]
        .width(fill_portion(1)),
        column![
            kv(
                t("ECRÃ"),
                match ph.and_then(|p| p.screen_on) {
                    Some(true) => tag(t("LIGADO"), PAPER, VOID),
                    Some(false) => tag_outline(t("DESLIGADO"), MUTED),
                    None => mono(fmt::DASH, MUTED).into(),
                }
            ),
            kv_text(
                t("MEMÓRIA"),
                ph.map(ram_line).unwrap_or_else(|| fmt::DASH.into())
            ),
            kv_text(
                t("TEMPERATURA"),
                ph.and_then(|p| p.battery_temp_c)
                    .map(|t| format!("{t:.1} °C"))
                    .unwrap_or_else(|| fmt::DASH.into())
            ),
        ]
        .width(fill_portion(2)),
    ];

    // ── Este PC: secondary, smaller, real. ──
    let h = &app.host;
    let up = h.uptime_s;
    let pc = column![
        row![
            kicker(t("ESTE PC")),
            hgap(space::M),
            text(h.hostname.as_str())
                .font(SANS_SEMI)
                .size(14)
                .color(PAPER),
            fill_x(),
            kicker(tr!(
                "{} · LINUX {}",
                h.compositor.to_uppercase(),
                h.kernel
            )),
        ]
        .align_y(Alignment::Center),
        gap(space::S),
        rule(),
        gap(space::L),
        row![
            container(column![
                small_stat("CPU", format!("{:.0}", h.cpu_pct), "%"),
                gap(space::S),
                spark(app.cpu_hist.to_vec(), SUB, 0.0, 100.0, 24.0),
            ])
            .width(Length::Fill),
            hgap(space::XL),
            container(small_stat(
                t("MEMÓRIA"),
                format!("{:.1}", h.ram_used_gb),
                format!("/ {:.0} GB", h.ram_total_gb)
            ))
            .width(Length::Fill),
            hgap(space::XL),
            container(match h.battery {
                Some((pct, chg)) => small_stat(
                    t("BATERIA DO PC"),
                    format!("{pct}"),
                    if chg { t("% · a carregar") } else { "%" }
                ),
                None => small_stat(t("ALIMENTAÇÃO"), "CA".to_string(), ""),
            })
            .width(Length::Fill),
            hgap(space::XL),
            container(small_stat(
                t("LIGADO HÁ"),
                format!("{}h{:02}", up / 3600, (up / 60) % 60),
                ""
            ))
            .width(Length::Fill),
        ],
        gap(space::S),
        mono(h.cpu_model.as_str(), FAINT),
    ];

    // Quick actions + the wire.
    let id = dev.map(|d| d.id).unwrap_or(0);
    let actions = column![
        subhead("A", t("Ações rápidas")),
        row![
            btn(t("PING"), theme::ghost, dev.map(|_| Message::Ping(id))).width(Length::Fill),
            btn(
                t("ENVIAR CLIPBOARD"),
                theme::ghost,
                dev.map(|_| Message::Clipboard(id))
            )
            .width(Length::Fill),
        ]
        .spacing(space::S),
        gap(space::S),
        row![
            btn(
                if app.mic_on {
                    t("FECHAR MICROFONE")
                } else {
                    t("ABRIR MICROFONE")
                },
                if app.mic_on {
                    theme::danger
                } else {
                    theme::ghost
                },
                Some(Message::Mic(!app.mic_on))
            )
            .width(Length::Fill),
            btn(
                if app.mirror_on {
                    t("PARAR ESPELHO")
                } else {
                    t("ESPELHAR ECRÃ")
                },
                if app.mirror_on {
                    theme::danger
                } else {
                    theme::primary
                },
                Some(if app.mirror_on {
                    Message::MirrorStop
                } else {
                    Message::MirrorStart
                })
            )
            .width(Length::Fill),
        ]
        .spacing(space::S),
        gap(space::XL),
        deck_s(t("“Cada pacote é uma stream: um frame, um propósito, e fecha-se.”")),
        gap(space::S),
        kicker(t("— NOTAS DE ARQUITETURA, HYPRLINK")),
    ]
    .width(fill_portion(2));

    let mut wire = column![subhead("B", t("O fio"))];
    for p in app.packets.iter().rev().take(7) {
        wire = wire.push(packet_row(p, true));
    }
    let wire = wire.width(fill_portion(3));

    column![
        top,
        gap(space::XXL),
        hero,
        gap(space::XXL),
        diagram,
        gap(space::XL),
        rule_c(PAPER, 1.0),
        gap(space::S),
        row![
            kicker_c(t("O TELEMÓVEL"), ACID),
            fill_x(),
            kicker(dev.map(fmt::model).unwrap_or_default())
        ],
        gap(space::XL),
        phone_stats,
        gap(space::XL),
        rule(),
        gap(space::XL),
        phone_now,
        gap(space::XXL),
        row![actions, hgap(space::GUTTER), wire],
        gap(space::GUTTER),
        pc,
    ]
    .into()
}

fn has_phone_state(app: &App, d: &Device) -> bool {
    app.phone.is_some() && d.state == LinkState::Linked && app.primary().map(|p| p.id) == Some(d.id)
}

/// Live state reported by the phone itself — shown before identity details.
fn phone_state<'a>(app: &'a App, d: &'a Device) -> El<'a> {
    let Some(p) = app.phone.as_ref().filter(|_| has_phone_state(app, d)) else {
        return Space::new().into();
    };
    column![
        subhead("A", t("Estado do telemóvel")),
        kv_text(t("REDE"), fmt::network_line(p)),
        kv_text(
            "WI-FI",
            p.wifi
                .as_ref()
                .map(|w| match (&w.ssid, w.rssi_dbm) {
                    (Some(s), Some(r)) => format!("{s} · {r} dBm"),
                    (Some(s), None) => s.clone(),
                    (None, Some(r)) => format!("{r} dBm"),
                    _ => fmt::DASH.into(),
                })
                .unwrap_or_else(|| fmt::DASH.into())
        ),
        kv(
            t("ARMAZENAMENTO"),
            match (fmt::gb(p.storage_used_b), fmt::gb(p.storage_total_b)) {
                (Some(u), Some(t)) if t > 0.0 => row![
                    mono(format!("{u:.0} / {t:.0} GB"), PAPER),
                    hgap(space::M),
                    container(bar(u / t, PAPER, 3.0)).width(120),
                ]
                .align_y(Alignment::Center)
                .into(),
                _ => El::from(mono(fmt::DASH, PAPER)),
            }
        ),
        kv_text(t("MEMÓRIA"), ram_line(p)),
        kv_text(
            t("TEMPERATURA"),
            p.battery_temp_c
                .map(|t| format!("{t:.1} °C"))
                .unwrap_or_else(|| fmt::DASH.into())
        ),
        kv_text(
            t("ECRÃ"),
            match p.screen_on {
                Some(true) => t("ligado"),
                Some(false) => t("desligado"),
                None => fmt::DASH,
            }
        ),
        kv_text(
            t("NÃO INCOMODAR"),
            match p.dnd {
                Some(true) => t("ativo"),
                Some(false) => t("inativo"),
                None => fmt::DASH,
            }
        ),
        kv_text(
            t("NOTIFICAÇÕES"),
            p.notifications
                .map(|n| tr!("{} por ler", n))
                .unwrap_or_else(|| fmt::DASH.into())
        ),
        gap(space::XXL),
    ]
    .into()
}

fn ram_line(p: &PhoneStatus) -> String {
    match (fmt::gb(p.ram_used_b), fmt::gb(p.ram_total_b)) {
        (Some(u), Some(t)) => format!("{u:.1} / {t:.0} GB"),
        (Some(u), None) => format!("{u:.1} GB"),
        _ => fmt::DASH.into(),
    }
}

pub fn small_stat<'a>(label: &'a str, value: String, unit: impl text::IntoFragment<'a>) -> El<'a> {
    column![
        kicker(label),
        gap(space::XS),
        row![
            text(value)
                .font(DISPLAY)
                .size(28)
                .color(SUB)
                .line_height(LineHeight::Relative(1.0)),
            hgap(5.0),
            mono(unit, MUTED).size(11),
        ]
        .align_y(Alignment::End),
    ]
    .into()
}

/// Four ascending bars, like the phone's own status bar.
pub fn signal_bars<'a>(n: u8) -> El<'a> {
    let mut r = row![].spacing(3).align_y(Alignment::End);
    for i in 0..4u8 {
        let h = 6.0 + i as f32 * 5.0;
        r = r.push(
            container(Space::new().width(5).height(h)).style(theme::fill(if i < n {
                PAPER
            } else {
                LINE_STRONG
            })),
        );
    }
    r.into()
}

fn packet_row<'a>(p: &'a crate::link::Packet, compact: bool) -> El<'a> {
    let (d, c) = match p.dir {
        Dir::Tx => ("TX", ACID),
        Dir::Rx => ("RX", COLD),
    };
    let r = row![
        text(format!("{:>9.3}", p.at))
            .font(MONO)
            .size(11.5)
            .color(FAINT)
            .width(84),
        container(text(d).font(MONO_SEMI).size(10.5).color(c)).width(36),
        text(p.kind.as_str())
            .font(MONO_MEDIUM)
            .size(12)
            .color(PAPER)
            .width(fill_portion(if compact { 5 } else { 4 }))
            .wrapping(Wrapping::None),
    ];
    let r = if compact {
        r
    } else {
        r.push(
            text(format!("{} B", p.bytes))
                .font(MONO)
                .size(11.5)
                .color(MUTED)
                .width(80),
        )
    };
    let r = r
        .push(
            text(p.note.as_str())
                .font(MONO)
                .size(11.5)
                .color(MUTED)
                .width(fill_portion(4))
                .wrapping(Wrapping::None),
        )
        .align_y(Alignment::Center)
        .padding(Padding::from([7, 0]));
    column![r, rule()].into()
}

// ═════════════════════════════ 02 DISPOSITIVOS ═════════════════════════════

pub fn devices(app: &App) -> El<'_> {
    let n = app.devices.len();
    let mut list = column![].spacing(space::S);
    for d in &app.devices {
        list = list.push(device_card(d, d.id == app.selected));
    }
    let list = column![
        list,
        gap(space::L),
        btn(
            t("＋  EMPARELHAR NOVO"),
            theme::primary,
            Some(Message::BeginPair)
        )
        .width(Length::Fill),
        gap(space::S),
        mono(t("QUIC · mTLS · porta 7443"), FAINT),
    ]
    .width(340);

    let detail: El = match app.device() {
        Some(d) => device_detail(app, d),
        None => column![
            headline(t("NENHUM APARELHO."), size::D3),
            deck_s(t("Emparelha um telemóvel para começar."))
        ]
        .into(),
    };

    column![
        opener(
            "02",
            t("DISPOSITIVOS"),
            format!(
                "{} {}.",
                number_word(n),
                if n == 1 { t("APARELHO") } else { t("APARELHOS") }
            ),
            t("Cada um com o seu certificado. Nenhum sem a tua autorização."),
        ),
        row![
            list,
            hgap(space::GUTTER),
            container(detail).width(Length::Fill)
        ],
    ]
    .into()
}

fn device_card(d: &Device, selected: bool) -> El<'_> {
    let sc = state_color(d.state);
    let meta: El = if d.state == LinkState::Offline {
        mono(tr!("emparelhado {}", fmt::date(d.paired_since)), FAINT).into()
    } else {
        let b = d.battery.unwrap_or(0);
        row![
            container(bar(b as f32 / 100.0, if b < 20 { HOT } else { PAPER }, 3.0)).width(48),
            hgap(space::S),
            mono(format!("{}%", fmt::battery(d)), SUB),
            fill_x(),
            mono(format!("{} ms", fmt::latency(d.latency_ms)), SUB),
        ]
        .align_y(Alignment::Center)
        .into()
    };
    button(
        column![
            row![
                kicker(fmt::kind(d.kind)),
                fill_x(),
                square(sc, 6.0),
                hgap(6.0),
                kicker_c(
                    fmt::state(d.state),
                    if d.state == LinkState::Offline {
                        MUTED
                    } else {
                        sc
                    }
                ),
            ]
            .align_y(Alignment::Center),
            gap(space::M),
            text(fmt::name(d))
                .font(DISPLAY)
                .size(30)
                .color(if d.state == LinkState::Offline {
                    SUB
                } else {
                    PAPER
                }),
            mono(fmt::model(d), MUTED),
            gap(space::M),
            meta,
        ]
        .padding(space::L),
    )
    .width(Length::Fill)
    .padding(0)
    .style(theme::card(selected))
    .on_press(Message::Select(d.id))
    .into()
}

fn device_detail<'a>(app: &'a App, d: &'a Device) -> El<'a> {
    let online = d.state != LinkState::Offline;
    let (fp_a, fp_b) = fmt::fingerprint(&d.fingerprint);
    let fp_lines = column![mono(fp_a, PAPER), mono(fp_b, PAPER)];
    let mut caps = row![].spacing(space::S);
    for c in &d.caps {
        caps = caps.push(tag_outline(fmt::cap(*c), SUB));
    }
    let kind_hint = match d.kind {
        Kind::Phone => t("O comando principal."),
        Kind::Wearable => t("Uma ponte para o pulso, através do telemóvel."),
        Kind::Tablet => t("Um segundo ecrã, quando aparece."),
    };
    column![
        row![
            kicker(fmt::kind(d.kind)),
            fill_x(),
            tag(
                format!("■ {}", fmt::state(d.state)),
                state_color(d.state),
                VOID
            )
        ]
        .align_y(Alignment::Center),
        gap(space::M),
        headline(fmt::name(d), size::D2),
        gap(space::S),
        deck_s(format!("{}. {}", fmt::os(d), kind_hint)),
        gap(space::XL),
        row![
            container(stat(
                t("BATERIA"),
                if online {
                    fmt::battery(d)
                } else {
                    fmt::DASH.into()
                },
                if d.charging { "% +" } else { "%" },
                PAPER
            ))
            .width(Length::Fill),
            container(stat(
                t("LATÊNCIA"),
                if online {
                    fmt::latency(d.latency_ms)
                } else {
                    fmt::DASH.into()
                },
                "ms",
                PAPER
            ))
            .width(Length::Fill),
            container(stat(
                t("SINAL"),
                if online {
                    fmt::opt(d.rssi)
                } else {
                    fmt::DASH.into()
                },
                "dBm",
                PAPER
            ))
            .width(Length::Fill),
        ],
        gap(space::XXL),
        phone_state(app, d),
        if has_phone_state(app, d) {
            crate::pages::battery_block(app)
        } else {
            Space::new().into()
        },
        subhead(
            if has_phone_state(app, d) { "C" } else { "A" },
            t("Identidade")
        ),
        rename_row(app, d),
        kv_text(t("MODELO"), fmt::model(d)),
        kv_text(t("SISTEMA"), fmt::os(d)),
        kv_text("APP HYPRLINK", fmt::opt(d.app_version.clone())),
        kv_text(t("ENDEREÇO"), fmt::opt(d.addr.clone())),
        kv(t("CERTIFICADO SHA-256"), fp_lines),
        kv_text(t("EMPARELHADO DESDE"), fmt::date(d.paired_since)),
        kv_text(t("PROTOCOLO"), "hyprlink/1 · QUIC · mTLS"),
        gap(space::XXL),
        subhead(
            if has_phone_state(app, d) { "D" } else { "B" },
            t("Capacidades")
        ),
        caps.wrap().vertical_spacing(space::S),
        gap(space::XXL),
        row![
            btn(t("PING"), theme::ghost, online.then_some(Message::Ping(d.id))),
            hgap(space::S),
            btn(
                t("ENVIAR CLIPBOARD"),
                theme::ghost,
                online.then_some(Message::Clipboard(d.id))
            ),
            fill_x(),
            btn(
                t("REVOGAR CERTIFICADO"),
                theme::danger,
                Some(Message::Unpair(d.id))
            ),
        ],
    ]
    .into()
}

pub fn pairing<'a>(app: &'a App, ticket: &'a PairingTicket) -> El<'a> {
    let mut code = row![].spacing(space::S);
    for ch in ticket.code.clone().unwrap_or_default().chars() {
        code = code.push(
            container(text(ch.to_string()).font(DISPLAY).size(56).color(PAPER))
                .width(52)
                .center_x(52)
                .padding(Padding::from([4, 0]))
                .style(theme::outline(LINE_STRONG)),
        );
    }
    let left = column![
        kicker_c(t("EMPARELHAR — PASSO 1 DE 2"), ACID),
        gap(space::M),
        headline(t("APONTA O TELEMÓVEL."), size::D3),
        gap(space::M),
        deck_s(t("Abre a HyprLink no Android e lê o código. Ou escreve-o à mão.")),
        gap(space::XL),
        if ticket.code.is_some() {
            column![
                kicker(t("CÓDIGO DE VERIFICAÇÃO")),
                gap(space::S),
                code,
                gap(space::XL)
            ]
        } else {
            column![]
        },
        match ticket.expires_in {
            Some(left) => column![
                row![
                    kicker(t("EXPIRA EM")),
                    fill_x(),
                    kicker_c(format!("{left:.0} s"), PAPER)
                ],
                gap(space::S),
                bar(left / 120.0, ACID, 2.0),
                gap(space::S),
            ],
            None => column![kicker(t("O CÓDIGO SERVE UMA VEZ")), gap(space::S)],
        },
        row![
            square(
                if (app.t * 2.0).fract() < 0.5 {
                    ACID
                } else {
                    ACID_DIM
                },
                6.0
            ),
            hgap(space::S),
            mono(t("à espera de resposta…"), SUB)
        ]
        .align_y(Alignment::Center),
        gap(space::XL),
        row![
            btn(t("CANCELAR"), theme::ghost, Some(Message::CancelPair)),
            fill_x(),
            kicker(t("ESC PARA FECHAR"))
        ]
        .align_y(Alignment::Center),
    ]
    .width(Length::Fill);

    let qr: El = match &app.qr {
        Some(data) => container(qr_code(data).cell_size(5).style(|_| qr_code::Style {
            cell: VOID,
            background: PAPER,
        }))
        .padding(14)
        .style(theme::fill(PAPER))
        .into(),
        None => Space::new().into(),
    };

    let card = container(
        row![
            left,
            hgap(space::GUTTER),
            column![qr, gap(space::S), mono(t("fp | host:porta | token"), MUTED)]
                .align_x(Alignment::Center)
        ]
        .align_y(Alignment::Center),
    )
    .width(820)
    .padding(space::GUTTER)
    .style(|th| {
        let mut s = theme::frame(th);
        s.background = Some(INK_0.into());
        s.border.color = LINE_STRONG;
        s
    });

    container(card)
        .center(Length::Fill)
        .style(theme::scrim)
        .into()
}

// ═════════════════════════════ 03 SECRETÁRIA ═════════════════════════════

pub fn desk(app: &App) -> El<'_> {
    fn tile<'a>(ws: &'a crate::link::Workspace, active_ws: u8) -> El<'a> {
        let active = ws.id == active_ws;
        let occupied = !ws.clients.is_empty();
        let fg = if active {
            VOID
        } else if occupied {
            PAPER
        } else {
            FAINT
        };
        let sub = if active { alpha(VOID, 0.72) } else { MUTED };
        let mut apps = column![].spacing(2);
        for c in ws.clients.iter().take(3) {
            apps = apps.push(
                text(c.as_str())
                    .font(MONO)
                    .size(11)
                    .color(sub)
                    .wrapping(Wrapping::None),
            );
        }
        if ws.clients.len() > 3 {
            apps = apps.push(
                text(format!("+{}", ws.clients.len() - 3))
                    .font(MONO)
                    .size(11)
                    .color(sub),
            );
        }
        if !occupied {
            apps = apps.push(text(t("vazio")).font(serif_italic()).size(15).color(FAINT));
        }
        button(
            column![
                row![
                    text(format!("{:02}", ws.id))
                        .font(DISPLAY)
                        .size(40)
                        .color(fg)
                        .line_height(LineHeight::Relative(1.0)),
                    fill_x(),
                    if active {
                        text(t("● ATIVO")).font(MONO_SEMI).size(10).color(VOID)
                    } else {
                        text(format!("{}", ws.clients.len()))
                            .font(MONO)
                            .size(10)
                            .color(FAINT)
                    },
                ]
                .align_y(Alignment::Start),
                iced::widget::space::vertical(),
                apps,
            ]
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(136)
        .padding(space::L)
        .style(theme::tile(active, occupied))
        .on_press(Message::SwitchWs(ws.id))
        .into()
    }

    let mut r1 = row![].spacing(space::S);
    let mut r2 = row![].spacing(space::S);
    for (i, ws) in app.workspaces.iter().enumerate() {
        if i < 5 {
            r1 = r1.push(tile(ws, app.active_ws));
        } else {
            r2 = r2.push(tile(ws, app.active_ws));
        }
    }

    let mut gestures = column![subhead("A", t("Gestos no telemóvel"))];
    for (i, g) in app.gestures.iter().enumerate() {
        gestures = gestures.push(setting(
            t(g.gesture),
            format!("hyprctl dispatch {}", g.action),
            switch(g.on, move |b| Message::Gesture(i, b)),
        ));
    }

    let active_clients = app
        .workspaces
        .iter()
        .find(|w| w.id == app.active_ws)
        .map(|w| {
            if w.clients.is_empty() {
                "—".to_string()
            } else {
                w.clients.join(", ")
            }
        })
        .unwrap_or_default();

    let compositor = column![
        subhead("B", t("Ligação ao compositor")),
        setting(
            t("Seguir o telemóvel"),
            t("o PC muda de workspace quando o telemóvel pede"),
            switch(app.follow_phone, Message::FollowPhone)
        ),
        kv_text(t("MONITOR"), "eDP-1 · 1920×1080 @ 60"),
        kv_text(
            t("ATIVO"),
            tr!("workspace {} — {}", app.active_ws, active_clients)
        ),
        kv_text(
            t("JANELA ATIVA"),
            app.active_window
                .as_ref()
                .map(|w| format!("{} — {}", w.class, w.title))
                .unwrap_or_else(|| fmt::DASH.into())
        ),
        kv_text("IPC", ".socket2.sock"),
        kv_text(t("EVENTOS"), "workspace>> activewindow>>"),
        gap(space::XL),
        deck_s(
            t("O telemóvel não controla o Hyprland: pede. O daemon decide, e o compositor obedece.")
        ),
    ];

    column![
        opener(
            "03",
            t("SECRETÁRIA"),
            t("A SECRETÁRIA, VISTA DO BOLSO."),
            t("Dez workspaces do Hyprland, ao alcance do polegar.")
        ),
        r1,
        gap(space::S),
        r2,
        gap(space::XXL),
        row![
            gestures.width(fill_portion(3)),
            hgap(space::GUTTER),
            compositor.width(fill_portion(2))
        ],
        gap(space::GUTTER),
        row![
            container(crate::pages::shortcuts(app)).width(fill_portion(3)),
            hgap(space::GUTTER),
            container(crate::pages::trackpad(app)).width(fill_portion(2)),
        ],
    ]
    .into()
}

// ═════════════════════════════ 04 ESPELHO ═════════════════════════════

/// The screen-mirror body, shown inside §04 «Câmara & Ecrã» in Ecrã mode.
pub fn mirror_body(app: &App) -> El<'_> {
    let live = app.mirror.is_some();
    let cfg = app.mirror_cfg;
    let phone = canvas(Phone {
        t: app.t,
        live,
        clock: Local::now().format("%H:%M").to_string(),
    })
    .width(400)
    .height(680);

    let status: El = if let Some(m) = app.mirror {
        column![
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
                container(stat(t("IMAGENS/S"), format!("{:.0}", m.fps), "fps", PAPER))
                    .width(Length::Fill),
                container(stat(
                    t("DESCODIFICAÇÃO"),
                    format!("{:.1}", m.decode_ms),
                    "ms",
                    PAPER
                ))
                .width(Length::Fill),
                container(stat(
                    t("DÉBITO"),
                    format!("{:.1}", m.kbps / 1000.0),
                    "Mb/s",
                    PAPER
                ))
                .width(Length::Fill),
                container(stat(
                    t("PERDIDAS"),
                    format!("{}", m.dropped),
                    "",
                    if m.dropped > 20 { HOT } else { PAPER }
                ))
                .width(Length::Fill),
            ],
        ]
        .into()
    } else {
        column![
            headline(t("PARADO"), size::D3).color(MUTED),
            gap(space::S),
            deck_s(t("Escolhe a codificação e carrega em iniciar. A janela abre-se flutuante, presa ao topo.")),
        ]
        .into()
    };

    let codecs = Codec::ALL.iter().fold(row![].spacing(space::S), |r, c| {
        r.push(chip(fmt::codec(*c), cfg.codec == *c, Message::Codec(*c)))
    });
    let fps = [30u32, 60, 90, 120]
        .iter()
        .fold(row![].spacing(space::S), |r, f| {
            r.push(chip(format!("{f}"), cfg.max_fps == *f, Message::Fps(*f)))
        });
    let scales = [0.5f32, 0.75, 1.0]
        .iter()
        .fold(row![].spacing(space::S), |r, s| {
            r.push(chip(
                format!("{:.0}%", s * 100.0),
                (cfg.scale - s).abs() < 0.01,
                Message::Scale(*s),
            ))
        });

    let (w, h) = ((1080.0 * cfg.scale) as u32, (2400.0 * cfg.scale) as u32);
    let code = container(
        column![
            mono("# ~/.config/hypr/hyprland.conf", FAINT),
            mono("windowrulev2 = float, class:^(hyprlink-mirror)$", PAPER),
            mono(
                format!(
                    "windowrulev2 = size {} {}, class:^(hyprlink-mirror)$",
                    w / 2,
                    h / 2
                ),
                PAPER
            ),
            mono("windowrulev2 = pin, class:^(hyprlink-mirror)$", PAPER),
            mono(
                "windowrulev2 = move 100%-w-24 64, class:^(hyprlink-mirror)$",
                PAPER
            ),
        ]
        .spacing(4),
    )
    .padding(space::L)
    .width(Length::Fill)
    .style(theme::frame);

    let controls = column![
        status,
        gap(space::XXL),
        subhead("A", t("Codificação")),
        setting(
            t("Codec"),
            t("MediaCodec no Android · VA-API no desktop"),
            codecs.into()
        ),
        setting(
            t("Imagens por segundo"),
            t("limite superior; o telemóvel pode baixar"),
            fps.into()
        ),
        setting(
            t("Escala"),
            tr!("{} a partir de 1080×2400", format!("{w}×{h}")),
            scales.into()
        ),
        column![
            row![
                column![
                    text(t("Débito"))
                        .font(SANS_MEDIUM)
                        .size(size::BODY)
                        .color(PAPER),
                    gap(3.0),
                    mono(t("stream unidirecional QUIC dedicado"), MUTED).size(11)
                ]
                .width(Length::Fill),
                text(format!("{:.0} Mb/s", cfg.bitrate_mbps))
                    .font(MONO_SEMI)
                    .size(12)
                    .color(ACID),
            ]
            .align_y(Alignment::Center),
            gap(space::M),
            slider(2.0..=40.0, cfg.bitrate_mbps, Message::Bitrate)
                .step(1.0_f32)
                .style(theme::fader),
        ]
        .padding(Padding::from([12, 0])),
        rule(),
        gap(space::XXL),
        subhead("B", t("Regra de janela")),
        code,
        gap(space::XL),
        if live {
            btn(
                t("■  PARAR TRANSMISSÃO"),
                theme::danger,
                Some(Message::MirrorStop),
            )
        } else {
            btn(
                t("▶  INICIAR ESPELHO"),
                theme::primary,
                Some(Message::MirrorStart),
            )
        },
    ]
    .width(Length::Fill);

    column![row![
        column![
            phone,
            gap(space::S),
            kicker(format!(
                "{} · {} FPS · {:.0} MB/S",
                fmt::codec(cfg.codec),
                cfg.max_fps,
                cfg.bitrate_mbps
            ))
        ]
        .align_x(Alignment::Center),
        hgap(space::GUTTER),
        controls
    ],]
    .into()
}

// ═════════════════════════════ 05 ÁUDIO ═════════════════════════════

fn dbfs(v: f32) -> f32 {
    if v <= 0.001 {
        -60.0
    } else {
        (20.0 * v.log10()).max(-60.0)
    }
}

#[allow(clippy::too_many_arguments)]
fn channel<'a>(
    num: &'a str,
    route: &'a str,
    title: &'a str,
    blurb: &'a str,
    on: bool,
    // `None` = the daemon has no meter for this channel (yet).
    level: Option<f32>,
    peak: f32,
    hist: Vec<f32>,
    gain: f32,
    toggle: fn(bool) -> Message,
    on_gain: fn(f32) -> Message,
    rows: Vec<El<'a>>,
) -> El<'a> {
    let measured = level.is_some();
    let level = level.unwrap_or(0.0);
    let db = dbfs(level);
    let scale = row![
        kicker("−60"),
        fill_x(),
        kicker("−45"),
        fill_x(),
        kicker("−30"),
        fill_x(),
        kicker("−15"),
        fill_x(),
        kicker("0 dBFS"),
    ];
    let mut col = column![
        row![
            kicker_c(num, ACID),
            hgap(space::M),
            kicker(route),
            fill_x(),
            switch(on, toggle)
        ]
        .align_y(Alignment::Center),
        gap(space::XL),
        headline(title, size::D3),
        gap(space::S),
        container(deck_s(blurb)).height(50),
        gap(space::L),
        row![
            text(match (on, measured) {
                (true, true) => format!("{db:.0}"),
                (true, false) => fmt::DASH.into(),
                _ => "OFF".into(),
            })
            .font(DISPLAY)
            .size(72)
            .color(if on {
                if db > -3.0 { HOT } else { PAPER }
            } else {
                FAINT
            })
            .line_height(LineHeight::Relative(1.0)),
            hgap(space::S),
            mono(
                if on && !measured {
                    t("dBFS · sem medidor")
                } else {
                    "dBFS"
                },
                MUTED
            ),
            fill_x(),
            if on {
                tag(t("● ABERTO"), ACID, VOID)
            } else {
                tag_outline(t("FECHADO"), MUTED)
            },
        ]
        .align_y(Alignment::End),
        gap(space::L),
        canvas(Meter {
            level: if on && measured {
                (dbfs(level) + 60.0) / 60.0
            } else {
                0.0
            },
            peak: if on && measured {
                (dbfs(peak) + 60.0) / 60.0
            } else {
                0.0
            },
            color: ACID,
            segments: 48
        })
        .width(Length::Fill)
        .height(20),
        gap(space::S),
        scale,
        gap(space::L),
        spark(
            hist,
            if on && measured { PAPER } else { FAINT },
            0.0,
            1.0,
            44.0
        ),
        gap(space::XL),
        column![
            row![
                text(t("Ganho"))
                    .font(SANS_MEDIUM)
                    .size(size::BODY)
                    .color(PAPER),
                fill_x(),
                text(format!("{:+.1} dB", 20.0 * (gain / 0.72).log10()))
                    .font(MONO_SEMI)
                    .size(12)
                    .color(ACID),
            ],
            gap(space::M),
            slider(0.0..=1.0, gain, on_gain)
                .step(0.01_f32)
                .style(theme::fader),
        ],
        gap(space::L),
        rule(),
    ];
    for r in rows {
        col = col.push(r);
    }
    container(col)
        .padding(space::XL)
        .width(Length::Fill)
        .style(if on {
            theme::frame_active
        } else {
            theme::frame
        })
        .into()
}

pub fn audio(app: &App) -> El<'_> {
    let mic = channel(
        "01",
        t("TELEMÓVEL → PC"),
        t("MICROFONE"),
        t("O PC pede, o telemóvel aceita — e passa a ser um microfone sem fios no PipeWire."),
        app.mic_on,
        app.mic_measured.then_some(app.mic),
        app.mic_peak,
        app.mic_hist.to_vec(),
        app.mic_gain,
        Message::Mic,
        Message::MicGain,
        vec![
            kv_text(t("PACOTES"), packets::MIC_FAMILY),
            kv_text(t("FONTE PIPEWIRE"), "hyprlink-mic"),
            kv_text(t("FORMATO"), "PCM · 48 kHz · mono"),
            kv_text(t("MÓDULO"), "mic.rs"),
        ],
    );
    let tap = channel(
        "02",
        t("PC → TELEMÓVEL"),
        t("RETORNO"),
        t("O som do desktop no telemóvel. Em modo coluna, o telemóvel é uma saída do PC."),
        app.tap_on,
        app.tap_measured.then_some(app.tap),
        app.tap_peak,
        app.tap_hist.to_vec(),
        app.tap_gain,
        Message::Tap,
        Message::TapGain,
        vec![
            setting(
                t("Modo coluna"),
                t("o telemóvel aparece no PC como a saída hyprlink-speaker"),
                switch(app.speaker, Message::Speaker),
            ),
            kv_text(
                t("ORIGEM"),
                if app.speaker {
                    "sink hyprlink-speaker"
                } else {
                    t("todo o áudio do PC")
                },
            ),
            kv_text(t("PACOTES"), packets::TAP_FAMILY),
            kv_text(t("TRANSPORTE"), t("datagramas QUIC")),
            kv_text(t("MÓDULO"), "tap.rs · speaker.rs"),
        ],
    );
    column![
        opener(
            "05",
            t("ÁUDIO"),
            t("DOIS SENTIDOS, UM FIO."),
            t("A voz sobe; a música desce. PCM a 48 kHz, por cima do mesmo QUIC.")
        ),
        crate::pages::phone_audio(app),
        gap(space::XXL),
        row![mic, hgap(space::XL), tap],
        gap(space::GUTTER),
        crate::pages::mixer(app),
    ]
    .into()
}

// ═════════════════════════════ 06 SENSORES ═════════════════════════════

pub fn sensors_grid(app: &App) -> El<'_> {
    let s = app.sensors;
    let card = |k: SensorKind| -> El<'_> {
        let (name, value, unit, detail, target, min, max) = match k {
            SensorKind::Accel => (
                t("ACELERÓMETRO"),
                format!(
                    "{:.2}",
                    (s.accel[0].powi(2) + s.accel[1].powi(2) + s.accel[2].powi(2)).sqrt()
                ),
                "m/s²",
                format!(
                    "x {:+.2}  y {:+.2}  z {:+.2}",
                    s.accel[0], s.accel[1], s.accel[2]
                ),
                "iio:hyprlink-accel",
                9.4,
                10.2,
            ),
            SensorKind::Gyro => (
                t("GIROSCÓPIO"),
                format!(
                    "{:.3}",
                    (s.gyro[0].powi(2) + s.gyro[1].powi(2) + s.gyro[2].powi(2)).sqrt()
                ),
                "rad/s",
                format!(
                    "x {:+.3}  y {:+.3}  z {:+.3}",
                    s.gyro[0], s.gyro[1], s.gyro[2]
                ),
                "iio:hyprlink-gyro",
                0.0,
                0.5,
            ),
            SensorKind::Light => (
                t("LUZ AMBIENTE"),
                format!("{:.0}", s.lux),
                "lux",
                t("→ brightnessctl, curva suave").into(),
                "iio:hyprlink-als",
                100.0,
                500.0,
            ),
            SensorKind::Proximity => (
                t("PROXIMIDADE"),
                if s.proximity_cm < 1.0 {
                    t("PERTO").into()
                } else {
                    t("LIVRE").into()
                },
                if s.proximity_cm < 1.0 { "0 cm" } else { "5 cm" },
                t("telemóvel virado para baixo → silêncio").into(),
                "dbus:dev.hyprlink.Proximity",
                -1.0,
                6.0,
            ),
            SensorKind::Pressure => (
                t("BARÓMETRO"),
                format!("{:.1}", s.pressure_hpa),
                "hPa",
                t("altitude relativa ±0.4 m").into(),
                "dbus:dev.hyprlink.Pressure",
                1011.6,
                1013.2,
            ),
            SensorKind::Thermal => (
                t("TEMPERATURA"),
                format!("{:.1}", s.battery_temp),
                "°C",
                t("bateria · throttling a 42 °C").into(),
                "hwmon:hyprlink",
                29.0,
                34.0,
            ),
        };
        let on = app.bridges.contains(&k);
        let hist = app
            .sensor_hist
            .iter()
            .find(|(kk, _)| *kk == k)
            .map(|(_, h)| h.to_vec())
            .unwrap_or_default();
        container(column![
            row![
                kicker(name),
                fill_x(),
                switch(on, move |b| Message::Bridge(k, b))
            ]
            .align_y(Alignment::Center),
            gap(space::L),
            row![
                text(value)
                    .font(DISPLAY)
                    .size(size::D3)
                    .color(if on { PAPER } else { SUB })
                    .line_height(LineHeight::Relative(1.0)),
                hgap(6.0),
                mono(unit, MUTED),
            ]
            .align_y(Alignment::End),
            gap(space::S),
            mono(detail, MUTED).size(11.5),
            gap(space::L),
            spark(hist, if on { ACID } else { FAINT }, min, max, 52.0),
            gap(space::M),
            row![
                kicker_c(
                    if on { t("PONTE →") } else { t("PONTE ·") },
                    if on { ACID } else { FAINT }
                ),
                hgap(space::S),
                kicker(target)
            ],
        ])
        .padding(space::XL)
        .width(Length::Fill)
        .style(if on {
            theme::frame_active
        } else {
            theme::frame
        })
        .into()
    };
    let k = SensorKind::ALL;
    column![
        row![card(k[0]), card(k[1]), card(k[2])].spacing(space::L),
        gap(space::L),
        row![card(k[3]), card(k[4]), card(k[5])].spacing(space::L),
        gap(space::XL),
        mono(
            tr!(
                "{} de 6 sensores em ponte · 20 Hz · {} (proposto)",
                app.bridges.len(),
                packets::SENSOR_FRAME
            ),
            MUTED
        ),
    ]
    .into()
}

// ═════════════════════════════ 07 PRESENÇA ═════════════════════════════

pub fn presence_body(app: &App) -> El<'_> {
    let (zone, zc) = if !app.rssi_known {
        (t("Ainda sem leitura — o telemóvel não envia RSSI."), MUTED)
    } else if app.rssi > app.unlock_at {
        (t("Perto — ao alcance da mão."), ACID)
    } else if app.rssi > app.lock_at {
        (t("Na sala, mas longe da secretária."), PAPER)
    } else {
        (t("Longe. A sessão vai trancar-se."), HOT)
    };
    let now = column![
        kicker(tr!(
            "SINAL AGORA · {}",
            app.device().map(fmt::name).unwrap_or_else(|| "—".into())
        )),
        gap(space::S),
        row![
            text(if app.rssi_known {
                format!("{:.0}", app.rssi)
            } else {
                "—".to_string()
            })
                .font(DISPLAY)
                .size(size::D1)
                .color(zc)
                .line_height(LineHeight::Relative(1.0)),
            hgap(space::S),
            mono("dBm", MUTED),
        ]
        .align_y(Alignment::End),
        gap(space::S),
        deck_s(zone),
    ]
    .width(300);

    let scale = canvas(RssiScale {
        rssi: app.rssi,
        known: app.rssi_known,
        lock_at: app.lock_at,
        unlock_at: app.unlock_at,
        history: app.rssi_hist.to_vec(),
    })
    .width(Length::Fill)
    .height(190);

    let threshold = |title: &'static str,
                     sub: &'static str,
                     v: f32,
                     c: Color,
                     f: fn(f32) -> Message|
     -> El<'static> {
        column![
            row![
                column![
                    text(title).font(SANS_MEDIUM).size(size::BODY).color(PAPER),
                    gap(3.0),
                    mono(sub, MUTED).size(11)
                ]
                .width(Length::Fill),
                text(format!("{v:.0} dBm"))
                    .font(MONO_SEMI)
                    .size(12)
                    .color(c),
            ]
            .align_y(Alignment::Center),
            gap(space::M),
            slider(-95.0..=-40.0, v, f)
                .step(1.0_f32)
                .style(theme::fader),
        ]
        .width(Length::Fill)
        .into()
    };

    let mut rules = column![subhead("·", t("Regras"))];
    for (i, r) in app.rules.iter().enumerate() {
        rules = rules.push(column![
            row![
                text(format!("{:02}", i + 1))
                    .font(DISPLAY)
                    .size(28)
                    .color(if r.on { ACID } else { FAINT })
                    .width(56),
                column![
                    text(t(r.trigger))
                        .font(SANS_SEMI)
                        .size(15)
                        .color(if r.on { PAPER } else { SUB }),
                    gap(2.0),
                    mono(t(r.detail), MUTED).size(11.5)
                ]
                .width(fill_portion(3)),
                row![
                    text("→").font(MONO).size(13).color(FAINT),
                    hgap(space::S),
                    mono(t(r.action), if r.on { PAPER } else { MUTED })
                ]
                .width(fill_portion(3))
                .align_y(Alignment::Center),
                switch(r.on, move |b| Message::Rule(i, b)),
            ]
            .align_y(Alignment::Center)
            .padding(Padding::from([14, 0])),
            rule(),
        ]);
    }

    column![
        row![now, hgap(space::GUTTER), scale].align_y(Alignment::End),
        gap(space::XXL),
        row![
            threshold(
                t("Limiar de bloqueio"),
                t("abaixo disto durante 10 s → hyprlock"),
                app.lock_at,
                HOT,
                Message::LockAt
            ),
            hgap(space::GUTTER),
            threshold(
                t("Limiar de desbloqueio"),
                t("acima disto, com mTLS válido → abre"),
                app.unlock_at,
                ACID,
                Message::UnlockAt
            ),
        ],
        gap(space::XXL),
        rules,
    ]
    .into()
}

// ═════════════════════════ 09 SENSORES & PRESENÇA ═════════════════════════

pub fn sensors(app: &App) -> El<'_> {
    column![
        opener(
            "09",
            t("SENSORES & PRESENÇA"),
            t("ESTÁS AQUI?"),
            t("A sessão tranca-se quando te afastas; o que o telemóvel sente chega ao desktop.")
        ),
        crate::pages::proposed_banner(
            t("Ainda sem pacotes no protocolo: presence.rssi e sensor.frame estão propostos. Esta página mostra o desenho com dados simulados.")
        ),
        gap(space::XXL),
        subhead("A", t("Presença")),
        presence_body(app),
        gap(space::GUTTER),
        subhead("B", t("Sensores em ponte")),
        sensors_grid(app),
    ]
    .into()
}

// ═════════════════════════════ 10 DIÁRIO ═════════════════════════════

pub fn journal(app: &App) -> El<'_> {
    let q = app.query.to_lowercase();
    let visible: Vec<_> = app
        .packets
        .iter()
        .rev()
        .filter(|p| match app.filter {
            Filter::All => true,
            Filter::Tx => p.dir == Dir::Tx,
            Filter::Rx => p.dir == Dir::Rx,
        })
        .filter(|p| q.is_empty() || p.kind.contains(&q) || p.note.to_lowercase().contains(&q))
        .take(120)
        .collect();
    let tx = app.packets.iter().filter(|p| p.dir == Dir::Tx).count();
    let rx = app.packets.len() - tx;

    let toolbar = row![
        chip(
            tr!("TODOS {}", app.packets.len()),
            app.filter == Filter::All,
            Message::Filter(Filter::All)
        ),
        chip(
            format!("TX {tx}"),
            app.filter == Filter::Tx,
            Message::Filter(Filter::Tx)
        ),
        chip(
            format!("RX {rx}"),
            app.filter == Filter::Rx,
            Message::Filter(Filter::Rx)
        ),
        hgap(space::L),
        text_input(t("filtrar: hypr.dispatch, mic, battery…"), &app.query)
            .on_input(Message::Query)
            .font(MONO)
            .size(12)
            .padding(Padding::from([8, 12]))
            .width(340)
            .style(theme::input),
        fill_x(),
        btn(
            if app.paused {
                t("▶  RETOMAR")
            } else {
                t("❚❚  PAUSAR")
            },
            theme::ghost,
            Some(Message::Pause(!app.paused))
        ),
        hgap(space::S),
        btn(t("LIMPAR"), theme::ghost, Some(Message::ClearJournal)),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);

    let header = row![
        kicker("T+ (S)").width(84),
        kicker(t("DIR")).width(36),
        kicker(t("TIPO")).width(fill_portion(4)),
        kicker(t("TAMANHO")).width(80),
        kicker(t("NOTA")).width(fill_portion(4)),
    ]
    .padding(Padding::from([8, 0]));

    let mut list = column![];
    if visible.is_empty() {
        list = list.push(
            container(deck_s(
                t("Nada a mostrar. O fio está calado — ou o filtro é demasiado exigente."),
            ))
            .padding(Padding::from([space::XL, 0.0])),
        );
    }
    for p in visible {
        list = list.push(packet_row(p, false));
    }

    column![
        opener(
            "10",
            t("DIÁRIO"),
            t("DIÁRIO DE BORDO."),
            t("Cada pacote, pela ordem em que aconteceu.")
        ),
        toolbar,
        gap(space::XL),
        header,
        rule_c(PAPER, 1.0),
        list,
    ]
    .into()
}


/// Campo «NOME»: o nome que o PC mostra para este telemóvel (vazio = o do telemóvel).
fn rename_row<'a>(app: &'a App, d: &'a Device) -> El<'a> {
    let editing = app.rename_for == Some(d.id);
    let value: &str = if editing { &app.rename_input } else { &d.name };
    let changed = editing && app.rename_input.trim() != d.name;
    column![
        kicker(t("NOME NO PC")),
        gap(space::XS),
        row![
            iced::widget::text_input(t("nome deste telemóvel"), value)
                .on_input(move |s| Message::RenameInput(d.id, s))
                .on_submit(Message::RenameSave(d.id))
                .size(14)
                .padding(Padding::from([9, 12]))
                .style(theme::input),
            hgap(space::S),
            btn(
                t("GUARDAR"),
                theme::primary,
                changed.then_some(Message::RenameSave(d.id))
            ),
        ]
        .align_y(Alignment::Center),
        gap(space::M),
    ]
    .into()
}
