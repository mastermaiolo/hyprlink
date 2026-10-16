//! `hyprlinkctl status --waybar`: uma linha JSON `{text, tooltip, class}`
//! para um módulo `custom` da Waybar (`"return-type": "json"`), a partir do
//! `$XDG_RUNTIME_DIR/hyprlink/status.json` que o daemon publica a cada 2 s.
//! Mesma lógica e textos do `contrib/waybar-hyprlink.sh`; sai sempre com 0
//! (a Waybar esconde o módulo se o comando falhar).
//!
//! A Waybar lê `text` e `tooltip` como markup Pango: `&`, `<` e `>` do nome
//! do telemóvel e do que está a tocar saem escapados.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::ExitCode;

fn status_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", rustix_uid())));
    dir.join("hyprlink").join("status.json")
}

/// O uid sem dependências: o dono de `/proc/self`.
fn rustix_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(0)
}

fn markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A linha para a Waybar, a partir do conteúdo do `status.json` (`None` =
/// o ficheiro não existe: o daemon não está a correr).
pub fn line(status: Option<&str>) -> Value {
    let Some(raw) = status else {
        return json!({
            "text": "📱✗",
            "tooltip": "daemon do HyprLink não está a correr",
            "class": "disconnected",
        });
    };
    let Ok(s) = serde_json::from_str::<Value>(raw) else {
        return json!({
            "text": "📱?",
            "tooltip": "estado do HyprLink ilegível",
            "class": "unknown",
        });
    };
    if s["connected"] != Value::Bool(true) {
        return json!({
            "text": "📱✗",
            "tooltip": "sem telemóvel conectado",
            "class": "disconnected",
        });
    }
    let on = |k: &str| s[k] == Value::Bool(true);
    let text_of = |k: &str| match &s[k] {
        Value::Null => None,
        Value::String(v) if v.is_empty() => None,
        Value::String(v) => Some(v.clone()),
        other => Some(other.to_string()),
    };
    let mut text = "📱".to_string();
    if let Some(b) = text_of("phone_battery_pct") {
        text += &format!(" {b}%");
    }
    if on("speaker") {
        text += " 🔊";
    }
    if on("audio_tap") {
        text += " 🎧";
    }
    if on("mic") {
        text += " 🎙";
    }
    let mut tip = markup(&text_of("device").unwrap_or_else(|| "?".into()));
    if let Some(m) = text_of("media") {
        tip += &format!(" · {}", markup(&m));
    }
    if on("speaker") {
        tip += " · som no telemóvel";
    }
    json!({ "text": text, "tooltip": tip, "class": "connected" })
}

pub fn run() -> ExitCode {
    let raw = std::fs::read_to_string(status_path()).ok();
    println!(
        "{}",
        serde_json::to_string(&line(raw.as_deref())).unwrap_or_default()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sem_status_e_desligado() {
        let v = line(None);
        assert_eq!(v["class"], "disconnected");
        assert_eq!(v["tooltip"], "daemon do HyprLink não está a correr");
        let v = line(Some(r#"{"connected":false,"device":null}"#));
        assert_eq!(v["class"], "disconnected");
        assert_eq!(v["tooltip"], "sem telemóvel conectado");
        assert_eq!(line(Some("{meio ficheiro"))["class"], "unknown");
    }

    #[test]
    fn ligado_com_nome_dificil() {
        let st = json!({
            "connected": true,
            "device": "Poco \"F4\"\n2.ª linha 🚀 <b>&co",
            "phone_battery_pct": 49,
            "media": "Ana & Rui — <ao vivo>",
            "audio_tap": true,
            "mic": true,
            "speaker": true,
        });
        let v = line(Some(&st.to_string()));
        assert_eq!(v["class"], "connected");
        assert_eq!(v["text"], "📱 49% 🔊 🎧 🎙");
        assert_eq!(
            v["tooltip"],
            "Poco \"F4\"\n2.ª linha 🚀 &lt;b&gt;&amp;co · Ana &amp; Rui — &lt;ao vivo&gt; · som no telemóvel"
        );
        // Uma só linha JSON, que volta a ler-se igual.
        let out = serde_json::to_string(&v).unwrap();
        assert!(!out.contains('\n'));
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), v);
    }

    #[test]
    fn campos_em_falta_como_o_jq() {
        let v = line(Some(
            r#"{"connected":true,"device":null,"phone_battery_pct":null,"media":""}"#,
        ));
        assert_eq!(v["text"], "📱");
        assert_eq!(v["tooltip"], "?");
    }
}
