//! Os endereços do PC por onde o telemóvel pode ligar (Wi-Fi, cabo, ancoragem
//! USB) e por onde a ligação ativa vai.
//!
//! O `core.hello` (D→P) leva `pc_addrs`; quando a lista muda (a interface USB
//! sobe ou desce) o daemon empurra `pc.addrs {pc_addrs}`. O IP local da
//! ligação QUIC dá o `link_path` (USB / Wi-Fi…) que a GUI e o
//! `hyprlinkctl status` mostram. Poll de 2 s (só com telemóvel ligado), sem
//! netlink: o `ip` chega.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value;
use hyprlink_env::net::{self, AddrTracker, LinkKind, PcAddr};
use hyprlink_proto::link::LinkPath;

use crate::active::{self, ActiveConn};
use crate::state::HudState;

/// `[{ip, kind, iface}]`: o formato do `core.hello` e do `pc.addrs`.
pub fn addrs_value(addrs: &[PcAddr]) -> Value {
    Value::Array(
        addrs
            .iter()
            .map(|a| {
                Value::Map(vec![
                    (Value::Text("ip".into()), Value::Text(a.ip.clone())),
                    (
                        Value::Text("kind".into()),
                        Value::Text(a.kind.as_str().into()),
                    ),
                    (Value::Text("iface".into()), Value::Text(a.iface.clone())),
                ])
            })
            .collect(),
    )
}

/// Corpo do push `pc.addrs`.
pub fn push_body(addrs: &[PcAddr]) -> Value {
    Value::Map(vec![(Value::Text("pc_addrs".into()), addrs_value(addrs))])
}

pub fn link_path(kind: LinkKind) -> LinkPath {
    match kind {
        LinkKind::Usb => LinkPath::Usb,
        LinkKind::Wifi => LinkPath::Wifi,
        LinkKind::Eth => LinkPath::Eth,
        LinkKind::Other => LinkPath::Other,
    }
}

/// Os endereços agora (corre o `ip`: chamar fora do executor).
pub fn current() -> Vec<PcAddr> {
    net::pc_addrs(&crate::envinfo::daemon_env())
}

/// A cada 2 s, com telemóvel ligado: atualiza o `link_path` e, se a lista de
/// endereços mudou, empurra `pc.addrs`.
pub async fn watch(active: ActiveConn, hud: Arc<Mutex<HudState>>) {
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut tracker = AddrTracker::default();
    loop {
        tick.tick().await;
        let conn = active.lock().unwrap().clone();
        let Some(conn) = conn else {
            tracker.reset();
            hud.lock().unwrap().modules.link_path = None;
            continue;
        };
        let Ok(addrs) = tokio::task::spawn_blocking(current).await else {
            continue;
        };
        let path = conn
            .local_ip()
            .map(|ip| link_path(net::kind_for_ip(&addrs, &ip.to_string())));
        hud.lock().unwrap().modules.link_path = path;
        if tracker.changed(&addrs) {
            active::push(&active, "pc.addrs", Some(push_body(&addrs))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Packet, body_get, body_get_str};

    fn addr(ip: &str, kind: LinkKind, iface: &str) -> PcAddr {
        PcAddr {
            ip: ip.into(),
            kind,
            iface: iface.into(),
        }
    }

    #[test]
    fn pc_addrs_no_hello_faz_round_trip_cbor() {
        let list = vec![
            addr("192.168.1.5", LinkKind::Wifi, "wlan0"),
            addr("192.168.42.129", LinkKind::Usb, "usb0"),
        ];
        let hello = Value::Map(vec![
            (Value::Text("device_name".into()), Value::Text("pc".into())),
            (Value::Text("pc_addrs".into()), addrs_value(&list)),
        ]);
        let bytes = Packet::new(7, "core.hello", Some(hello), false).encode();
        let back = Packet::decode(&bytes).expect("o frame descodifica");
        let body = back.body.unwrap();
        // Os campos antigos continuam lá (apps antigas ignoram o novo).
        assert_eq!(body_get_str(&body, "device_name"), Some("pc"));
        let arr = body_get(&body, "pc_addrs").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(body_get_str(&arr[1], "ip"), Some("192.168.42.129"));
        assert_eq!(body_get_str(&arr[1], "kind"), Some("usb"));
        assert_eq!(body_get_str(&arr[1], "iface"), Some("usb0"));
    }

    #[test]
    fn push_pc_addrs_leva_a_lista() {
        let b = push_body(&[addr("10.0.0.2", LinkKind::Eth, "enp3s0")]);
        let arr = body_get(&b, "pc_addrs").unwrap().as_array().unwrap();
        assert_eq!(body_get_str(&arr[0], "kind"), Some("eth"));
        let empty = push_body(&[]);
        assert!(
            body_get(&empty, "pc_addrs")
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn link_path_segue_o_tipo() {
        assert_eq!(link_path(LinkKind::Usb), LinkPath::Usb);
        assert_eq!(link_path(LinkKind::Wifi), LinkPath::Wifi);
        assert_eq!(link_path(LinkKind::Other).as_str(), "other");
    }
}
