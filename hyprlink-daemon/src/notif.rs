//! Notificações — cliente normal do servidor `org.freedesktop.Notifications`
//! que já existe no sistema (aqui, o Quickshell do ryoku-shell) — não tenta
//! substituí-lo. `notification.post` chama `Notify()` como qualquer app
//! faria; ações/dispensar vêm dos sinais padrão `ActionInvoked`/
//! `NotificationClosed`, que qualquer cliente pode escutar.
//!
//! ponytail: resposta por texto (`notification.reply`) não é suportada —
//! confirmado ao vivo que este servidor só anuncia `["body","actions",
//! "icon-static"]` em `GetCapabilities`, sem `inline-reply`. Um botão de
//! ação marcado como resposta ainda aparece (vira clique comum via
//! `notification.action`), só não coleta texto nenhum.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ciborium::Value;
use futures_util::StreamExt;
use zbus::zvariant::Value as ZValue;
use zbus::{Connection, Proxy};

use crate::active::{push, ActiveConn};
use crate::state::{push_log, HudState};

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const IFACE: &str = "org.freedesktop.Notifications";

/// id do D-Bus (o que o servidor de notificações retorna) -> `key` original
/// da notificação do Android, pra correlacionar os sinais de volta.
pub type Registry = Arc<Mutex<HashMap<u32, String>>>;

pub fn new_registry() -> Registry {
    Arc::new(Mutex::new(HashMap::new()))
}

/// `notification.post` (telemóvel → PC): espelha no servidor de
/// notificações existente. `actions` é `(idx, label)` — o `idx` vira a
/// action key crua, sem tradução, pra bater de volta em `ActionInvoked`.
pub async fn post(app: &str, title: &str, text: &str, key: &str, actions: &[(i64, String)], registry: &Registry) {
    let Ok(conn) = Connection::session().await else { return };
    let Ok(proxy) = Proxy::new(&conn, DEST, PATH, IFACE).await else { return };

    let action_pairs: Vec<String> =
        actions.iter().flat_map(|(idx, label)| vec![idx.to_string(), label.clone()]).collect();
    let hints: HashMap<&str, ZValue> = HashMap::new();

    let result: zbus::Result<u32> =
        proxy.call("Notify", &(app, 0u32, "", title, text, action_pairs, hints, -1i32)).await;
    if let Ok(id) = result {
        registry.lock().unwrap().insert(id, key.to_string());
    }
}

/// `notification.dismissed` (telemóvel → PC): o utilizador fechou no
/// telemóvel — fecha a notificação espelhada aqui também, se existir.
pub async fn dismiss_local(key: &str, registry: &Registry) {
    let id = {
        let mut map = registry.lock().unwrap();
        let found = map.iter().find(|(_, v)| v.as_str() == key).map(|(k, _)| *k);
        if let Some(id) = found {
            map.remove(&id);
        }
        found
    };
    let Some(id) = id else { return };
    let Ok(conn) = Connection::session().await else { return };
    let Ok(proxy) = Proxy::new(&conn, DEST, PATH, IFACE).await else { return };
    let _: zbus::Result<()> = proxy.call("CloseNotification", &(id,)).await;
}

/// Escuta `ActionInvoked`/`NotificationClosed` do servidor existente e
/// repassa pro telemóvel — reconecta sozinho se a sessão D-Bus cair.
pub async fn watch(active: ActiveConn, registry: Registry, hud: Arc<Mutex<HudState>>) {
    loop {
        if let Err(e) = watch_once(&active, &registry).await {
            push_log(&hud, format!("[!] notificações: {e}"));
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

async fn watch_once(active: &ActiveConn, registry: &Registry) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = Proxy::new(&conn, DEST, PATH, IFACE).await?;
    let mut action_invoked = proxy.receive_signal("ActionInvoked").await?;
    let mut closed = proxy.receive_signal("NotificationClosed").await?;

    loop {
        tokio::select! {
            msg = action_invoked.next() => {
                let Some(msg) = msg else { break };
                if let Ok((id, action_key)) = msg.body().deserialize::<(u32, String)>() {
                    let key = registry.lock().unwrap().get(&id).cloned();
                    if let (Some(key), Ok(idx)) = (key, action_key.parse::<i64>()) {
                        let body = Value::Map(vec![
                            (Value::Text("key".into()), Value::Text(key)),
                            (Value::Text("idx".into()), Value::Integer(idx.into())),
                        ]);
                        push(active, "notification.action", Some(body)).await;
                    }
                }
            }
            msg = closed.next() => {
                let Some(msg) = msg else { break };
                if let Ok((id, _reason)) = msg.body().deserialize::<(u32, u32)>() {
                    let key = registry.lock().unwrap().remove(&id);
                    if let Some(key) = key {
                        let body = Value::Map(vec![(Value::Text("key".into()), Value::Text(key))]);
                        push(active, "notification.dismiss", Some(body)).await;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Não roda em CI — dispara uma notificação de verdade no servidor da
    /// sessão. `cargo test -- --ignored manual_post`.
    #[tokio::test]
    #[ignore]
    async fn manual_post() {
        let registry = new_registry();
        post(
            "HyprLink (teste)",
            "Funciona!",
            "Se isso apareceu na tela, o Notify() está ok.",
            "test-key-123",
            &[(0, "Marcar como lida".into()), (1, "Responder".into())],
            &registry,
        )
        .await;
        assert_eq!(registry.lock().unwrap().len(), 1, "esperava um id registado após o Notify()");
    }
}
