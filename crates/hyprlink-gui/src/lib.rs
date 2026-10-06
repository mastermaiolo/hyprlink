//! Shared between the GUI binary and its tests: the daemon contract lives in
//! `hyprlink-proto`; this crate re-exports it under the paths the views use
//! (`crate::link`, `crate::host`, `hyprlink_gui::fmt`…), so the design files
//! stay identical to the ones in the design repository.

pub mod instance;
pub mod langcfg;

pub use hyprlink_proto::tr;
pub use hyprlink_proto::{fmt, host, i18n, link, snapshot};

use link::Transport;

/// `HYPRLINK_MOCK=1`: the simulated daemon (design work, captures).
pub fn mock_requested() -> bool {
    std::env::var("HYPRLINK_MOCK").is_ok_and(|v| v == "1")
}

/// The transport the GUI talks through.
///
/// - default: the real `hyprlinkd`, over `$XDG_RUNTIME_DIR/hyprlink.sock`,
///   reconnecting in the background ([`hyprlink_proto::client::Socket`]);
/// - `HYPRLINK_MOCK=1` (needs the `mock` feature, on by default): the
///   simulator.
pub fn transport() -> Box<dyn Transport> {
    #[cfg(feature = "mock")]
    if mock_requested() {
        return Box::new(link::mock::Simulator::new());
    }
    Box::new(hyprlink_proto::client::Socket::connect())
}
