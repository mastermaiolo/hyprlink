//! `pc.status` (D→P): telemetria real do PC para o dashboard do telemóvel —
//! o contrato está em `prompt_ai_studio_2026-10-04_3_telemetria_pc_real.md`
//! (que substitui os valores inventados 14/6/43/4 que a app mostrava).
//!
//! Cada 2 s enquanto ligado: hostname, CPU (delta de `/proc/stat`), RAM
//! (`/proc/meminfo`), temperatura (hwmon primeiro, depois thermal zones),
//! uptime e o RTT do QUIC medido pela própria conexão; mais, a cada 30 s, o
//! disco livre da pasta pessoal e, se houver GPU AMD/NVIDIA, a sua carga.
//! Campo que não se consegue ler (ex: PC sem sensor de temperatura) sai
//! **omitido**, nunca a zeros — a app mostra «—».
//!
//! Tudo vem de `/proc` e `/sys` (e `statvfs`); só a GPU NVIDIA precisa de um
//! processo externo (`nvidia-smi`), e esse corre fora do ciclo.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ciborium::Value;

use crate::active::ActiveConn;
use crate::config::{self, SharedConfig};
use crate::state::HudState;

const SYS: &str = "/sys";
/// O disco muda devagar; o `nvidia-smi` é um processo.
const DISK_EVERY: Duration = Duration::from_secs(30);
const NVIDIA_EVERY: Duration = Duration::from_secs(6);
const NVIDIA_TIMEOUT: Duration = Duration::from_secs(1);

/// Primeira linha de `/proc/stat`: `cpu  user nice system idle iowait irq
/// softirq steal …` → (idle_total, busy_total). Para calcular a percentagem
/// entre duas amostras.
fn cpu_sample() -> Option<(u64, u64)> {
    let line = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .next()?
        .to_string();
    let nums: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|n| n.parse().ok())
        .collect();
    if nums.len() < 5 {
        return None;
    }
    let idle = nums[3] + *nums.get(4).unwrap_or(&0); // idle + iowait
    let busy: u64 = nums.iter().take(8.min(nums.len())).sum::<u64>() - idle;
    Some((idle, busy))
}

/// `MemTotal`/`MemAvailable` do `/proc/meminfo` → (usada, total) em bytes.
fn ram_sample() -> Option<(u64, u64)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let get_kb = |key: &str| {
        info.lines()
            .find(|l| l.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
            .map(|kb| kb * 1024)
    };
    let total = get_kb("MemTotal:")?;
    let avail = get_kb("MemAvailable:")?;
    Some((total - avail, total))
}

/// Temperatura plausível de um `…_input` (milligraus). Zero, negativo ou
/// absurdo → `None`.
fn read_temp(path: &Path) -> Option<f32> {
    let millic = std::fs::read_to_string(path).ok()?.trim().parse::<i64>().ok()?;
    let c = millic as f32 / 1000.0;
    (c > 0.0 && c < 120.0).then_some(c)
}

fn read_trim(path: &Path) -> Option<String> {
    Some(std::fs::read_to_string(path).ok()?.trim().to_string())
}

/// Procura o melhor sensor da CPU sob `sys` (a raiz de `/sys`, parametrizada
/// para os testes). Preferência, da melhor para a pior:
/// 0 k10temp `Tdie` · 1 k10temp `Tctl` · 2 coretemp `Package id 0` ·
/// 3 zenpower · 4 hwmon `cpu_thermal`/`soc_thermal` · 5 thermal zone
/// `x86_pkg_temp`/`cpu*` · 6 outra thermal zone · 7 `acpitz`.
/// Só devolve sensores com leitura plausível agora. Uma entrada ilegível
/// salta-se (`continue`), nunca aborta a procura.
fn find_cpu_sensor(sys: &Path) -> Option<PathBuf> {
    let mut best: Option<(u8, PathBuf)> = None;
    let mut consider = |rank: u8, input: PathBuf| {
        if best.as_ref().is_some_and(|(r, _)| *r <= rank) || read_temp(&input).is_none() {
            return;
        }
        best = Some((rank, input));
    };

    let mut hwmons: Vec<PathBuf> = std::fs::read_dir(sys.join("class/hwmon"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    hwmons.sort();
    for dir in hwmons {
        let Some(name) = read_trim(&dir.join("name")) else {
            continue;
        };
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd {
            let Ok(entry) = entry else { continue };
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = file
                .strip_prefix("temp")
                .and_then(|r| r.strip_suffix("_label"))
            else {
                continue;
            };
            let Some(label) = read_trim(&entry.path()) else {
                continue;
            };
            let input = dir.join(format!("temp{stem}_input"));
            let rank = match (name.as_str(), label.as_str()) {
                ("k10temp", "Tdie") => 0,
                ("k10temp", "Tctl") => 1,
                ("coretemp", "Package id 0") => 2,
                ("zenpower", "Tdie" | "Tctl") => 3,
                _ => continue,
            };
            consider(rank, input);
        }
        // Sem rótulos (k10temp antigo, ARM): `temp1_input` pelo nome do chip.
        let rank = match name.as_str() {
            "k10temp" => Some(1),
            "coretemp" => Some(2),
            "zenpower" => Some(3),
            "cpu_thermal" | "soc_thermal" => Some(4),
            _ => None,
        };
        if let Some(rank) = rank {
            consider(rank, dir.join("temp1_input"));
        }
    }

    let mut zones: Vec<PathBuf> = std::fs::read_dir(sys.join("class/thermal"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    zones.sort();
    for zone in zones {
        let Some(kind) = read_trim(&zone.join("type")) else {
            continue;
        };
        let rank = if kind == "x86_pkg_temp" || kind.starts_with("cpu") {
            5
        } else if kind == "acpitz" {
            7
        } else {
            6
        };
        consider(rank, zone.join("temp"));
    }
    best.map(|(_, p)| p)
}

/// Escolhe o sensor **uma vez** e depois só relê o ficheiro; se a leitura
/// falhar, refaz a procura.
#[derive(Default)]
struct CpuTemp {
    sensor: Option<PathBuf>,
    announced: bool,
}

impl CpuTemp {
    fn read(&mut self, sys: &Path) -> Option<f32> {
        if let Some(t) = self.sensor.as_deref().and_then(read_temp) {
            return Some(t);
        }
        self.sensor = find_cpu_sensor(sys);
        match (&self.sensor, self.announced) {
            (Some(p), false) => {
                eprintln!("[telemetry] temperatura da CPU: {}", p.display());
                self.announced = true;
            }
            (None, false) => {
                eprintln!("[telemetry] sem sensor de temperatura da CPU");
                self.announced = true;
            }
            _ => {}
        }
        self.sensor.as_deref().and_then(read_temp)
    }
}

/// (livre, total) em bytes do sistema de ficheiros de `path`. Livre =
/// `f_bavail × f_frsize` (o que um utilizador normal pode usar); total =
/// `f_blocks × f_frsize`. Falha ou total 0 → `None` (nunca 0).
fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    let st = rustix::fs::statvfs(path).ok()?;
    let free = st.f_bavail.saturating_mul(st.f_frsize);
    let total = st.f_blocks.saturating_mul(st.f_frsize);
    (total > 0).then_some((free, total))
}

/// `gpu_busy_percent` do amdgpu: um inteiro 0–100.
fn parse_gpu_busy(raw: &str) -> Option<f32> {
    let v: f32 = raw.trim().parse().ok()?;
    (0.0..=100.0).contains(&v).then_some(v)
}

/// `nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader,nounits`:
/// uma linha por GPU; vence a mais carregada. Linhas vazias ou texto de erro
/// são ignorados; sem nenhum número → `None`.
fn parse_nvidia_smi(out: &str) -> Option<f32> {
    out.lines()
        .filter_map(parse_gpu_busy)
        .reduce(f32::max)
}

/// Carga de cada placa AMD sob `sys/class/drm/card*/device/gpu_busy_percent`.
fn amd_gpu_busy(sys: &Path) -> Vec<f32> {
    let Ok(rd) = std::fs::read_dir(sys.join("class/drm")) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        // `card1`, não `card1-eDP-1`.
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("card") && !n.contains('-')
        })
        .filter_map(|e| {
            parse_gpu_busy(&std::fs::read_to_string(e.path().join("device/gpu_busy_percent")).ok()?)
        })
        .collect()
}

fn nvidia_smi_exists() -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| d.join("nvidia-smi").is_file())
    })
}

/// Corre o `nvidia-smi` com tempo limite (bloqueante: chamar em `spawn_blocking`).
fn nvidia_smi_busy() -> Option<f32> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let t0 = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(st) if st.success() => break,
            Some(_) => return None,
            None if t0.elapsed() > NVIDIA_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    parse_nvidia_smi(&out)
}

/// Carga da GPU: AMD pelo sysfs a cada ciclo (barato), NVIDIA pelo
/// `nvidia-smi` de 6 em 6 s e fora do ciclo. Várias GPUs → a mais carregada.
struct Gpu {
    nvidia: bool,
    nvidia_last: Option<f32>,
    nvidia_at: Option<Instant>,
    pending: Option<tokio::task::JoinHandle<Option<f32>>>,
    announced: bool,
}

impl Gpu {
    fn new() -> Self {
        Self {
            nvidia: nvidia_smi_exists(),
            nvidia_last: None,
            nvidia_at: None,
            pending: None,
            announced: false,
        }
    }

    async fn sample(&mut self, sys: &Path) -> Option<f32> {
        let amd = amd_gpu_busy(sys);
        if !self.announced {
            self.announced = true;
            let how = match (amd.is_empty(), self.nvidia) {
                (false, true) => "amdgpu (sysfs) + nvidia-smi",
                (false, false) => "amdgpu (sysfs gpu_busy_percent)",
                (true, true) => "nvidia-smi",
                (true, false) => "nenhum (sem GPU AMD/NVIDIA compatível)",
            };
            eprintln!("[telemetry] GPU: {how}");
        }
        if self.nvidia {
            if let Some(h) = &self.pending
                && h.is_finished()
            {
                self.nvidia_last = self.pending.take().unwrap().await.ok().flatten();
            }
            if self.pending.is_none()
                && self.nvidia_at.is_none_or(|t| t.elapsed() >= NVIDIA_EVERY)
            {
                self.nvidia_at = Some(Instant::now());
                self.pending = Some(tokio::task::spawn_blocking(nvidia_smi_busy));
            }
        }
        amd.into_iter().chain(self.nvidia_last).reduce(f32::max)
    }
}

fn uptime_secs() -> Option<u64> {
    std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "PC".to_string())
}

fn kv(key: &str, val: Value) -> (Value, Value) {
    (Value::Text(key.into()), val)
}

/// Tudo o que `pc.status` pode levar; `None` = chave ausente.
#[derive(Default)]
struct Sample {
    cpu_pct: Option<f64>,
    ram: Option<(u64, u64)>,
    cpu_temp: Option<f32>,
    uptime: Option<u64>,
    rtt_ms: Option<f64>,
    disk: Option<(u64, u64)>,
    gpu_pct: Option<f32>,
}

fn status_body(host: &str, s: &Sample) -> Value {
    let mut pairs: Vec<(Value, Value)> = vec![kv("hostname", Value::Text(host.to_string()))];
    if let Some(p) = s.cpu_pct {
        pairs.push(kv("cpu_pct", Value::Float(p)));
    }
    if let Some((used, total)) = s.ram {
        pairs.push(kv("ram_used_b", Value::Integer(used.into())));
        pairs.push(kv("ram_total_b", Value::Integer(total.into())));
    }
    if let Some(t) = s.cpu_temp {
        pairs.push(kv("cpu_temp_c", Value::Float(t as f64)));
    }
    if let Some(up) = s.uptime {
        pairs.push(kv("uptime_s", Value::Integer(up.into())));
    }
    if let Some(r) = s.rtt_ms {
        pairs.push(kv("rtt_ms", Value::Float(r)));
    }
    if let Some((free, total)) = s.disk {
        pairs.push(kv("disk_free_b", Value::Integer(free.into())));
        pairs.push(kv("disk_total_b", Value::Integer(total.into())));
    }
    if let Some(g) = s.gpu_pct {
        pairs.push(kv("gpu_pct", Value::Float(g as f64)));
    }
    Value::Map(pairs)
}

/// Onde medir o disco: `disk_path` da config, senão a pasta pessoal.
fn disk_path(config: &SharedConfig) -> Option<PathBuf> {
    config::disk_path(config).or_else(dirs::home_dir)
}

/// Envia `pc.status` a cada 2 s enquanto houver telemóvel ligado.
pub async fn poll_and_push(
    active: ActiveConn,
    _hud: Arc<Mutex<HudState>>,
    config: SharedConfig,
) {
    let host = hostname();
    let sys = Path::new(SYS);
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_cpu: Option<(u64, u64)> = None;
    let mut temp = CpuTemp::default();
    let mut gpu = Gpu::new();
    // Disco: lê de 30 em 30 s e reenvia o último valor entre leituras.
    let mut disk: Option<(u64, u64)> = None;
    let mut disk_at: Option<Instant> = None;
    loop {
        interval.tick().await;
        let Some(connection) = active.lock().unwrap().clone() else {
            last_cpu = None; // reseta a base de delta — CPU entre sessões não se compara
            continue;
        };

        let mut s = Sample::default();
        // CPU: percentagem entre a amostra anterior e esta.
        if let (Some(now), Some(prev)) = (cpu_sample(), last_cpu) {
            let d_idle = now.0.saturating_sub(prev.0);
            let d_busy = now.1.saturating_sub(prev.1);
            let d_total = d_idle + d_busy;
            if d_total > 0 {
                s.cpu_pct = Some(d_busy as f64 * 100.0 / d_total as f64);
            }
        }
        last_cpu = cpu_sample().or(last_cpu);
        s.ram = ram_sample();
        s.cpu_temp = temp.read(sys);
        s.uptime = uptime_secs();
        // RTT do QUIC — o mesmo número que a GUI vê no Event::Telemetry.
        let rtt = connection.rtt();
        if !rtt.is_zero() {
            s.rtt_ms = Some(rtt.as_secs_f64() * 1000.0);
        }
        if disk_at.is_none_or(|t| t.elapsed() >= DISK_EVERY) {
            disk_at = Some(Instant::now());
            disk = disk_path(&config).and_then(|p| disk_usage(&p));
        }
        s.disk = disk;
        s.gpu_pct = gpu.sample(sys).await;

        let _ = crate::active::push(&active, "pc.status", Some(status_body(&host, &s))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_sample_has_sane_shape() {
        // Máquina real (CI incluído): 4+ campos e valores crescentes.
        if let Some((idle, busy)) = cpu_sample() {
            assert!(idle > 0 || busy > 0);
        }
    }

    #[test]
    fn ram_total_exceeds_used() {
        if let Some((used, total)) = ram_sample() {
            assert!(total > used, "MemTotal tem de exceder a usada");
        }
    }

    #[test]
    fn body_kv_shapes() {
        let pairs = vec![
            kv("hostname", Value::Text("pc".into())),
            kv("cpu_pct", Value::Float(12.5)),
        ];
        let Value::Map(m) = Value::Map(pairs) else {
            unreachable!()
        };
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].0.as_text(), Some("hostname"));
        assert_eq!(m[1].1.as_float(), Some(12.5));
    }

    // ── árvore /sys falsa ──────────────────────────────────────────────

    fn fake_sys(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hyprlink-sys-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("class/hwmon")).unwrap();
        std::fs::create_dir_all(d.join("class/thermal")).unwrap();
        d
    }

    fn hwmon(sys: &Path, n: u8, name: &str, temps: &[(&str, &str, &str)]) {
        let d = sys.join(format!("class/hwmon/hwmon{n}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("name"), format!("{name}\n")).unwrap();
        for (i, label, val) in temps {
            if !label.is_empty() {
                std::fs::write(d.join(format!("temp{i}_label")), format!("{label}\n")).unwrap();
            }
            std::fs::write(d.join(format!("temp{i}_input")), format!("{val}\n")).unwrap();
        }
    }

    fn zone(sys: &Path, n: u8, kind: &str, val: &str) {
        let d = sys.join(format!("class/thermal/thermal_zone{n}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("type"), format!("{kind}\n")).unwrap();
        std::fs::write(d.join("temp"), format!("{val}\n")).unwrap();
    }

    fn temp_of(sys: &Path) -> Option<f32> {
        CpuTemp::default().read(sys)
    }

    #[test]
    fn k10temp_tctl_como_nesta_maquina() {
        let sys = fake_sys("k10");
        hwmon(&sys, 2, "nvme", &[("1", "Composite", "39850")]);
        hwmon(&sys, 3, "amdgpu", &[("1", "edge", "50000")]);
        hwmon(&sys, 4, "k10temp", &[("1", "Tctl", "55000")]);
        assert_eq!(temp_of(&sys), Some(55.0));
        std::fs::remove_dir_all(&sys).ok();
    }

    #[test]
    fn k10temp_prefere_tdie_a_tctl() {
        let sys = fake_sys("tdie");
        hwmon(&sys, 0, "k10temp", &[("1", "Tctl", "75000"), ("2", "Tdie", "45000")]);
        assert_eq!(temp_of(&sys), Some(45.0));
        std::fs::remove_dir_all(&sys).ok();
    }

    #[test]
    fn coretemp_package_id_0() {
        let sys = fake_sys("core");
        hwmon(
            &sys,
            1,
            "coretemp",
            &[("1", "Package id 0", "61000"), ("2", "Core 0", "58000")],
        );
        assert_eq!(temp_of(&sys), Some(61.0));
        std::fs::remove_dir_all(&sys).ok();
    }

    #[test]
    fn hwmon_vence_as_thermal_zones_e_acpitz_fica_em_ultimo() {
        let sys = fake_sys("rank");
        zone(&sys, 0, "acpitz", "27800");
        zone(&sys, 1, "x86_pkg_temp", "48000");
        assert_eq!(temp_of(&sys), Some(48.0), "pkg_temp antes de acpitz");
        hwmon(&sys, 0, "coretemp", &[("1", "Package id 0", "52000")]);
        assert_eq!(temp_of(&sys), Some(52.0), "hwmon antes das zones");
        std::fs::remove_dir_all(&sys).ok();
    }

    #[test]
    fn so_thermal_zones_e_so_acpitz() {
        let sys = fake_sys("zones");
        zone(&sys, 0, "acpitz", "27800");
        assert_eq!(temp_of(&sys), Some(27.8), "acpitz serve se não há melhor");
        zone(&sys, 1, "cpu-thermal", "40000");
        assert_eq!(temp_of(&sys), Some(40.0));
        std::fs::remove_dir_all(&sys).ok();
    }

    #[test]
    fn sem_sensor_da_none_e_nvme_nao_conta_como_cpu() {
        let sys = fake_sys("none");
        assert_eq!(temp_of(&sys), None);
        hwmon(&sys, 2, "nvme", &[("1", "Composite", "39850")]);
        hwmon(&sys, 3, "amdgpu", &[("1", "edge", "50000")]);
        assert_eq!(temp_of(&sys), None);
        // Sem a árvore sequer.
        assert_eq!(temp_of(Path::new("/nao/existe")), None);
        std::fs::remove_dir_all(&sys).ok();
    }

    #[test]
    fn entrada_ilegivel_no_meio_nao_aborta() {
        let sys = fake_sys("bad");
        // `hwmon0` sem `name` e um ficheiro solto onde devia haver pasta.
        std::fs::create_dir_all(sys.join("class/hwmon/hwmon0")).unwrap();
        std::fs::write(sys.join("class/hwmon/hwmon1"), b"lixo").unwrap();
        // Zone com lixo e outra com valor absurdo/zero antes da boa.
        zone(&sys, 0, "cpu0", "lixo");
        zone(&sys, 1, "cpu1", "0");
        zone(&sys, 2, "cpu2", "250000");
        hwmon(&sys, 5, "k10temp", &[("1", "Tctl", "55000")]);
        assert_eq!(temp_of(&sys), Some(55.0));
        std::fs::remove_dir_all(&sys).ok();
    }

    #[test]
    fn escolha_guardada_e_refeita_quando_a_leitura_falha() {
        let sys = fake_sys("redo");
        hwmon(&sys, 0, "k10temp", &[("1", "Tctl", "50000")]);
        let mut t = CpuTemp::default();
        assert_eq!(t.read(&sys), Some(50.0));
        let escolhido = t.sensor.clone().unwrap();
        // Muda o valor: relê o mesmo ficheiro, sem nova procura.
        std::fs::write(&escolhido, "51000\n").unwrap();
        assert_eq!(t.read(&sys), Some(51.0));
        assert_eq!(t.sensor.as_ref(), Some(&escolhido));
        // O ficheiro some (hwmon renumerado): procura outra vez.
        std::fs::remove_dir_all(sys.join("class/hwmon/hwmon0")).unwrap();
        hwmon(&sys, 7, "k10temp", &[("1", "Tctl", "60000")]);
        assert_eq!(t.read(&sys), Some(60.0));
        assert_ne!(t.sensor.as_ref(), Some(&escolhido));
        std::fs::remove_dir_all(&sys).ok();
    }

    // ── GPU ────────────────────────────────────────────────────────────

    #[test]
    fn gpu_busy_percent_do_amdgpu() {
        assert_eq!(parse_gpu_busy("59\n"), Some(59.0));
        assert_eq!(parse_gpu_busy("0"), Some(0.0));
        assert_eq!(parse_gpu_busy("100"), Some(100.0));
        assert_eq!(parse_gpu_busy("101"), None);
        assert_eq!(parse_gpu_busy(""), None);
        assert_eq!(parse_gpu_busy("N/A"), None);
    }

    #[test]
    fn saida_do_nvidia_smi() {
        assert_eq!(parse_nvidia_smi("37\n"), Some(37.0));
        // Duas GPUs: a mais carregada.
        assert_eq!(parse_nvidia_smi("12\n88\n"), Some(88.0));
        assert_eq!(parse_nvidia_smi(""), None);
        assert_eq!(parse_nvidia_smi("\n\n"), None);
        assert_eq!(
            parse_nvidia_smi("NVIDIA-SMI has failed because it couldn't communicate with the NVIDIA driver."),
            None
        );
        assert_eq!(parse_nvidia_smi("No devices were found\n"), None);
        // Texto no meio não estraga a linha boa.
        assert_eq!(parse_nvidia_smi("[N/A]\n41\n"), Some(41.0));
    }

    #[test]
    fn amd_varias_placas_e_ignora_conectores() {
        let sys = fake_sys("gpu");
        for (card, v) in [("card0", "10"), ("card1", "70")] {
            let d = sys.join(format!("class/drm/{card}/device"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("gpu_busy_percent"), format!("{v}\n")).unwrap();
        }
        // `card1-eDP-1` não é uma placa.
        let c = sys.join("class/drm/card1-eDP-1/device");
        std::fs::create_dir_all(&c).unwrap();
        std::fs::write(c.join("gpu_busy_percent"), "99\n").unwrap();
        let mut v = amd_gpu_busy(&sys);
        v.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(v, vec![10.0, 70.0]);
        assert!(amd_gpu_busy(Path::new("/nao/existe")).is_empty());
        std::fs::remove_dir_all(&sys).ok();
    }

    // ── disco ──────────────────────────────────────────────────────────

    #[test]
    fn statvfs_numa_pasta_temporaria() {
        let (free, total) = disk_usage(&std::env::temp_dir()).expect("statvfs");
        assert!(total > 0 && free > 0, "{free}/{total}");
        assert!(free <= total);
        assert_eq!(disk_usage(Path::new("/nao/existe")), None);
    }

    // ── corpo CBOR ─────────────────────────────────────────────────────

    fn has(v: &Value, k: &str) -> bool {
        matches!(v, Value::Map(m) if m.iter().any(|(a, _)| a.as_text() == Some(k)))
    }

    #[test]
    fn corpo_sem_as_chaves_quando_faltam_dados() {
        let vazio = status_body("pc", &Sample::default());
        for k in [
            "cpu_pct", "ram_used_b", "ram_total_b", "cpu_temp_c", "uptime_s", "rtt_ms",
            "disk_free_b", "disk_total_b", "gpu_pct",
        ] {
            assert!(!has(&vazio, k), "{k} devia faltar");
        }
        assert!(has(&vazio, "hostname"));

        let cheio = status_body(
            "pc",
            &Sample {
                disk: Some((100, 500)),
                gpu_pct: Some(59.0),
                cpu_temp: Some(55.0),
                ..Default::default()
            },
        );
        for k in ["disk_free_b", "disk_total_b", "gpu_pct", "cpu_temp_c"] {
            assert!(has(&cheio, k), "{k}");
        }
        // Só o disco falha: as outras ficam.
        let sem_disco = status_body("pc", &Sample { gpu_pct: Some(1.0), ..Default::default() });
        assert!(!has(&sem_disco, "disk_free_b") && !has(&sem_disco, "disk_total_b"));
        assert!(has(&sem_disco, "gpu_pct"));
    }

    /// À mão, na máquina real: `cargo test -p hyprlinkd -- --ignored manual_sys_real --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn manual_sys_real() {
        let sys = Path::new(SYS);
        println!("sensor = {:?}", find_cpu_sensor(sys));
        println!("temp   = {:?}", CpuTemp::default().read(sys));
        println!("gpu    = {:?}", Gpu::new().sample(sys).await);
        println!("disco  = {:?}", dirs::home_dir().and_then(|h| disk_usage(&h)));
    }
}
