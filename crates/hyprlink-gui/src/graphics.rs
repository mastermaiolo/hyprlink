//! Canvas-drawn editorial graphics: sparklines, LED meters, the link
//! diagram, the phone mirror, the RSSI scale and the bottom ticker.

use crate::theme::*;
use iced::alignment::Vertical;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::widget::text::Alignment;
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme};

fn stroke(c: Color, w: f32) -> Stroke<'static> {
    Stroke::default().with_color(c).with_width(w)
}

fn label(
    frame: &mut Frame,
    s: impl Into<String>,
    at: Point,
    size: f32,
    color: Color,
    font: Font,
    align: Alignment,
) {
    frame.fill_text(Text {
        content: s.into(),
        position: at,
        color,
        size: Pixels(size),
        font,
        align_x: align,
        align_y: Vertical::Top,
        ..Text::default()
    });
}

/// Cheap hash noise for deterministic static.
fn hash(mut x: u32) -> f32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    (x & 0xFFFF) as f32 / 65535.0
}

// ───────────────────────────── sparkline ─────────────────────────────

pub struct Spark {
    pub data: Vec<f32>,
    pub color: Color,
    pub min: f32,
    pub max: f32,
    pub grid: bool,
}

impl<M> canvas::Program<M> for Spark {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        b: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, b.size());
        let (w, h) = (b.width, b.height);
        if self.grid {
            for i in 1..4 {
                let y = (h * i as f32 / 4.0).round() + 0.5;
                f.stroke(
                    &Path::line(Point::new(0.0, y), Point::new(w, y)),
                    Stroke {
                        line_dash: LineDash {
                            segments: &[1.0, 3.0],
                            offset: 0,
                        },
                        ..stroke(LINE_STRONG, 1.0)
                    },
                );
            }
        }
        let n = self.data.len();
        if n < 2 {
            return vec![f.into_geometry()];
        }
        let span = (self.max - self.min).max(f32::EPSILON);
        let pt = |i: usize, v: f32| {
            Point::new(
                w * i as f32 / (n - 1) as f32,
                (h - 2.0) - ((v - self.min) / span).clamp(0.0, 1.0) * (h - 4.0),
            )
        };
        let line = Path::new(|p| {
            p.move_to(pt(0, self.data[0]));
            for (i, v) in self.data.iter().enumerate().skip(1) {
                p.line_to(pt(i, *v));
            }
        });
        let area = Path::new(|p| {
            p.move_to(Point::new(0.0, h));
            for (i, v) in self.data.iter().enumerate() {
                p.line_to(pt(i, *v));
            }
            p.line_to(Point::new(w, h));
            p.close();
        });
        f.fill(&area, alpha(self.color, 0.07));
        f.stroke(&line, stroke(self.color, 1.25));
        let last = pt(n - 1, self.data[n - 1]);
        f.fill_rectangle(
            Point::new(last.x - 3.0, last.y - 3.0),
            Size::new(6.0, 6.0),
            self.color,
        );
        vec![f.into_geometry()]
    }
}

// ───────────────────────────── LED meter ─────────────────────────────

pub struct Meter {
    pub level: f32,
    pub peak: f32,
    pub color: Color,
    pub segments: usize,
}

impl<M> canvas::Program<M> for Meter {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        b: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, b.size());
        let gap = 2.0;
        let n = self.segments.max(1);
        let sw = (b.width - gap * (n - 1) as f32) / n as f32;
        let lit = (self.level.clamp(0.0, 1.0) * n as f32).round() as usize;
        let peak = ((self.peak.clamp(0.0, 1.0) * n as f32).round() as usize).min(n);
        for i in 0..n {
            let x = i as f32 * (sw + gap);
            let hot_zone = i as f32 / n as f32 > 0.86;
            let c = if i < lit {
                if hot_zone { HOT } else { self.color }
            } else if peak > 0 && i == peak - 1 {
                if hot_zone { HOT } else { PAPER }
            } else {
                INK_2
            };
            f.fill_rectangle(Point::new(x, 0.0), Size::new(sw, b.height), c);
        }
        vec![f.into_geometry()]
    }
}

// ───────────────────────────── link diagram ─────────────────────────────

/// The cover graphic: desktop ←→ phone, with packets travelling the wire.
pub struct LinkDiagram {
    pub t: f32,
    pub left: (String, String),
    pub right: (String, String),
    pub latency: f32,
    pub online: bool,
}

impl<M> canvas::Program<M> for LinkDiagram {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        b: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, b.size());
        let (w, h) = (b.width, b.height);
        let cy = (h * 0.5).round() + 0.5;
        let node = 14.0;
        let lx = 6.0;
        let rx = w - 6.0 - node;

        // Wire: two rails, TX above, RX below.
        let a = lx + node + 10.0;
        let z = rx - 10.0;
        let tx_y = cy - 6.0;
        let rx_y = cy + 6.0;
        let dash = |c| Stroke {
            line_dash: LineDash {
                segments: &[2.0, 4.0],
                offset: 0,
            },
            ..stroke(c, 1.0)
        };
        f.stroke(
            &Path::line(Point::new(a, tx_y), Point::new(z, tx_y)),
            dash(if self.online { ACID_DIM } else { FAINT }),
        );
        f.stroke(
            &Path::line(Point::new(a, rx_y), Point::new(z, rx_y)),
            dash(if self.online {
                alpha(COLD, 0.35)
            } else {
                FAINT
            }),
        );

        // Nodes.
        f.fill_rectangle(
            Point::new(lx, cy - node / 2.0),
            Size::new(node, node),
            PAPER,
        );
        f.stroke_rectangle(
            Point::new(rx, cy - node / 2.0),
            Size::new(node, node),
            stroke(PAPER, 1.5),
        );
        if self.online {
            f.fill_rectangle(
                Point::new(rx + 4.0, cy - node / 2.0 + 4.0),
                Size::new(node - 8.0, node - 8.0),
                ACID,
            );
        }

        // Packets.
        if self.online {
            let len = z - a;
            for i in 0..7 {
                let p = (self.t * 0.42 + i as f32 / 7.0 + hash(i) * 0.05).fract();
                // TX = PC → phone: travels right to left (the phone sits on the left).
                let x = z - p * len;
                let sz = if i % 3 == 0 { 6.0 } else { 3.0 };
                f.fill_rectangle(
                    Point::new(x - sz / 2.0, tx_y - sz / 2.0),
                    Size::new(sz, sz),
                    ACID,
                );
            }
            for i in 0..5 {
                let p = (self.t * 0.31 + i as f32 / 5.0 + hash(i + 40) * 0.07).fract();
                let x = a + p * len;
                let sz = if i % 2 == 0 { 4.0 } else { 2.0 };
                f.fill_rectangle(
                    Point::new(x - sz / 2.0, rx_y - sz / 2.0),
                    Size::new(sz, sz),
                    COLD,
                );
            }
        }

        // Captions.
        label(
            &mut f,
            &self.left.0,
            Point::new(lx, cy + 26.0),
            11.0,
            PAPER,
            MONO_SEMI,
            Alignment::Left,
        );
        label(
            &mut f,
            &self.left.1,
            Point::new(lx, cy + 42.0),
            11.0,
            MUTED,
            MONO,
            Alignment::Left,
        );
        label(
            &mut f,
            &self.right.0,
            Point::new(rx + node, cy + 26.0),
            11.0,
            PAPER,
            MONO_SEMI,
            Alignment::Right,
        );
        label(
            &mut f,
            &self.right.1,
            Point::new(rx + node, cy + 42.0),
            11.0,
            MUTED,
            MONO,
            Alignment::Right,
        );
        let mid = format!(
            "QUIC · mTLS · :7443 — {}",
            if self.online {
                format!("{:.1} ms", self.latency)
            } else {
                "—".into()
            }
        );
        label(
            &mut f,
            mid,
            Point::new(w / 2.0, cy - 30.0),
            11.0,
            SUB,
            MONO,
            Alignment::Center,
        );
        label(
            &mut f,
            "← TX",
            Point::new(z, tx_y - 18.0),
            9.5,
            ACID,
            MONO_SEMI,
            Alignment::Right,
        );
        label(
            &mut f,
            "RX →",
            Point::new(z, rx_y + 6.0),
            9.5,
            COLD,
            MONO_SEMI,
            Alignment::Right,
        );
        vec![f.into_geometry()]
    }
}

// ───────────────────────────── phone mirror ─────────────────────────────

pub struct Phone {
    pub t: f32,
    pub live: bool,
    pub clock: String,
}

impl<M> canvas::Program<M> for Phone {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        b: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, b.size());
        // Fit a 9:20 device into the bounds.
        let h = b.height - 2.0;
        let w = (h * 9.0 / 20.0).min(b.width - 2.0);
        let h = w * 20.0 / 9.0;
        let o = Point::new(
            ((b.width - w) / 2.0).round() + 0.5,
            ((b.height - h) / 2.0).round() + 0.5,
        );
        let screen = Rectangle::new(
            Point::new(o.x + 6.0, o.y + 6.0),
            Size::new(w - 12.0, h - 12.0),
        );

        f.fill_rectangle(o, Size::new(w, h), INK_0);
        f.stroke_rectangle(o, Size::new(w, h), stroke(LINE_STRONG, 1.0));
        // Crop marks — print-shop detail.
        for (cx, cy, dx, dy) in [
            (o.x, o.y, -1.0, -1.0),
            (o.x + w, o.y, 1.0, -1.0),
            (o.x, o.y + h, -1.0, 1.0),
            (o.x + w, o.y + h, 1.0, 1.0),
        ] {
            f.stroke(
                &Path::line(
                    Point::new(cx + dx * 6.0, cy),
                    Point::new(cx + dx * 18.0, cy),
                ),
                stroke(MUTED, 1.0),
            );
            f.stroke(
                &Path::line(
                    Point::new(cx, cy + dy * 6.0),
                    Point::new(cx, cy + dy * 18.0),
                ),
                stroke(MUTED, 1.0),
            );
        }

        if self.live {
            f.fill_rectangle(screen.position(), screen.size(), color(0x0B0B0B));
            let sx = screen.x;
            let sy = screen.y;
            let sw = screen.width;
            // Status bar.
            label(
                &mut f,
                &self.clock[..5.min(self.clock.len())],
                Point::new(sx + 12.0, sy + 8.0),
                10.0,
                PAPER,
                MONO_MEDIUM,
                Alignment::Left,
            );
            label(
                &mut f,
                "5G ▮▮▮▯ 78%",
                Point::new(sx + sw - 12.0, sy + 8.0),
                10.0,
                SUB,
                MONO,
                Alignment::Right,
            );
            // Big clock.
            label(
                &mut f,
                &self.clock[..5.min(self.clock.len())],
                Point::new(sx + sw / 2.0, sy + screen.height * 0.12),
                (sw * 0.30).min(72.0),
                PAPER,
                DISPLAY,
                Alignment::Center,
            );
            label(
                &mut f,
                "DOMINGO · 4 OUTUBRO",
                Point::new(
                    sx + sw / 2.0,
                    sy + screen.height * 0.12 + (sw * 0.30).min(72.0) * 1.25,
                ),
                9.5,
                SUB,
                MONO,
                Alignment::Center,
            );
            // App grid.
            let cols = 4;
            let pad = 16.0;
            let gap = 12.0;
            let cell = (sw - pad * 2.0 - gap * (cols - 1) as f32) / cols as f32;
            let top = sy + screen.height * 0.55;
            for r in 0..3 {
                for c in 0..cols {
                    let i = r * cols + c;
                    let p = Point::new(
                        sx + pad + c as f32 * (cell + gap),
                        top + r as f32 * (cell + gap + 6.0),
                    );
                    let tone = if i == 5 {
                        ACID
                    } else {
                        [INK_2, LINE_STRONG, color(0x222222)][(i as usize) % 3]
                    };
                    f.fill_rectangle(p, Size::new(cell, cell), tone);
                }
            }
            // Dock bar.
            f.fill_rectangle(
                Point::new(sx + sw / 2.0 - 30.0, sy + screen.height - 10.0),
                Size::new(60.0, 2.0),
                SUB,
            );
            // Scanlines + rolling refresh band.
            let mut y = sy;
            while y < sy + screen.height {
                f.fill_rectangle(Point::new(sx, y), Size::new(sw, 1.0), alpha(VOID, 0.22));
                y += 3.0;
            }
            let band = sy + ((self.t * 0.25).fract() * (screen.height + 60.0)) - 30.0;
            f.fill_rectangle(
                Point::new(sx, band.max(sy)),
                Size::new(sw, 30.0f32.min(sy + screen.height - band.max(sy)).max(0.0)),
                alpha(PAPER, 0.025),
            );
            // REC tag.
            f.fill_rectangle(
                Point::new(o.x + w + 10.0, o.y),
                Size::new(6.0, 6.0),
                if (self.t * 1.5).fract() < 0.6 {
                    HOT
                } else {
                    INK_2
                },
            );
        } else {
            // Static.
            let cell = 6.0;
            let frame_no = (self.t * 12.0) as u32;
            let cols = (screen.width / cell) as u32;
            let rows = (screen.height / cell) as u32;
            for r in 0..rows {
                for c in 0..cols {
                    let v = hash(r * 977 + c * 131 + frame_no * 7919);
                    if v > 0.55 {
                        let g = 0.04 + (v - 0.55) * 0.18;
                        f.fill_rectangle(
                            Point::new(screen.x + c as f32 * cell, screen.y + r as f32 * cell),
                            Size::new(cell - 1.0, cell - 1.0),
                            Color::from_rgb(g, g, g),
                        );
                    }
                }
            }
            let cx = screen.x + screen.width / 2.0;
            let cy = screen.y + screen.height / 2.0;
            f.fill_rectangle(
                Point::new(cx - 70.0, cy - 22.0),
                Size::new(140.0, 44.0),
                VOID,
            );
            label(
                &mut f,
                "SEM SINAL",
                Point::new(cx, cy - 14.0),
                22.0,
                PAPER,
                DISPLAY,
                Alignment::Center,
            );
        }
        vec![f.into_geometry()]
    }
}

// ───────────────────────────── RSSI scale ─────────────────────────────

pub struct RssiScale {
    pub rssi: f32,
    pub lock_at: f32,
    pub unlock_at: f32,
    pub history: Vec<f32>,
}

impl<M> canvas::Program<M> for RssiScale {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        b: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, b.size());
        let (lo, hi) = (-100.0f32, -30.0f32);
        let w = b.width;
        let x = |v: f32| ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * w;
        let base = b.height - 22.0;

        // History trace above the scale.
        let n = self.history.len();
        if n > 1 {
            let top = 18.0;
            let ht = base - 34.0 - top;
            let path = Path::new(|p| {
                for (i, v) in self.history.iter().enumerate() {
                    let pt = Point::new(x(*v), top + ht * i as f32 / (n - 1) as f32);
                    if i == 0 { p.move_to(pt) } else { p.line_to(pt) }
                }
            });
            f.stroke(&path, stroke(alpha(PAPER, 0.35), 1.0));
        }

        // Zones.
        f.fill_rectangle(
            Point::new(0.0, base - 10.0),
            Size::new(x(self.lock_at), 10.0),
            alpha(HOT, 0.16),
        );
        f.fill_rectangle(
            Point::new(x(self.unlock_at), base - 10.0),
            Size::new(w - x(self.unlock_at), 10.0),
            alpha(ACID, 0.14),
        );
        f.stroke(
            &Path::line(Point::new(0.0, base + 0.5), Point::new(w, base + 0.5)),
            stroke(LINE_STRONG, 1.0),
        );

        for v in (-100..=-30).step_by(5) {
            let xx = x(v as f32).round() + 0.5;
            let major = v % 10 == 0;
            f.stroke(
                &Path::line(
                    Point::new(xx, base),
                    Point::new(xx, base + if major { 6.0 } else { 3.0 }),
                ),
                stroke(if major { MUTED } else { FAINT }, 1.0),
            );
            if major {
                label(
                    &mut f,
                    format!("{v}"),
                    Point::new(xx, base + 8.0),
                    9.5,
                    MUTED,
                    MONO,
                    Alignment::Center,
                );
            }
        }

        // Thresholds.
        for (v, c, s) in [
            (self.lock_at, HOT, "BLOQUEIA"),
            (self.unlock_at, ACID, "DESBLOQUEIA"),
        ] {
            let xx = x(v).round() + 0.5;
            f.stroke(
                &Path::line(Point::new(xx, 0.0), Point::new(xx, base)),
                Stroke {
                    line_dash: LineDash {
                        segments: &[3.0, 3.0],
                        offset: 0,
                    },
                    ..stroke(c, 1.0)
                },
            );
            label(
                &mut f,
                format!("{s} {v:.0}"),
                Point::new(xx + 6.0, 0.0),
                9.5,
                c,
                MONO_SEMI,
                Alignment::Left,
            );
        }

        // Marker.
        let mx = x(self.rssi);
        let marker = Path::new(|p| {
            p.move_to(Point::new(mx, base - 12.0));
            p.line_to(Point::new(mx - 6.0, base - 22.0));
            p.line_to(Point::new(mx + 6.0, base - 22.0));
            p.close();
        });
        f.fill(&marker, PAPER);
        f.fill_rectangle(
            Point::new(mx - 0.5, base - 12.0),
            Size::new(1.0, 12.0),
            PAPER,
        );
        vec![f.into_geometry()]
    }
}

// ───────────────────────────── ticker ─────────────────────────────

pub struct Ticker {
    pub t: f32,
    pub items: Vec<(String, Color)>,
}

impl<M> canvas::Program<M> for Ticker {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        b: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, b.size());
        // Approximate advance for Plex Mono at 10.5px: 0.6em.
        let adv = 10.5 * 0.6;
        let sep = "   ■   ";
        let total: f32 = self
            .items
            .iter()
            .map(|(s, _)| (s.chars().count() + sep.chars().count()) as f32 * adv)
            .sum::<f32>()
            .max(1.0);
        let offset = (self.t * 38.0) % total;
        let mut x = -offset;
        let y = ((b.height - 13.0) / 2.0).round();
        // Canvas text is not clipped by every backend, so clip per glyph
        // (the face is monospaced, so this is exact).
        let put = |f: &mut Frame, s: &str, x: f32, c: Color| {
            let n = s.chars().count();
            if x + n as f32 * adv <= 0.0 || x >= b.width {
                return;
            }
            let start = if x < 0.0 {
                (-x / adv).ceil() as usize
            } else {
                0
            };
            let end = (((b.width - x) / adv).floor().max(0.0) as usize).min(n);
            if start >= end {
                return;
            }
            let part: String = s.chars().skip(start).take(end - start).collect();
            label(
                f,
                part,
                Point::new(x + start as f32 * adv, y),
                10.5,
                c,
                MONO,
                Alignment::Left,
            );
        };
        while x < b.width {
            for (s, c) in &self.items {
                put(&mut f, s, x, *c);
                x += s.chars().count() as f32 * adv;
                put(&mut f, sep, x, FAINT);
                x += sep.chars().count() as f32 * adv;
            }
        }
        vec![f.into_geometry()]
    }
}

pub fn color(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

// ───────────────────────────── camera viewfinder ─────────────────────────────

/// The phone camera as a 16:9 viewfinder: thirds, corner brackets, crosshair,
/// timecode. Abstract scene when live, static when not.
pub struct CameraFrame {
    pub t: f32,
    pub live: bool,
    pub label: String,
    pub elapsed: f32,
}

impl<M> canvas::Program<M> for CameraFrame {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        b: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, b.size());
        let w = b.width.min(b.height * 16.0 / 9.0);
        let h = w * 9.0 / 16.0;
        let o = Point::new(
            ((b.width - w) / 2.0).round() + 0.5,
            ((b.height - h) / 2.0).round() + 0.5,
        );

        if self.live {
            f.fill_rectangle(o, Size::new(w, h), color(0x0C0C0C));
            // Abstract scene: a horizon, a lamp, a slow drifting light band.
            let horizon = o.y + h * 0.62;
            f.fill_rectangle(
                Point::new(o.x, horizon),
                Size::new(w, h * 0.38),
                color(0x121212),
            );
            let lamp = Point::new(
                o.x + w * (0.68 + (self.t * 0.07).sin() * 0.01),
                o.y + h * 0.34,
            );
            for (r, a) in [(h * 0.20, 0.03), (h * 0.12, 0.05), (h * 0.06, 0.10)] {
                f.fill(&Path::circle(lamp, r), alpha(PAPER, a));
            }
            f.fill(&Path::circle(lamp, h * 0.025), alpha(PAPER, 0.85));
            f.stroke(
                &Path::line(Point::new(o.x, horizon), Point::new(o.x + w, horizon)),
                stroke(alpha(PAPER, 0.08), 1.0),
            );
            // Scanlines.
            let mut y = o.y;
            while y < o.y + h {
                f.fill_rectangle(Point::new(o.x, y), Size::new(w, 1.0), alpha(VOID, 0.18));
                y += 3.0;
            }
        } else {
            f.fill_rectangle(o, Size::new(w, h), INK_0);
            let cell = 6.0;
            let frame_no = (self.t * 12.0) as u32;
            for r in 0..(h / cell) as u32 {
                for c in 0..(w / cell) as u32 {
                    let v = hash(r * 977 + c * 131 + frame_no * 7919);
                    if v > 0.6 {
                        let g = 0.035 + (v - 0.6) * 0.15;
                        f.fill_rectangle(
                            Point::new(o.x + c as f32 * cell, o.y + r as f32 * cell),
                            Size::new(cell - 1.0, cell - 1.0),
                            Color::from_rgb(g, g, g),
                        );
                    }
                }
            }
        }

        // Rule of thirds.
        for i in 1..3 {
            let x = (o.x + w * i as f32 / 3.0).round() + 0.5;
            let y = (o.y + h * i as f32 / 3.0).round() + 0.5;
            let dash = Stroke {
                line_dash: LineDash {
                    segments: &[2.0, 6.0],
                    offset: 0,
                },
                ..stroke(alpha(PAPER, 0.12), 1.0)
            };
            f.stroke(
                &Path::line(Point::new(x, o.y), Point::new(x, o.y + h)),
                dash.clone(),
            );
            f.stroke(
                &Path::line(Point::new(o.x, y), Point::new(o.x + w, y)),
                dash,
            );
        }
        // Corner brackets.
        let k = 22.0;
        let c = if self.live { ACID } else { MUTED };
        for (cx, cy, dx, dy) in [
            (o.x + 12.0, o.y + 12.0, 1.0, 1.0),
            (o.x + w - 12.0, o.y + 12.0, -1.0, 1.0),
            (o.x + 12.0, o.y + h - 12.0, 1.0, -1.0),
            (o.x + w - 12.0, o.y + h - 12.0, -1.0, -1.0),
        ] {
            f.stroke(
                &Path::line(Point::new(cx, cy), Point::new(cx + dx * k, cy)),
                stroke(c, 1.5),
            );
            f.stroke(
                &Path::line(Point::new(cx, cy), Point::new(cx, cy + dy * k)),
                stroke(c, 1.5),
            );
        }
        // Crosshair.
        let m = Point::new(o.x + w / 2.0, o.y + h / 2.0);
        f.stroke(
            &Path::line(Point::new(m.x - 8.0, m.y), Point::new(m.x + 8.0, m.y)),
            stroke(alpha(PAPER, 0.5), 1.0),
        );
        f.stroke(
            &Path::line(Point::new(m.x, m.y - 8.0), Point::new(m.x, m.y + 8.0)),
            stroke(alpha(PAPER, 0.5), 1.0),
        );

        // Overlays.
        if self.live {
            let rec = (self.t * 1.5).fract() < 0.6;
            f.fill(
                &Path::circle(Point::new(o.x + 34.0, o.y + 40.0), 4.0),
                if rec { HOT } else { INK_2 },
            );
            let s = self.elapsed as u32;
            label(
                &mut f,
                format!("REC {:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60),
                Point::new(o.x + 44.0, o.y + 33.0),
                11.0,
                PAPER,
                MONO_SEMI,
                Alignment::Left,
            );
        } else {
            f.fill_rectangle(
                Point::new(m.x - 70.0, m.y - 22.0),
                Size::new(140.0, 44.0),
                VOID,
            );
            label(
                &mut f,
                "SEM SINAL",
                Point::new(m.x, m.y - 14.0),
                22.0,
                PAPER,
                DISPLAY,
                Alignment::Center,
            );
        }
        label(
            &mut f,
            &self.label,
            Point::new(o.x + w - 34.0, o.y + 33.0),
            11.0,
            if self.live { PAPER } else { MUTED },
            MONO_SEMI,
            Alignment::Right,
        );
        f.stroke_rectangle(o, Size::new(w, h), stroke(LINE_STRONG, 1.0));
        vec![f.into_geometry()]
    }
}
