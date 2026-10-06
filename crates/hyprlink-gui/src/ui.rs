//! Editorial building blocks. Every screen is composed from these, so the
//! rhythm (kicker → headline → deck → rule) stays identical everywhere.

#![allow(dead_code)] // design-system / protocol surface: not every token is used yet

use crate::app::Message;
use crate::theme::{self, *};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{Space, button, column, container, row, text, toggler};
use iced::{Alignment, Color, Element, Font, Length, Padding};

pub type El<'a> = Element<'a, Message>;

pub fn t<'a>(
    s: impl text::IntoFragment<'a>,
    font: Font,
    size: f32,
    color: Color,
) -> text::Text<'a> {
    text(s).font(font).size(size).color(color)
}

/// Small mono caps label — the "kicker" above everything.
pub fn kicker<'a>(s: impl text::IntoFragment<'a>) -> text::Text<'a> {
    t(s, MONO_MEDIUM, size::LABEL, MUTED).wrapping(Wrapping::None)
}

pub fn kicker_c<'a>(s: impl text::IntoFragment<'a>, c: Color) -> text::Text<'a> {
    t(s, MONO_SEMI, size::LABEL, c).wrapping(Wrapping::None)
}

/// Anton, tight leading.
pub fn headline<'a>(s: impl text::IntoFragment<'a>, sz: f32) -> text::Text<'a> {
    t(s, DISPLAY, sz, PAPER).line_height(LineHeight::Relative(0.98))
}

/// The magazine voice: Instrument Serif italic.
pub fn deck<'a>(s: impl text::IntoFragment<'a>) -> text::Text<'a> {
    t(s, serif_italic(), size::DECK, SUB).line_height(LineHeight::Relative(1.15))
}

pub fn deck_s<'a>(s: impl text::IntoFragment<'a>) -> text::Text<'a> {
    t(s, serif_italic(), size::DECK_S, SUB).line_height(LineHeight::Relative(1.2))
}

pub fn body<'a>(s: impl text::IntoFragment<'a>) -> text::Text<'a> {
    t(s, SANS, size::BODY, SUB).line_height(LineHeight::Relative(1.5))
}

pub fn mono<'a>(s: impl text::IntoFragment<'a>, c: Color) -> text::Text<'a> {
    t(s, MONO, size::DATA, c)
}

pub fn rule<'a>() -> El<'a> {
    container(Space::new().height(1).width(Length::Fill))
        .height(1)
        .width(Length::Fill)
        .style(fill(LINE))
        .into()
}

pub fn rule_c<'a>(c: Color, h: f32) -> El<'a> {
    container(Space::new().height(h).width(Length::Fill))
        .height(h)
        .width(Length::Fill)
        .style(fill(c))
        .into()
}

pub fn vrule<'a>() -> El<'a> {
    container(Space::new().width(1).height(Length::Fill))
        .width(1)
        .height(Length::Fill)
        .style(fill(LINE))
        .into()
}

pub fn gap<'a>(h: f32) -> El<'a> {
    Space::new().height(h).into()
}

pub fn hgap<'a>(w: f32) -> El<'a> {
    Space::new().width(w).into()
}

pub fn fill_x<'a>() -> El<'a> {
    iced::widget::space::horizontal().into()
}

pub fn square<'a>(c: Color, sz: f32) -> El<'a> {
    container(Space::new().width(sz).height(sz))
        .width(sz)
        .height(sz)
        .style(fill(c))
        .into()
}

/// Solid tag: `■ LIGADO`.
pub fn tag<'a>(s: impl text::IntoFragment<'a>, bg: Color, fg: Color) -> El<'a> {
    container(t(s, MONO_SEMI, size::LABEL, fg).wrapping(Wrapping::None))
        .padding(Padding::from([3, 7]))
        .style(fill(bg))
        .into()
}

pub fn tag_outline<'a>(s: impl text::IntoFragment<'a>, c: Color) -> El<'a> {
    container(t(s, MONO_SEMI, size::LABEL, c).wrapping(Wrapping::None))
        .padding(Padding::from([2, 6]))
        .style(outline(c))
        .into()
}

/// Section opener used at the top of every page.
pub fn opener<'a>(
    num: &'a str,
    title: &'a str,
    headline_text: impl text::IntoFragment<'a>,
    deck_text: impl text::IntoFragment<'a>,
) -> El<'a> {
    column![
        row![
            kicker_c(format!("§{num}"), ACID),
            hgap(space::M),
            kicker(title),
            fill_x(),
            kicker(hyprlink_gui::i18n::t("HYPRLINK · EDIÇÃO 0.1")),
        ]
        .align_y(Alignment::Center),
        gap(space::S),
        rule_c(PAPER, 2.0),
        gap(space::XL),
        headline(headline_text, size::D2),
        gap(space::M),
        deck(deck_text),
        gap(space::XXL),
    ]
    .into()
}

/// Sub-section header inside a page: `01 — Título ———————`.
pub fn subhead<'a>(num: &'a str, title: impl text::IntoFragment<'a>) -> El<'a> {
    column![
        row![
            kicker_c(num, ACID),
            hgap(space::M),
            t(title, SANS_SEMI, size::BODY, PAPER),
        ]
        .align_y(Alignment::Center),
        gap(space::S),
        rule(),
        gap(space::L),
    ]
    .into()
}

/// A big editorial number with label and unit.
pub fn stat<'a>(
    label: impl text::IntoFragment<'a>,
    value: impl text::IntoFragment<'a>,
    unit: impl text::IntoFragment<'a>,
    c: Color,
) -> El<'a> {
    column![
        kicker(label),
        gap(space::S),
        row![
            t(value, DISPLAY, size::D3, c).line_height(LineHeight::Relative(1.0)),
            hgap(6.0),
            t(unit, MONO, size::SMALL, MUTED),
        ]
        .align_y(Alignment::End),
    ]
    .into()
}

/// Key/value line in a data table.
pub fn kv<'a>(k: impl text::IntoFragment<'a>, v: impl Into<El<'a>>) -> El<'a> {
    column![
        row![
            kicker(k).width(Length::FillPortion(2)),
            container(v.into()).width(Length::FillPortion(3))
        ]
        .align_y(Alignment::Center)
        .padding(Padding::from([9, 0])),
        rule(),
    ]
    .into()
}

pub fn kv_text<'a>(k: impl text::IntoFragment<'a>, v: impl text::IntoFragment<'a>) -> El<'a> {
    kv(k, mono(v, PAPER))
}

pub fn btn<'a>(
    label: impl text::IntoFragment<'a>,
    style: impl Fn(&iced::Theme, button::Status) -> button::Style + 'a,
    msg: Option<Message>,
) -> button::Button<'a, Message> {
    button(
        text(label)
            .font(MONO_SEMI)
            .size(11.5)
            .wrapping(Wrapping::None),
    )
    .padding(Padding::from([10, 16]))
    .style(style)
    .on_press_maybe(msg)
}

pub fn chip<'a>(label: impl text::IntoFragment<'a>, selected: bool, msg: Message) -> El<'a> {
    button(
        text(label)
            .font(MONO_MEDIUM)
            .size(11.0)
            .wrapping(Wrapping::None),
    )
    .padding(Padding::from([6, 12]))
    .style(theme::chip_style(selected))
    .on_press(msg)
    .into()
}

pub fn switch<'a>(on: bool, f: impl Fn(bool) -> Message + 'a) -> El<'a> {
    toggler(on)
        .on_toggle(f)
        .size(18)
        .style(theme::switch_style)
        .into()
}

/// A setting line: title + explanation on the left, control on the right.
pub fn setting<'a>(
    title: impl text::IntoFragment<'a>,
    sub: impl text::IntoFragment<'a>,
    control: El<'a>,
) -> El<'a> {
    column![
        row![
            column![
                t(title, SANS_MEDIUM, size::BODY, PAPER),
                gap(3.0),
                t(sub, MONO, 11.0, MUTED),
            ]
            .width(Length::Fill),
            hgap(space::L),
            control,
        ]
        .align_y(Alignment::Center)
        .padding(Padding::from([12, 0])),
        rule(),
    ]
    .into()
}

/// A framed panel with consistent inner padding.
pub fn panel<'a>(content: impl Into<El<'a>>) -> container::Container<'a, Message> {
    container(content).padding(space::XL).style(theme::frame)
}
