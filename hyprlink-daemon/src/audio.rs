//! Mixer de áudio via `pactl` (PipeWire-pulse) — sem bindings C do
//! libpipewire, só parseando o JSON que o próprio `pactl` já sabe gerar.

use std::collections::HashMap;
use std::process::Command;

use ciborium::Value;
use serde::Deserialize;

/// Nome convencionado do sink virtual do telemóvel (audio tap) — ainda não
/// criado nesta fase (isso é o "ouvir no telemóvel", parte do tap de
/// verdade), só reservado aqui pra já bater com o protocolo.
pub const PHONE_SINK_NAME: &str = "hyprlink-speaker";

#[derive(Deserialize)]
struct PaVolumeChannel {
    value_percent: String,
}

type PaVolume = HashMap<String, PaVolumeChannel>;

#[derive(Deserialize)]
struct PaSink {
    index: u64,
    name: String,
    description: String,
    mute: bool,
    volume: PaVolume,
    #[serde(default)]
    properties: HashMap<String, String>,
}

#[derive(Deserialize)]
struct PaSinkInput {
    index: u64,
    sink: u64,
    mute: bool,
    volume: PaVolume,
    #[serde(default)]
    properties: HashMap<String, String>,
}

fn run(args: &[&str]) -> bool {
    Command::new("pactl").args(args).output().map(|o| o.status.success()).unwrap_or(false)
}

fn run_text(args: &[&str]) -> String {
    Command::new("pactl")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn run_json<T: serde::de::DeserializeOwned>(args: &[&str]) -> Vec<T> {
    let out = Command::new("pactl").args(args).output();
    let Ok(out) = out else { return Vec::new() };
    serde_json::from_slice(&out.stdout).unwrap_or_default()
}

fn avg_percent(volume: &PaVolume) -> i64 {
    let vals: Vec<i64> =
        volume.values().filter_map(|c| c.value_percent.trim_end_matches('%').parse().ok()).collect();
    if vals.is_empty() {
        100
    } else {
        vals.iter().sum::<i64>() / vals.len() as i64
    }
}

fn sink_description(s: &PaSink) -> String {
    s.properties
        .get("device.description")
        .filter(|d| d.as_str() != "(null)")
        .cloned()
        .or_else(|| (s.description != "(null)").then(|| s.description.clone()))
        .unwrap_or_else(|| s.name.clone())
}

/// `audio.state_reply`: sinks (saídas de som) + apps (streams por aplicação).
pub fn state_body() -> Value {
    let default_sink = run_text(&["get-default-sink"]);
    let sinks: Vec<PaSink> = run_json(&["-f", "json", "list", "sinks"]);
    let inputs: Vec<PaSinkInput> = run_json(&["-f", "json", "list", "sink-inputs"]);

    let sinks_val: Vec<Value> = sinks
        .iter()
        .map(|s| {
            Value::Map(vec![
                (Value::Text("id".into()), Value::Integer(s.index.into())),
                (Value::Text("name".into()), Value::Text(s.name.clone())),
                (Value::Text("description".into()), Value::Text(sink_description(s))),
                (Value::Text("volume".into()), Value::Integer(avg_percent(&s.volume).into())),
                (Value::Text("muted".into()), Value::Bool(s.mute)),
                (Value::Text("is_default".into()), Value::Bool(s.name == default_sink)),
                (Value::Text("is_phone".into()), Value::Bool(s.name == PHONE_SINK_NAME)),
            ])
        })
        .collect();

    let apps_val: Vec<Value> = inputs
        .iter()
        .map(|i| {
            let name = i
                .properties
                .get("application.name")
                .or_else(|| i.properties.get("node.name"))
                .cloned()
                .unwrap_or_else(|| "App".to_string());
            let media = i.properties.get("media.name").map(|m| Value::Text(m.clone())).unwrap_or(Value::Null);
            Value::Map(vec![
                (Value::Text("id".into()), Value::Integer(i.index.into())),
                (Value::Text("name".into()), Value::Text(name)),
                (Value::Text("media".into()), media),
                (Value::Text("volume".into()), Value::Integer(avg_percent(&i.volume).into())),
                (Value::Text("muted".into()), Value::Bool(i.mute)),
                (Value::Text("sink_id".into()), Value::Integer(i.sink.into())),
            ])
        })
        .collect();

    Value::Map(vec![
        (Value::Text("default_sink".into()), Value::Text(default_sink)),
        (Value::Text("sinks".into()), Value::Array(sinks_val)),
        (Value::Text("apps".into()), Value::Array(apps_val)),
    ])
}

pub fn set_volume(kind: &str, id: i64, volume: i64) {
    let vol = format!("{}%", volume.clamp(0, 150));
    match kind {
        "sink" => {
            run(&["set-sink-volume", &id.to_string(), &vol]);
        }
        "app" => {
            run(&["set-sink-input-volume", &id.to_string(), &vol]);
        }
        _ => {}
    }
}

pub fn set_mute(kind: &str, id: i64, muted: bool) {
    let val = if muted { "1" } else { "0" };
    match kind {
        "sink" => {
            run(&["set-sink-mute", &id.to_string(), val]);
        }
        "app" => {
            run(&["set-sink-input-mute", &id.to_string(), val]);
        }
        _ => {}
    }
}

/// Muda o sink padrão E move os streams já tocando pra ele — trocar de
/// saída só pra novos sons e deixar o que já está tocando pra trás seria
/// uma experiência ruim (o utilizador espera que "mude tudo agora").
pub fn set_default_sink(name: &str) {
    run(&["set-default-sink", name]);
    let inputs: Vec<PaSinkInput> = run_json(&["-f", "json", "list", "sink-inputs"]);
    for input in inputs {
        run(&["move-sink-input", &input.index.to_string(), name]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Não roda em CI — depende do PipeWire/pactl reais da sessão.
    /// `cargo test -- --ignored manual_state`.
    #[test]
    #[ignore]
    fn manual_state() {
        let body = state_body();
        println!("{body:#?}");
        let Value::Map(pairs) = &body else { panic!("esperava um mapa") };
        let sinks = pairs.iter().find(|(k, _)| k.as_text() == Some("sinks")).map(|(_, v)| v);
        assert!(matches!(sinks, Some(Value::Array(v)) if !v.is_empty()), "esperava pelo menos um sink real");
    }
}
