//! Velocidade e tempo restante de uma transferência (prompt 61). Só dados;
//! o texto é do `fmt` (`fmt::transfer_speed`, `fmt::transfer_summary`). A app
//! faz o mesmo cálculo em Kotlin: as constantes têm de ser iguais nos dois
//! lados.
//!
//! - Velocidade: média móvel exponencial dos bytes por segundo, com uma
//!   amostra a cada ≥ [`SAMPLE_EVERY`] e peso [`ALPHA`] para a nova.
//! - Tempo restante: (total − feito) / velocidade, só depois de
//!   [`ETA_AFTER`] de transferência e com velocidade > 0.

use std::time::Duration;

/// Intervalo mínimo entre amostras (mais amostras não entram na média).
pub const SAMPLE_EVERY: Duration = Duration::from_millis(500);
/// Peso da amostra nova na média móvel.
pub const ALPHA: f64 = 0.3;
/// Antes disto, o tempo restante fica «a calcular…».
pub const ETA_AFTER: Duration = Duration::from_secs(2);

/// Média móvel da velocidade de uma transferência. Começa em `(0 s, 0 B)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RateMeter {
    /// Instante (desde o início) e bytes feitos na última amostra.
    last: (Duration, u64),
    /// Bytes por segundo; `None` até à primeira amostra.
    ema: Option<f64>,
}

impl RateMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `done` bytes feitos `elapsed` depois do início. Chamar sempre que se
    /// queira (a cada bloco, a cada refresco da GUI): só conta como amostra
    /// passados [`SAMPLE_EVERY`] desde a anterior. Sem bytes novos (pausa),
    /// a amostra é 0 e a média desce.
    pub fn sample(&mut self, elapsed: Duration, done: u64) {
        let (t0, b0) = self.last;
        let dt = elapsed.saturating_sub(t0);
        if dt < SAMPLE_EVERY {
            return;
        }
        let now = done.saturating_sub(b0) as f64 / dt.as_secs_f64();
        self.ema = Some(match self.ema {
            None => now,
            Some(e) => ALPHA * now + (1.0 - ALPHA) * e,
        });
        self.last = (elapsed, done);
    }

    /// Bytes por segundo (arredondado); `None` antes da primeira amostra.
    pub fn bytes_per_sec(&self) -> Option<u64> {
        self.ema.map(|e| e.round() as u64)
    }

    /// Segundos que faltam; `None` antes de [`ETA_AFTER`] ou parado.
    pub fn eta_s(&self, elapsed: Duration, done: u64, total: u64) -> Option<u64> {
        if elapsed < ETA_AFTER {
            return None;
        }
        let rate = self.ema.filter(|r| *r >= 1.0)?;
        Some((total.saturating_sub(done) as f64 / rate).ceil() as u64)
    }
}

/// Média de uma transferência acabada: `bytes` em `elapsed`.
pub fn average(bytes: u64, elapsed: Duration) -> Option<u64> {
    let s = elapsed.as_secs_f64();
    (s > 0.0).then(|| (bytes as f64 / s).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn arranque_sem_amostra_nao_tem_velocidade_nem_eta() {
        let mut m = RateMeter::new();
        assert_eq!(m.bytes_per_sec(), None);
        m.sample(ms(200), 1_000_000); // ainda não passou meio segundo
        assert_eq!(m.bytes_per_sec(), None);
        assert_eq!(m.eta_s(ms(200), 1_000_000, 10_000_000), None);
    }

    #[test]
    fn ritmo_constante_da_a_velocidade_e_o_eta_certos() {
        let mut m = RateMeter::new();
        // 10 MB/s, amostras a cada 500 ms (e refrescos a meio, que não contam).
        for i in 1..=8u64 {
            m.sample(ms(i * 500 - 250), (i * 500 - 250) * 10_000);
            m.sample(ms(i * 500), i * 5_000_000);
        }
        assert_eq!(m.bytes_per_sec(), Some(10_000_000));
        // 4 s, 40 MB de 100 MB → faltam 6 s.
        assert_eq!(m.eta_s(ms(4000), 40_000_000, 100_000_000), Some(6));
    }

    #[test]
    fn eta_so_depois_de_dois_segundos() {
        let mut m = RateMeter::new();
        m.sample(ms(500), 5_000_000);
        m.sample(ms(1000), 10_000_000);
        assert!(m.bytes_per_sec().is_some());
        assert_eq!(m.eta_s(ms(1999), 10_000_000, 100_000_000), None);
        assert!(m.eta_s(ms(2000), 10_000_000, 100_000_000).is_some());
    }

    #[test]
    fn um_pico_nao_salta_a_velocidade() {
        let mut m = RateMeter::new();
        m.sample(ms(500), 5_000_000); // 10 MB/s
        m.sample(ms(1000), 10_000_000); // 10 MB/s
        m.sample(ms(1500), 25_000_000); // pico de 30 MB/s
        // 0,3·30 + 0,7·10 = 16 MB/s, não 30.
        assert_eq!(m.bytes_per_sec(), Some(16_000_000));
    }

    #[test]
    fn pausa_faz_descer_a_velocidade_e_parado_nao_tem_eta() {
        let mut m = RateMeter::new();
        m.sample(ms(500), 5_000_000);
        m.sample(ms(1000), 10_000_000);
        let before = m.bytes_per_sec().unwrap();
        for i in 3..=60u64 {
            m.sample(ms(i * 500), 10_000_000);
        }
        assert!(m.bytes_per_sec().unwrap() < before / 100);
        // Abaixo de 1 B/s conta como parado.
        assert_eq!(m.eta_s(ms(30_000), 10_000_000, 20_000_000), None);
    }

    #[test]
    fn fim_eta_zero_e_media() {
        let mut m = RateMeter::new();
        m.sample(ms(1000), 10_000_000);
        m.sample(ms(2000), 20_000_000);
        assert_eq!(m.eta_s(ms(2000), 20_000_000, 20_000_000), Some(0));
        assert_eq!(
            average(1_900_000_000, Duration::from_secs(151)),
            Some(12_582_781)
        );
        assert_eq!(average(10, Duration::ZERO), None);
    }
}
