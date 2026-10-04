//! StatusNotifierItem tray (ksni). Lives inside the GUI process, which runs as
//! an `iced::daemon`, so closing the window leaves HYPRLINK in the tray.
//!
//! The icon state follows the link, and plays well with Noctalia's tray
//! (`hide_passive = true` by default):
//!   Passive        → no phone linked (hidden by Noctalia)
//!   Active         → phone linked
//!   NeedsAttention → pairing in progress

use crate::app::Message;
use iced::Subscription;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, Status, ToolTip, TrayMethods};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Open,
    Ping,
    Clipboard,
    Mic(bool),
    Mirror(bool),
    Quit,
}

/// What the tray needs to know. Phone first, always.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub device: Option<String>,
    pub battery: Option<u8>,
    pub charging: bool,
    pub network: String,
    pub latency_ms: f32,
    pub mic: bool,
    pub mirror: bool,
    pub pairing: bool,
}

pub struct HyprTray {
    tx: mpsc::UnboundedSender<Action>,
    pub snap: Snapshot,
}

/// Handle the app keeps to push new snapshots into the tray.
#[derive(Clone)]
pub struct Link(pub ksni::Handle<HyprTray>);

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("tray::Link")
    }
}

impl HyprTray {
    fn send(&self, a: Action) {
        let _ = self.tx.unbounded_send(a);
    }
}

impl ksni::Tray for HyprTray {
    fn id(&self) -> String {
        "hyprlink".into()
    }

    fn title(&self) -> String {
        "HYPRLINK".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Communications
    }

    fn status(&self) -> Status {
        if self.snap.pairing {
            Status::NeedsAttention
        } else if self.snap.device.is_some() {
            Status::Active
        } else {
            Status::Passive
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        icons(if self.snap.device.is_some() {
            Core::Linked
        } else {
            Core::Idle
        })
    }

    fn attention_icon_pixmap(&self) -> Vec<Icon> {
        icons(Core::Attention)
    }

    fn tool_tip(&self) -> ToolTip {
        let s = &self.snap;
        let description = match &s.device {
            Some(name) => format!(
                "{name} · {}%{} · {} · {:.1} ms",
                s.battery
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "—".into()),
                if s.charging { "+" } else { "" },
                s.network,
                s.latency_ms
            ),
            None => "Nenhum telemóvel ligado".into(),
        };
        ToolTip {
            title: "HYPRLINK".into(),
            description,
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Action::Open);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let linked = self.snap.device.is_some();
        let header = match &self.snap.device {
            Some(name) => match self.snap.battery {
                Some(b) => format!("{name} — {b}%"),
                None => name.clone(),
            },
            None => "Sem telemóvel".into(),
        };
        vec![
            StandardItem {
                label: header,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Abrir HYPRLINK".into(),
                activate: Box::new(|t: &mut Self| t.send(Action::Open)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Ping".into(),
                enabled: linked,
                activate: Box::new(|t: &mut Self| t.send(Action::Ping)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Enviar área de transferência".into(),
                enabled: linked,
                activate: Box::new(|t: &mut Self| t.send(Action::Clipboard)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Microfone do telemóvel".into(),
                enabled: linked,
                checked: self.snap.mic,
                activate: Box::new(|t: &mut Self| {
                    let on = !t.snap.mic;
                    t.send(Action::Mic(on))
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Espelhar ecrã".into(),
                enabled: linked,
                checked: self.snap.mirror,
                activate: Box::new(|t: &mut Self| {
                    let on = !t.snap.mirror;
                    t.send(Action::Mirror(on))
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Sair".into(),
                activate: Box::new(|t: &mut Self| t.send(Action::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn subscription() -> Subscription<Message> {
    Subscription::run(worker)
}

fn worker() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(16, async |mut out: mpsc::Sender<Message>| {
        let (tx, mut rx) = mpsc::unbounded();
        match (HyprTray {
            tx,
            snap: Snapshot::default(),
        })
        .spawn()
        .await
        {
            Ok(handle) => {
                let _ = out.send(Message::TrayReady(Link(handle))).await;
            }
            Err(e) => {
                eprintln!("hyprlink: tray indisponível ({e})");
                let _ = out.send(Message::TrayFailed).await;
                return;
            }
        }
        while let Some(action) = rx.next().await {
            let _ = out.send(Message::Tray(action)).await;
        }
    })
}

// ───────────────────────────── icon ─────────────────────────────

#[derive(Clone, Copy)]
enum Core {
    Linked,
    Idle,
    Attention,
}

/// The brand mark, rasterised at tray sizes: two squares joined by a dotted
/// wire — the same motif as the cover's link diagram. The phone (left) is the
/// filled one: it is the protagonist.
fn icons(core: Core) -> Vec<Icon> {
    [16, 22, 24, 32, 48, 64]
        .into_iter()
        .map(|n| mark(n, core))
        .collect()
}

fn mark(n: i32, core: Core) -> Icon {
    let mut px = vec![0u8; (n * n * 4) as usize];
    let mut put = |x: i32, y: i32, (r, g, b): (u8, u8, u8)| {
        if x >= 0 && y >= 0 && x < n && y < n {
            let i = ((y * n + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[0xFF, r, g, b]); // ARGB, network order
        }
    };
    const PAPER: (u8, u8, u8) = (0xED, 0xEB, 0xE4);
    const ACID: (u8, u8, u8) = (0xD4, 0xFF, 0x3A);
    const HOT: (u8, u8, u8) = (0xFF, 0x33, 0x55);
    const GREY: (u8, u8, u8) = (0x6A, 0x69, 0x65);

    let q = ((n as f32) * 0.32).round() as i32; // square size
    let m = ((n as f32) * 0.04).round().max(1.0) as i32; // margin
    let top = (n - q) / 2;
    let stroke = (n / 16).max(1);

    // Left: phone, solid.
    for y in top..top + q {
        for x in m..m + q {
            put(x, y, PAPER);
        }
    }
    // Right: PC, outlined, with a status core.
    let rx = n - m - q;
    for y in top..top + q {
        for x in rx..rx + q {
            let edge = x < rx + stroke
                || x >= rx + q - stroke
                || y < top + stroke
                || y >= top + q - stroke;
            if edge {
                put(x, y, PAPER);
            }
        }
    }
    let c = match core {
        Core::Linked => ACID,
        Core::Idle => GREY,
        Core::Attention => HOT,
    };
    let inset = stroke + (q / 5).max(1);
    for y in top + inset..top + q - inset {
        for x in rx + inset..rx + q - inset {
            put(x, y, c);
        }
    }
    // Wire: dotted, between the two.
    let mid = n / 2;
    let mut x = m + q + stroke;
    while x < rx - stroke {
        for d in 0..stroke {
            put(
                x,
                mid - stroke / 2 + d,
                if matches!(core, Core::Idle) {
                    GREY
                } else {
                    PAPER
                },
            );
        }
        x += stroke * 2;
    }
    Icon {
        width: n,
        height: n,
        data: px,
    }
}
