//! `gesture {name}` (P→D, one-way): o telemóvel diz que fez um gesto; o PC faz
//! a ação que o utilizador ligou para ele. O contrato está em PROTOCOL.md.
//!
//! O daemon decide o que correr (a regra é do utilizador, guardada em
//! `config.json`): ou um `hyprctl dispatch` ("workspace e-1") ou, com o
//! prefixo `pc:`, uma ação de `action.rs` ("pc:volume_up").

use std::sync::Mutex;

use hyprlink_proto::link::{GestureLast, GestureRule};

/// O último gesto recebido nesta sessão do daemon.
static LAST: Mutex<Option<GestureLast>> = Mutex::new(None);

pub fn last() -> Option<GestureLast> {
    LAST.lock().unwrap().clone()
}

fn record(name: &str, at_unix: u64) {
    *LAST.lock().unwrap() = Some(GestureLast {
        name: name.to_string(),
        at_unix,
    });
}

/// O que fazer para um gesto.
#[derive(Debug, PartialEq)]
pub enum Act {
    /// `hyprctl dispatch <cmd>`.
    Dispatch(String),
    /// Uma ação de `action.rs`.
    Pc(String),
}

/// Porque é que um gesto não faz nada.
#[derive(Debug, PartialEq)]
pub enum Skip {
    /// Nome que não conhecemos.
    Unknown,
    /// Gesto desligado pelo utilizador.
    Off,
    /// `volume` sem `dir` válido (`up`/`down`).
    BadDir,
    /// Regra sem ação.
    Empty,
}

/// Mapeia o gesto recebido para uma ação, pelas regras guardadas. Puro:
/// não executa nada. `dir` só conta no `volume`.
pub fn resolve(rules: &[GestureRule], name: &str, dir: Option<&str>) -> Result<Act, Skip> {
    let rule = rules.iter().find(|r| r.name == name).ok_or(Skip::Unknown)?;
    if !rule.on {
        return Err(Skip::Off);
    }
    let action = rule.action.trim();
    if action.is_empty() {
        return Err(Skip::Empty);
    }
    let action = if action.contains("{dir}") {
        match dir {
            Some(d @ ("up" | "down")) => action.replace("{dir}", d),
            _ => return Err(Skip::BadDir),
        }
    } else {
        action.to_string()
    };
    Ok(match action.strip_prefix("pc:") {
        Some(a) => Act::Pc(a.to_string()),
        None => Act::Dispatch(action),
    })
}

/// Trata um `gesture` recebido: regista-o (aparece na GUI mesmo desligado) e,
/// se estiver ligado, executa a ação. Devolve o texto a pôr no Diário.
pub async fn handle(config: &crate::config::SharedConfig, name: &str, dir: Option<&str>) -> String {
    record(name, crate::state::now_unix());
    let rules = crate::config::gesture_rules(config);
    match resolve(&rules, name, dir) {
        Err(Skip::Off) => format!("[i] gesture {name} (desligado)"),
        Err(Skip::Unknown) => format!("[!] gesture {name}: gesto desconhecido"),
        Err(Skip::BadDir) => format!("[!] gesture {name}: falta dir=up|down"),
        Err(Skip::Empty) => format!("[!] gesture {name}: regra sem ação"),
        Ok(Act::Pc(a)) => match crate::action::run(&a).await {
            Ok(()) => format!("[i] gesture {name} → pc:{a}"),
            Err(e) => format!("[!] gesture {name} → pc:{a}: {e}"),
        },
        Ok(Act::Dispatch(cmd)) => {
            let c = cmd.clone();
            match tokio::task::spawn_blocking(move || crate::hypr::dispatch_checked(&c)).await {
                Ok(Ok(_)) => format!("[i] gesture {name} → {cmd}"),
                Ok(Err(e)) => format!("[!] gesture {name} → {cmd}: {e}"),
                Err(_) => format!("[!] gesture {name} → {cmd}: tarefa falhou"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprlink_proto::link::default_gesture_rules;

    fn rules() -> Vec<GestureRule> {
        let mut r = default_gesture_rules();
        // Liga todos para testar o mapeamento (`rotate_landscape` vem desligado).
        r.iter_mut().for_each(|g| g.on = true);
        r
    }

    #[test]
    fn cada_gesto_mapeia_para_a_sua_acao() {
        let r = rules();
        let d = |c: &str| Ok(Act::Dispatch(c.to_string()));
        assert_eq!(resolve(&r, "swipe_left_3", None), d("workspace e-1"));
        assert_eq!(resolve(&r, "swipe_right_3", None), d("workspace e+1"));
        assert_eq!(
            resolve(&r, "double_tap_back", None),
            d("togglespecialworkspace")
        );
        assert_eq!(resolve(&r, "rotate_landscape", None), d("fullscreen 1"));
        assert_eq!(
            resolve(&r, "volume", Some("up")),
            Ok(Act::Pc("volume_up".into()))
        );
        assert_eq!(
            resolve(&r, "volume", Some("down")),
            Ok(Act::Pc("volume_down".into()))
        );
    }

    #[test]
    fn desligado_desconhecido_e_dir_invalida_nao_fazem_nada() {
        let mut r = rules();
        assert_eq!(resolve(&r, "pinch", None), Err(Skip::Unknown));
        assert_eq!(resolve(&r, "volume", None), Err(Skip::BadDir));
        assert_eq!(resolve(&r, "volume", Some("sideways")), Err(Skip::BadDir));
        // `dir` não conta fora do `volume`.
        assert!(resolve(&r, "swipe_left_3", Some("up")).is_ok());
        r.iter_mut().find(|g| g.name == "swipe_left_3").unwrap().on = false;
        assert_eq!(resolve(&r, "swipe_left_3", None), Err(Skip::Off));
        r.iter_mut()
            .find(|g| g.name == "swipe_right_3")
            .unwrap()
            .action = "  ".into();
        assert_eq!(resolve(&r, "swipe_right_3", None), Err(Skip::Empty));
    }

    #[test]
    fn por_omissao_so_o_rodar_vem_desligado() {
        let off: Vec<_> = default_gesture_rules()
            .into_iter()
            .filter(|g| !g.on)
            .map(|g| g.name)
            .collect();
        assert_eq!(off, ["rotate_landscape"]);
    }

    #[test]
    fn ultimo_gesto_fica_registado() {
        record("swipe_left_3", 1_000);
        assert_eq!(
            last(),
            Some(GestureLast {
                name: "swipe_left_3".into(),
                at_unix: 1_000
            })
        );
    }
}
