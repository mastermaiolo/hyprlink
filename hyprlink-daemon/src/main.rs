mod active;
mod audio;
mod battery;
mod clip;
mod config;
mod gui;
mod hypr;
mod identity;
mod input;
mod media;
mod notif;
mod pairing;
mod protocol;
mod server;
mod share;
mod state;
mod tap;
mod tls_verifier;
mod webcam;

use std::sync::{Arc, Mutex};

use qrcode::render::unicode;
use qrcode::QrCode;

const PORT: u16 = 7443;

fn main() -> anyhow::Result<()> {
    let identity = identity::load_or_generate()?;
    let pairing = Arc::new(Mutex::new(pairing::PairingStore::load()?));
    let local_ip = local_ip_guess();

    let token_hex = pairing.lock().unwrap().current_token_hex.clone();
    let hud = state::HudState::new(format!("{local_ip}:{PORT}"), identity.fingerprint_hex.clone(), token_hex.clone());
    let config = config::load();
    // Compartilhado com a GUI só pra permitir um `close()` educado da conexão
    // QUIC (frame CONNECTION_CLOSE de verdade) no botão de fechar — sem isso
    // o telemóvel fica "conectado" até o idle timeout expirar sozinho.
    let active = active::new_registry();
    // Compartilhado com a GUI pra ela poder pedir "iniciar/parar stream" no
    // ecrã da webcam sem precisar de todo o `Ctx` do servidor.
    let pending_webcam = webcam::new_pending();

    print_terminal_qr(&identity.fingerprint_hex, local_ip, &token_hex)?;

    {
        let hud = hud.clone();
        let config = config.clone();
        let active = active.clone();
        let pending_webcam = pending_webcam.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("falha ao criar runtime tokio");
            rt.block_on(run_daemon(identity, pairing, hud, config, active, pending_webcam, local_ip));
        });
    }

    gui::run(hud, config, active, pending_webcam).map_err(|e| anyhow::anyhow!("erro na GUI: {e}"))
}

async fn run_daemon(
    identity: identity::ServerIdentity,
    pairing: Arc<Mutex<pairing::PairingStore>>,
    hud: Arc<Mutex<state::HudState>>,
    config: config::SharedConfig,
    active: active::ActiveConn,
    pending_webcam: webcam::PendingWebcam,
    local_ip: std::net::IpAddr,
) {
    let addr: std::net::SocketAddr = format!("0.0.0.0:{PORT}").parse().expect("porta fixa válida");
    let _ = local_ip;
    let endpoint = match server::build_endpoint(&identity, addr) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[fatal] não foi possível subir o servidor QUIC: {e}");
            return;
        }
    };
    let ctx = server::Ctx::new(hud, config, active, pending_webcam);
    server::spawn_background_tasks(ctx.clone());
    server::run(endpoint, pairing, ctx).await;
}

fn print_terminal_qr(fingerprint_hex: &str, local_ip: std::net::IpAddr, token_hex: &str) -> anyhow::Result<()> {
    let qr_payload = format!("{fingerprint_hex}|{local_ip}:{PORT}|{token_hex}");

    println!("{}", "=".repeat(60));
    println!("           HYPRLINK DESKTOP DAEMON (Rust)");
    println!("{}", "=".repeat(60));
    println!("IP Local:     {local_ip}:{PORT}");
    println!("Fingerprint:  {fingerprint_hex}");
    println!("Token:        {token_hex}");
    println!("{}", "=".repeat(60));

    let code = QrCode::new(qr_payload.as_bytes())?;
    let qr_ascii = code.render::<unicode::Dense1x2>().quiet_zone(true).build();
    println!("{qr_ascii}");
    println!("\n[+] Aponte a câmara do app HyprLink para o QR Code acima, ou use a GUI que vai abrir.\n");
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
