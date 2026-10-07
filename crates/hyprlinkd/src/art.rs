//! Capa do álbum para o `media.state`: lê o `mpris:artUrl` (`file://` ou
//! `https://`), reduz para caber em 320×320 e recodifica em JPEG ≤ 96 KiB.
//! Só se envia o JPEG recodificado, nunca os bytes originais.
//!
//! Tudo o que é rede/descodificação é bloqueante e corre em `spawn_blocking`;
//! qualquer erro dá `None` (nunca *panic*, nunca prende o poll de 2 s).

use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};

/// Lado máximo da capa enviada.
pub const MAX_SIDE: u32 = 320;
/// Teto do JPEG enviado.
pub const MAX_JPEG: usize = 96 * 1024;
/// Teto de um `file://`.
const MAX_FILE: u64 = 8 * 1024 * 1024;
/// Teto de um download `https://`.
const MAX_HTTP: u64 = 5 * 1024 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
/// Qualidades tentadas, da melhor para a pior, até caber em `MAX_JPEG`.
const QUALITIES: [u8; 6] = [80, 70, 60, 50, 40, 30];

#[derive(Debug, Clone, PartialEq)]
pub struct Art {
    /// Hash curto (12 hex) do URL + tamanho do JPEG.
    pub id: String,
    pub jpeg: Vec<u8>,
}

/// Última capa carregada, por URL. Guarda também a falha (`None`) para não a
/// repetir a cada poll enquanto o `artUrl` for o mesmo.
pub type Cache = Mutex<Option<(String, Option<Arc<Art>>)>>;

/// Resultado em cache para `url`, se for o último URL tratado.
pub fn peek(cache: &Cache, url: &str) -> Option<Option<Arc<Art>>> {
    match &*cache.lock().unwrap() {
        Some((u, a)) if u == url => Some(a.clone()),
        _ => None,
    }
}

/// Como `load_art`, mas só faz o trabalho se `url` não for o último tratado.
pub async fn cached_art(cache: &Cache, url: &str) -> Option<Arc<Art>> {
    cached_with(cache, url, |u| async move { load_art(&u).await }).await
}

async fn cached_with<F, Fut>(cache: &Cache, url: &str, load: F) -> Option<Arc<Art>>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Option<Art>>,
{
    if let Some(hit) = peek(cache, url) {
        return hit;
    }
    let art = load(url.to_string()).await.map(Arc::new);
    *cache.lock().unwrap() = Some((url.to_string(), art.clone()));
    art
}

pub async fn load_art(url: &str) -> Option<Art> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || load_art_blocking(&url))
        .await
        .ok()
        .flatten()
}

fn load_art_blocking(url: &str) -> Option<Art> {
    let raw = if let Some(path) = url.strip_prefix("file://") {
        read_file(path)?
    } else if url.starts_with("https://") {
        fetch_https(url)?
    } else {
        eprintln!("[art] esquema não suportado: {}", scheme_of(url));
        return None;
    };
    let jpeg = recode(&raw)?;
    Some(Art {
        id: art_id(url, jpeg.len()),
        jpeg,
    })
}

fn scheme_of(url: &str) -> &str {
    url.split_once(':').map_or("?", |(s, _)| s)
}

/// `hex(sha256(url ‖ tamanho))[..12]`.
fn art_id(url: &str, jpeg_len: usize) -> String {
    let mut h = Sha256::new();
    h.update(url.as_bytes());
    h.update((jpeg_len as u64).to_be_bytes());
    h.finalize()
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/// `file:///caminho` (já sem o prefixo `file://`): só ficheiros regulares até 8 MiB.
fn read_file(path_part: &str) -> Option<Vec<u8>> {
    // `file://localhost/x` também é válido.
    let path_part = path_part.strip_prefix("localhost").unwrap_or(path_part);
    if !path_part.starts_with('/') {
        return None;
    }
    let bytes = percent_decode(path_part)?;
    let path = std::path::PathBuf::from(std::ffi::OsString::from(
        String::from_utf8(bytes).ok()?,
    ));
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    // `take` fecha a janela entre o `metadata` e a leitura (ficheiro a crescer).
    let mut buf = Vec::new();
    std::fs::File::open(&path)
        .ok()?
        .take(MAX_FILE + 1)
        .read_to_end(&mut buf)
        .ok()?;
    (buf.len() as u64 <= MAX_FILE).then_some(buf)
}

/// Endereço que um `artUrl` pode servir: nada de loopback, rede privada,
/// *link-local*, CGNAT, multicast ou não especificado.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => is_public_v4(a),
        IpAddr::V6(a) => match a.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(a),
        },
    }
}

fn is_public_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_unspecified()
        || a.is_broadcast()
        || a.is_multicast()
        || a.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64/10 (CGNAT)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
        || (o[0] == 198 && (o[1] & 0xfe) == 18)) // 198.18/15
}

fn is_public_v6(a: Ipv6Addr) -> bool {
    let s = a.segments();
    !(a.is_loopback()
        || a.is_unspecified()
        || a.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // fc00::/7 (ULA)
        || (s[0] & 0xffc0) == 0xfe80) // fe80::/10
}

/// Resolve com o resolvedor normal e **descarta** o que não for público. Corre
/// em cada ligação (também nos redirecionamentos), por isso um nome que
/// aponte para a rede local não passa mesmo que o primeiro salto pareça limpo.
#[derive(Debug, Default)]
struct PublicOnly(ureq::unversioned::resolver::DefaultResolver);

impl ureq::unversioned::resolver::Resolver for PublicOnly {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let all = self.0.resolve(uri, config, timeout)?;
        let mut out = self.empty();
        for a in all.iter().filter(|a| is_public(a.ip())) {
            out.push(*a);
        }
        if out.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(out)
    }
}

fn fetch_https(url: &str) -> Option<Vec<u8>> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        // `https_only` também recusa um redirecionamento para `http://`.
        .https_only(true)
        .max_redirects(3)
        .proxy(None)
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        ureq::unversioned::transport::DefaultConnector::default(),
        PublicOnly::default(),
    );
    let resp = match agent.get(url).call() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[art] pedido falhou: {e}");
            return None;
        }
    };
    let is_image = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().to_ascii_lowercase().starts_with("image/"));
    if !is_image {
        eprintln!("[art] resposta que não é image/*");
        return None;
    }
    resp.into_body()
        .with_config()
        .limit(MAX_HTTP)
        .read_to_vec()
        .ok()
}

/// Descodifica (só JPEG/PNG), reduz para caber em 320×320 e recodifica em JPEG.
fn recode(raw: &[u8]) -> Option<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(raw))
        .with_guessed_format()
        .ok()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Jpeg) | Some(ImageFormat::Png)
    ) {
        return None;
    }
    // Contra bombas de descompressão: o tamanho em disco já é limitado, o
    // descodificado ainda não.
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, FilterType::Lanczos3)
    } else {
        img
    };
    // JPEG não tem alfa.
    let rgb = img.to_rgb8();
    for q in QUALITIES {
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, q)
            .encode_image(&rgb)
            .ok()?;
        if out.len() <= MAX_JPEG {
            return Some(out);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Imagem com ruído determinístico (não comprime bem) de `w`×`h`.
    fn noisy(w: u32, h: u32) -> DynamicImage {
        let mut x = 0x1234_5678u32;
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |_, _| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            Rgb([x as u8, (x >> 8) as u8, (x >> 16) as u8])
        }))
    }

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hyprlink-art-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn jpeg_dims(b: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory_with_format(b, ImageFormat::Jpeg).unwrap();
        (img.width(), img.height())
    }

    #[tokio::test]
    async fn file_png_e_jpeg_dao_jpeg_pequeno() {
        let dir = tmpdir("ok");
        let png = dir.join("capa grande.png"); // espaço → %20 no URL
        let jpg = dir.join("capa.jpg");
        noisy(1000, 600).save(&png).unwrap();
        noisy(900, 900).save(&jpg).unwrap();

        let a = load_art(&format!("file://{}", png.display()).replace(' ', "%20"))
            .await
            .expect("png");
        assert!(a.jpeg.len() <= MAX_JPEG);
        let (w, h) = jpeg_dims(&a.jpeg);
        assert!(w <= 320 && h <= 320 && w == 320, "{w}x{h}");
        assert_eq!(h, 192, "mantém a proporção 1000x600");
        assert_eq!(a.id.len(), 12);

        let b = load_art(&format!("file://{}", jpg.display()))
            .await
            .expect("jpeg");
        assert!(b.jpeg.len() <= MAX_JPEG);
        assert_eq!(jpeg_dims(&b.jpeg), (320, 320));
        assert_ne!(a.id, b.id);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn imagem_pequena_nao_e_ampliada() {
        let dir = tmpdir("small");
        let f = dir.join("p.png");
        noisy(100, 80).save(&f).unwrap();
        let a = load_art(&format!("file://{}", f.display())).await.unwrap();
        assert_eq!(jpeg_dims(&a.jpeg), (100, 80));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn entradas_invalidas_dao_none() {
        let dir = tmpdir("bad");
        let lixo = dir.join("lixo.png");
        std::fs::write(&lixo, b"isto nao e uma imagem").unwrap();
        let grande = dir.join("grande.png");
        std::fs::write(&grande, vec![0u8; (MAX_FILE + 1) as usize]).unwrap();

        for url in [
            "".to_string(),
            "nao e um url".to_string(),
            "file:///nao/existe/capa.jpg".to_string(),
            format!("file://{}", dir.display()), // pasta, não ficheiro regular
            format!("file://{}", lixo.display()), // não descodifica
            format!("file://{}", grande.display()), // > 8 MiB
            "file://relativo/capa.jpg".to_string(),
            "http://example.com/capa.jpg".to_string(),
            "ftp://example.com/capa.jpg".to_string(),
        ] {
            assert!(load_art(&url).await.is_none(), "devia ser None: {url}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn https_para_rede_local_e_recusado() {
        // Sem rede: o resolvedor rejeita antes de ligar.
        for url in [
            "https://127.0.0.1/c.jpg",
            "https://localhost/c.jpg",
            "https://10.0.0.1/c.jpg",
            "https://192.168.1.1/c.jpg",
            "https://[::1]/c.jpg",
        ] {
            assert!(load_art(url).await.is_none(), "{url}");
        }
    }

    #[test]
    fn enderecos_publicos_e_privados() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:192.168.0.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn cache_nao_repete_o_trabalho_para_o_mesmo_url() {
        let cache: Cache = Mutex::new(None);
        let calls = AtomicUsize::new(0);
        let load = |ok: bool| {
            let calls = &calls;
            move |_u: String| async move {
                calls.fetch_add(1, Ordering::SeqCst);
                ok.then(|| Art {
                    id: "x".into(),
                    jpeg: vec![1, 2, 3],
                })
            }
        };
        let a = cached_with(&cache, "file:///a.jpg", load(true)).await;
        let b = cached_with(&cache, "file:///a.jpg", load(true)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(a, b);
        assert!(peek(&cache, "file:///a.jpg").is_some());
        assert!(peek(&cache, "file:///b.jpg").is_none());

        // URL novo → trabalha de novo; a falha também fica em cache.
        assert!(cached_with(&cache, "file:///b.jpg", load(false)).await.is_none());
        assert!(cached_with(&cache, "file:///b.jpg", load(false)).await.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
