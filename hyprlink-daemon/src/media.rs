//! Controlo de mídia via MPRIS (D-Bus de sessão) — sem `playerctl`, direto
//! por zbus.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ciborium::Value;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, Proxy};

use crate::active::{push, ActiveConn};
use crate::state::HudState;

const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

async fn active_player_name(conn: &Connection) -> Option<String> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await.ok()?;
    let names = dbus.list_names().await.ok()?;
    names
        .into_iter()
        .map(|n| n.to_string())
        .find(|n| n.starts_with("org.mpris.MediaPlayer2."))
}

/// `media.command` vindo do telemóvel: play_pause/play/pause/next/previous.
pub async fn handle_command(cmd: &str) {
    let method = match cmd {
        "play_pause" | "playpause" => "PlayPause",
        "play" => "Play",
        "pause" => "Pause",
        "next" => "Next",
        "previous" => "Previous",
        _ => return,
    };
    let Ok(conn) = Connection::session().await else { return };
    let Some(name) = active_player_name(&conn).await else { return };
    let Ok(proxy) = Proxy::new(&conn, name, PLAYER_PATH, PLAYER_IFACE).await else { return };
    let _: Result<(), _> = proxy.call(method, &()).await;
}

fn metadata_str(meta: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let value = meta.get(key)?;
    String::try_from(value.clone()).ok()
}

fn metadata_first_str_in_array(meta: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let value = meta.get(key)?;
    let list: Vec<String> = Vec::try_from(value.clone()).ok()?;
    list.into_iter().next()
}

struct NowPlaying {
    player: String,
    status: String,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}

async fn snapshot(conn: &Connection) -> Option<NowPlaying> {
    let name = active_player_name(conn).await?;
    let proxy = Proxy::new(conn, name.clone(), PLAYER_PATH, PLAYER_IFACE).await.ok()?;
    let status: String = proxy.get_property("PlaybackStatus").await.unwrap_or_default();
    let meta: HashMap<String, OwnedValue> = proxy.get_property("Metadata").await.unwrap_or_default();
    let player = name
        .trim_start_matches("org.mpris.MediaPlayer2.")
        .split('.')
        .next()
        .unwrap_or(&name)
        .to_string();
    Some(NowPlaying {
        player,
        status,
        title: metadata_str(&meta, "xesam:title"),
        artist: metadata_first_str_in_array(&meta, "xesam:artist"),
        album: metadata_str(&meta, "xesam:album"),
    })
}

fn to_body(np: &NowPlaying) -> Value {
    let mut map = vec![
        (Value::Text("player".into()), Value::Text(np.player.clone())),
        (Value::Text("status".into()), Value::Text(np.status.clone())),
    ];
    if let Some(t) = &np.title {
        map.push((Value::Text("title".into()), Value::Text(t.clone())));
    }
    if let Some(a) = &np.artist {
        map.push((Value::Text("artist".into()), Value::Text(a.clone())));
    }
    if let Some(a) = &np.album {
        map.push((Value::Text("album".into()), Value::Text(a.clone())));
    }
    Value::Map(map)
}

/// Poll baixo (2s) do player MPRIS ativo, empurra `media.state` só quando
/// algo muda de verdade.
pub async fn poll_and_push(active: ActiveConn, _hud: Arc<Mutex<HudState>>) {
    let mut last_key: Option<(String, String, Option<String>)> = None;
    loop {
        if let Ok(conn) = Connection::session().await {
            if let Some(np) = snapshot(&conn).await {
                let key = (np.player.clone(), np.status.clone(), np.title.clone());
                if last_key.as_ref() != Some(&key) {
                    last_key = Some(key);
                    push(&active, "media.state", Some(to_body(&np))).await;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
