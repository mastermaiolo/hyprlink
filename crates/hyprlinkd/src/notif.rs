//! Notificações — cliente normal do servidor `org.freedesktop.Notifications`
//! que já existe no sistema (aqui, o Quickshell do ryoku-shell) — não tenta
//! substituí-lo. `notification.post` chama `Notify()` como qualquer app
//! faria; ações/dispensar vêm dos sinais padrão `ActionInvoked`/
//! `NotificationClosed`, que qualquer cliente pode escutar.
//!
//! Resposta por texto: o balão do PC não a recolhe — este servidor só anuncia
//! `["body","actions","icon-static"]` em `GetCapabilities`, sem `inline-reply`
//! — e um botão de resposta vira clique comum (`notification.action`). A
//! resposta faz-se na página Notificações da GUI (`Command2::ReplyNotification`),
//! que o daemon valida com `reply_body` e envia como `notification.reply`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ciborium::Value;
use futures_util::StreamExt;
use zbus::zvariant::Value as ZValue;
use zbus::{Connection, Proxy};

use crate::active::{ActiveConn, push};
use crate::state::{HudState, NotifEntry, push_log};

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
pub async fn post(
    app: &str,
    title: &str,
    text: &str,
    key: &str,
    actions: &[(i64, String)],
    registry: &Registry,
    dbus: &Option<Connection>,
) {
    let Some(conn) = dbus else { return };
    let Ok(proxy) = Proxy::new(conn, DEST, PATH, IFACE).await else {
        return;
    };

    let action_pairs: Vec<String> = actions
        .iter()
        .flat_map(|(idx, label)| vec![idx.to_string(), label.clone()])
        .collect();
    let hints: HashMap<&str, ZValue> = HashMap::new();

    let result: zbus::Result<u32> = proxy
        .call(
            "Notify",
            &(app, 0u32, "", title, text, action_pairs, hints, -1i32),
        )
        .await;
    if let Ok(id) = result {
        registry.lock().unwrap().insert(id, key.to_string());
    }
}

/// `notification.dismissed` (telemóvel → PC): o utilizador fechou no
/// telemóvel — fecha a notificação espelhada aqui também, se existir.
pub async fn dismiss_local(key: &str, registry: &Registry, dbus: &Option<Connection>) {
    let id = {
        let mut map = registry.lock().unwrap();
        let found = map.iter().find(|(_, v)| v.as_str() == key).map(|(k, _)| *k);
        if let Some(id) = found {
            map.remove(&id);
        }
        found
    };
    let Some(id) = id else { return };
    let Some(conn) = dbus else { return };
    let Ok(proxy) = Proxy::new(conn, DEST, PATH, IFACE).await else {
        return;
    };
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

/// Limite do texto de uma resposta (o mesmo de `notification.post.text`).
pub const REPLY_MAX_CHARS: usize = 2000;

/// Valida uma resposta contra as notificações ativas e monta o corpo do
/// `notification.reply` (`{key, idx, text}`). O erro é a frase para o Diário.
pub fn reply_body(
    active: &[NotifEntry],
    key: &str,
    idx: u32,
    text: &str,
) -> Result<Value, &'static str> {
    let entry = active
        .iter()
        .find(|n| n.key == key)
        .ok_or("a notificação já não está ativa")?;
    if !entry.actions.iter().any(|a| a.idx == idx && a.is_reply) {
        return Err("a ação não é de resposta");
    }
    if text.trim().is_empty() {
        return Err("texto vazio");
    }
    if text.chars().count() > REPLY_MAX_CHARS {
        return Err("texto com mais de 2000 caracteres");
    }
    Ok(Value::Map(vec![
        (Value::Text("key".into()), Value::Text(key.to_string())),
        (Value::Text("idx".into()), Value::Integer(idx.into())),
        (Value::Text("text".into()), Value::Text(text.to_string())),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{body_get, body_get_i64, body_get_str};
    use hyprlink_proto::link::NotifAction;

    fn entry() -> NotifEntry {
        NotifEntry {
            key: "k1".into(),
            at_unix: 0,
            app: "Signal".into(),
            title: "Rita".into(),
            text: "Olá".into(),
            actions: vec![
                NotifAction {
                    idx: 0,
                    label: "Marcar como lida".into(),
                    is_reply: false,
                },
                NotifAction {
                    idx: 1,
                    label: "Responder".into(),
                    is_reply: true,
                },
            ],
        }
    }

    #[test]
    fn reply_builds_packet_with_key_idx_text() {
        let body = reply_body(&[entry()], "k1", 1, "Já vou").unwrap();
        assert_eq!(body_get_str(&body, "key"), Some("k1"));
        assert_eq!(body_get_i64(&body, "idx"), Some(1));
        assert_eq!(body_get_str(&body, "text"), Some("Já vou"));
        assert!(body_get(&body, "extra").is_none());
    }

    #[test]
    fn reply_refuses_non_reply_action_unknown_key_and_bad_text() {
        let list = [entry()];
        assert!(reply_body(&list, "k1", 0, "x").is_err(), "idx sem is_reply");
        assert!(reply_body(&list, "k1", 7, "x").is_err(), "idx inexistente");
        assert!(reply_body(&list, "outra", 1, "x").is_err(), "chave inativa");
        assert!(reply_body(&list, "k1", 1, "").is_err());
        assert!(reply_body(&list, "k1", 1, "  \n").is_err());
        let longo = "a".repeat(REPLY_MAX_CHARS + 1);
        assert!(reply_body(&list, "k1", 1, &longo).is_err());
        let limite = "ã".repeat(REPLY_MAX_CHARS);
        assert!(
            reply_body(&list, "k1", 1, &limite).is_ok(),
            "conta caracteres, não bytes"
        );
    }

    /// Não roda em CI — dispara uma notificação de verdade no servidor da
    /// sessão. `cargo test -- --ignored manual_post`.
    #[tokio::test]
    #[ignore]
    async fn manual_post() {
        let registry = new_registry();
        let dbus = Some(Connection::session().await.expect("D-Bus de sessão"));
        post(
            "HyprLink (teste)",
            "Funciona!",
            "Se isso apareceu na tela, o Notify() está ok.",
            "test-key-123",
            &[(0, "Marcar como lida".into()), (1, "Responder".into())],
            &registry,
            &dbus,
        )
        .await;
        assert_eq!(
            registry.lock().unwrap().len(),
            1,
            "esperava um id registado após o Notify()"
        );
    }
}
