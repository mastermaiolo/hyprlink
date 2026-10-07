//! Completude das traduções: todo o texto passado a `t("…")` / `tr!("…", …)` no
//! código da GUI e em `hyprlink-proto/src/fmt.rs` tem de existir nas tabelas en,
//! es e zh (pt-BR pode faltar: cai para pt-PT); sem chaves repetidas numa
//! tabela; e o número de `{}` / `{N}` da tradução tem de ser o da chave.
//!
//! Falha se alguém acrescentar um texto sem o traduzir. Para acrescentar um
//! idioma ou um texto, ver «Idiomas» no README da GUI.

use hyprlink_gui::i18n::{Lang, table};
use std::collections::{BTreeSet, HashSet};

/// Ficheiros-fonte analisados (nome para as mensagens, conteúdo).
const SOURCES: &[(&str, &str)] = &[
    ("app.rs", include_str!("../src/app.rs")),
    ("views.rs", include_str!("../src/views.rs")),
    ("pages.rs", include_str!("../src/pages.rs")),
    ("ui.rs", include_str!("../src/ui.rs")),
    ("tray.rs", include_str!("../src/tray.rs")),
    ("graphics.rs", include_str!("../src/graphics.rs")),
    ("fmt.rs", include_str!("../../hyprlink-proto/src/fmt.rs")),
];

/// Meses de `fmt::month`: passam por `t(M[i])`, não por um literal.
const MONTHS: [&str; 12] = [
    "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
];

/// Campos de dados de demonstração que a vista traduz com `t(campo)`.
const FIELDS: &[&str] = &["gesture", "trigger", "detail"];

/// Lê um literal de string Rust a partir de `s` (que começa em `"`).
/// Devolve o texto já sem escapes e o resto depois do `"` final.
fn literal(s: &str) -> Option<(String, &str)> {
    let mut chars = s.strip_prefix('"')?.char_indices();
    let body = &s[1..];
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &body[i + 1..])),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '\\' => out.push('\\'),
                'u' => {
                    let mut hex = String::new();
                    for (_, h) in chars.by_ref() {
                        if h == '}' {
                            break;
                        }
                        if h != '{' {
                            hex.push(h);
                        }
                    }
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                '\n' => {
                    // continuação de linha: salta o espaço em branco seguinte
                    while let Some((_, w)) = chars.clone().next() {
                        if w.is_whitespace() {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

/// Todas as chaves usadas num ficheiro: `t("…")`, `tr!("…", …)`,
/// `i18n::t("…")` e os campos de demonstração (`gesture: "…"`).
fn keys_in(src: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let bytes = src.as_bytes();
    for (pos, _) in src.match_indices("t(").chain(src.match_indices("tr!(")) {
        let before = &src[..pos];
        let is_tr = src[pos..].starts_with("tr!(");
        let prev = before.chars().last();
        if !is_tr {
            // `t(` isolado: não é parte de outro identificador, nem método,
            // nem `ui::t(` (o helper de texto de 4 argumentos).
            if prev.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                continue;
            }
            if prev == Some(':') && !before.ends_with("i18n::") {
                continue;
            }
        } else if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let after = &src[pos + if is_tr { 4 } else { 2 }..];
        let after = after.trim_start();
        if after.as_bytes().first() == Some(&b'"') {
            if let Some((k, _)) = literal(after) {
                keys.push(k);
            }
        }
    }
    let _ = bytes;
    for f in FIELDS {
        for (pos, _) in src.match_indices(&format!("{f}:")) {
            let prev = src[..pos].chars().last();
            if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let after = src[pos + f.len() + 1..].trim_start();
            if let Some((k, _)) = literal(after) {
                keys.push(k);
            }
        }
    }
    keys
}

/// Nº de marcadores `{}` / `{N}` (ignora `{{` e `}}`).
fn placeholders(s: &str) -> usize {
    let mut n = 0;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => {
                it.next();
            }
            '{' => {
                let mut spec = String::new();
                for d in it.by_ref() {
                    if d == '}' {
                        break;
                    }
                    spec.push(d);
                }
                if spec.is_empty() || spec.chars().all(|d| d.is_ascii_digit()) {
                    n += 1;
                }
            }
            _ => {}
        }
    }
    n
}

fn used_keys() -> BTreeSet<String> {
    let mut all: BTreeSet<String> = MONTHS.iter().map(|m| m.to_string()).collect();
    for (_, src) in SOURCES {
        all.extend(keys_in(src));
    }
    all
}

#[test]
fn every_used_text_is_translated_into_en_es_and_zh() {
    let used = used_keys();
    assert!(
        used.len() > 300,
        "o extrator só achou {} chaves",
        used.len()
    );
    for lang in [Lang::EnGb, Lang::EsEs, Lang::Zh] {
        let have: HashSet<&str> = table(lang).iter().map(|(k, _)| *k).collect();
        let missing: Vec<&String> = used.iter().filter(|k| !have.contains(k.as_str())).collect();
        assert!(
            missing.is_empty(),
            "faltam {} traduções em {:?} (acrescenta-as ao ficheiro do idioma em \
             hyprlink-proto/src/i18n/):\n{:#?}",
            missing.len(),
            lang,
            missing
        );
    }
}

#[test]
fn no_table_has_duplicate_keys_or_empty_translations() {
    for lang in [Lang::PtBr, Lang::EnGb, Lang::EsEs, Lang::Zh] {
        let mut seen = HashSet::new();
        for (k, v) in table(lang) {
            assert!(seen.insert(*k), "chave repetida em {lang:?}: {k:?}");
            assert!(!v.trim().is_empty(), "tradução vazia em {lang:?}: {k:?}");
        }
    }
}

#[test]
fn translations_keep_the_placeholder_count_of_the_key() {
    for lang in [Lang::PtBr, Lang::EnGb, Lang::EsEs, Lang::Zh] {
        for (k, v) in table(lang) {
            assert_eq!(
                placeholders(k),
                placeholders(v),
                "{lang:?}: {k:?} → {v:?} (nº de {{}} / {{N}} diferente)"
            );
        }
    }
}

#[test]
fn indexed_placeholders_stay_within_range() {
    for lang in [Lang::PtBr, Lang::EnGb, Lang::EsEs, Lang::Zh] {
        for (k, v) in table(lang) {
            let n = placeholders(k);
            let mut rest = v.replace("{{", "").replace("}}", "");
            while let Some(i) = rest.find('{') {
                let end = rest[i..].find('}').map_or(rest.len(), |e| i + e);
                if let Ok(idx) = rest[i + 1..end].parse::<usize>() {
                    assert!(idx < n, "{lang:?}: {{{idx}}} fora de alcance em {v:?}");
                }
                rest = rest[end.min(rest.len() - 1) + 1..].to_string();
            }
        }
    }
}

// ── o próprio extrator ──

#[test]
fn extractor_finds_t_tr_and_demo_fields_but_not_the_text_helper() {
    let src = r#"
        kicker(t("UM")), tr!("{} dois {}", a, b), crate::tr!("três"),
        hyprlink_gui::i18n::t("quatro"), ui::t("não", MONO, 1.0, X), x.t("nem isto"),
        format!("nem isto"), not("nem"), gesture: "cinco",
        t("com \"aspas\" e \u{e9}"),
    "#;
    let mut got = keys_in(src);
    got.sort();
    let mut want = vec![
        "UM",
        "{} dois {}",
        "três",
        "quatro",
        "com \"aspas\" e é",
        "cinco",
    ];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn an_untranslated_text_is_reported() {
    let have: HashSet<&str> = table(Lang::Zh).iter().map(|(k, _)| *k).collect();
    let fresh = keys_in(r#"kicker(t("texto novo ainda sem tradução"))"#);
    assert_eq!(fresh, vec!["texto novo ainda sem tradução"]);
    assert!(!have.contains(fresh[0].as_str()));
}
