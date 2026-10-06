//! `pc.status` (D→P): telemetria real do PC para o dashboard do telemóvel —
//! o contrato está em `prompt_ai_studio_2026-10-04_3_telemetria_pc_real.md`
//! (que substitui os valores inventados 14/6/43/4 que a app mostrava).
//!
//! Cada 2 s enquanto ligado: hostname, CPU (delta de `/proc/stat`), RAM
//! (`/proc/meminfo`), temperatura (best-effort pelos thermal zones), uptime
//! e o RTT do QUIC medido pela própria conexão. Campo que não se consegue
//! ler (ex: PC sem sensor de temperatura) sai **omitido**, nunca a zeros —
//! a app mostra «—».
//!
//! Sem crates novas: tudo vem de `/proc` e `/sys`, que são estáveis há
//! décadas e não há "sensor API" multi-distro melhor que isto.

use std::sync::{Arc, Mutex};

use ciborium::Value;

use crate::active::ActiveConn;
use crate::state::HudState;

/// Primeira linha de `/proc/stat`: `cpu  user nice system idle iowait irq
/// softirq steal …` → (idle_total, busy_total). Para calcular a percentagem
/// entre duas amostras.
fn cpu_sample() -> Option<(u64, u64)> {
    let line = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .next()?
        .to_string();
    let nums: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|n| n.parse().ok())
        .collect();
    if nums.len() < 5 {
        return None;
    }
    let idle = nums[3] + *nums.get(4).unwrap_or(&0); // idle + iowait
    let busy: u64 = nums.iter().take(8.min(nums.len())).sum::<u64>() - idle;
    Some((idle, busy))
}

/// `MemTotal`/`MemAvailable` do `/proc/meminfo` → (usada, total) em bytes.
fn ram_sample() -> Option<(u64, u64)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let get_kb = |key: &str| {
        info.lines()
            .find(|l| l.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
            .map(|kb| kb * 1024)
    };
    let total = get_kb("MemTotal:")?;
    let avail = get_kb("MemAvailable:")?;
    Some((total - avail, total))
}

/// Primeiro sensor de temperatura plausível: thermal zones padrão
/// (`/sys/class/thermal/thermal_zone*/temp`, em miligrelsius). Sem sensor
/// ou valor absurdo → `None` (a app mostra «—»).
fn cpu_temp() -> Option<f32> {
    for entry in std::fs::read_dir("/sys/class/thermal").ok()? {
        let path = entry.ok()?.path().join("temp");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(millic) = raw.trim().parse::<i64>() else {
            continue;
        };
        let c = millic as f32 / 1000.0;
        if (0.0..120.0).contains(&c) {
            return Some(c);
        }
    }
    None
}

fn uptime_secs() -> Option<u64> {
    std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "PC".to_string())
}

fn kv(key: &str, val: Value) -> (Value, Value) {
    (Value::Text(key.into()), val)
}

/// Envia `pc.status` a cada 2 s enquanto houver telemóvel ligado.
pub async fn poll_and_push(active: ActiveConn, _hud: Arc<Mutex<HudState>>) {
    let host = hostname();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_cpu: Option<(u64, u64)> = None;
    loop {
        interval.tick().await;
        let Some(connection) = active.lock().unwrap().clone() else {
            last_cpu = None; // reseta a base de delta — CPU entre sessões não se compara
            continue;
        };

        let mut pairs: Vec<(Value, Value)> = vec![kv("hostname", Value::Text(host.clone()))];

        // CPU: percentagem entre a amostra anterior e esta.
        if let (Some(now), Some(prev)) = (cpu_sample(), last_cpu) {
            let d_idle = now.0.saturating_sub(prev.0);
            let d_busy = now.1.saturating_sub(prev.1);
            let d_total = d_idle + d_busy;
            if d_total > 0 {
                let pct = d_busy as f64 * 100.0 / d_total as f64;
                pairs.push(kv("cpu_pct", Value::Float(pct)));
            }
        }
        last_cpu = cpu_sample().or(last_cpu);

        if let Some((used, total)) = ram_sample() {
            pairs.push(kv("ram_used_b", Value::Integer(used.into())));
            pairs.push(kv("ram_total_b", Value::Integer(total.into())));
        }
        if let Some(t) = cpu_temp() {
            pairs.push(kv("cpu_temp_c", Value::Float(t as f64)));
        }
        if let Some(up) = uptime_secs() {
            pairs.push(kv("uptime_s", Value::Integer(up.into())));
        }
        // RTT do QUIC — o mesmo número que a GUI vê no Event::Telemetry.
        let rtt = connection.rtt();
        if !rtt.is_zero() {
            pairs.push(kv("rtt_ms", Value::Float(rtt.as_secs_f64() * 1000.0)));
        }

        let _ = crate::active::push(&active, "pc.status", Some(Value::Map(pairs))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_sample_has_sane_shape() {
        // Máquina real (CI incluído): 4+ campos e valores crescentes.
        if let Some((idle, busy)) = cpu_sample() {
            assert!(idle > 0 || busy > 0);
        }
    }

    #[test]
    fn ram_total_exceeds_used() {
        if let Some((used, total)) = ram_sample() {
            assert!(total > used, "MemTotal tem de exceder a usada");
        }
    }

    #[test]
    fn body_kv_shapes() {
        let pairs = vec![
            kv("hostname", Value::Text("pc".into())),
            kv("cpu_pct", Value::Float(12.5)),
        ];
        let Value::Map(m) = Value::Map(pairs) else {
            unreachable!()
        };
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].0.as_text(), Some("hostname"));
        assert_eq!(m[1].1.as_float(), Some(12.5));
    }
}
