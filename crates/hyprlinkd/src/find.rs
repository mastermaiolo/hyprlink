//! «Onde está o telemóvel?» (PROTOCOL.md §phone): o PC pede ao telemóvel que
//! toque, vibre e/ou pisque a lanterna (`phone.find`), pode parar
//! (`phone.find_stop`) e o telemóvel conta o estado (`phone.find.state`).
//!
//! O estado vive no `HudState` (`modules.find_phone`); a bridge publica-o à
//! GUI/`hyprlinkctl` como `Event2::FindPhone`. O `phone.find` é limitado
//! (1 a cada 3 s, 10 por minuto, por ligação); o `phone.find_stop` nunca.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ciborium::Value;
use hyprlink_proto::link::{FindPhoneRequest, FindPhoneState, FindStoppedBy};

use crate::active::ActiveConn;
use crate::state::{self, HudState};

const MIN_GAP: Duration = Duration::from_secs(3);
const WINDOW: Duration = Duration::from_secs(60);
const MAX_PER_WINDOW: usize = 10;
/// Folga depois do `until_ms` antes de o daemon dar o toque por acabado
/// sozinho (o telemóvel devia ter mandado `phone.find.state` antes).
const EXPIRE_SLACK_MS: u64 = 3_000;

#[derive(Debug, Default)]
pub struct Limiter {
    sent: VecDeque<Instant>,
}

impl Limiter {
    /// `Ok` e regista o envio, ou `Err` com quanto falta esperar.
    pub fn allow(&mut self, now: Instant) -> Result<(), Duration> {
        while self
            .sent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= WINDOW)
        {
            self.sent.pop_front();
        }
        if let Some(last) = self.sent.back() {
            let since = now.duration_since(*last);
            if since < MIN_GAP {
                return Err(MIN_GAP - since);
            }
        }
        if self.sent.len() >= MAX_PER_WINDOW {
            let oldest = *self.sent.front().expect("cheio, logo não vazio");
            return Err(WINDOW - now.duration_since(oldest));
        }
        self.sent.push_back(now);
        Ok(())
    }

    /// Nova ligação: recomeça.
    pub fn reset(&mut self) {
        self.sent.clear();
    }
}

pub type SharedLimiter = Arc<Mutex<Limiter>>;

pub fn new_limiter() -> SharedLimiter {
    Arc::new(Mutex::new(Limiter::default()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindError {
    /// Nenhum modo escolhido.
    NoMode,
    /// Sem telemóvel ligado (não fica em fila).
    Offline,
    /// Limite de frequência; quanto falta.
    RateLimited(Duration),
}

fn text(k: &str) -> Value {
    Value::Text(k.into())
}

/// Corpo CBOR do `phone.find`.
pub fn body(req: &FindPhoneRequest) -> Value {
    Value::Map(vec![
        (text("sound"), Value::Bool(req.sound)),
        (text("vibrate"), Value::Bool(req.vibrate)),
        (text("torch"), Value::Bool(req.torch)),
        (text("duration_s"), Value::Integer(req.duration_s.into())),
    ])
}

/// «som, vibração, lanterna» para o Diário.
pub fn modes_label(req: &FindPhoneRequest) -> String {
    let mut v = Vec::new();
    if req.sound {
        v.push("som");
    }
    if req.vibrate {
        v.push("vibração");
    }
    if req.torch {
        v.push("lanterna");
    }
    v.join(", ")
}

/// `phone.find.state` → estado. Um `active:false` sem `stopped_by` conhecido
/// conta como `timeout` se não trouxer erro, senão `error`.
pub fn parse_state(body: &Value) -> FindPhoneState {
    let get = |k: &str| match body {
        Value::Map(m) => m
            .iter()
            .find(|(key, _)| matches!(key, Value::Text(t) if t == k))
            .map(|(_, v)| v),
        _ => None,
    };
    let active = matches!(get("active"), Some(Value::Bool(true)));
    let until_ms = match get("until_ms") {
        Some(Value::Integer(i)) => u64::try_from(*i).ok(),
        _ => None,
    };
    let error = match get("error") {
        Some(Value::Text(t)) if !t.trim().is_empty() => {
            Some(t.chars().take(200).collect::<String>())
        }
        _ => None,
    };
    if active {
        return FindPhoneState {
            active: true,
            until_ms,
            last_stopped_by: None,
            last_error: None,
        };
    }
    let by = match get("stopped_by") {
        Some(Value::Text(t)) => FindStoppedBy::parse(t),
        _ => None,
    }
    .unwrap_or(if error.is_some() {
        FindStoppedBy::Error
    } else {
        FindStoppedBy::Timeout
    });
    FindPhoneState {
        active: false,
        until_ms: None,
        last_stopped_by: Some(by),
        last_error: error,
    }
}

/// Linha do Diário quando o toque acaba.
pub fn stopped_line(s: &FindPhoneState) -> String {
    match (s.last_stopped_by, &s.last_error) {
        (Some(FindStoppedBy::User), _) => "[i] encontrado (parado no telemóvel)".into(),
        (Some(FindStoppedBy::Pc), _) => "[i] toque parado no PC".into(),
        (Some(FindStoppedBy::Error), Some(e)) => format!("[!] toque no telemóvel falhou · {e}"),
        (Some(FindStoppedBy::Error), None) => "[!] toque no telemóvel interrompido".into(),
        _ => "[i] toque no telemóvel terminou (tempo esgotado)".into(),
    }
}

pub fn get(hud: &Arc<Mutex<HudState>>) -> FindPhoneState {
    hud.lock().unwrap().modules.find_phone.clone()
}

/// Guarda o estado e publica-o (GUI/`hyprlinkctl`).
pub fn set(hud: &Arc<Mutex<HudState>>, s: FindPhoneState) {
    hud.lock().unwrap().modules.find_phone = s.clone();
    publish(s);
}

fn publish(s: FindPhoneState) {
    crate::hub::global().publish(hyprlink_proto::link::Event::More(
        hyprlink_proto::link::Event2::FindPhone(s),
    ));
}

/// Recebido `phone.find.state`: atualiza e regista no Diário.
pub fn on_state(hud: &Arc<Mutex<HudState>>, body: &Value) {
    let new = parse_state(body);
    let was_active = get(hud).active;
    if !new.active && was_active {
        state::push_log(hud, stopped_line(&new));
    }
    set(hud, new);
}

/// A ligação caiu: um toque ativo passa a `error` (o telemóvel para sozinho
/// pelo tempo).
pub fn on_disconnect(hud: &Arc<Mutex<HudState>>) {
    if get(hud).active {
        set(
            hud,
            FindPhoneState {
                active: false,
                until_ms: None,
                last_stopped_by: Some(FindStoppedBy::Error),
                // Sem texto: a GUI traduz «interrompido»; `last_error` fica
                // só para o que o telemóvel disser.
                last_error: None,
            },
        );
    }
}

/// Rede de segurança: ativo há mais de `until_ms` + folga sem notícias do
/// telemóvel → acabado por tempo. Devolve `true` se mudou.
pub fn expire(hud: &Arc<Mutex<HudState>>, now_ms: u64) -> bool {
    let cur = get(hud);
    match cur.until_ms {
        Some(until) if cur.active && now_ms > until + EXPIRE_SLACK_MS => {
            let s = FindPhoneState {
                active: false,
                until_ms: None,
                last_stopped_by: Some(FindStoppedBy::Timeout),
                last_error: None,
            };
            state::push_log(hud, stopped_line(&s));
            set(hud, s);
            true
        }
        _ => false,
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `phone.find`: valida, limita e envia ao telemóvel ligado.
pub async fn start(
    active: &ActiveConn,
    hud: &Arc<Mutex<HudState>>,
    limiter: &SharedLimiter,
    req: FindPhoneRequest,
) -> Result<FindPhoneRequest, FindError> {
    let req = req.normalized().ok_or(FindError::NoMode)?;
    if active.lock().unwrap().is_none() {
        return Err(FindError::Offline);
    }
    limiter
        .lock()
        .unwrap()
        .allow(Instant::now())
        .map_err(FindError::RateLimited)?;
    if crate::active::push(active, "phone.find", Some(body(&req)))
        .await
        .is_none()
    {
        return Err(FindError::Offline);
    }
    state::push_log(
        hud,
        format!(
            "[i] a tocar no telemóvel ({}) {} s",
            modes_label(&req),
            req.duration_s
        ),
    );
    Ok(req)
}

/// `phone.find_stop`: nunca limitado. O estado passa logo a parado no PC (o
/// `phone.find.state` do telemóvel confirma depois).
pub async fn stop(active: &ActiveConn, hud: &Arc<Mutex<HudState>>) -> Result<(), FindError> {
    if crate::active::push(active, "phone.find_stop", Some(Value::Map(vec![])))
        .await
        .is_none()
    {
        return Err(FindError::Offline);
    }
    if get(hud).active {
        let s = FindPhoneState {
            active: false,
            until_ms: None,
            last_stopped_by: Some(FindStoppedBy::Pc),
            last_error: None,
        };
        state::push_log(hud, stopped_line(&s));
        set(hud, s);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: Vec<(&str, Value)>) -> Value {
        Value::Map(pairs.into_iter().map(|(k, v)| (text(k), v)).collect())
    }

    #[test]
    fn um_a_cada_3_s() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        assert!(l.allow(t0).is_ok());
        let wait = l.allow(t0 + Duration::from_secs(1)).unwrap_err();
        assert_eq!(wait, Duration::from_secs(2));
        assert!(l.allow(t0 + Duration::from_millis(2_999)).is_err());
        assert!(l.allow(t0 + Duration::from_secs(3)).is_ok());
    }

    #[test]
    fn dez_por_minuto() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        for i in 0..10 {
            assert!(
                l.allow(t0 + Duration::from_secs(3 * i)).is_ok(),
                "envio {i}"
            );
        }
        // 11.º aos 30 s: o mais antigo (0 s) só sai da janela aos 60 s.
        let wait = l.allow(t0 + Duration::from_secs(30)).unwrap_err();
        assert_eq!(wait, Duration::from_secs(30));
        assert!(l.allow(t0 + Duration::from_secs(59)).is_err());
        assert!(l.allow(t0 + Duration::from_secs(60)).is_ok());
    }

    #[test]
    fn recusas_nao_contam() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        assert!(l.allow(t0).is_ok());
        for ms in [100, 500, 2_000] {
            assert!(l.allow(t0 + Duration::from_millis(ms)).is_err());
        }
        assert!(l.allow(t0 + Duration::from_secs(3)).is_ok());
    }

    #[test]
    fn corpo_do_pedido() {
        let b = body(&FindPhoneRequest {
            sound: true,
            vibrate: false,
            torch: true,
            duration_s: 30,
        });
        assert_eq!(
            b,
            map(vec![
                ("sound", Value::Bool(true)),
                ("vibrate", Value::Bool(false)),
                ("torch", Value::Bool(true)),
                ("duration_s", Value::Integer(30.into())),
            ])
        );
        assert_eq!(
            modes_label(&FindPhoneRequest::all(30)),
            "som, vibração, lanterna"
        );
    }

    #[test]
    fn estado_ativo_e_parado() {
        let s = parse_state(&map(vec![
            ("active", Value::Bool(true)),
            ("until_ms", Value::Integer(1_791_100_030_000u64.into())),
        ]));
        assert!(s.active);
        assert_eq!(s.until_ms, Some(1_791_100_030_000));

        let s = parse_state(&map(vec![
            ("active", Value::Bool(false)),
            ("stopped_by", text("user")),
        ]));
        assert_eq!(s.last_stopped_by, Some(FindStoppedBy::User));
        assert_eq!(stopped_line(&s), "[i] encontrado (parado no telemóvel)");

        let s = parse_state(&map(vec![
            ("active", Value::Bool(false)),
            ("stopped_by", text("error")),
            ("error", text("torch unavailable")),
        ]));
        assert_eq!(s.last_stopped_by, Some(FindStoppedBy::Error));
        assert_eq!(s.last_error.as_deref(), Some("torch unavailable"));

        // stopped_by desconhecido: sem erro → timeout; com erro → error.
        let s = parse_state(&map(vec![
            ("active", Value::Bool(false)),
            ("stopped_by", text("found")),
        ]));
        assert_eq!(s.last_stopped_by, Some(FindStoppedBy::Timeout));
        let s = parse_state(&map(vec![("error", text("x"))]));
        assert_eq!(s.last_stopped_by, Some(FindStoppedBy::Error));
    }

    fn hud() -> Arc<Mutex<HudState>> {
        HudState::new(String::new())
    }

    #[test]
    fn ligacao_perdida_com_toque_ativo_fica_em_erro() {
        let h = hud();
        set(
            &h,
            FindPhoneState {
                active: true,
                until_ms: Some(1),
                ..Default::default()
            },
        );
        on_disconnect(&h);
        let s = get(&h);
        assert!(!s.active);
        assert_eq!(s.last_stopped_by, Some(FindStoppedBy::Error));
        // Sem toque ativo, a queda não inventa um erro.
        let h = hud();
        on_disconnect(&h);
        assert_eq!(get(&h), FindPhoneState::default());
    }

    #[test]
    fn expira_sem_noticias_do_telemovel() {
        let h = hud();
        set(
            &h,
            FindPhoneState {
                active: true,
                until_ms: Some(10_000),
                ..Default::default()
            },
        );
        assert!(!expire(&h, 12_000), "ainda dentro da folga");
        assert!(get(&h).active);
        assert!(expire(&h, 13_001));
        assert_eq!(get(&h).last_stopped_by, Some(FindStoppedBy::Timeout));
    }

    #[tokio::test]
    async fn sem_telemovel_recusa_sem_fila_e_sem_gastar_o_limite() {
        let (h, active, lim) = (hud(), crate::active::new_registry(), new_limiter());
        let none = FindPhoneRequest {
            sound: false,
            vibrate: false,
            torch: false,
            duration_s: 30,
        };
        assert_eq!(start(&active, &h, &lim, none).await, Err(FindError::NoMode));
        assert_eq!(
            start(&active, &h, &lim, FindPhoneRequest::all(30)).await,
            Err(FindError::Offline)
        );
        assert!(lim.lock().unwrap().sent.is_empty(), "offline não conta");
        // O stop nunca é limitado: sem ligação só diz offline.
        assert_eq!(stop(&active, &h).await, Err(FindError::Offline));
    }
}
