//! Gate antes da autenticação: o que um peer que ainda não provou ser um
//! telemóvel emparelhado pode fazer ao daemon.
//!
//! O TLS aceita qualquer certificado de cliente (`tls_verifier.rs`): quem
//! autentica é a fingerprint fixada, verificada só depois de ler o
//! `core.hello`. Até lá, o peer é anónimo, e este módulo limita o que ele
//! custa ao daemon:
//!
//! - quantas ligações por autenticar existem ao mesmo tempo (no total e por
//!   IP) — acima disso o `Incoming` é recusado logo à entrada;
//! - quantos tokens errados um IP pode tentar (bloqueio com recuo) e quantos
//!   no total até o token rodar (o QR da GUI passa a ter o novo).
//!
//! O tamanho e o tempo-limite do hello estão em [`Limits`], aplicados em
//! `server.rs`. Tudo aqui é síncrono e recebe o `now`, para os testes.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Limites do gate. [`Limits::DEFAULT`] é o que o daemon usa; os testes
/// encurtam os tempos.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Handshake QUIC/TLS completo.
    pub handshake_timeout: Duration,
    /// `accept_bi` + leitura do `core.hello`, no total.
    pub hello_timeout: Duration,
    /// Tamanho máximo do frame do `core.hello` (o da app tem ~350 bytes).
    pub hello_max: u32,
    /// Ligações por autenticar ao mesmo tempo, no total.
    pub max_pending: usize,
    /// Ligações por autenticar ao mesmo tempo, por IP (ou prefixo /64).
    pub max_pending_per_ip: usize,
    /// Janela em que se contam os tokens errados.
    pub fail_window: Duration,
    /// Tokens errados de um IP, dentro da janela, até o bloquear.
    pub fails_per_ip: usize,
    /// Tokens errados no total, dentro da janela, até rodar o token.
    pub fails_global: usize,
    /// Primeiro bloqueio; cada bloqueio seguinte do mesmo IP dobra.
    pub block_base: Duration,
    /// Teto do bloqueio.
    pub block_max: Duration,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        handshake_timeout: Duration::from_secs(10),
        hello_timeout: Duration::from_secs(10),
        hello_max: 64 * 1024,
        max_pending: 8,
        max_pending_per_ip: 2,
        fail_window: Duration::from_secs(10 * 60),
        fails_per_ip: 5,
        fails_global: 20,
        block_base: Duration::from_secs(60),
        block_max: Duration::from_secs(60 * 60),
    };
}

/// Entradas guardadas por IP, no máximo. Acima disto saem as mais antigas
/// que não estejam bloqueadas.
const MAX_TRACKED_IPS: usize = 4096;

/// Intervalo mínimo entre duas linhas agregadas de «ligações recusadas».
const REFUSED_LOG_EVERY: Duration = Duration::from_secs(60);

/// IPv4 tal e qual; IPv6 pelo prefixo /64 (quem tem um /64 escolhe à
/// vontade os 64 bits de baixo, contá-los um a um não limitava nada).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IpKey(IpAddr);

impl IpKey {
    pub fn of(ip: IpAddr) -> Self {
        match ip.to_canonical() {
            IpAddr::V4(v4) => IpKey(IpAddr::V4(v4)),
            IpAddr::V6(v6) => {
                let bits = u128::from(v6) & !((1u128 << 64) - 1);
                IpKey(IpAddr::V6(bits.into()))
            }
        }
    }
}

impl std::fmt::Display for IpKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            IpAddr::V4(v4) => write!(f, "{v4}"),
            IpAddr::V6(v6) => write!(f, "{v6}/64"),
        }
    }
}

/// Porque é que um `Incoming` foi recusado à entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    TooManyPending,
    TooManyFromIp,
}

impl Refusal {
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::TooManyPending => "demasiadas ligações por autenticar",
            Refusal::TooManyFromIp => "demasiadas ligações por autenticar deste IP",
        }
    }
}

/// O que um token errado provocou.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FailOutcome {
    /// O IP ficou bloqueado agora, por este tempo.
    pub blocked: Option<Duration>,
    /// Chegou-se ao limite global: rodar o token.
    pub rotate: bool,
}

#[derive(Debug)]
struct IpFails {
    recent: VecDeque<Instant>,
    blocked_until: Option<Instant>,
    /// Quantas vezes já foi bloqueado (o recuo dobra a cada uma).
    strikes: u32,
    last: Instant,
}

#[derive(Debug)]
pub struct GateState {
    pub limits: Limits,
    pending_total: usize,
    pending: HashMap<IpKey, usize>,
    fails: HashMap<IpKey, IpFails>,
    global: VecDeque<Instant>,
    refused_unlogged: u64,
    last_refused_log: Option<Instant>,
}

pub type Gate = Arc<Mutex<GateState>>;

pub fn new(limits: Limits) -> Gate {
    Arc::new(Mutex::new(GateState {
        limits,
        pending_total: 0,
        pending: HashMap::new(),
        fails: HashMap::new(),
        global: VecDeque::new(),
        refused_unlogged: 0,
        last_refused_log: None,
    }))
}

/// Lugar de uma ligação por autenticar. Liberta-se ao sair do âmbito
/// (autenticada, recusada, expirada ou com erro).
#[derive(Debug)]
pub struct PendingSlot {
    gate: Gate,
    key: IpKey,
}

impl Drop for PendingSlot {
    fn drop(&mut self) {
        let mut g = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        g.pending_total = g.pending_total.saturating_sub(1);
        if let Some(n) = g.pending.get_mut(&self.key) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                g.pending.remove(&self.key);
            }
        }
    }
}

/// Reserva um lugar de ligação por autenticar para `ip`, ou diz porque não.
pub fn admit(gate: &Gate, ip: IpAddr) -> Result<PendingSlot, Refusal> {
    let key = IpKey::of(ip);
    let mut g = gate.lock().unwrap();
    if g.pending_total >= g.limits.max_pending {
        return Err(Refusal::TooManyPending);
    }
    let per_ip = g.limits.max_pending_per_ip;
    let n = g.pending.entry(key).or_insert(0);
    if *n >= per_ip {
        return Err(Refusal::TooManyFromIp);
    }
    *n += 1;
    g.pending_total += 1;
    drop(g);
    Ok(PendingSlot {
        gate: gate.clone(),
        key,
    })
}

impl GateState {
    /// Ligações por autenticar neste momento (total).
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.pending_total
    }

    /// Quanto falta para acabar o bloqueio de `ip`, se estiver bloqueado.
    pub fn blocked_for(&self, ip: IpAddr, now: Instant) -> Option<Duration> {
        self.fails
            .get(&IpKey::of(ip))?
            .blocked_until
            .and_then(|until| until.checked_duration_since(now))
            .filter(|left| !left.is_zero())
    }

    /// Regista um token errado de `ip`.
    pub fn record_failure(&mut self, ip: IpAddr, now: Instant) -> FailOutcome {
        let limits = self.limits;
        let key = IpKey::of(ip);
        if !self.fails.contains_key(&key) && self.fails.len() >= MAX_TRACKED_IPS {
            self.prune(now);
        }
        let entry = self.fails.entry(key).or_insert_with(|| IpFails {
            recent: VecDeque::new(),
            blocked_until: None,
            strikes: 0,
            last: now,
        });
        entry.last = now;
        drop_older(&mut entry.recent, now, limits.fail_window);
        entry.recent.push_back(now);
        let mut outcome = FailOutcome::default();
        if entry.recent.len() >= limits.fails_per_ip {
            entry.recent.clear();
            let block = limits
                .block_base
                .saturating_mul(1u32 << entry.strikes.min(16))
                .min(limits.block_max);
            entry.strikes = entry.strikes.saturating_add(1);
            entry.blocked_until = Some(now + block);
            outcome.blocked = Some(block);
        }

        drop_older(&mut self.global, now, limits.fail_window);
        self.global.push_back(now);
        if self.global.len() >= limits.fails_global {
            self.global.clear();
            outcome.rotate = true;
        }
        outcome
    }

    /// Emparelhou com sucesso a partir de `ip`: esquece as falhas dele.
    pub fn record_success(&mut self, ip: IpAddr) {
        self.fails.remove(&IpKey::of(ip));
    }

    /// Conta uma ligação recusada (à entrada ou no hello). Devolve quantas
    /// houve desde a última linha no HUD quando é altura de escrever outra
    /// (no máximo uma por minuto), senão `None`.
    pub fn note_refused(&mut self, now: Instant) -> Option<u64> {
        self.refused_unlogged += 1;
        let due = self
            .last_refused_log
            .is_none_or(|t| now.saturating_duration_since(t) >= REFUSED_LOG_EVERY);
        if !due {
            return None;
        }
        self.last_refused_log = Some(now);
        Some(std::mem::take(&mut self.refused_unlogged))
    }

    /// Tira as entradas que já não contam (sem bloqueio nem falhas na
    /// janela, e sem atividade há mais do que o teto do bloqueio); se ainda
    /// estiver cheio, a mais antiga não bloqueada.
    fn prune(&mut self, now: Instant) {
        let limits = self.limits;
        self.fails.retain(|_, f| {
            let blocked = f.blocked_until.is_some_and(|u| u > now);
            let recent = f
                .recent
                .back()
                .is_some_and(|t| now.saturating_duration_since(*t) < limits.fail_window);
            blocked || recent || now.saturating_duration_since(f.last) < limits.block_max
        });
        if self.fails.len() >= MAX_TRACKED_IPS
            && let Some(oldest) = self
                .fails
                .iter()
                .filter(|(_, f)| f.blocked_until.is_none_or(|u| u <= now))
                .min_by_key(|(_, f)| f.last)
                .map(|(k, _)| *k)
        {
            self.fails.remove(&oldest);
        }
    }
}

fn drop_older(q: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while q
        .front()
        .is_some_and(|t| now.saturating_duration_since(*t) >= window)
    {
        q.pop_front();
    }
}

/// Fingerprint abreviada para registos (`AB:CD:EF:01…`).
pub fn short_fp(fp: &str) -> String {
    let short: String = fp.chars().take(11).collect();
    if short.len() < fp.len() {
        format!("{short}…")
    } else {
        short
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn limits() -> Limits {
        Limits::DEFAULT
    }

    #[test]
    fn limite_por_ip_e_total() {
        let gate = new(limits());
        let a1 = admit(&gate, ip("192.168.1.10")).unwrap();
        let _a2 = admit(&gate, ip("192.168.1.10")).unwrap();
        assert_eq!(
            admit(&gate, ip("192.168.1.10")).unwrap_err(),
            Refusal::TooManyFromIp
        );
        // Liberta-se ao sair do âmbito.
        drop(a1);
        let _a3 = admit(&gate, ip("192.168.1.10")).unwrap();

        // Total: 8 no máximo, venham de onde vierem.
        let mut slots = Vec::new();
        for i in 0..6 {
            slots.push(admit(&gate, ip(&format!("10.0.0.{i}"))).unwrap());
        }
        assert_eq!(gate.lock().unwrap().pending(), 8);
        assert_eq!(
            admit(&gate, ip("10.0.0.99")).unwrap_err(),
            Refusal::TooManyPending
        );
        slots.clear();
        assert_eq!(gate.lock().unwrap().pending(), 2);
        admit(&gate, ip("10.0.0.99")).unwrap();
    }

    #[test]
    fn ipv6_conta_por_prefixo_64_e_ipv4_mapeado_como_ipv4() {
        let gate = new(limits());
        let _a = admit(&gate, ip("2001:db8:1:2::1")).unwrap();
        let _b = admit(&gate, ip("2001:db8:1:2:ffff::2")).unwrap();
        assert_eq!(
            admit(&gate, ip("2001:db8:1:2::3")).unwrap_err(),
            Refusal::TooManyFromIp
        );
        let _c = admit(&gate, ip("2001:db8:1:3::1")).unwrap();

        assert_eq!(
            IpKey::of(ip("::ffff:192.168.1.5")),
            IpKey::of(ip("192.168.1.5"))
        );
    }

    #[test]
    fn cinco_tokens_errados_bloqueiam_o_ip_com_recuo() {
        let mut l = limits();
        l.fails_global = 1000;
        let gate = new(l);
        let mut s = gate.lock().unwrap();
        let t0 = Instant::now();
        let a = ip("192.168.1.66");
        for i in 0..4 {
            let out = s.record_failure(a, t0 + Duration::from_secs(i));
            assert_eq!(out, FailOutcome::default());
            assert_eq!(s.blocked_for(a, t0 + Duration::from_secs(i)), None);
        }
        let t5 = t0 + Duration::from_secs(4);
        assert_eq!(
            s.record_failure(a, t5).blocked,
            Some(Duration::from_secs(60))
        );
        assert!(s.blocked_for(a, t5).is_some());
        assert_eq!(s.blocked_for(ip("192.168.1.67"), t5), None);
        // Fim do bloqueio.
        let fim = t5 + Duration::from_secs(60);
        assert_eq!(s.blocked_for(a, fim), None);
        // Segundo bloqueio: dobra.
        for i in 0..4 {
            s.record_failure(a, fim + Duration::from_secs(i));
        }
        let out = s.record_failure(a, fim + Duration::from_secs(4));
        assert_eq!(out.blocked, Some(Duration::from_secs(120)));
        // Teto de 1 h.
        let mut t = fim + Duration::from_secs(200);
        let mut last = None;
        for _ in 0..10 {
            for _ in 0..5 {
                last = s.record_failure(a, t).blocked.or(last);
            }
            t += Duration::from_secs(4000);
        }
        assert_eq!(last, Some(Duration::from_secs(3600)));
    }

    #[test]
    fn falhas_fora_da_janela_nao_contam() {
        let gate = new(limits());
        let mut s = gate.lock().unwrap();
        let t0 = Instant::now();
        let a = ip("192.168.1.66");
        for i in 0..4 {
            s.record_failure(a, t0 + Duration::from_secs(i));
        }
        // A 5.ª chega depois de passar a janela das primeiras.
        let tarde = t0 + Duration::from_secs(10 * 60 + 5);
        assert_eq!(s.record_failure(a, tarde).blocked, None);
    }

    #[test]
    fn sucesso_esquece_as_falhas_do_ip() {
        let gate = new(limits());
        let mut s = gate.lock().unwrap();
        let t0 = Instant::now();
        let a = ip("192.168.1.66");
        for _ in 0..4 {
            s.record_failure(a, t0);
        }
        s.record_success(a);
        assert_eq!(s.record_failure(a, t0).blocked, None);
    }

    #[test]
    fn vinte_falhas_no_total_rodam_o_token() {
        let gate = new(limits());
        let mut s = gate.lock().unwrap();
        let t0 = Instant::now();
        let mut rotations = 0;
        for i in 0..40u32 {
            // Cada IP só erra uma vez: nenhum é bloqueado, mas o total conta.
            let who = IpAddr::from([10, 0, (i / 250) as u8, (i % 250) as u8]);
            let out = s.record_failure(who, t0 + Duration::from_millis(i.into()));
            assert_eq!(out.blocked, None);
            if out.rotate {
                rotations += 1;
                assert!(i == 19 || i == 39, "rodou na tentativa {i}");
            }
        }
        assert_eq!(rotations, 2);
    }

    #[test]
    fn recusas_no_hud_no_maximo_uma_linha_por_minuto() {
        let gate = new(limits());
        let mut s = gate.lock().unwrap();
        let t0 = Instant::now();
        assert_eq!(s.note_refused(t0), Some(1));
        for i in 1..=30 {
            assert_eq!(s.note_refused(t0 + Duration::from_secs(i)), None);
        }
        assert_eq!(s.note_refused(t0 + Duration::from_secs(60)), Some(31));
    }

    #[test]
    fn tabela_de_ips_tem_teto() {
        let gate = new(limits());
        let mut s = gate.lock().unwrap();
        let t0 = Instant::now();
        for i in 0..(MAX_TRACKED_IPS as u32 + 50) {
            s.record_failure(IpAddr::from(i.to_be_bytes()), t0);
        }
        assert!(s.fails.len() <= MAX_TRACKED_IPS);
    }

    #[test]
    fn fingerprint_abreviada() {
        assert_eq!(short_fp("AB:CD:EF:01:23:45"), "AB:CD:EF:01…");
        assert_eq!(short_fp("AB:CD"), "AB:CD");
    }
}
