//! JSON contract v1 for shell plugins (Noctalia, Ryoku, Waybar…): one object
//! per line from `hyprlinkctl watch --json`.
//!
//! - Field order is reading order: the phone first, this PC last.
//! - Unknown = `null`. Never a fake 0.
//! - Data only: the phone's network is structured (`phone.network`,
//!   `cell_gen`, `carrier`, `wifi`), sizes are bytes; plugins format.
//! - Additive changes keep `v = 1`; anything breaking bumps `v`.

use crate::host::Host;
use crate::link::{Device, Dir, Event, Event2, Kind, LinkState, PhoneStatus, TransferState};
use serde::{Deserialize, Serialize};

pub const VERSION: u8 = 1;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeviceBrief {
    pub id: u32,
    /// Raw name, as set on the phone.
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub android: Option<String>,
    pub kind: Kind,
    pub state: LinkState,
    pub battery: Option<u8>,
    pub charging: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
pub struct Channel {
    pub on: bool,
    /// Linear 0–1; `null` when not measured.
    pub level: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
pub struct Tap {
    pub on: bool,
    pub level: Option<f32>,
    /// `true`: source is the `hyprlink-speaker` sink (phone as an output device).
    pub speaker: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Mirror {
    pub on: bool,
    pub fps: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PacketBrief {
    pub dir: Dir,
    pub kind: String,
    pub note: String,
}

/// The file transfer in progress (one at a time). Sizes in bytes.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TransferBrief {
    pub id: u64,
    pub name: String,
    pub dir: Dir,
    pub done: u64,
    pub bytes: u64,
    /// Moving average (`rate::RateMeter`); `null` until the first sample.
    pub bytes_per_sec: Option<u64>,
    /// Seconds left; `null` while it is still "calculating".
    pub eta_s: Option<u64>,
}

/// "Find my phone" as the daemon knows it (`phone.find.state`).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct FindBrief {
    pub active: bool,
    /// Rings until this time (epoch ms), if the phone said so.
    pub until_ms: Option<u64>,
    /// Who stopped the last ring: "user" (found on the phone), "timeout",
    /// "pc" or "error"; `null` if it never rang.
    pub last_stopped_by: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Snapshot {
    pub v: u8,
    /// "up" | "mock" | "down"
    pub daemon: String,
    /// Added in 0.1.1 (additive): the daemon's version (from the socket
    /// handshake); `null` when it is down. Plugins compare it with
    /// `ctl_version` to spot an old `hyprlinkctl`.
    #[serde(default)]
    pub daemon_version: Option<String>,
    /// Added in 0.1.1: the version of the `hyprlinkctl` that wrote this line.
    /// Missing in older binaries.
    #[serde(default)]
    pub ctl_version: Option<String>,
    /// "connected" | "connecting" | "disconnected" — the LINK colour.
    pub link: crate::link::LinkPhase,
    // ── the phone ──
    pub device: Option<DeviceBrief>,
    pub phone: Option<PhoneStatus>,
    pub devices: usize,
    pub latency_ms: Option<f32>,
    pub latency_hist: Vec<f32>,
    pub rssi: Option<i32>,
    pub mic: Channel,
    pub tap: Tap,
    pub mirror: Mirror,
    pub pairing: Option<String>,
    pub last_packet: Option<PacketBrief>,
    // ── the link ──
    pub down_kbps: Option<f32>,
    pub up_kbps: Option<f32>,
    pub workspace: Option<u8>,
    /// Added in 0.1.1 (additive, still v1): `null` when nothing is moving.
    #[serde(default)]
    pub transfer: Option<TransferBrief>,
    /// Added in 0.1.1 (additive, still v1). `active: false` with an older
    /// daemon or before any ring.
    #[serde(default)]
    pub find_phone: FindBrief,
    /// Added in 0.1.1 (additive, still v1): `"usb" | "wifi" | "eth" | "other"`,
    /// the path of the active link; `null` when not connected.
    #[serde(default)]
    pub link_path: Option<crate::link::LinkPath>,
    // ── this PC ──
    pub host: Option<Host>,
}

impl Snapshot {
    pub fn new(daemon: &str) -> Self {
        Self {
            v: VERSION,
            daemon: daemon.to_string(),
            ..Default::default()
        }
    }

    fn primary(devices: &[Device]) -> Option<&Device> {
        devices
            .iter()
            .find(|d| d.state == LinkState::Linked)
            .or(devices.first())
    }

    pub fn apply(&mut self, e: &Event) {
        match e {
            Event::Devices(list) => {
                self.devices = list.len();
                self.device = Self::primary(list).map(|d| DeviceBrief {
                    id: d.id,
                    name: d.name.clone(),
                    manufacturer: d.manufacturer.clone(),
                    model: d.model.clone(),
                    android: d.android.clone(),
                    kind: d.kind,
                    state: d.state,
                    battery: d.battery,
                    charging: d.charging,
                });
                if let Some(d) = Self::primary(list) {
                    if d.rssi.is_some() {
                        self.rssi = d.rssi;
                    }
                }
            }
            Event::Phone(p) => self.phone = Some(p.clone()),
            Event::Link(p) => self.link = *p,
            Event::Telemetry {
                latency_ms,
                up_kbps,
                down_kbps,
            } => {
                let l = (latency_ms * 10.0).round() / 10.0;
                self.latency_ms = Some(l);
                self.up_kbps = Some(up_kbps.round());
                self.down_kbps = Some(down_kbps.round());
                self.latency_hist.push(l);
                if self.latency_hist.len() > 30 {
                    self.latency_hist.remove(0);
                }
            }
            Event::Levels { mic, tap } => {
                self.mic.on = mic.is_some();
                self.tap.on = tap.is_some();
                self.mic.level = mic.map(|v| (v * 100.0).round() / 100.0);
                self.tap.level = tap.map(|v| (v * 100.0).round() / 100.0);
            }
            Event::SpeakerMode(on) => self.tap.speaker = *on,
            Event::Mirror(m) => {
                self.mirror.on = m.is_some();
                self.mirror.fps = m.map(|m| m.fps.round());
            }
            Event::Rssi(r) => self.rssi = Some(*r),
            Event::Workspaces { active, .. } => self.workspace = Some(*active),
            Event::Pairing(p) => {
                self.pairing = p
                    .as_ref()
                    .map(|p| p.code.clone().unwrap_or_else(|| "qr".into()))
            }
            Event::More(Event2::Transfers(list)) => {
                self.transfer = list
                    .iter()
                    .find(|t| t.state == TransferState::Active)
                    .map(|t| TransferBrief {
                        id: t.id,
                        name: t.name.clone(),
                        dir: t.dir,
                        done: t.done,
                        bytes: t.bytes,
                        bytes_per_sec: t.bytes_per_sec,
                        eta_s: t.eta_s,
                    });
            }
            Event::More(Event2::LinkPath(p)) => self.link_path = *p,
            Event::More(Event2::FindPhone(f)) => {
                self.find_phone = FindBrief {
                    active: f.active,
                    until_ms: f.until_ms,
                    last_stopped_by: f.last_stopped_by.map(|b| b.as_str().to_string()),
                };
            }
            Event::Packet(p) => {
                self.last_packet = Some(PacketBrief {
                    dir: p.dir,
                    kind: p.kind.clone(),
                    note: p.note.clone(),
                })
            }
            _ => {}
        }
        if self.device.as_ref().map(|d| d.state) != Some(LinkState::Linked) {
            // Nothing to measure without a phone.
            self.latency_ms = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Transport, mock::Simulator};
    use std::time::Duration;

    /// Golden shape of v1: the fields the Noctalia plugin reads must exist
    /// with these JSON types (null allowed where documented).
    /// `Levels` with `None` means the channel is off: `/mic/on` and
    /// `/tap/on` must follow it (the real daemon sends exactly that).
    #[test]
    fn levels_drive_on_off() {
        let mut s = Snapshot::new("up");
        s.apply(&Event::Levels {
            mic: Some(0.0),
            tap: None,
        });
        assert!(s.mic.on && !s.tap.on);
        s.apply(&Event::Levels {
            mic: None,
            tap: Some(0.4),
        });
        assert!(!s.mic.on && s.tap.on);
    }

    #[test]
    fn transfer_in_progress_is_in_the_snapshot() {
        use crate::link::Transfer;
        let t = |id, state| Transfer {
            id,
            name: "a.mp4".into(),
            dir: Dir::Tx,
            bytes: 100,
            done: 40,
            state,
            at: 0,
            bytes_per_sec: Some(10),
            eta_s: Some(6),
            duration_ms: None,
        };
        let mut s = Snapshot::new("up");
        s.apply(&Event::More(Event2::Transfers(vec![
            t(1, TransferState::Done),
            t(2, TransferState::Active),
        ])));
        let b = s.transfer.clone().unwrap();
        assert_eq!(
            (b.id, b.done, b.bytes_per_sec, b.eta_s),
            (2, 40, Some(10), Some(6))
        );
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["transfer"]["eta_s"], 6);
        s.apply(&Event::More(Event2::Transfers(vec![t(
            1,
            TransferState::Done,
        )])));
        assert!(s.transfer.is_none());
    }

    #[test]
    fn find_phone_is_in_the_snapshot() {
        use crate::link::{FindPhoneState, FindStoppedBy};
        let mut s = Snapshot::new("up");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["find_phone"]["active"], false);
        assert!(v["find_phone"]["last_stopped_by"].is_null());
        s.apply(&Event::More(Event2::FindPhone(FindPhoneState {
            active: true,
            until_ms: Some(1_700_000_030_000),
            last_stopped_by: None,
            last_error: None,
        })));
        assert_eq!(
            s.find_phone,
            FindBrief {
                active: true,
                until_ms: Some(1_700_000_030_000),
                last_stopped_by: None
            }
        );
        s.apply(&Event::More(Event2::FindPhone(FindPhoneState {
            active: false,
            until_ms: None,
            last_stopped_by: Some(FindStoppedBy::User),
            last_error: None,
        })));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["find_phone"]["active"], false);
        assert_eq!(v["find_phone"]["last_stopped_by"], "user");
    }

    #[test]
    fn v1_shape() {
        let mut sim = Simulator::new();
        let mut s = Snapshot::new("mock");
        for _ in 0..80 {
            for e in sim.poll(Duration::from_millis(50)) {
                s.apply(&e);
            }
        }
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        let t = |p: &str| v.pointer(p).unwrap_or_else(|| panic!("falta {p}")).clone();
        assert_eq!(t("/v"), 1);
        assert!(t("/daemon").is_string());
        assert!(t("/link").is_string());
        assert!(t("/device/name").is_string());
        assert!(t("/device/state").is_string());
        assert!(t("/device/battery").is_number() || t("/device/battery").is_null());
        assert!(t("/device/charging").is_boolean());
        assert!(t("/phone/network").is_string() || t("/phone/network").is_null());
        assert!(t("/phone/storage_used_b").is_u64() || t("/phone/storage_used_b").is_null());
        assert!(t("/phone/now_playing/title").is_string());
        assert!(t("/latency_ms").is_number() || t("/latency_ms").is_null());
        assert!(t("/latency_hist").is_array());
        assert!(t("/mic/on").is_boolean());
        assert!(t("/tap/speaker").is_boolean());
        assert!(t("/mirror/on").is_boolean());
        let host = t("/host");
        assert!(host.is_null() || host.pointer("/hostname").is_some_and(|h| h.is_string()));
        // Round-trip.
        let back: Snapshot = serde_json::from_value(v).unwrap();
        assert_eq!(back.v, 1);
    }
}
