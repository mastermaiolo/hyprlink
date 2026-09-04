//! Volume e modo de toque (som/vibra/silencioso) do telemóvel, controlados
//! pelo PC — o inverso do mixer local (`audio.rs`, que é o PC controlado
//! pelo telemóvel). Todo em percentual (0-100) pra GUI não precisar saber
//! os steps reais do `AudioManager` de cada stream.

use ciborium::Value;

use crate::active::{self, ActiveConn};
use crate::protocol::{body_get_bool, body_get_i64, body_get_str};

#[derive(Debug, Clone, Default)]
pub struct PhoneAudioState {
    pub ring_percent: i64,
    pub media_percent: i64,
    pub alarm_percent: i64,
    /// "normal" | "vibrate" | "silent".
    pub ringer_mode: String,
    /// Se o telemóvel não concedeu acesso "Não Perturbe", `set_ringer_mode`
    /// pra vibrar/silencioso e `set_dnd` são ignorados do lado dele — a GUI
    /// avisa disso.
    pub dnd_access: bool,
    /// Filtro de interrupção do Android (`NotificationManager`) — diferente
    /// do `ringer_mode` (som/vibra/silencioso), é o "Não Perturbe" de
    /// verdade (deixa passar só notificações prioritárias).
    pub dnd_enabled: bool,
}

pub async fn get_state(active: &ActiveConn) -> Option<PhoneAudioState> {
    let packet = active::request(active, "phone_audio.state", None).await?;
    let body = packet.body?;
    Some(PhoneAudioState {
        ring_percent: body_get_i64(&body, "ring_percent").unwrap_or(0),
        media_percent: body_get_i64(&body, "media_percent").unwrap_or(0),
        alarm_percent: body_get_i64(&body, "alarm_percent").unwrap_or(0),
        ringer_mode: body_get_str(&body, "ringer_mode").unwrap_or("normal").to_string(),
        dnd_access: body_get_bool(&body, "dnd_access").unwrap_or(false),
        dnd_enabled: body_get_bool(&body, "dnd_enabled").unwrap_or(false),
    })
}

pub async fn set_volume(active: &ActiveConn, stream: &str, percent: i64) {
    let body = Value::Map(vec![
        (Value::Text("stream".into()), Value::Text(stream.to_string())),
        (Value::Text("percent".into()), Value::Integer(percent.clamp(0, 100).into())),
    ]);
    active::push(active, "phone_audio.set_volume", Some(body)).await;
}

pub async fn set_ringer_mode(active: &ActiveConn, mode: &str) {
    let body = Value::Map(vec![(Value::Text("mode".into()), Value::Text(mode.to_string()))]);
    active::push(active, "phone_audio.set_ringer_mode", Some(body)).await;
}

pub async fn set_dnd(active: &ActiveConn, enabled: bool) {
    let body = Value::Map(vec![(Value::Text("enabled".into()), Value::Bool(enabled))]);
    active::push(active, "phone_audio.set_dnd", Some(body)).await;
}
