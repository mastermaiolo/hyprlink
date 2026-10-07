//! GPUs: fabricante, driver, híbrido, e de onde vem a carga (`gpu_pct`).

use std::path::Path;

use serde::Serialize;

use crate::overrides::GpuChoice;
use crate::read_trim;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    Amd,
    Nvidia,
    Intel,
    Other,
}

impl Vendor {
    /// `vendor` do PCI (`0x1002` AMD, `0x10de` NVIDIA, `0x8086` Intel).
    pub fn from_pci(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "0x1002" => Vendor::Amd,
            "0x10de" => Vendor::Nvidia,
            "0x8086" => Vendor::Intel,
            _ => Vendor::Other,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Vendor::Amd => "AMD",
            Vendor::Nvidia => "NVIDIA",
            Vendor::Intel => "Intel",
            Vendor::Other => "outro",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gpu {
    /// `card0`, `card1`…
    pub card: String,
    pub vendor: Vendor,
    /// Driver do kernel (`amdgpu`, `nvidia`, `i915`, `xe`, `nouveau`…).
    pub driver: Option<String>,
}

/// As placas sob `sys/class/drm/card*` (não as saídas `card1-eDP-1`).
pub fn enumerate(sys: &Path) -> Vec<Gpu> {
    let Ok(rd) = std::fs::read_dir(sys.join("class/drm")) else {
        return Vec::new();
    };
    let mut cards: Vec<Gpu> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n = name.strip_prefix("card")?;
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let dev = e.path().join("device");
            let vendor = Vendor::from_pci(&read_trim(&dev.join("vendor"))?);
            let driver = std::fs::read_link(dev.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
            Some(Gpu {
                card: name,
                vendor,
                driver,
            })
        })
        .collect();
    cards.sort_by(|a, b| a.card.cmp(&b.card));
    cards
}

/// Híbrido = mais de uma GPU (por exemplo iGPU + dGPU, AMD+NVIDIA ou
/// Intel+NVIDIA).
pub fn is_hybrid(gpus: &[Gpu]) -> bool {
    gpus.len() > 1
}

/// `gpu_busy_percent` do amdgpu: um inteiro 0–100.
pub fn parse_gpu_busy(raw: &str) -> Option<f32> {
    let v: f32 = raw.trim().parse().ok()?;
    (0.0..=100.0).contains(&v).then_some(v)
}

/// `nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader,nounits`:
/// uma linha por GPU; vence a mais carregada. Linhas vazias ou texto de erro
/// são ignorados; sem nenhum número → `None`.
pub fn parse_nvidia_smi(out: &str) -> Option<f32> {
    out.lines().filter_map(parse_gpu_busy).reduce(f32::max)
}

/// Carga de cada placa AMD sob `sys/class/drm/card*/device/gpu_busy_percent`.
pub fn amd_gpu_busy(sys: &Path) -> Vec<f32> {
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

/// Há uma GPU NVIDIA **já acordada**? Lê só `power/runtime_status` de cada
/// função de vídeo (classe `0x03…`, fabricante `0x10de`) em
/// `sys/bus/pci/devices` — ler esse ficheiro não acorda a placa, ao contrário
/// do `nvidia-smi`, que a tira do repouso num portátil híbrido. Sem
/// `runtime_status` (placa sem gestão de energia em tempo de execução, por
/// exemplo num desktop) conta como acordada: já está sempre ligada.
/// Sem nenhuma placa NVIDIA → `false`.
pub fn nvidia_awake(sys: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(sys.join("bus/pci/devices")) else {
        return false;
    };
    rd.filter_map(|e| e.ok()).any(|e| {
        let d = e.path();
        let is_nvidia_video = read_trim(&d.join("vendor")).as_deref() == Some("0x10de")
            && read_trim(&d.join("class")).is_some_and(|c| c.starts_with("0x03"));
        is_nvidia_video
            && read_trim(&d.join("power/runtime_status")).is_none_or(|st| st == "active")
    })
}

/// Saída de `intel_gpu_top -J -n 1`: o maior `"busy"` entre os motores.
/// Leitura tolerante (procura os `"busy": N` no texto) porque o formato exato
/// **está a confirmar** em hardware real; sem nenhum número → `None`.
pub fn parse_intel_gpu_top(out: &str) -> Option<f32> {
    let mut best: Option<f32> = None;
    let mut rest = out;
    while let Some(i) = rest.find("\"busy\"") {
        rest = &rest[i + 6..];
        let num: String = rest
            .trim_start_matches([' ', ':', '\t'])
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(v) = num.parse::<f32>()
            && (0.0..=100.0).contains(&v)
        {
            best = Some(best.map_or(v, |b| b.max(v)));
        }
    }
    best
}

/// De onde vem a carga da GPU, e porquê.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GpuPlan {
    pub amd: bool,
    pub nvidia: bool,
    pub intel: bool,
    pub why: String,
}

/// Decide as fontes da carga da GPU. `have` diz que executáveis existem.
/// Automático: AMD pelo sysfs se houver carga legível, NVIDIA pelo
/// `nvidia-smi` se existir; **Intel nunca por omissão** (só com
/// `gpu_source = "intel"`, e só se o driver for o `i915` e existir
/// `intel_gpu_top`).
pub fn plan(sys: &Path, choice: GpuChoice, have: &dyn Fn(&str) -> bool) -> GpuPlan {
    let gpus = enumerate(sys);
    let amd_ok = !amd_gpu_busy(sys).is_empty();
    let nv_ok = have("nvidia-smi");
    let intel_i915 = gpus
        .iter()
        .any(|g| g.vendor == Vendor::Intel && g.driver.as_deref() == Some("i915"));
    let intel_xe = gpus
        .iter()
        .any(|g| g.vendor == Vendor::Intel && g.driver.as_deref() == Some("xe"));
    let intel_ok = intel_i915 && have("intel_gpu_top");
    let (amd, nvidia, intel, why) = match choice {
        GpuChoice::None => (
            false,
            false,
            false,
            "desligada na opção gpu_source".to_string(),
        ),
        GpuChoice::Amd => (
            amd_ok,
            false,
            false,
            if amd_ok {
                "AMD (sysfs gpu_busy_percent), por gpu_source".to_string()
            } else {
                "gpu_source=amd, mas nenhuma placa AMD dá gpu_busy_percent".to_string()
            },
        ),
        GpuChoice::Nvidia => (
            false,
            nv_ok,
            false,
            if nv_ok {
                "NVIDIA (nvidia-smi, só com a placa acordada), por gpu_source".to_string()
            } else {
                "gpu_source=nvidia, mas o nvidia-smi não existe".to_string()
            },
        ),
        GpuChoice::Intel => (
            false,
            false,
            intel_ok,
            if intel_ok {
                "Intel (intel_gpu_top -J -n 1, driver i915), por gpu_source".to_string()
            } else if intel_xe {
                "gpu_source=intel, mas o driver é o xe: o intel_gpu_top só suporta o i915"
                    .to_string()
            } else if intel_i915 {
                "gpu_source=intel, mas o intel_gpu_top não está instalado".to_string()
            } else {
                "gpu_source=intel, mas não há GPU Intel com driver i915".to_string()
            },
        ),
        GpuChoice::Auto => {
            let why = match (amd_ok, nv_ok) {
                (true, true) => "amdgpu (sysfs) + nvidia-smi (só com a placa acordada)",
                (true, false) => "amdgpu (sysfs gpu_busy_percent)",
                (false, true) => "nvidia-smi (só com a placa acordada)",
                (false, false) if intel_i915 || intel_xe => {
                    "nenhuma (GPU Intel: sem valor por omissão; usa gpu_source = \"intel\" com i915 e intel_gpu_top)"
                }
                (false, false) => "nenhuma (sem GPU AMD/NVIDIA compatível)",
            };
            (amd_ok, nv_ok, false, why.to_string())
        }
    };
    GpuPlan {
        amd,
        nvidia,
        intel,
        why,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn fake(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hyprlink-env-gpu-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("class/drm")).unwrap();
        d
    }

    fn card(sys: &Path, n: u8, vendor: &str, driver: &str, busy: Option<&str>) {
        let dev = sys.join(format!("class/drm/card{n}/device"));
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("vendor"), format!("{vendor}\n")).unwrap();
        let target = sys.join(format!("drivers-{n}/{driver}"));
        fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, dev.join("driver")).unwrap();
        if let Some(b) = busy {
            fs::write(dev.join("gpu_busy_percent"), format!("{b}\n")).unwrap();
        }
        // uma saída que não é placa
        fs::create_dir_all(sys.join(format!("class/drm/card{n}-eDP-1"))).unwrap();
    }

    fn no_tools(_: &str) -> bool {
        false
    }

    #[test]
    fn hibrido_amd_nvidia() {
        let sys = fake("hib");
        card(&sys, 0, "0x10de", "nvidia", None);
        card(&sys, 1, "0x1002", "amdgpu", Some("7"));
        let g = enumerate(&sys);
        assert_eq!(g.len(), 2, "só as placas, não card1-eDP-1");
        assert_eq!(g[0].vendor, Vendor::Nvidia);
        assert_eq!(g[1].driver.as_deref(), Some("amdgpu"));
        assert!(is_hybrid(&g));
        let p = plan(&sys, GpuChoice::Auto, &|t| t == "nvidia-smi");
        assert!(p.amd && p.nvidia && !p.intel);
    }

    #[test]
    fn intel_sem_valor_por_omissao_e_so_i915() {
        let sys = fake("intel");
        card(&sys, 0, "0x8086", "i915", None);
        let auto = plan(&sys, GpuChoice::Auto, &|t| t == "intel_gpu_top");
        assert!(!auto.intel && !auto.amd, "{auto:?}");
        assert!(auto.why.contains("Intel"));
        let forced = plan(&sys, GpuChoice::Intel, &|t| t == "intel_gpu_top");
        assert!(forced.intel);
        let sem_ferramenta = plan(&sys, GpuChoice::Intel, &no_tools);
        assert!(!sem_ferramenta.intel);
        assert!(sem_ferramenta.why.contains("não está instalado"));
    }

    #[test]
    fn intel_xe_nao_usa_intel_gpu_top() {
        let sys = fake("xe");
        card(&sys, 0, "0x8086", "xe", None);
        let p = plan(&sys, GpuChoice::Intel, &|_| true);
        assert!(!p.intel);
        assert!(p.why.contains("xe"));
    }

    #[test]
    fn opcao_none_desliga_tudo() {
        let sys = fake("none");
        card(&sys, 0, "0x1002", "amdgpu", Some("50"));
        let p = plan(&sys, GpuChoice::None, &|_| true);
        assert!(!p.amd && !p.nvidia && !p.intel);
    }

    #[test]
    fn amd_forcado_sem_carga_legivel() {
        let sys = fake("amd0");
        card(&sys, 0, "0x1002", "amdgpu", None);
        let p = plan(&sys, GpuChoice::Amd, &no_tools);
        assert!(!p.amd);
    }

    #[test]
    fn parsers() {
        assert_eq!(parse_gpu_busy(" 37\n"), Some(37.0));
        assert_eq!(parse_gpu_busy("250"), None);
        assert_eq!(parse_nvidia_smi("12\n40\nerro\n"), Some(40.0));
        assert_eq!(parse_nvidia_smi("NVIDIA-SMI has failed"), None);
        let j = r#"[{"engines":{"Render/3D":{"busy": 12.5,"unit":"%"},"Video/0":{"busy":3.0}}}]"#;
        assert_eq!(parse_intel_gpu_top(j), Some(12.5));
        assert_eq!(parse_intel_gpu_top("[]"), None);
        assert_eq!(parse_intel_gpu_top(r#""busy": 900"#), None);
    }

    #[test]
    fn nvidia_acordada_so_com_runtime_status_ativo() {
        let sys = fake("awake");
        let d = sys.join("bus/pci/devices/0000:01:00.0");
        fs::create_dir_all(d.join("power")).unwrap();
        fs::write(d.join("vendor"), "0x10de\n").unwrap();
        fs::write(d.join("class"), "0x030000\n").unwrap();
        fs::write(d.join("power/runtime_status"), "suspended\n").unwrap();
        assert!(!nvidia_awake(&sys));
        fs::write(d.join("power/runtime_status"), "active\n").unwrap();
        assert!(nvidia_awake(&sys));
        fs::remove_file(d.join("power/runtime_status")).unwrap();
        assert!(
            nvidia_awake(&sys),
            "sem runtime_status = sempre ligada (desktop)"
        );
    }
}
