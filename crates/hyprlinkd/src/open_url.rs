//! Abrir URLs no PC com `xdg-open` — só `http`/`https`.
//!
//! O URL pode vir do telemóvel (`share.url`) ou do socket local
//! (`hyprlinkctl url`). Entregar texto arbitrário ao `xdg-open` abria
//! `file://`, `smb://`, caminhos locais ou esquemas de apps instaladas; por
//! isso tudo passa por [`safe_open_url`], que valida com a crate `url`.

use std::fmt;

/// Comprimento máximo aceite (bytes).
const MAX_URL_BYTES: usize = 4096;

/// Porque é que um URL foi recusado. O `Display` é curto e nunca inclui o
/// URL completo (pode trazer dados pessoais ou tokens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Motivo {
    Vazio,
    Longo,
    /// Espaços, quebras de linha ou outros caracteres de controlo.
    Controlo,
    /// Começa por `-`: o `xdg-open` lia-o como opção.
    Opcao,
    /// Não é um URL absoluto (ex.: `/etc/passwd`).
    Malformado,
    /// Esquema fora da lista (truncado a 32 caracteres).
    Esquema(String),
    SemAnfitriao,
    /// O `xdg-open` não arrancou.
    Arranque,
}

impl fmt::Display for Motivo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Motivo::Vazio => write!(f, "vazio"),
            Motivo::Longo => write!(f, "mais de {MAX_URL_BYTES} bytes"),
            Motivo::Controlo => write!(f, "espaços ou caracteres de controlo"),
            Motivo::Opcao => write!(f, "começa por «-»"),
            Motivo::Malformado => write!(f, "malformado"),
            Motivo::Esquema(s) => write!(f, "esquema {s}"),
            Motivo::SemAnfitriao => write!(f, "sem anfitrião"),
            Motivo::Arranque => write!(f, "xdg-open não arrancou"),
        }
    }
}

/// Valida sem abrir. Devolve o URL normalizado pelo parser.
pub fn check_url(raw: &str) -> Result<url::Url, Motivo> {
    if raw.is_empty() {
        return Err(Motivo::Vazio);
    }
    if raw.len() > MAX_URL_BYTES {
        return Err(Motivo::Longo);
    }
    if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(Motivo::Controlo);
    }
    if raw.starts_with('-') {
        return Err(Motivo::Opcao);
    }
    let parsed = url::Url::parse(raw).map_err(|_| Motivo::Malformado)?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(Motivo::Esquema(other.chars().take(32).collect())),
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(Motivo::SemAnfitriao);
    }
    Ok(parsed)
}

/// Valida e abre com o `xdg-open` (sem esperar). Chamar dentro do runtime
/// tokio: o processo filho é recolhido em segundo plano (sem zombies).
pub fn safe_open_url(raw: &str) -> Result<(), Motivo> {
    let parsed = check_url(raw)?;
    // O serializado de um Url http(s) começa sempre por "http" — nunca por
    // "-" — mas fica a verificação explícita, por ser barata.
    let s = parsed.as_str();
    if s.starts_with('-') {
        return Err(Motivo::Opcao);
    }
    crate::session::tokio_command("xdg-open")
        .arg(s)
        .spawn()
        .map(drop)
        .map_err(|_| Motivo::Arranque)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aceita_http_e_https() {
        for bom in [
            "https://exemplo.pt/a?b=c#d",
            "http://192.168.1.2:8080",
            "https://[::1]:8443/x",
            "HTTPS://Exemplo.PT",
        ] {
            assert!(check_url(bom).is_ok(), "devia aceitar {bom:?}");
        }
    }

    #[test]
    fn recusa_esquemas_caminhos_e_controlos() {
        let casos: [(&str, Motivo); 12] = [
            ("file:///etc/passwd", Motivo::Esquema("file".into())),
            ("javascript:alert(1)", Motivo::Esquema("javascript".into())),
            ("smb://servidor/partilha", Motivo::Esquema("smb".into())),
            ("data:text/html,oi", Motivo::Esquema("data".into())),
            ("mailto:a@b.pt", Motivo::Esquema("mailto".into())),
            ("steam://run/1", Motivo::Esquema("steam".into())),
            ("/etc/passwd", Motivo::Malformado),
            ("-x", Motivo::Opcao),
            ("--help", Motivo::Opcao),
            ("https://a.pt/\nfile:///x", Motivo::Controlo),
            ("https://a.pt/ b", Motivo::Controlo),
            ("", Motivo::Vazio),
        ];
        for (mau, motivo) in casos {
            assert_eq!(check_url(mau).unwrap_err(), motivo, "{mau:?}");
        }
        assert_eq!(check_url("https://"), Err(Motivo::Malformado));
        assert!(check_url("http:/a").is_ok_and(|u| u.host_str() == Some("a")));
    }

    #[test]
    fn recusa_longos_e_esquema_truncado_no_motivo() {
        let longo = format!("https://a.pt/{}", "x".repeat(MAX_URL_BYTES));
        assert_eq!(check_url(&longo), Err(Motivo::Longo));
        let esquema = "a".repeat(100);
        let Err(Motivo::Esquema(s)) = check_url(&format!("{esquema}:x")) else {
            panic!("devia recusar pelo esquema");
        };
        assert_eq!(s.len(), 32);
    }
}
