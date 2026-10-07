//! Sensor de temperatura da CPU (hwmon, depois zonas térmicas).

use std::path::{Path, PathBuf};

use crate::read_trim;

/// Temperatura plausível de um `…_input` (milligraus). Zero, negativo ou
/// absurdo → `None`.
pub fn read_temp(path: &Path) -> Option<f32> {
    let millic = std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<i64>()
        .ok()?;
    let c = millic as f32 / 1000.0;
    (c > 0.0 && c < 120.0).then_some(c)
}

/// O sensor escolhido e porquê.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Sensor {
    pub path: PathBuf,
    /// Frase curta (pt-PT) com o motivo da escolha.
    pub why: String,
    /// `true` se veio da opção `temp_sensor`.
    pub forced: bool,
}

const RANKS: [&str; 8] = [
    "k10temp (AMD), rótulo Tdie",
    "k10temp (AMD), rótulo Tctl",
    "coretemp (Intel), Package id 0",
    "zenpower (AMD)",
    "hwmon cpu_thermal/soc_thermal (ARM)",
    "zona térmica x86_pkg_temp ou cpu*",
    "outra zona térmica",
    "acpitz (genérico da placa; último recurso)",
];

/// Procura o melhor sensor da CPU sob `sys` (a raiz de `/sys`, parametrizada
/// para os testes). Preferência, da melhor para a pior:
/// 0 k10temp `Tdie` · 1 k10temp `Tctl` · 2 coretemp `Package id 0` ·
/// 3 zenpower · 4 hwmon `cpu_thermal`/`soc_thermal` · 5 thermal zone
/// `x86_pkg_temp`/`cpu*` · 6 outra thermal zone · 7 `acpitz`.
/// Só devolve sensores com leitura plausível agora. Uma entrada ilegível
/// salta-se (`continue`), nunca aborta a procura.
pub fn find_cpu_sensor(sys: &Path) -> Option<Sensor> {
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
    best.map(|(rank, path)| Sensor {
        path,
        why: RANKS[rank as usize].to_string(),
        forced: false,
    })
}

/// O sensor a usar: o da opção `temp_sensor` (já validada) ou o melhor
/// encontrado.
pub fn pick(sys: &Path, forced: Option<&Path>) -> Option<Sensor> {
    if let Some(p) = forced {
        return Some(Sensor {
            path: p.to_path_buf(),
            why: "escolhido na opção temp_sensor".into(),
            forced: true,
        });
    }
    find_cpu_sensor(sys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fake(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("hyprlink-env-sens-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("class/hwmon")).unwrap();
        fs::create_dir_all(d.join("class/thermal")).unwrap();
        d
    }

    fn hwmon(sys: &Path, n: u8, name: &str, temps: &[(&str, &str, &str)]) {
        let d = sys.join(format!("class/hwmon/hwmon{n}"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("name"), format!("{name}\n")).unwrap();
        for (i, label, val) in temps {
            if !label.is_empty() {
                fs::write(d.join(format!("temp{i}_label")), format!("{label}\n")).unwrap();
            }
            fs::write(d.join(format!("temp{i}_input")), format!("{val}\n")).unwrap();
        }
    }

    #[test]
    fn amd_prefere_tdie_a_tctl() {
        let sys = fake("amd");
        hwmon(
            &sys,
            0,
            "k10temp",
            &[("1", "Tctl", "61000"), ("2", "Tdie", "51000")],
        );
        let s = find_cpu_sensor(&sys).unwrap();
        assert!(s.path.ends_with("hwmon0/temp2_input"));
        assert!(s.why.contains("Tdie"));
    }

    #[test]
    fn intel_usa_coretemp_package() {
        let sys = fake("intel");
        hwmon(
            &sys,
            1,
            "coretemp",
            &[("1", "Package id 0", "55000"), ("2", "Core 0", "50000")],
        );
        let s = find_cpu_sensor(&sys).unwrap();
        assert!(s.path.ends_with("hwmon1/temp1_input"));
        assert!(s.why.contains("coretemp"));
    }

    #[test]
    fn acpitz_e_o_ultimo_recurso() {
        let sys = fake("acpi");
        let z = sys.join("class/thermal/thermal_zone0");
        fs::create_dir_all(&z).unwrap();
        fs::write(z.join("type"), "acpitz\n").unwrap();
        fs::write(z.join("temp"), "42000\n").unwrap();
        let s = find_cpu_sensor(&sys).unwrap();
        assert!(s.why.contains("acpitz"));
        // um hwmon bom ganha-lhe
        hwmon(&sys, 0, "coretemp", &[("1", "Package id 0", "50000")]);
        assert!(find_cpu_sensor(&sys).unwrap().why.contains("coretemp"));
    }

    #[test]
    fn leitura_implausivel_e_ignorada_e_sem_sensor_da_none() {
        let sys = fake("zero");
        hwmon(&sys, 0, "k10temp", &[("1", "Tctl", "0")]);
        assert_eq!(find_cpu_sensor(&sys), None);
        assert_eq!(find_cpu_sensor(Path::new("/nao/existe")), None);
    }

    #[test]
    fn forcado_vence() {
        let sys = fake("forca");
        hwmon(&sys, 0, "k10temp", &[("1", "Tdie", "50000")]);
        let p = sys.join("class/hwmon/hwmon0/temp1_input");
        let s = pick(&sys, Some(&p)).unwrap();
        assert!(s.forced);
    }
}
