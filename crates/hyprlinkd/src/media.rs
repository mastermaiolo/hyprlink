//! Controlo de mídia via MPRIS (D-Bus de sessão) — sem `playerctl`, direto
//! por zbus.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ciborium::Value;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, Proxy};

use crate::active::{ActiveConn, push};
use crate::state::{self, HudState};

const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

/// Nomes MPRIS candidatos — exclui `playerctld`: é um proxy/agregador (não
/// um player de verdade) e quebra ao ser consultado diretamente se não
/// estiver ativamente delegando (visto ao vivo: "Object does not exist at
/// path" mesmo listado no bus). Consultar os players reais direto é mais
/// simples e mais robusto.
async fn candidate_names(conn: &Connection) -> Vec<String> {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else {
        return Vec::new();
    };
    let Ok(names) = dbus.list_names().await else {
        return Vec::new();
    };
    names
        .into_iter()
        .map(|n| n.to_string())
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2.") && !n.contains("playerctld"))
        .collect()
}

/// Entre os players reais, prefere um que esteja "Playing"; sem nenhum
/// tocando, usa o primeiro que responder de verdade (ex: pausado ainda
/// mostra a faixa atual) — nunca um nome que nem existe mais no bus.
async fn best_player(conn: &Connection) -> Option<(String, String)> {
    let names = candidate_names(conn).await;
    let mut fallback = None;
    for name in names {
        let Ok(proxy) = Proxy::new(conn, name.clone(), PLAYER_PATH, PLAYER_IFACE).await else {
            continue;
        };
        let Ok(status) = proxy.get_property::<String>("PlaybackStatus").await else {
            continue;
        };
        if status == "Playing" {
            return Some((name, status));
        }
        if fallback.is_none() {
            fallback = Some((name, status));
        }
    }
    fallback
}

/// `media.command` vindo do telemóvel: play_pause/play/pause/next/previous.
///
/// ponytail: não reusa a conexão D-Bus compartilhada do `Ctx` — quem chama
/// isto é a GUI (`gui/mod.rs`), que roda no executor do `iced`/runtime tokio
/// próprio, diferente do runtime do daemon onde essa conexão foi criada;
/// misturar os dois é terreno arriscado (I/O preso a um runtime específico)
/// pra um caminho raro (clique manual do usuário, não um poll contínuo).
/// Se algum dia isso for chamado com frequência, mover a chamada pro lado do
/// daemon (via `ctx.active`/canal) resolve isso também.
pub async fn handle_command(cmd: &str) {
    let method = match cmd {
        "play_pause" | "playpause" => "PlayPause",
        "play" => "Play",
        "pause" => "Pause",
        "next" => "Next",
        "previous" => "Previous",
        _ => return,
    };
    let Ok(conn) = Connection::session().await else {
        return;
    };
    let Some((name, _)) = best_player(&conn).await else {
        return;
    };
    let Ok(proxy) = Proxy::new(&conn, name, PLAYER_PATH, PLAYER_IFACE).await else {
        return;
    };
    let _: Result<(), _> = proxy.call(method, &()).await;
}

/// Um player MPRIS, em bruto, para o socket local.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerInfo {
    /// Sufixo do nome no bus: `org.mpris.MediaPlayer2.<id>`.
    pub id: String,
    pub identity: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub playing: bool,
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
}

/// Todos os players reais no bus (sem o playerctld).
pub async fn players(conn: &Connection) -> Vec<PlayerInfo> {
    let mut out = Vec::new();
    for name in candidate_names(conn).await {
        let Ok(player) = Proxy::new(conn, name.clone(), PLAYER_PATH, PLAYER_IFACE).await else {
            continue;
        };
        let Ok(status) = player.get_property::<String>("PlaybackStatus").await else {
            continue;
        };
        let meta: HashMap<String, OwnedValue> =
            player.get_property("Metadata").await.unwrap_or_default();
        let identity =
            match Proxy::new(conn, name.clone(), PLAYER_PATH, "org.mpris.MediaPlayer2").await {
                Ok(root) => root.get_property::<String>("Identity").await.ok(),
                Err(_) => None,
            };
        let id = name
            .trim_start_matches("org.mpris.MediaPlayer2.")
            .to_string();
        // MPRIS dá microssegundos.
        let position_ms = player
            .get_property::<i64>("Position")
            .await
            .ok()
            .and_then(|us| u64::try_from(us).ok())
            .map(|us| us / 1000);
        let duration_ms = meta
            .get("mpris:length")
            .and_then(|v| {
                i64::try_from(v.clone())
                    .ok()
                    .or_else(|| u64::try_from(v.clone()).ok().map(|u| u as i64))
            })
            .and_then(|us| u64::try_from(us).ok())
            .map(|us| us / 1000);
        out.push(PlayerInfo {
            identity: identity.unwrap_or_else(|| id.clone()),
            id,
            title: metadata_str(&meta, "xesam:title"),
            artist: metadata_first_str_in_array(&meta, "xesam:artist"),
            playing: status == "Playing",
            position_ms,
            duration_ms,
        });
    }
    out
}

/// Controla um player específico (`id` = sufixo do nome no bus).
/// `method`: "Previous" | "PlayPause" | "Next".
pub async fn control(conn: &Connection, id: &str, method: &str) -> bool {
    let name = format!("org.mpris.MediaPlayer2.{id}");
    let Ok(proxy) = Proxy::new(conn, name, PLAYER_PATH, PLAYER_IFACE).await else {
        return false;
    };
    proxy.call::<_, _, ()>(method, &()).await.is_ok()
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
    let (name, status) = best_player(conn).await?;
    let proxy = Proxy::new(conn, name.clone(), PLAYER_PATH, PLAYER_IFACE)
        .await
        .ok()?;
    let meta: HashMap<String, OwnedValue> =
        proxy.get_property("Metadata").await.unwrap_or_default();
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
pub async fn poll_and_push(
    active: ActiveConn,
    hud: Arc<Mutex<HudState>>,
    dbus: Option<Connection>,
) {
    let mut last_key: Option<(String, String, Option<String>)> = None;
    loop {
        if let Some(conn) = &dbus {
            match snapshot(conn).await {
                Some(np) => {
                    let key = (np.player.clone(), np.status.clone(), np.title.clone());
                    if last_key.as_ref() != Some(&key) {
                        last_key = Some(key);
                        let label = match (&np.title, &np.artist) {
                            (Some(t), Some(a)) => format!("{a} — {t}"),
                            (Some(t), None) => t.clone(),
                            _ => format!("{} ({})", np.player, np.status),
                        };
                        state::set_media_status(&hud, Some(label));
                        push(&active, "media.state", Some(to_body(&np))).await;
                    }
                }
                None if last_key.is_some() => {
                    last_key = None;
                    state::set_media_status(&hud, None);
                }
                None => {}
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Não roda em CI (depende de D-Bus de sessão real com um player MPRIS
    /// tocando) — `cargo test -- --ignored manual_snapshot` pra checar à
    /// mão que a seleção de player e o parsing de metadata funcionam contra
    /// o ambiente real.
    #[tokio::test]
    #[ignore]
    async fn manual_snapshot() {
        let conn = Connection::session().await.expect("D-Bus de sessão");
        let np = snapshot(&conn)
            .await
            .expect("nenhum player MPRIS respondeu");
        println!(
            "player={} status={} title={:?} artist={:?} album={:?}",
            np.player, np.status, np.title, np.artist, np.album
        );
        assert!(
            np.title.is_some(),
            "esperava título com um player real tocando/pausado"
        );
    }
}
