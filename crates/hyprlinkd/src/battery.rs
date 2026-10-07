//! Bateria real do PC via `/sys/class/power_supply`.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ciborium::Value;

use crate::active::{ActiveConn, push};
use crate::state::{self, HudState};

const SYS: &str = "/sys";

/// Batimento: mesmo sem mudanças, o `battery.state` repete-se de 5 em 5 min.
const HEARTBEAT: Duration = Duration::from_secs(300);

/// Estado da bateria do PC e a regra de escolha (a do sistema, não a de
/// periféricos): ver `hyprlink_env::battery`. `None` = o PC não tem bateria.
pub use hyprlink_env::battery::{PcBattery, read_at};

pub fn read() -> Option<PcBattery> {
    read_at(Path::new(SYS))
}

/// Corpo do `battery.state` D→P: `{level, charging, plugged, present}`.
/// Sem bateria só vai `present:false` — nada de valores inventados.
pub fn body_of(b: Option<PcBattery>) -> Value {
    let kv = |k: &str, v: Value| (Value::Text(k.into()), v);
    Value::Map(match b {
        Some(b) => vec![
            kv("level", Value::Integer(b.level.into())),
            kv("charging", Value::Bool(b.charging)),
            kv("plugged", Value::Bool(b.plugged)),
            kv("present", Value::Bool(true)),
        ],
        None => vec![kv("present", Value::Bool(false))],
    })
}

pub fn state_body() -> Value {
    body_of(read())
}

/// Quando empurrar o `battery.state`: só com telemóvel ligado, quando
/// `level`/`charging`/`plugged`/`present` mudam e no batimento de 5 min.
#[derive(Default)]
struct PushGate {
    last: Option<(Option<PcBattery>, Instant)>,
}

impl PushGate {
    fn due(&mut self, cur: Option<PcBattery>, now: Instant, connected: bool) -> bool {
        if !connected {
            // O telemóvel volta a receber o estado logo que ligar.
            self.last = None;
            return false;
        }
        match self.last {
            Some((prev, at)) if prev == cur && now.duration_since(at) < HEARTBEAT => false,
            _ => {
                self.last = Some((cur, now));
                true
            }
        }
    }
}

/// Poll de baixa frequência (30 s) que empurra `battery.state` ao telemóvel
/// segundo o `PushGate`. Também alimenta `HudState` (bateria do PC pra GUI) e
/// amostra o histórico de 12h a cada ~5 min (10 iterações).
pub async fn poll_and_push(active: ActiveConn, hud: Arc<Mutex<HudState>>) {
    let mut gate = PushGate::default();
    let mut ticks_since_sample = 0u32;
    loop {
        let current = read();
        if let Some(b) = current {
            state::set_pc_battery(&hud, b.level, b.charging);
        }
        let connected = active.lock().unwrap().is_some();
        if gate.due(current, Instant::now(), connected) {
            push(&active, "battery.state", Some(body_of(current))).await;
        }
        ticks_since_sample += 1;
        if ticks_since_sample >= 10 {
            ticks_since_sample = 0;
            state::sample_battery_history(&hud);
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{body_get, body_get_bool, body_get_i64};
    use std::path::PathBuf;

    fn fake_sys(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hyprlink-bat-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("class/power_supply")).unwrap();
        d
    }

    fn supply(sys: &Path, name: &str, files: &[(&str, &str)]) {
        let d = sys.join("class/power_supply").join(name);
        std::fs::create_dir_all(&d).unwrap();
        for (f, v) in files {
            std::fs::write(d.join(f), format!("{v}\n")).unwrap();
        }
    }

    fn laptop(tag: &str, capacity: &str, status: &str, online: &str) -> PathBuf {
        let sys = fake_sys(tag);
        supply(&sys, "ADP0", &[("type", "Mains"), ("online", online)]);
        supply(
            &sys,
            "BAT0",
            &[
                ("type", "Battery"),
                ("capacity", capacity),
                ("status", status),
            ],
        );
        sys
    }

    fn bat(b: Option<PcBattery>) -> (i64, bool, bool) {
        let b = b.expect("bateria presente");
        (b.level, b.charging, b.plugged)
    }

    #[test]
    fn on_battery() {
        let sys = laptop("usar", "63", "Discharging", "0");
        assert_eq!(bat(read_at(&sys)), (63, false, false));
    }

    #[test]
    fn charging() {
        let sys = laptop("carregar", "41", "Charging", "1");
        assert_eq!(bat(read_at(&sys)), (41, true, true));
    }

    #[test]
    fn full_on_mains() {
        let sys = laptop("cheia", "100", "Full", "1");
        assert_eq!(bat(read_at(&sys)), (100, false, true));
    }

    #[test]
    fn charge_limit_at_80_is_plugged_but_not_charging() {
        let sys = laptop("limite", "80", "Not charging", "1");
        assert_eq!(bat(read_at(&sys)), (80, false, true));
    }

    #[test]
    fn charging_without_a_mains_supply_is_still_plugged() {
        let sys = fake_sys("usbc");
        supply(
            &sys,
            "ucsi-source-psy-1",
            &[("type", "USB"), ("online", "1")],
        );
        supply(
            &sys,
            "BAT0",
            &[
                ("type", "Battery"),
                ("capacity", "30"),
                ("status", "Charging"),
            ],
        );
        assert_eq!(bat(read_at(&sys)), (30, true, true));
    }

    #[test]
    fn no_battery_means_not_present() {
        let sys = fake_sys("desktop");
        supply(&sys, "AC", &[("type", "Mains"), ("online", "1")]);
        assert_eq!(read_at(&sys), None);
        assert_eq!(read_at(Path::new("/nao/existe")), None);
        let body = body_of(None);
        assert_eq!(body_get_bool(&body, "present"), Some(false));
        for k in ["level", "charging", "plugged"] {
            assert!(body_get(&body, k).is_none(), "{k} não se inventa");
        }
    }

    #[test]
    fn body_has_all_four_fields() {
        let sys = laptop("corpo", "80", "Not charging", "1");
        let body = body_of(read_at(&sys));
        assert_eq!(body_get_i64(&body, "level"), Some(80));
        assert_eq!(body_get_bool(&body, "charging"), Some(false));
        assert_eq!(body_get_bool(&body, "plugged"), Some(true));
        assert_eq!(body_get_bool(&body, "present"), Some(true));
    }

    #[test]
    fn gate_pushes_on_change_and_on_heartbeat_only_when_connected() {
        let b = |level, charging, plugged| {
            Some(PcBattery {
                level,
                charging,
                plugged,
            })
        };
        let t0 = Instant::now();
        let mut g = PushGate::default();
        assert!(!g.due(b(50, false, false), t0, false), "sem telemóvel");
        assert!(g.due(b(50, false, false), t0, true), "primeiro envio");
        assert!(!g.due(b(50, false, false), t0 + Duration::from_secs(30), true));
        assert!(
            g.due(b(49, false, false), t0 + Duration::from_secs(60), true),
            "level"
        );
        assert!(
            g.due(b(49, true, true), t0 + Duration::from_secs(90), true),
            "charging+plugged"
        );
        assert!(
            g.due(b(49, false, true), t0 + Duration::from_secs(120), true),
            "só charging"
        );
        assert!(
            g.due(b(49, false, false), t0 + Duration::from_secs(150), true),
            "só plugged"
        );
        assert!(!g.due(b(49, false, false), t0 + Duration::from_secs(200), true));
        assert!(
            g.due(b(49, false, false), t0 + Duration::from_secs(450), true),
            "batimento"
        );
        // Desligou e voltou: reenvia mesmo sem mudanças.
        assert!(!g.due(b(49, false, false), t0 + Duration::from_secs(480), false));
        assert!(g.due(b(49, false, false), t0 + Duration::from_secs(510), true));
        // Sem bateria também se anuncia (present:false), uma vez.
        assert!(g.due(None, t0 + Duration::from_secs(540), true));
        assert!(!g.due(None, t0 + Duration::from_secs(570), true));
    }
}
