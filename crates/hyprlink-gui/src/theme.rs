//! HYPRLINK design system.
//!
//! Black-first, editorial. One accent (acid), one alarm (hot), one cold
//! channel colour reserved for inbound traffic. Square corners, 1px
//! hairlines, a 4px spacing grid. Typography does the heavy lifting:
//!
//!   Anton              — display / headlines / numerals (always caps)
//!   Instrument Serif   — italic "decks" and pull quotes (the magazine voice)
//!   Inter              — interface body copy
//!   IBM Plex Mono      — labels, data, protocol identifiers

#![allow(dead_code)] // design-system / protocol surface: not every token is used yet

use iced::border::Radius;
use iced::font::{Family, Style as FontStyle, Weight};
use iced::theme::Palette;
use iced::widget::{button, container, scrollable, slider, text_input, toggler};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector, color};

// ───────────────────────────── colour ─────────────────────────────

pub const VOID: Color = color!(0x000000);
/// Panel surface — barely lifted from the void.
pub const INK_0: Color = color!(0x080808);
/// Raised surface (cards, inputs).
pub const INK_1: Color = color!(0x0E0E0E);
/// Hover surface.
pub const INK_2: Color = color!(0x161616);
/// Hairline rules.
pub const LINE: Color = color!(0x1C1C1C);
pub const LINE_STRONG: Color = color!(0x2C2C2C);

/// Primary text: warm newsprint white, never pure #FFF.
pub const PAPER: Color = color!(0xEDEBE4);
pub const SUB: Color = color!(0xA3A19A);
pub const MUTED: Color = color!(0x6A6965);
pub const FAINT: Color = color!(0x3D3C3A);

/// The accent. Used sparingly: active state, live values, the one thing to look at.
pub const ACID: Color = color!(0xD4FF3A);
pub const ACID_DIM: Color = color!(0x5B6B1E);
/// Alarm / recording / live-on-air.
pub const HOT: Color = color!(0xFF3355);
/// Inbound traffic only.
pub const COLD: Color = color!(0x5FD4E8);

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

// ───────────────────────────── type ─────────────────────────────

pub const DISPLAY: Font = Font::with_name("Anton");
pub const SERIF: Font = Font::with_name("Instrument Serif");
pub const SERIF_ITALIC: Font = Font {
    family: Family::Name("Instrument Serif"),
    style: FontStyle::Italic,
    ..Font::DEFAULT
};
pub const SANS: Font = Font::with_name("Inter");
pub const SANS_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..SANS
};
pub const SANS_SEMI: Font = Font {
    weight: Weight::Semibold,
    ..SANS
};
pub const SANS_BOLD: Font = Font {
    weight: Weight::Bold,
    ..SANS
};
pub const MONO: Font = Font::with_name("IBM Plex Mono");
pub const MONO_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..MONO
};
pub const MONO_SEMI: Font = Font {
    weight: Weight::Semibold,
    ..MONO
};

/// Type scale (px). Display sizes step by ~1.333 (perfect fourth).
pub mod size {
    pub const MASTHEAD: f32 = 132.0;
    pub const D1: f32 = 96.0;
    pub const D2: f32 = 64.0;
    pub const D3: f32 = 44.0;
    pub const H1: f32 = 30.0;
    pub const H2: f32 = 22.0;
    pub const DECK: f32 = 26.0;
    pub const DECK_S: f32 = 19.0;
    pub const BODY: f32 = 14.0;
    pub const SMALL: f32 = 12.5;
    pub const LABEL: f32 = 10.5;
    pub const DATA: f32 = 12.5;
}

/// Spacing on a 4px grid.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
    pub const GUTTER: f32 = 40.0;
}

pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Anton-Regular.ttf"),
    include_bytes!("../assets/fonts/InstrumentSerif-Regular.ttf"),
    include_bytes!("../assets/fonts/InstrumentSerif-Italic.ttf"),
    include_bytes!("../assets/fonts/Inter-Regular.otf"),
    include_bytes!("../assets/fonts/Inter-Medium.otf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.otf"),
    include_bytes!("../assets/fonts/Inter-Bold.otf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-SemiBold.ttf"),
];

pub fn theme() -> Theme {
    Theme::custom(
        "Hyprlink Noir",
        Palette {
            background: VOID,
            text: PAPER,
            primary: ACID,
            success: ACID,
            warning: color!(0xFFB020),
            danger: HOT,
        },
    )
}

fn hairline(c: Color) -> Border {
    Border {
        color: c,
        width: 1.0,
        radius: Radius::from(0.0),
    }
}

const NO_BORDER: Border = Border {
    color: Color::TRANSPARENT,
    width: 0.0,
    radius: Radius {
        top_left: 0.0,
        top_right: 0.0,
        bottom_right: 0.0,
        bottom_left: 0.0,
    },
};

// ───────────────────────────── containers ─────────────────────────────

pub fn void(_: &Theme) -> container::Style {
    container::Style {
        background: Some(VOID.into()),
        text_color: Some(PAPER),
        ..Default::default()
    }
}

pub fn rail(_: &Theme) -> container::Style {
    container::Style {
        background: Some(INK_0.into()),
        border: hairline(LINE),
        text_color: Some(PAPER),
        ..Default::default()
    }
}

/// An outlined editorial "box" — no fill, hairline frame.
pub fn frame(_: &Theme) -> container::Style {
    container::Style {
        background: Some(INK_0.into()),
        border: hairline(LINE),
        ..Default::default()
    }
}

pub fn frame_active(_: &Theme) -> container::Style {
    container::Style {
        background: Some(INK_1.into()),
        border: hairline(alpha(ACID, 0.55)),
        ..Default::default()
    }
}

pub fn fill(c: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(c.into()),
        ..Default::default()
    }
}

/// Solid acid block with black text: tags, active tiles.
pub fn acid_block(_: &Theme) -> container::Style {
    container::Style {
        background: Some(ACID.into()),
        text_color: Some(VOID),
        ..Default::default()
    }
}

pub fn hot_block(_: &Theme) -> container::Style {
    container::Style {
        background: Some(HOT.into()),
        text_color: Some(VOID),
        ..Default::default()
    }
}

pub fn outline(c: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        border: hairline(c),
        ..Default::default()
    }
}

pub fn toast(_: &Theme) -> container::Style {
    container::Style {
        background: Some(INK_1.into()),
        border: Border {
            color: alpha(ACID, 0.6),
            width: 1.0,
            radius: 0.0.into(),
        },
        shadow: Shadow {
            color: alpha(VOID, 0.9),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        text_color: Some(PAPER),
        ..Default::default()
    }
}

pub fn scrim(_: &Theme) -> container::Style {
    container::Style {
        background: Some(alpha(VOID, 0.82).into()),
        ..Default::default()
    }
}

// ───────────────────────────── buttons ─────────────────────────────

/// Navigation entry in the left rail.
pub fn nav(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if active {
                INK_2
            } else if hovered {
                INK_1
            } else {
                Color::TRANSPARENT
            })),
            text_color: if active || hovered { PAPER } else { SUB },
            border: NO_BORDER,
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Primary call to action: solid acid, black text. Inverts on hover.
pub fn primary(_: &Theme, status: button::Status) -> button::Style {
    let (bg, fg, border) = match status {
        button::Status::Hovered => (VOID, ACID, hairline(ACID)),
        button::Status::Pressed => (ACID_DIM, VOID, hairline(ACID_DIM)),
        button::Status::Disabled => (INK_1, FAINT, hairline(LINE)),
        button::Status::Active => (ACID, VOID, hairline(ACID)),
    };
    button::Style {
        background: Some(bg.into()),
        text_color: fg,
        border,
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Danger action (stop, unpair).
pub fn danger(_: &Theme, status: button::Status) -> button::Style {
    let (bg, fg) = match status {
        button::Status::Hovered | button::Status::Pressed => (HOT, VOID),
        _ => (Color::TRANSPARENT, HOT),
    };
    button::Style {
        background: Some(bg.into()),
        text_color: fg,
        border: hairline(HOT),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Secondary: hairline outline, paper text, acid on hover.
pub fn ghost(_: &Theme, status: button::Status) -> button::Style {
    let (bg, fg, line) = match status {
        button::Status::Hovered => (INK_2, ACID, alpha(ACID, 0.7)),
        button::Status::Pressed => (INK_1, ACID, ACID),
        button::Status::Disabled => (Color::TRANSPARENT, FAINT, LINE),
        button::Status::Active => (Color::TRANSPARENT, PAPER, LINE_STRONG),
    };
    button::Style {
        background: Some(bg.into()),
        text_color: fg,
        border: hairline(line),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Flat text-only button.
pub fn bare(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: match status {
            button::Status::Hovered | button::Status::Pressed => ACID,
            button::Status::Disabled => FAINT,
            button::Status::Active => SUB,
        },
        border: NO_BORDER,
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A selectable chip / segmented option.
pub fn chip_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, fg, line) = if selected {
            (PAPER, VOID, PAPER)
        } else if hovered {
            (INK_2, PAPER, LINE_STRONG)
        } else {
            (Color::TRANSPARENT, SUB, LINE_STRONG)
        };
        button::Style {
            background: Some(bg.into()),
            text_color: fg,
            border: hairline(line),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Workspace tile.
pub fn tile(active: bool, occupied: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, fg, line) = if active {
            (ACID, VOID, ACID)
        } else if hovered {
            (INK_2, PAPER, alpha(ACID, 0.6))
        } else if occupied {
            (INK_1, PAPER, LINE_STRONG)
        } else {
            (INK_0, MUTED, LINE)
        };
        button::Style {
            background: Some(bg.into()),
            text_color: fg,
            border: hairline(line),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Selectable card (device list).
pub fn card(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(
                if selected {
                    INK_1
                } else if hovered {
                    INK_1
                } else {
                    INK_0
                }
                .into(),
            ),
            text_color: PAPER,
            border: hairline(if selected {
                alpha(ACID, 0.6)
            } else if hovered {
                LINE_STRONG
            } else {
                LINE
            }),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

// ───────────────────────────── inputs ─────────────────────────────

pub fn switch_style(_: &Theme, status: toggler::Status) -> toggler::Style {
    let (on, hovered, disabled) = match status {
        toggler::Status::Active { is_toggled } => (is_toggled, false, false),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true, false),
        toggler::Status::Disabled { is_toggled } => (is_toggled, false, true),
    };
    let track = if disabled {
        INK_1
    } else if on {
        ACID
    } else {
        INK_1
    };
    let knob = if disabled {
        FAINT
    } else if on {
        VOID
    } else if hovered {
        PAPER
    } else {
        SUB
    };
    toggler::Style {
        background: track.into(),
        background_border_width: 1.0,
        background_border_color: if on && !disabled {
            ACID
        } else if hovered {
            alpha(ACID, 0.6)
        } else {
            LINE_STRONG
        },
        foreground: knob.into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: Some(PAPER),
        border_radius: Some(Radius::from(1.0)),
        padding_ratio: 0.18,
    }
}

pub fn fader(_: &Theme, status: slider::Status) -> slider::Style {
    let handle = match status {
        slider::Status::Active => PAPER,
        _ => ACID,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (ACID.into(), LINE_STRONG.into()),
            width: 2.0,
            border: NO_BORDER,
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Rectangle {
                width: 6,
                border_radius: 0.0.into(),
            },
            background: handle.into(),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

pub fn input(_: &Theme, status: text_input::Status) -> text_input::Style {
    let line = match status {
        text_input::Status::Focused { .. } => ACID,
        text_input::Status::Hovered => LINE_STRONG,
        _ => LINE,
    };
    text_input::Style {
        background: INK_0.into(),
        border: hairline(line),
        icon: MUTED,
        placeholder: FAINT,
        value: PAPER,
        selection: alpha(ACID, 0.35),
    }
}

pub fn scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut s = scrollable::default(theme, status);
    let dragged = matches!(status, scrollable::Status::Dragged { .. });
    let hovered = matches!(status, scrollable::Status::Hovered { .. });
    let rail = scrollable::Rail {
        background: None,
        border: NO_BORDER,
        scroller: scrollable::Scroller {
            background: (if dragged {
                ACID
            } else if hovered {
                MUTED
            } else {
                FAINT
            })
            .into(),
            border: NO_BORDER,
        },
    };
    s.vertical_rail = rail;
    s.horizontal_rail = rail;
    s.container = container::Style::default();
    s
}
