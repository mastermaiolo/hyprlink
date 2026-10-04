//! Golden do JSON v1 (`hyprlinkctl watch --json`): os campos que o plugin do
//! Noctalia lê têm de existir, com estes tipos JSON, e pela ordem de leitura
//! (o telemóvel primeiro, este PC no fim). Só se pode acrescentar.

use hyprlink_proto::host::Probe;
use hyprlink_proto::link::Transport;
use hyprlink_proto::link::mock::Simulator;
use hyprlink_proto::snapshot::{Snapshot, VERSION};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum T {
    Str,
    Num,
    Bool,
    Arr,
    Obj,
}

fn is(v: &Value, t: T) -> bool {
    match t {
        T::Str => v.is_string(),
        T::Num => v.is_number(),
        T::Bool => v.is_boolean(),
        T::Arr => v.is_array(),
        T::Obj => v.is_object(),
    }
}

/// (caminho, tipo, pode ser null)
const FIELDS: &[(&str, T, bool)] = &[
    ("/v", T::Num, false),
    ("/daemon", T::Str, false),
    ("/device", T::Obj, true),
    ("/device/name", T::Str, false),
    ("/device/manufacturer", T::Str, true),
    ("/device/model", T::Str, true),
    ("/device/state", T::Str, false),
    ("/device/battery", T::Num, true),
    ("/device/charging", T::Bool, false),
    ("/phone", T::Obj, true),
    ("/phone/network", T::Str, true),
    ("/phone/cell_gen", T::Str, true),
    ("/phone/carrier", T::Str, true),
    ("/phone/wifi", T::Obj, true),
    ("/phone/storage_used_b", T::Num, true),
    ("/phone/storage_total_b", T::Num, true),
    ("/phone/battery_temp_c", T::Num, true),
    ("/phone/screen_on", T::Bool, true),
    ("/phone/notifications", T::Num, true),
    ("/phone/now_playing", T::Obj, true),
    ("/phone/now_playing/title", T::Str, false),
    ("/phone/now_playing/artist", T::Str, true),
    ("/phone/now_playing/app", T::Str, true),
    ("/phone/now_playing/playing", T::Bool, false),
    ("/phone/now_playing/position_ms", T::Num, true),
    ("/phone/now_playing/duration_ms", T::Num, true),
    ("/latency_ms", T::Num, true),
    ("/latency_hist", T::Arr, false),
    ("/mic/on", T::Bool, false),
    ("/tap/on", T::Bool, false),
    ("/tap/speaker", T::Bool, false),
    ("/mirror/on", T::Bool, false),
    ("/mirror/fps", T::Num, true),
    ("/host", T::Obj, true),
    ("/host/hostname", T::Str, false),
    ("/host/compositor", T::Str, false),
    ("/host/cpu_pct", T::Num, false),
    ("/host/ram_used_gb", T::Num, false),
    ("/host/ram_total_gb", T::Num, false),
    // [percent, charging] — o plugin lê h.battery[1].
    ("/host/battery", T::Arr, true),
    ("/host/uptime_s", T::Num, false),
];

fn check(v: &Value) {
    for &(path, t, nullable) in FIELDS {
        // Descendente de um objeto null: não há nada a verificar.
        let null_ancestor = path
            .match_indices('/')
            .skip(1)
            .any(|(i, _)| v.pointer(&path[..i]).is_some_and(Value::is_null));
        if null_ancestor {
            continue;
        }
        let x = v
            .pointer(path)
            .unwrap_or_else(|| panic!("falta {path} em {v}"));
        assert!(
            is(x, t) || (nullable && x.is_null()),
            "{path}: esperado {t:?}{}, veio {x}",
            if nullable { " ou null" } else { "" }
        );
    }
}

fn mock_snapshot() -> Value {
    let mut sim = Simulator::new();
    let mut s = Snapshot::new("mock");
    for _ in 0..80 {
        for e in sim.poll(Duration::from_millis(50)) {
            s.apply(&e);
        }
    }
    s.host = Some(Probe::default().sample());
    serde_json::to_value(&s).unwrap()
}

#[test]
fn v1_fields_and_types_mock() {
    let v = mock_snapshot();
    assert_eq!(v["v"], VERSION);
    assert_eq!(VERSION, 1);
    // O mock tem de exercitar os campos aninhados, não só os null.
    for p in ["/device/name", "/phone/now_playing/title", "/host/hostname"] {
        assert!(v.pointer(p).is_some_and(|x| !x.is_null()), "mock sem {p}");
    }
    check(&v);
}

#[test]
fn v1_fields_and_types_empty() {
    // Daemon parado / sem telemóvel: tudo o que é opcional vem null.
    let v = serde_json::to_value(Snapshot::new("down")).unwrap();
    check(&v);
    assert!(v["device"].is_null() && v["phone"].is_null() && v["latency_ms"].is_null());
}

/// Ordem de leitura: telemóvel antes do link, link antes deste PC.
#[test]
fn v1_reading_order() {
    let line = serde_json::to_string(&Snapshot::new("mock")).unwrap();
    let pos = |k: &str| {
        line.find(&format!("\"{k}\":"))
            .unwrap_or_else(|| panic!("falta {k}"))
    };
    let order = [
        "v",
        "daemon",
        "device",
        "phone",
        "latency_ms",
        "mic",
        "tap",
        "mirror",
        "host",
    ];
    for w in order.windows(2) {
        assert!(
            pos(w[0]) < pos(w[1]),
            "{} devia vir antes de {}",
            w[0],
            w[1]
        );
    }
}
