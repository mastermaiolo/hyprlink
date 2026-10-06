//! JSON contract v1 for shell plugins (Noctalia, Ryoku, Waybar…): one object
//! per line from `hyprlinkctl watch --json`.
//!
//! - Field order is reading order: the phone first, this PC last.
//! - Unknown = `null`. Never a fake 0.
//! - Data only: the phone's network is structured (`phone.network`,
//!   `cell_gen`, `carrier`, `wifi`), sizes are bytes; plugins format.
//! - Additive changes keep `v = 1`; anything breaking bumps `v`.

use crate::host::Host;
use crate::link::{Device, Dir, Event, Kind, LinkState, PhoneStatus};
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Snapshot {
    pub v: u8,
    /// "up" | "mock" | "down"
    pub daemon: String,
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
