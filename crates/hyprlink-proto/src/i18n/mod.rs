//! Idiomas da GUI e do `hyprlinkctl`. A **chave é o próprio texto em pt-PT**
//! (o texto-fonte, legível no código); cada tabela dá a tradução.
//!
//! - [`t`]: `t("Guardar")` devolve a tradução no idioma atual, ou o próprio
//!   texto pt-PT se não existir (nunca falha, nunca devolve vazio).
//! - [`tr!`](crate::tr): texto com valores, `tr!("{} por ler", n)`. A *template*
//!   pt-PT é a chave; a tradução usa `{}` por ordem ou `{0}`, `{1}` para
//!   reordenar. `{{` e `}}` dão chavetas literais.
//! - [`set`] / [`get`]: o idioma atual (global, aplica-se na hora).
//! - [`detect`]: `LC_ALL` / `LC_MESSAGES` / `LANG`.
//!
//! O daemon não usa este módulo: envia dados, nunca texto.

mod en;
mod es;
mod pt_br;
mod zh;

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// Mesmos nomes de variante que o `Lang` do `hyprlinkd` (`config.rs`), para o
/// JSON ser intercambiável.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    PtPt,
    PtBr,
    EnGb,
    EsEs,
    Zh,
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::PtPt, Lang::PtBr, Lang::EnGb, Lang::EsEs, Lang::Zh];

    /// Etiqueta curta para o seletor («PT-PT», «PT-BR», «EN», «ES», «中文»).
    pub fn label(self) -> &'static str {
        match self {
            Lang::PtPt => "PT-PT",
            Lang::PtBr => "PT-BR",
            Lang::EnGb => "EN",
            Lang::EsEs => "ES",
            Lang::Zh => "中文",
        }
    }

    /// Código da flag `--lang` do `hyprlinkctl`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::PtPt => "pt-PT",
            Lang::PtBr => "pt-BR",
            Lang::EnGb => "en",
            Lang::EsEs => "es",
            Lang::Zh => "zh",
        }
    }

    /// `pt-PT|pt-BR|en|es|zh` (e variantes comuns: `pt_pt`, `en-GB`, `zh-CN`…).
    pub fn parse(s: &str) -> Option<Lang> {
        let s = s.trim().to_ascii_lowercase().replace('_', "-");
        match s.as_str() {
            "pt-pt" => Some(Lang::PtPt),
            "pt-br" | "pt" => Some(Lang::PtBr),
            "en" | "en-gb" | "en-us" => Some(Lang::EnGb),
            "es" | "es-es" => Some(Lang::EsEs),
            "zh" | "zh-cn" | "zh-hans" => Some(Lang::Zh),
            _ => None,
        }
    }

    /// Vírgula decimal (pt, es) ou ponto (en, zh).
    pub fn decimal_comma(self) -> bool {
        matches!(self, Lang::PtPt | Lang::PtBr | Lang::EsEs)
    }

    fn index(self) -> u8 {
        self as u8
    }

    fn from_index(i: u8) -> Lang {
        Lang::ALL.get(i as usize).copied().unwrap_or(Lang::PtPt)
    }

    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Lang::PtPt => &[],
            Lang::PtBr => pt_br::TABLE,
            Lang::EnGb => en::TABLE,
            Lang::EsEs => es::TABLE,
            Lang::Zh => zh::TABLE,
        }
    }
}

/// Tabelas em bruto, para o teste de completude.
pub fn table(lang: Lang) -> &'static [(&'static str, &'static str)] {
    lang.table()
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
}

pub fn get() -> Lang {
    Lang::from_index(CURRENT.load(Ordering::Relaxed))
}

/// Idioma pelo ambiente: `LC_ALL`, `LC_MESSAGES`, `LANG` (o primeiro que
/// estiver definido e não vazio). `pt_PT`→PtPt, `pt_BR`/`pt`→PtBr, `en*`→EnGb,
/// `es*`→EsEs, `zh*`→Zh; outro → EnGb.
pub fn detect() -> Lang {
    detect_with(|k| std::env::var(k).ok())
}

pub fn detect_with(var: impl Fn(&str) -> Option<String>) -> Lang {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| var(k))
        .find(|v| !v.trim().is_empty());
    let Some(raw) = raw else { return Lang::EnGb };
    let l = raw.to_ascii_lowercase();
    // «pt_PT.UTF-8» → «pt_pt»
    let l = l.split(['.', '@']).next().unwrap_or("");
    if l == "c" || l == "posix" {
        return Lang::EnGb;
    }
    if l.starts_with("pt_pt") {
        Lang::PtPt
    } else if l.starts_with("pt") {
        Lang::PtBr
    } else if l.starts_with("es") {
        Lang::EsEs
    } else if l.starts_with("zh") {
        Lang::Zh
    } else {
        Lang::EnGb
    }
}

type Map = HashMap<&'static str, &'static str>;

fn map(lang: Lang) -> Option<&'static Map> {
    static MAPS: [OnceLock<Map>; 5] = [const { OnceLock::new() }; 5];
    if lang == Lang::PtPt {
        return None;
    }
    Some(MAPS[lang.index() as usize].get_or_init(|| lang.table().iter().copied().collect()))
}

/// Tradução no idioma atual; o próprio `pt` se faltar (ou vier vazia).
pub fn t(pt: &'static str) -> &'static str {
    t_in(get(), pt)
}

pub fn t_in(lang: Lang, pt: &'static str) -> &'static str {
    match map(lang).and_then(|m| m.get(pt)) {
        Some(s) if !s.is_empty() => s,
        _ => pt,
    }
}

/// Substitui `{}` (por ordem) e `{N}` (por índice) na template. Um marcador
/// sem argumento correspondente fica como está.
pub fn render(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 8);
    let mut next = 0usize;
    let mut chars = template.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '{' if matches!(chars.peek(), Some((_, '{'))) => {
                chars.next();
                out.push('{');
            }
            '}' if matches!(chars.peek(), Some((_, '}'))) => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let rest = &template[i + 1..];
                let Some(end) = rest.find('}') else {
                    out.push(c);
                    continue;
                };
                let spec = &rest[..end];
                let idx = if spec.is_empty() {
                    let n = next;
                    next += 1;
                    Some(n)
                } else {
                    spec.parse::<usize>().ok()
                };
                match idx.and_then(|n| args.get(n)) {
                    Some(a) => {
                        use std::fmt::Write;
                        let _ = write!(out, "{a}");
                    }
                    None => {
                        out.push('{');
                        out.push_str(spec);
                        out.push('}');
                    }
                }
                // consome até ao «}»
                for _ in 0..spec.chars().count() + 1 {
                    chars.next();
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// `tr!("{} por ler, a última de {}", n, app)` → `String`. A chave tem de ser
/// um literal (o teste de completude extrai-as do código-fonte).
#[macro_export]
macro_rules! tr {
    ($key:literal $(, $arg:expr)* $(,)?) => {
        $crate::i18n::render(
            $crate::i18n::t($key),
            &[$(&$arg as &dyn ::std::fmt::Display),*],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn t_returns_pt_when_the_key_is_missing() {
        for l in Lang::ALL {
            assert_eq!(
                t_in(l, "chave que não existe em lado nenhum"),
                "chave que não existe em lado nenhum"
            );
        }
    }

    #[test]
    fn render_handles_sequential_and_indexed_placeholders() {
        assert_eq!(
            render("{} por ler, de {}", &[&3, &"Woo"]),
            "3 por ler, de Woo"
        );
        assert_eq!(render("{1} / {0}", &[&"a", &"b"]), "b / a");
        assert_eq!(render("{0} e {0}", &[&"x"]), "x e x");
        assert_eq!(render("{{ {} }}", &[&1]), "{ 1 }");
        assert_eq!(render("{} {}", &[&1]), "1 {}");
    }

    #[test]
    fn tr_macro_formats_with_the_key_as_fallback() {
        set(Lang::PtPt);
        assert_eq!(tr!("{} aparelhos", 4), "4 aparelhos");
        assert_eq!(tr!("sem argumentos"), "sem argumentos");
    }

    #[test]
    fn detect_reads_the_locale_variables_in_order() {
        assert_eq!(detect_with(env(&[("LANG", "pt_PT.UTF-8")])), Lang::PtPt);
        assert_eq!(detect_with(env(&[("LANG", "pt_BR.UTF-8")])), Lang::PtBr);
        assert_eq!(detect_with(env(&[("LANG", "pt")])), Lang::PtBr);
        assert_eq!(detect_with(env(&[("LANG", "en_GB.UTF-8")])), Lang::EnGb);
        assert_eq!(detect_with(env(&[("LANG", "es_ES.UTF-8")])), Lang::EsEs);
        assert_eq!(detect_with(env(&[("LANG", "zh_CN.UTF-8")])), Lang::Zh);
        assert_eq!(detect_with(env(&[("LANG", "de_DE.UTF-8")])), Lang::EnGb);
        assert_eq!(detect_with(env(&[])), Lang::EnGb);
        // LC_ALL ganha a LC_MESSAGES e a LANG; vazio salta para a seguinte.
        assert_eq!(
            detect_with(env(&[("LC_ALL", "es_ES"), ("LANG", "pt_PT")])),
            Lang::EsEs
        );
        assert_eq!(
            detect_with(env(&[("LC_ALL", ""), ("LANG", "zh_CN")])),
            Lang::Zh
        );
    }

    #[test]
    fn parse_accepts_the_cli_codes() {
        for l in Lang::ALL {
            assert_eq!(Lang::parse(l.code()), Some(l));
        }
        assert_eq!(Lang::parse("xx"), None);
    }
}
