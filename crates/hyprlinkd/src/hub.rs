//! O estado do link tal como os clientes do socket local o veem.
//!
//! Cada `Event` de estado (dispositivos, níveis, misturador…) tem uma
//! [`StateKey`]: o hub guarda o último de cada uma — é o "estado completo"
//! que um cliente recebe ao ligar — e só difunde um evento se o valor mudou.
//! `Packet` e `Notice` são acontecimentos, não estado: passam sempre e não
//! ficam guardados.

use hyprlink_proto::link::{Event, Event2};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateKey {
    Devices,
    Workspaces,
    Telemetry,
    Sensors,
    Levels,
    SpeakerMode,
    Mirror,
    Rssi,
    Pairing,
    Phone,
    Link,
    BatteryHistory,
    BatteryAlerts,
    ActiveWindow,
    Shortcuts,
    Trackpad,
    Webcam,
    NetTest,
    PhoneAudio,
    Mixer,
    Notifications,
    Clipboard,
    Transfers,
    Players,
    Settings,
    Gestures,
}

/// `None` = acontecimento (Packet, Notice): não se guarda nem se coalesce.
pub fn key(e: &Event) -> Option<StateKey> {
    use StateKey as K;
    Some(match e {
        Event::Devices(_) => K::Devices,
        Event::Workspaces { .. } => K::Workspaces,
        Event::Telemetry { .. } => K::Telemetry,
        Event::Sensors(_) => K::Sensors,
        Event::Levels { .. } => K::Levels,
        Event::SpeakerMode(_) => K::SpeakerMode,
        Event::Mirror(_) => K::Mirror,
        Event::Rssi(_) => K::Rssi,
        Event::Pairing(_) => K::Pairing,
        Event::Phone(_) => K::Phone,
        Event::Link(_) => K::Link,
        Event::Packet(_) | Event::Notice(_) => return None,
        Event::More(m) => match m {
            Event2::BatteryHistory(_) => K::BatteryHistory,
            Event2::BatteryAlerts(_) => K::BatteryAlerts,
            Event2::ActiveWindow(_) => K::ActiveWindow,
            Event2::Shortcuts(_) => K::Shortcuts,
            Event2::Trackpad(_) => K::Trackpad,
            Event2::Webcam(_) => K::Webcam,
            Event2::NetTest(_) => K::NetTest,
            Event2::PhoneAudio(_) => K::PhoneAudio,
            Event2::Mixer { .. } => K::Mixer,
            Event2::Notifications(_) => K::Notifications,
            Event2::Clipboard(_) => K::Clipboard,
            Event2::Transfers(_) => K::Transfers,
            Event2::Players(_) => K::Players,
            Event2::Settings(_) => K::Settings,
            Event2::Gestures { .. } => K::Gestures,
        },
    })
}

struct Inner {
    /// Último valor de cada estado + a sua forma CBOR (para comparar).
    latest: Mutex<BTreeMap<StateKey, (Vec<u8>, Event)>>,
    tx: broadcast::Sender<Event>,
}

#[derive(Clone)]
pub struct Hub(Arc<Inner>);

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Hub(Arc::new(Inner {
            latest: Mutex::new(BTreeMap::new()),
            tx,
        }))
    }

    /// Difunde `e`. Estado igual ao último publicado não passa (devolve
    /// `false`), por isso as fontes podem publicar por amostragem sem custo.
    pub fn publish(&self, e: Event) -> bool {
        if let Some(k) = key(&e) {
            let mut bytes = Vec::new();
            if ciborium::into_writer(&e, &mut bytes).is_err() {
                return false;
            }
            let mut latest = self.0.latest.lock().unwrap();
            if latest.get(&k).is_some_and(|(b, _)| *b == bytes) {
                return false;
            }
            latest.insert(k, (bytes, e.clone()));
        }
        // Sem clientes, `send` falha — normal.
        let _ = self.0.tx.send(e);
        true
    }

    /// O estado completo, por ordem estável de chave.
    pub fn snapshot(&self) -> Vec<Event> {
        let latest = self.0.latest.lock().unwrap();
        latest.values().map(|(_, e)| e.clone()).collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.0.tx.subscribe()
    }
}

static GLOBAL: OnceLock<Hub> = OnceLock::new();

/// O hub do processo, para os módulos que só emitem acontecimentos
/// (registo de pacotes, avisos) sem lhes passar mais um parâmetro.
pub fn global() -> &'static Hub {
    GLOBAL.get_or_init(Hub::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprlink_proto::link::Notice;

    #[test]
    fn dedups_state_but_not_happenings() {
        let hub = Hub::new();
        let mut rx = hub.subscribe();
        assert!(hub.publish(Event::Rssi(-50)));
        assert!(!hub.publish(Event::Rssi(-50)));
        assert!(hub.publish(Event::Rssi(-51)));
        assert!(hub.publish(Event::Notice(Notice::MirrorStarted)));
        assert!(hub.publish(Event::Notice(Notice::MirrorStarted)));
        let mut got = Vec::new();
        while let Ok(e) = rx.try_recv() {
            got.push(e);
        }
        assert_eq!(got.len(), 4);
        // Só o estado fica no instantâneo.
        let snap = hub.snapshot();
        assert_eq!(snap.len(), 1);
        assert!(matches!(snap[0], Event::Rssi(-51)));
    }

    #[test]
    fn every_event2_has_its_own_key() {
        use hyprlink_proto::link::{BatteryAlerts, Settings};
        let a = key(&Event::More(Event2::BatteryAlerts(BatteryAlerts {
            low: None,
            full: false,
        })));
        let b = key(&Event::More(Event2::Settings(Settings {
            downloads_dir: String::new(),
            daemon_version: String::new(),
            socket: String::new(),
            started_unix: 0,
        })));
        assert!(a.is_some() && b.is_some() && a != b);
    }
}
