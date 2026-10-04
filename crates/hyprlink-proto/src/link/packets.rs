//! Wire names, `module.action`, as they appear in the Diário and in captions.
//!
//! Two groups:
//! - **real** — exist in the current daemon / Android app;
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
pub const MIC_DATA: &str = "webcam.mic_data";
pub const TAP_START: &str = "audio.tap_start";
pub const TAP_STOP: &str = "audio.tap_stop";
pub const TAP_DATA: &str = "audio.tap_data";
pub const WEBCAM_START: &str = "webcam.start";
pub const SHARE_PROGRESS: &str = "share.progress";
pub const PAIR_REVOKE: &str = "pair.revoke";

// ── exist in the daemon, wire name to confirm against PROTOCOL.md ──
pub const WEBCAM_STOP: &str = "webcam.stop";
pub const WEBCAM_NETTEST: &str = "webcam.nettest";
pub const PHONE_VOLUME: &str = "phone.volume_set";
pub const PHONE_RINGER: &str = "phone.ringer_set";
pub const PHONE_DND: &str = "phone.dnd_set";
pub const INPUT_CONFIG: &str = "input.config";
pub const SHARE_OFFER: &str = "share.offer";
pub const NOTIF_DISMISS: &str = "notif.dismiss";

// ── proposed (not in the protocol yet) ──
pub const PHONE_STATUS: &str = "phone.status";
pub const MIRROR_START: &str = "mirror.start";
pub const MIRROR_STOP: &str = "mirror.stop";
pub const SENSOR_FRAME: &str = "sensor.frame";
pub const SENSOR_SUBSCRIBE: &str = "sensor.subscribe";
pub const PRESENCE_RSSI: &str = "presence.rssi";
pub const PRESENCE_RULE: &str = "presence.rule";
pub const NOTIF_POSTED: &str = "notif.posted";

/// Names used as module prefixes in captions (e.g. "webcam.mic_*").
pub const MIC_FAMILY: &str = "webcam.mic_*";
pub const TAP_FAMILY: &str = "audio.tap_*";
