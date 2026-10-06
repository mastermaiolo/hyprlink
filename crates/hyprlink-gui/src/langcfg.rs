//! Idioma da GUI: `$XDG_CONFIG_HOME/hyprlink/gui.json`, campo `"lang"` (mesmos
//! nomes de variante do `Lang` do daemon). Não faz parte do contrato nem do
//! daemon. Sem ficheiro (ou com um valor ilegível), deteta-se pelo ambiente.

use std::path::PathBuf;

use hyprlink_proto::i18n::{self, Lang};

pub fn path() -> Option<PathBuf> {
    path_in(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn path_in(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let base = xdg
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".config")))?;
    Some(base.join("hyprlink").join("gui.json"))
}

fn read(path: &std::path::Path) -> Option<Lang> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    serde_json::from_value(v.get("lang")?.clone()).ok()
}

/// Lê o idioma guardado; senão, deteta-o.
pub fn load() -> Lang {
    path().and_then(|p| read(&p)).unwrap_or_else(i18n::detect)
}

fn write(path: &std::path::Path, lang: Lang) -> std::io::Result<()> {
    // Mantém os outros campos que o ficheiro venha a ter.
    let mut v = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    v["lang"] = serde_json::to_value(lang).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(&v).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, path)
}

/// Aplica o idioma já e grava-o. A gravação falhar não impede a mudança.
pub fn apply_and_save(lang: Lang) {
    i18n::set(lang);
    if let Some(p) = path() {
        if let Err(e) = write(&p, lang) {
            eprintln!("hyprlink-gui: não consegui guardar {}: {e}", p.display());
        }
    }
}

/// No arranque: lê (ou deteta) e aplica.
pub fn init() {
    i18n::set(load());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_prefers_an_absolute_xdg_config_home() {
        assert_eq!(
            path_in(Some("/x/cfg".into()), Some("/home/u".into())),
            Some(PathBuf::from("/x/cfg/hyprlink/gui.json"))
        );
        assert_eq!(
            path_in(Some("relativo".into()), Some("/home/u".into())),
            Some(PathBuf::from("/home/u/.config/hyprlink/gui.json"))
        );
        assert_eq!(path_in(None, None), None);
    }

    #[test]
    fn write_then_read_round_trips_and_keeps_other_fields() {
        let dir = std::env::temp_dir().join(format!("hyprlink-langcfg-{}", std::process::id()));
        let file = dir.join("hyprlink").join("gui.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"outro": 7}"#).unwrap();
        write(&file, Lang::Zh).unwrap();
        assert_eq!(read(&file), Some(Lang::Zh));
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(v["outro"], 7);
        assert_eq!(v["lang"], "Zh");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_bad_value_reads_as_none() {
        let dir = std::env::temp_dir().join(format!("hyprlink-langcfg-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gui.json");
        std::fs::write(&file, r#"{"lang": "Klingon"}"#).unwrap();
        assert_eq!(read(&file), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
