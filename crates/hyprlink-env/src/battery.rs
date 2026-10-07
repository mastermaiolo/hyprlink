//! Baterias em `/sys/class/power_supply`.
//!
//! Regra de escolha: a bateria **do sistema** é a primeira (por nome) com
//! `type=Battery` e `scope` diferente de `Device`. As baterias de periféricos
//! (ratos, teclados, auscultadores, o próprio telemóvel por USB) trazem
//! `scope=Device` e ficam de fora. Sem o ficheiro `scope`, conta como do
//! sistema (o kernel só o põe nas baterias de periféricos).

use std::path::Path;

use serde::Serialize;

use crate::read_trim;

/// Estado da bateria do PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcBattery {
    /// 0–100.
    pub level: i64,
    /// A carregar de facto (`Charging`); cheia ou com limite de carga, não.
    pub charging: bool,
    /// Fio ligado: alguma fonte `Mains` com `online=1` (ou a carregar, que
    /// só é possível com fio — cobre carregadores USB-C que o kernel não
    /// classifica como `Mains`).
    pub plugged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Supply {
    pub name: String,
    /// `Battery`, `Mains`, `USB`…
    pub kind: String,
    pub scope: Option<String>,
    /// `true` se é a bateria que o daemon usa.
    pub used: bool,
}

struct Raw {
    name: String,
    kind: String,
    scope: Option<String>,
    path: std::path::PathBuf,
}

fn raw_supplies(sys: &Path) -> Vec<Raw> {
    let Ok(rd) = std::fs::read_dir(sys.join("class/power_supply")) else {
        return Vec::new();
    };
    let mut v: Vec<Raw> = rd
        .filter_map(|e| e.ok())
        .map(|e| Raw {
            name: e.file_name().to_string_lossy().into_owned(),
            kind: read_trim(&e.path().join("type")).unwrap_or_default(),
            scope: read_trim(&e.path().join("scope")),
            path: e.path(),
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn is_system_battery(r: &Raw) -> bool {
    r.kind.eq_ignore_ascii_case("Battery")
        && !r
            .scope
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("Device"))
        && read_trim(&r.path.join("capacity")).is_some_and(|c| c.parse::<i64>().is_ok())
}

/// Todas as fontes de energia, com a que se usa marcada.
pub fn list(sys: &Path) -> Vec<Supply> {
    let raw = raw_supplies(sys);
    let used = raw.iter().position(is_system_battery);
    raw.iter()
        .enumerate()
        .map(|(i, r)| Supply {
            name: r.name.clone(),
            kind: r.kind.clone(),
            scope: r.scope.clone(),
            used: Some(i) == used,
        })
        .collect()
}

/// Lê a bateria do sistema; `None` = o PC não tem (desktop).
pub fn read_at(sys: &Path) -> Option<PcBattery> {
    let raw = raw_supplies(sys);
    let mains_online = raw.iter().any(|r| {
        r.kind.eq_ignore_ascii_case("Mains")
            && read_trim(&r.path.join("online")).as_deref() == Some("1")
    });
    let bat = raw.iter().find(|r| is_system_battery(r))?;
    let level = read_trim(&bat.path.join("capacity"))?.parse::<i64>().ok()?;
    let status = read_trim(&bat.path.join("status")).unwrap_or_default();
    let charging = status.eq_ignore_ascii_case("Charging");
    Some(PcBattery {
        level: level.clamp(0, 100),
        charging,
        plugged: mains_online || charging,
    })
}
