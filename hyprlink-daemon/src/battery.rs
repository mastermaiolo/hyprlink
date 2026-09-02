//! Bateria real do PC via `/sys/class/power_supply`.

use ciborium::Value;

use crate::active::{push, ActiveConn};

/// `(nível 0-100, a carregar)` — `None` se a máquina não tiver bateria
/// (desktop), caso em que o daemon simplesmente não responde a esse módulo.
pub fn read() -> Option<(i64, bool)> {
    let entries = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("BAT") {
            continue;
        }
        let dir = entry.path();
        let level: i64 = std::fs::read_to_string(dir.join("capacity"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
        let charging = status.trim().eq_ignore_ascii_case("Charging");
        return Some((level, charging));
    }
    None
}

fn body(level: i64, charging: bool) -> Value {
    Value::Map(vec![
        (Value::Text("level".into()), Value::Integer(level.into())),
        (Value::Text("charging".into()), Value::Bool(charging)),
    ])
}

pub fn state_body() -> Value {
    let (level, charging) = read().unwrap_or((100, false));
    body(level, charging)
}

/// Poll de baixa frequência empurrando `battery.state` só quando muda —
/// espelha o que o app já faz do lado dele.
pub async fn poll_and_push(active: ActiveConn) {
    let mut last: Option<(i64, bool)> = None;
    loop {
        if let Some(current) = read() {
            if last != Some(current) {
                last = Some(current);
                push(&active, "battery.state", Some(body(current.0, current.1))).await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
