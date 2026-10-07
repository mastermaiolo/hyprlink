//! Atalhos da GUI no telemóvel (PROTOCOL.md §«shortcuts»).
//!
//! - `shortcuts.list` (D→P): `{shortcuts: [{id, label}]}` — **sem o comando**.
//!   Vai ao ligar (depois do `core.hello`) e a cada alteração.
//! - `shortcut.run` (P→D, pedido/resposta): `{id}` → `shortcut.run_result
//!   {id, ok, error?}`.
//!
//! Segurança: só corre atalhos que existem na config (id desconhecido → erro,
//! nada corre); no máximo 5 execuções por segundo por ligação; o Diário leva o
//! **rótulo**, nunca o comando; o erro devolvido ao telemóvel é fixo e curto
//! (nunca o texto do Hyprland, que pode citar partes do comando). O comando
//! corre pelo caminho dos atalhos da GUI: `hypr::dispatch_checked` (modo
//! clássico/Lua detetado e prefixo `lua:`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ciborium::Value;

use crate::active::ActiveConn;
use crate::config::{self, SharedConfig};
use crate::state::{HudState, push_log};

/// Execuções por janela de um segundo, por ligação.
pub const MAX_RUNS_PER_SEC: usize = 5;

/// Limite de frequência (janela deslizante de 1 s).
#[derive(Default)]
pub struct RateLimiter {
    runs: VecDeque<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` se pode executar agora (e regista a execução).
    pub fn allow(&mut self, now: Instant) -> bool {
        while self
            .runs
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(1))
        {
            self.runs.pop_front();
        }
        if self.runs.len() >= MAX_RUNS_PER_SEC {
            return false;
        }
        self.runs.push_back(now);
        true
    }

    /// Nova ligação: recomeça.
    pub fn reset(&mut self) {
        self.runs.clear();
    }
}

pub type SharedLimiter = Arc<Mutex<RateLimiter>>;

pub fn new_limiter() -> SharedLimiter {
    Arc::new(Mutex::new(RateLimiter::new()))
}

fn kv(k: &str, v: Value) -> (Value, Value) {
    (Value::Text(k.into()), v)
}

/// Corpo do `shortcuts.list`: `{shortcuts: [{id, label}]}`. Nunca leva o
/// comando (nem `icon`: a config não tem ícones).
pub fn list_body(list: &[config::Shortcut]) -> Value {
    let items = list
        .iter()
        .map(|s| {
            Value::Map(vec![
                kv("id", Value::Text(s.id.clone())),
                kv("label", Value::Text(s.name.clone())),
            ])
        })
        .collect();
    Value::Map(vec![kv("shortcuts", Value::Array(items))])
}

/// Corpo do `shortcut.run_result`: `{id, ok, error?}`.
pub fn result_body(id: &str, result: Result<(), &str>) -> Value {
    let mut m = vec![
        kv("id", Value::Text(id.to_string())),
        kv("ok", Value::Bool(result.is_ok())),
    ];
    if let Err(e) = result {
        m.push(kv("error", Value::Text(e.to_string())));
    }
    Value::Map(m)
}

/// Corpos de `shortcuts.list` enviados (só nos testes).
#[cfg(test)]
pub static SENT: Mutex<Vec<Value>> = Mutex::new(Vec::new());

/// Envia a lista atual ao telemóvel (se houver um ligado).
pub async fn push_list(active: &ActiveConn, config: &SharedConfig) {
    let list = config::shortcuts(config);
    let body = list_body(&list);
    #[cfg(test)]
    SENT.lock().unwrap().push(body.clone());
    crate::active::push(active, "shortcuts.list", Some(body)).await;
}

/// Edição vinda da GUI: valida (ids novos para os atalhos novos), grava e
/// envia a lista nova ao telemóvel. Lista recusada: nada muda nem se envia.
pub async fn apply_edit(
    active: &ActiveConn,
    config: &SharedConfig,
    hud: &Arc<Mutex<HudState>>,
    list: Vec<config::Shortcut>,
) -> Result<(), &'static str> {
    match config::validate_shortcuts(list) {
        Ok(list) => {
            config::set_shortcuts(config, list);
            push_list(active, config).await;
            Ok(())
        }
        Err(why) => {
            push_log(hud, format!("[!] atalhos recusados · {why}"));
            Err(why)
        }
    }
}

/// Ao ligar: a lista logo a seguir ao `core.hello` e outra vez 2 s depois
/// (a app pode ainda não estar a aceitar streams do PC no primeiro instante;
/// a lista é idempotente).
pub fn spawn_push_on_connect(active: ActiveConn, config: SharedConfig) {
    tokio::spawn(async move {
        push_list(&active, &config).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        push_list(&active, &config).await;
    });
}

/// Erros fixos devolvidos ao telemóvel (em inglês, curtos).
pub const ERR_UNKNOWN: &str = "unknown shortcut";
pub const ERR_RATE: &str = "rate limited";
pub const ERR_FAILED: &str = "failed";

/// Executa o atalho `id`. `runner` corre o comando (em produção,
/// `hypr::dispatch_checked`); o seu erro **não** sai daqui.
pub fn run(
    config: &SharedConfig,
    limiter: &SharedLimiter,
    hud: &Arc<Mutex<HudState>>,
    id: &str,
    now: Instant,
    runner: &dyn Fn(&str) -> Result<String, String>,
) -> Result<(), &'static str> {
    if !limiter.lock().unwrap().allow(now) {
        push_log(hud, "[!] atalho do telemóvel recusado · demasiado depressa");
        return Err(ERR_RATE);
    }
    let Some(sc) = config::shortcut_by_id(config, id) else {
        push_log(
            hud,
            "[!] atalho do telemóvel recusado · identificação desconhecida",
        );
        return Err(ERR_UNKNOWN);
    };
    push_log(hud, format!("[i] atalho do telemóvel · {}", sc.name));
    match runner(&sc.command) {
        Ok(_) => Ok(()),
        Err(_) => {
            push_log(hud, format!("[!] atalho «{}» falhou", sc.name));
            Err(ERR_FAILED)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{body_get, body_get_bool, body_get_str};
    use std::cell::RefCell;

    fn sc(id: &str, name: &str, command: &str) -> config::Shortcut {
        config::Shortcut {
            id: id.into(),
            name: name.into(),
            command: command.into(),
        }
    }

    fn cfg(list: Vec<config::Shortcut>) -> SharedConfig {
        let c = config::test_config();
        c.lock().unwrap().shortcuts = list;
        c
    }

    fn hud() -> Arc<Mutex<HudState>> {
        HudState::new(String::new())
    }

    fn log_text(h: &Arc<Mutex<HudState>>) -> String {
        format!("{:?}", h.lock().unwrap().logs)
    }

    #[test]
    fn a_lista_nunca_leva_o_comando() {
        let list = vec![
            sc("a1", "Terminal", "exec kitty --super-secreto"),
            sc("b2", "Navegador", "exec firefox"),
        ];
        let body = list_body(&list);
        let bytes = format!("{body:?}");
        assert!(
            !bytes.contains("super-secreto")
                && !bytes.contains("kitty")
                && !bytes.contains("firefox"),
            "{bytes}"
        );
        let items = body_get(&body, "shortcuts").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(body_get_str(&items[0], "id"), Some("a1"));
        assert_eq!(body_get_str(&items[0], "label"), Some("Terminal"));
        for it in items {
            let Value::Map(m) = it else { panic!() };
            assert_eq!(m.len(), 2, "só id e label (sem comando nem icon)");
        }
        // CBOR real: nem os bytes codificados levam o comando.
        let mut buf = Vec::new();
        ciborium::into_writer(&body, &mut buf).unwrap();
        assert!(!String::from_utf8_lossy(&buf).contains("kitty"));
    }

    #[test]
    fn id_desconhecido_nao_corre_nada_e_nao_cita_nada() {
        let c = cfg(vec![sc("a1", "Terminal", "exec kitty")]);
        let (l, h) = (new_limiter(), hud());
        let ran = RefCell::new(0);
        let r = run(&c, &l, &h, "nao-existe", Instant::now(), &|_| {
            *ran.borrow_mut() += 1;
            Ok(String::new())
        });
        assert_eq!(r, Err(ERR_UNKNOWN));
        assert_eq!(*ran.borrow(), 0, "nada correu");
        // Um «id» que é o comando também não corre (só se procura por id).
        assert_eq!(
            run(&c, &l, &h, "exec kitty", Instant::now(), &|_| Ok(
                String::new()
            )),
            Err(ERR_UNKNOWN)
        );
    }

    #[test]
    fn corre_o_comando_gravado_e_regista_so_o_rotulo() {
        let c = cfg(vec![sc(
            "a1",
            "Navegador",
            "exec firefox --comando-longo-123",
        )]);
        let (l, h) = (new_limiter(), hud());
        let seen = RefCell::new(String::new());
        let r = run(&c, &l, &h, "a1", Instant::now(), &|cmd| {
            *seen.borrow_mut() = cmd.to_string();
            Ok("ok".into())
        });
        assert_eq!(r, Ok(()));
        assert_eq!(*seen.borrow(), "exec firefox --comando-longo-123");
        let log = log_text(&h);
        assert!(log.contains("Navegador"), "{log}");
        assert!(
            !log.contains("firefox") && !log.contains("comando-longo"),
            "o comando não vai para o Diário: {log}"
        );
    }

    #[test]
    fn falha_do_comando_nao_vaza_o_texto_do_erro() {
        let c = cfg(vec![sc("a1", "Música", "exec spotify")]);
        let (l, h) = (new_limiter(), hud());
        let r = run(&c, &l, &h, "a1", Instant::now(), &|_| {
            Err("Hyprland refused `exec` (Lua syntax) spotify".into())
        });
        assert_eq!(r, Err(ERR_FAILED));
        let log = log_text(&h);
        assert!(
            log.contains("Música") && !log.contains("spotify") && !log.contains("Hyprland refused"),
            "{log}"
        );
        let b = result_body("a1", r);
        assert_eq!(body_get_bool(&b, "ok"), Some(false));
        assert_eq!(body_get_str(&b, "error"), Some("failed"));
    }

    #[test]
    fn limite_de_frequencia_5_por_segundo_por_ligacao() {
        let c = cfg(vec![sc("a1", "T", "exec x")]);
        let (l, h) = (new_limiter(), hud());
        let t0 = Instant::now();
        let ran = RefCell::new(0);
        let go = |t: Instant| {
            run(&c, &l, &h, "a1", t, &|_| {
                *ran.borrow_mut() += 1;
                Ok(String::new())
            })
        };
        for i in 0..5 {
            assert_eq!(
                go(t0 + Duration::from_millis(i * 10)),
                Ok(()),
                "a {}ª passa",
                i + 1
            );
        }
        assert_eq!(go(t0 + Duration::from_millis(60)), Err(ERR_RATE));
        assert_eq!(*ran.borrow(), 5, "a 6ª não correu");
        // Passado 1 s, volta a poder.
        assert_eq!(go(t0 + Duration::from_millis(1100)), Ok(()));
        // Nova ligação recomeça.
        l.lock().unwrap().reset();
        assert_eq!(go(t0 + Duration::from_millis(1110)), Ok(()));
    }

    #[test]
    fn resultado_ok_nao_leva_erro() {
        let b = result_body("a1", Ok(()));
        assert_eq!(body_get_bool(&b, "ok"), Some(true));
        assert!(body_get(&b, "error").is_none());
        assert_eq!(body_get_str(&b, "id"), Some("a1"));
    }

    fn labels_of(body: &Value) -> Vec<String> {
        body_get(body, "shortcuts")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| body_get_str(x, "label").map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Os envios deste teste (a lista global é partilhada entre testes):
    /// só os que têm o rótulo marcado.
    fn sent_with(marca: &str) -> Vec<Value> {
        SENT.lock()
            .unwrap()
            .iter()
            .filter(|b| labels_of(b).iter().any(|l| l.contains(marca)))
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn a_lista_vai_ao_ligar_e_outra_vez_dois_segundos_depois() {
        let c = cfg(vec![sc("a1", "Ligar-T1", "exec kitty")]);
        let active = crate::active::new_registry();
        spawn_push_on_connect(active, c);
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(sent_with("Ligar-T1").len(), 1, "logo a seguir ao handshake");
        tokio::time::sleep(Duration::from_millis(2300)).await;
        let all = sent_with("Ligar-T1");
        assert_eq!(all.len(), 2, "e de novo passados 2 s");
        assert_eq!(labels_of(&all[0]), vec!["Ligar-T1"]);
    }

    #[tokio::test]
    async fn a_lista_vai_depois_de_cada_edicao_com_ids_estaveis() {
        let c = cfg(vec![sc("keep0001", "Edicao-A", "exec a")]);
        let (active, h) = (crate::active::new_registry(), hud());
        // A GUI acrescenta um atalho novo (sem id) mantendo o que existe.
        let r = apply_edit(
            &active,
            &c,
            &h,
            vec![
                sc("keep0001", "Edicao-A", "exec a"),
                sc("", "Edicao-B", "exec b"),
            ],
        )
        .await;
        assert_eq!(r, Ok(()));
        let sent = sent_with("Edicao-B");
        assert_eq!(sent.len(), 1, "enviada após a edição");
        assert_eq!(labels_of(&sent[0]), vec!["Edicao-A", "Edicao-B"]);
        let list = config::shortcuts(&c);
        assert_eq!(list[0].id, "keep0001", "o id existente não muda");
        assert_eq!(list[1].id.len(), 8, "o novo ganhou um id");
        assert_ne!(list[0].id, list[1].id);
        // O id novo vai no payload, o comando não.
        let txt = format!("{:?}", sent[0]);
        assert!(
            txt.contains(&list[1].id) && !txt.contains("exec b"),
            "{txt}"
        );
        // Edição recusada: nada muda e nada se envia.
        let r = apply_edit(&active, &c, &h, vec![sc("", "Edicao-C", "  ")]).await;
        assert!(r.is_err());
        assert!(sent_with("Edicao-C").is_empty());
        assert_eq!(config::shortcuts(&c).len(), 2);
    }

    #[test]
    fn migracao_de_ids_nao_perde_nenhum_atalho() {
        // Config antiga, sem `id`.
        let antigo = r#"{"download_dir":"/x","shortcuts":[
            {"name":"Terminal","command":"exec kitty"},
            {"name":"Bloquear","command":"exec hyprlock"},
            {"id":"fixo1234","name":"Já tinha","command":"exec x"}]}"#;
        let mut cfg: config::AppConfig = serde_json::from_str(antigo).unwrap();
        assert!(cfg.shortcuts.iter().take(2).all(|s| s.id.is_empty()));
        assert!(config::ensure_shortcut_ids(&mut cfg.shortcuts), "migrou");
        let l = &cfg.shortcuts;
        assert_eq!(l.len(), 3, "nenhum perdido");
        assert_eq!(
            l.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["Terminal", "Bloquear", "Já tinha"]
        );
        assert_eq!(l[2].id, "fixo1234", "o id que já existia não muda");
        let ids: Vec<&String> = l.iter().map(|s| &s.id).collect();
        assert!(ids.iter().all(|i| i.len() == 8));
        assert!(
            ids[0] != ids[1] && ids[0] != ids[2] && ids[1] != ids[2],
            "únicos"
        );
        // Segunda passagem: nada a fazer, ids iguais.
        let before = cfg.shortcuts.clone();
        assert!(!config::ensure_shortcut_ids(&mut cfg.shortcuts));
        assert_eq!(
            before.iter().map(|s| &s.id).collect::<Vec<_>>(),
            cfg.shortcuts.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
        // Ids repetidos: o segundo troca.
        let mut dup = vec![sc("x", "a", "c"), sc("x", "b", "c")];
        assert!(config::ensure_shortcut_ids(&mut dup));
        assert_ne!(dup[0].id, dup[1].id);
        assert_eq!(dup[0].id, "x");
    }
}
