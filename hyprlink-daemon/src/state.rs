//! Estado partilhado entre o daemon (tokio, thread própria) e a GUI (iced,
//! thread principal). A GUI só lê uma cópia (via `snapshot()`); o daemon é
//! quem escreve.

use std::sync::{Arc, Mutex};

const MAX_LOG_LINES: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub enum ConnState {
    /// Nenhum dispositivo pareado, aguardando QR/entrada manual.
    Pairing,
    /// Handshake em andamento (QUIC/mTLS negociando).
    Connecting,
    /// core.hello trocado e autorizado.
    Connected { device_name: String, fingerprint_hex: String },
}

#[derive(Debug, Clone)]
pub struct HudState {
    pub conn: ConnState,
    pub local_addr: String,
    pub server_fingerprint_hex: String,
    pub pairing_token_hex: String,
    pub logs: Vec<String>,
}

impl HudState {
    pub fn new(local_addr: String, server_fingerprint_hex: String, pairing_token_hex: String) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            conn: ConnState::Pairing,
            local_addr,
            server_fingerprint_hex,
            pairing_token_hex,
            logs: Vec::new(),
        }))
    }
}

pub fn push_log(state: &Arc<Mutex<HudState>>, line: impl Into<String>) {
    let line = line.into();
    println!("{line}");
    let mut s = state.lock().unwrap();
    s.logs.push(line);
    let overflow = s.logs.len().saturating_sub(MAX_LOG_LINES);
    if overflow > 0 {
        s.logs.drain(0..overflow);
    }
}

pub fn set_connecting(state: &Arc<Mutex<HudState>>) {
    state.lock().unwrap().conn = ConnState::Connecting;
}

pub fn set_connected(state: &Arc<Mutex<HudState>>, device_name: String, fingerprint_hex: String) {
    state.lock().unwrap().conn = ConnState::Connected { device_name, fingerprint_hex };
}

pub fn set_pairing(state: &Arc<Mutex<HudState>>) {
    state.lock().unwrap().conn = ConnState::Pairing;
}
