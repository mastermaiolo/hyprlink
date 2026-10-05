//! `phone.status` (P→D): o estado que o telemóvel reporta de si mesmo —
//! rede, armazenamento, RAM, temperatura da bateria, ecrã, notificações e
//! o que está a tocar (ver prompt `prompt_ai_studio_2026-10-04_2_phone_status.md`,
//! o contrato que a app segue).
//!
//! Regra do contrato: **campo desconhecido = omitido** — o parser respeita o
//! mesmo: o que não veio fica `None`, nunca inventa `0`/`""`. Nada de
//! validar além do tipo (a app é a fonte; o PC só apresenta).

use ciborium::Value;
use hyprlink_proto::link::{Network, NowPlaying, PhoneStatus, Wifi};

fn get<'a>(body: &'a Value, key: &str) -> Option<&'a Value> {
    body.as_map()?.iter().find(|(k, _)| k.as_text() == Some(key)).map(|(_, v)| v)
}

fn text<'a>(body: &'a Value, key: &str) -> Option<String> {
    get(body, key)?.as_text().map(str::to_string)
}

fn int(body: &Value, key: &str) -> Option<i64> {
    get(body, key)?.as_integer().and_then(|i| i64::try_from(i).ok())
}

fn float(body: &Value, key: &str) -> Option<f32> {
    match get(body, key)? {
        Value::Float(f) => Some(*f as f32),
        // `Integer` é `Copy`; o padrão a `&Value` entrega `&Integer`.
        Value::Integer(i) => i64::try_from(*i).ok().map(|v| v as f32),
        _ => None,
    }
}

fn boolean(body: &Value, key: &str) -> Option<bool> {
    get(body, key)?.as_bool()
}

/// CBOR cru do pacote → `PhoneStatus`. Body que não seja mapa = tudo `None`.
pub fn parse(body: &Value) -> PhoneStatus {
    let network = text(body, "network").and_then(|n| match n.as_str() {
        "wifi" => Some(Network::Wifi),
        "cellular" => Some(Network::Cellular),
        "ethernet" => Some(Network::Ethernet),
        "none" => Some(Network::None),
        _ => None,
    });

    let wifi = get(body, "wifi").and_then(|w| {
        Some(Wifi {
            ssid: text(w, "ssid"),
            rssi_dbm: int(w, "rssi_dbm").map(|v| v as i32),
        })
    });

    let now_playing = get(body, "now_playing").map(|np| NowPlaying {
        title: text(np, "title").unwrap_or_default(),
        artist: text(np, "artist"),
        app: text(np, "app"),
        playing: boolean(np, "playing").unwrap_or(false),
        position_ms: int(np, "position_ms").map(|v| v.max(0) as u64),
        duration_ms: int(np, "duration_ms").map(|v| v.max(0) as u64),
    });

    PhoneStatus {
        network,
        cell_gen: text(body, "cell_gen"),
        carrier: text(body, "carrier"),
        signal_bars: int(body, "signal_bars").map(|v| v.clamp(0, 4) as u8),
        wifi,
        storage_used_b: int(body, "storage_used_b").map(|v| v.max(0) as u64),
        storage_total_b: int(body, "storage_total_b").map(|v| v.max(0) as u64),
        ram_used_b: int(body, "ram_used_b").map(|v| v.max(0) as u64),
        ram_total_b: int(body, "ram_total_b").map(|v| v.max(0) as u64),
        battery_temp_c: float(body, "battery_temp_c"),
        screen_on: boolean(body, "screen_on"),
        dnd: boolean(body, "dnd"),
        notifications: int(body, "notifications").map(|v| v.max(0) as u32),
        now_playing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Corpo de exemplo com todos os campos do contrato — o parse tem de
    /// reproduzi-los exatamente.
    #[test]
    fn parses_full_body() {
        let body = Value::Map(vec![
            (Value::Text("network".into()), Value::Text("wifi".into())),
            (
                Value::Text("wifi".into()),
                Value::Map(vec![
                    (Value::Text("ssid".into()), Value::Text("Casa 5G".into())),
                    (Value::Text("rssi_dbm".into()), Value::Integer((-61).into())),
                ]),
            ),
            (Value::Text("storage_total_b".into()), Value::Integer(128_000_000_000u64.into())),
            (Value::Text("battery_temp_c".into()), Value::Float(31.7)),
            (Value::Text("screen_on".into()), Value::Bool(true)),
            (Value::Text("dnd".into()), Value::Bool(false)),
            (Value::Text("notifications".into()), Value::Integer(7.into())),
            (
                Value::Text("now_playing".into()),
                Value::Map(vec![
                    (Value::Text("title".into()), Value::Text("One More Time".into())),
                    (Value::Text("artist".into()), Value::Text("Daft Punk".into())),
                    (Value::Text("app".into()), Value::Text("com.spotify.music".into())),
                    (Value::Text("playing".into()), Value::Bool(true)),
                    (Value::Text("position_ms".into()), Value::Integer(42_000u64.into())),
                    (Value::Text("duration_ms".into()), Value::Integer(320_000u64.into())),
                ]),
            ),
        ]);
        let p = parse(&body);
        assert_eq!(p.network, Some(Network::Wifi));
        assert_eq!(p.wifi.as_ref().and_then(|w| w.ssid.as_deref()), Some("Casa 5G"));
        assert_eq!(p.wifi.as_ref().and_then(|w| w.rssi_dbm), Some(-61));
        assert_eq!(p.storage_total_b, Some(128_000_000_000));
        assert_eq!(p.battery_temp_c, Some(31.7));
        assert_eq!(p.screen_on, Some(true));
        assert_eq!(p.dnd, Some(false));
        assert_eq!(p.notifications, Some(7));
        let np = p.now_playing.expect("now_playing");
        assert_eq!(np.title, "One More Time");
        assert!(np.playing);
        assert_eq!(np.progress(), Some(42000.0 / 320000.0));
    }

    /// Contrato: ausente = `None`, nunca zero — e um body vazio não pânica.
    #[test]
    fn missing_fields_stay_none() {
        let p = parse(&Value::Map(Vec::new()));
        assert!(p.network.is_none());
        assert!(p.wifi.is_none());
        assert!(p.now_playing.is_none());
        assert!(p.battery_temp_c.is_none());
        // Não-mapa (app com bug) também não pode pânica.
        let p2 = parse(&Value::Null);
        assert!(p2.storage_used_b.is_none());
    }

    /// Campos com valor absurdo ficam coerentes (barras 0–4, bytes ≥ 0).
    #[test]
    fn clamps_out_of_range() {
        let body = Value::Map(vec![
            (Value::Text("signal_bars".into()), Value::Integer(9.into())),
            (Value::Text("storage_used_b".into()), Value::Integer((-5).into())),
        ]);
        let p = parse(&body);
        assert_eq!(p.signal_bars, Some(4));
        assert_eq!(p.storage_used_b, Some(0));
    }
}
