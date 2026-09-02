//! Identidade criptográfica do daemon: certificado ECDSA P-256 autoassinado,
//! persistido em `~/.config/hyprlink/` para não mudar de fingerprint a cada boot
//! (o app Android confia no fingerprint via pin manual, não numa CA).

use std::path::PathBuf;

use rcgen::{generate_simple_self_signed, CertifiedKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

pub struct ServerIdentity {
    pub cert_der: CertificateDer<'static>,
    pub key_der: PrivateKeyDer<'static>,
    pub fingerprint_hex: String,
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .expect("sem diretório de config do usuário (XDG_CONFIG_HOME/HOME)")
        .join("hyprlink")
}

fn fingerprint_of(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Carrega a identidade salva em disco, ou gera uma nova (e salva) na primeira execução.
pub fn load_or_generate() -> std::io::Result<ServerIdentity> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let cert_path = dir.join("cert.der");
    let key_path = dir.join("key.der");

    if cert_path.exists() && key_path.exists() {
        let cert_bytes = std::fs::read(&cert_path)?;
        let key_bytes = std::fs::read(&key_path)?;
        let fingerprint_hex = fingerprint_of(&cert_bytes);
        let cert_der = CertificateDer::from(cert_bytes);
        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_bytes));
        return Ok(ServerIdentity {
            cert_der,
            key_der,
            fingerprint_hex,
        });
    }

    let CertifiedKey { cert, signing_key } = generate_simple_self_signed(vec![
        "HyprLink-Desktop".to_string(),
    ])
    .expect("falha ao gerar certificado ECDSA P-256 autoassinado");

    let cert_der = cert.der().clone();
    let key_bytes = signing_key.serialize_der();

    std::fs::write(&cert_path, cert_der.as_ref())?;
    std::fs::write(&key_path, &key_bytes)?;

    let fingerprint_hex = fingerprint_of(cert_der.as_ref());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_bytes));

    Ok(ServerIdentity {
        cert_der,
        key_der,
        fingerprint_hex,
    })
}

/// Fingerprint SHA-256 (hex maiúsculo, com `:`) de um certificado de peer em DER cru —
/// usado tanto para o certificado do próprio servidor quanto para o pin do cliente.
pub fn fingerprint_der(der: &[u8]) -> String {
    fingerprint_of(der)
}
