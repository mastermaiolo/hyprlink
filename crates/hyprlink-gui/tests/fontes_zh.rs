//! O subconjunto do Noto Sans CJK SC (`scripts/subset-fontes.sh`) tem de ter
//! todos os caracteres CJK da UI em 中文 — senão aparecem quadrados. Falha
//! quando uma tradução zh nova usa um carácter que ficou de fora: regenerar
//! com `scripts/subset-fontes.sh`.

use hyprlink_gui::i18n::{self, Lang};

const FONTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts");
const CJK: [&str; 2] = ["NotoSansSC-Regular.otf", "NotoSansSC-Bold.otf"];
/// Já hoje sem glifo em nenhuma fonte embutida (vem das fontes do sistema,
/// em todas as línguas); não depende deste subconjunto.
const KNOWN_SYSTEM_FALLBACK: &[char] = &['❚'];

fn fonts() -> Vec<(String, Vec<u8>)> {
    std::fs::read_dir(FONTS_DIR)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".otf") || n.ends_with(".ttf"))
        .map(|n| {
            let data = std::fs::read(format!("{FONTS_DIR}/{n}")).unwrap();
            (n, data)
        })
        .collect()
}

fn has(data: &[u8], c: char) -> bool {
    ttf_parser::Face::parse(data, 0)
        .unwrap()
        .glyph_index(c)
        .is_some()
}

fn is_cjk(c: char) -> bool {
    // CJK (inclui a pontuação U+3000–303F), compatibilidade e formas largas.
    matches!(c as u32, 0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

fn zh_texts() -> Vec<&'static str> {
    let mut t: Vec<&str> = i18n::table(Lang::Zh).iter().map(|(_, zh)| *zh).collect();
    // Aparecem em todas as línguas: o seletor de língua e o «＋».
    t.extend([Lang::Zh.label(), "＋"]);
    t
}

/// Os caracteres CJK de `texts` que faltam em `font`.
fn cjk_missing(font: &[u8], texts: &[&str]) -> Vec<char> {
    let mut m: Vec<char> = texts
        .iter()
        .flat_map(|t| t.chars())
        .filter(|c| is_cjk(*c) && !has(font, *c))
        .collect();
    m.sort();
    m.dedup();
    m
}

#[test]
fn subconjunto_cobre_todos_os_caracteres_cjk_da_ui() {
    let all = fonts();
    let texts = zh_texts();
    for name in CJK {
        let (_, data) = all.iter().find(|(n, _)| n == name).unwrap();
        let m = cjk_missing(data, &texts);
        assert!(
            m.is_empty(),
            "{name} sem {m:?}: corre scripts/subset-fontes.sh"
        );
    }
}

#[test]
fn o_resto_tem_glifo_em_alguma_fonte_embutida() {
    let all = fonts();
    let mut m: Vec<char> = zh_texts()
        .iter()
        .flat_map(|t| t.chars())
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .filter(|c| !KNOWN_SYSTEM_FALLBACK.contains(c))
        .filter(|c| !all.iter().any(|(_, d)| has(d, *c)))
        .collect();
    m.sort();
    m.dedup();
    assert!(m.is_empty(), "sem glifo em nenhuma fonte embutida: {m:?}");
}

/// O teste apanha mesmo um carácter em falta (um hanzi raro fora da UI).
#[test]
fn deteta_um_caracter_em_falta() {
    let all = fonts();
    let (_, data) = all.iter().find(|(n, _)| n == CJK[0]).unwrap();
    assert_eq!(cjk_missing(data, &["龘"]), vec!['龘']);
}
