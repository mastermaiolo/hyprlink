//! Wire names, `module.action`, as they appear in the Diário and in captions.
//!
//! Groups:
//! - **real** — exist in the current daemon / Android app;
//! - **confirmed** — were "to confirm", now checked against PROTOCOL.md;
//! - **proposed** — features the GUI already shows but the protocol does not
//!   have yet. Their names follow the same convention and must be confirmed
//!   (or renamed here, in one place) when they land in PROTOCOL.md.

// ── real ──
pub const CORE_HELLO: &str = "core.hello";
pub const CORE_PING: &str = "core.ping";
pub const BATTERY_STATE: &str = "battery.state";
pub const CLIPBOARD_SET: &str = "clipboard.set";
pub const HYPR_DISPATCH: &str = "hypr.dispatch";
pub const MIC_START_REQUEST: &str = "webcam.mic_start_request";
pub const MIC_STOP_REQUEST: &str = "webcam.mic_stop_request";
pub const MIC_START: &str = "webcam.mic_start";
/// Not a control packet: the PCM rides a uni-stream after `webcam.mic_start`.
pub const MIC_DATA: &str = "webcam.mic_data";
pub const TAP_START: &str = "audio.tap_start";
pub const TAP_STOP: &str = "audio.tap_stop";
/// Not a control packet: QUIC DATAGRAMs after `audio.tap_ready`.
pub const TAP_DATA: &str = "audio.tap_data";
pub const WEBCAM_START: &str = "webcam.start";
pub const SHARE_PROGRESS: &str = "share.progress";
/// Not a wire packet: revoking is local (the daemon drops the fingerprint
/// and closes the connection).
pub const PAIR_REVOKE: &str = "pair.revoke";

// ── confirmed against PROTOCOL.md (were "to confirm") ──
pub const WEBCAM_STOP: &str = "webcam.stop";
/// No wire packet of its own: the daemon's network test is a 60 s
/// `webcam.start` (1080p30 H.264) measured locally.
pub const WEBCAM_NETTEST: &str = "webcam.start";
pub const PHONE_VOLUME: &str = "phone_audio.set_volume";
pub const PHONE_RINGER: &str = "phone_audio.set_ringer_mode";
pub const PHONE_DND: &str = "phone_audio.set_dnd";
/// No wire packet: trackpad settings live in the daemon's config and apply
/// to the incoming `input.*` packets.
pub const INPUT_CONFIG: &str = "input.move";
pub const SHARE_OFFER: &str = "share.file";
pub const NOTIF_DISMISS: &str = "notification.dismiss";
pub const NOTIF_REPLY: &str = "notification.reply";
/// Was in "proposed"; the phone already sends it.
pub const NOTIF_POSTED: &str = "notification.post";

// ── proposed (not in the protocol yet) ──
pub const PHONE_STATUS: &str = "phone.status";
pub const MIRROR_START: &str = "mirror.start";
pub const MIRROR_STOP: &str = "mirror.stop";
pub const SENSOR_FRAME: &str = "sensor.frame";
pub const SENSOR_SUBSCRIBE: &str = "sensor.subscribe";
pub const PRESENCE_RSSI: &str = "presence.rssi";
pub const PRESENCE_RULE: &str = "presence.rule";

/// Names used as module prefixes in captions (e.g. "webcam.mic_*").
pub const MIC_FAMILY: &str = "webcam.mic_*";
pub const TAP_FAMILY: &str = "audio.tap_*";
