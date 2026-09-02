mod identity;
mod pairing;
mod protocol;
mod server;
mod tls_verifier;

use std::sync::{Arc, Mutex};

use qrcode::render::unicode;
use qrcode::QrCode;

const PORT: u16 = 7443;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let identity = identity::load_or_generate()?;
    let pairing = Arc::new(Mutex::new(pairing::PairingStore::load()?));

    let local_ip = local_ip_guess();
    let addr: std::net::SocketAddr = format!("0.0.0.0:{PORT}").parse()?;
    let endpoint = server::build_endpoint(&identity, addr)?;

    let token_hex = pairing.lock().unwrap().current_token_hex.clone();
    let qr_payload = format!("{}|{}:{}|{}", identity.fingerprint_hex, local_ip, PORT, token_hex);

    println!("{}", "=".repeat(60));
    println!("           HYPRLINK DESKTOP DAEMON (Rust)");
    println!("{}", "=".repeat(60));
    println!("IP Local:     {local_ip}:{PORT}");
    println!("Fingerprint:  {}", identity.fingerprint_hex);
    println!("Token:        {token_hex}");
    println!("{}", "=".repeat(60));

    let code = QrCode::new(qr_payload.as_bytes())?;
    let qr_ascii = code
        .render::<unicode::Dense1x2>()
        .quiet_zone(true)
        .build();
    println!("{qr_ascii}");
    println!("\n[+] Aponte a câmara do app HyprLink para o QR Code acima para emparelhar.\n");

    server::run(endpoint, pairing).await;
    Ok(())
}

fn local_ip_guess() -> std::net::IpAddr {
    // Mesma técnica do daemon Python: abre um socket UDP "fake" pra descobrir
    // qual interface local o SO escolheria pra sair, sem enviar nada de fato.
    use std::net::UdpSocket;
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("10.255.255.255:1")?;
            s.local_addr()
        })
        .map(|addr| addr.ip())
        .unwrap_or_else(|_| std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
}
