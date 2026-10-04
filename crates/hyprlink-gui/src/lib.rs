//! Shared between the GUI binary and its tests: the daemon contract lives in
//! `hyprlink-proto`; this crate re-exports it under the paths the views use
//! (`crate::link`, `crate::host`, `hyprlink_gui::fmt`…), so the design files
//! stay identical to the ones in the design repository.

pub use hyprlink_proto::{fmt, host, link, snapshot};

use link::Transport;

/// The transport the GUI talks through.
///
/// - feature `mock` (default until the real socket lands in Fase 3): the
///   simulated daemon. `HYPRLINK_MOCK=1` will force it once a real transport
///   exists.
/// - without `mock`: [`Offline`] — no events, as if the daemon were stopped.
pub fn transport() -> Box<dyn Transport> {
    #[cfg(feature = "mock")]
    {
        Box::new(link::mock::Simulator::new())
    }
    #[cfg(not(feature = "mock"))]
    {
        Box::new(Offline)
    }
}

/// A transport with no daemon behind it.
pub struct Offline;

impl Transport for Offline {
    fn send(&mut self, _command: link::Command) {}
    fn poll(&mut self, _dt: std::time::Duration) -> Vec<link::Event> {
        Vec::new()
    }
}
