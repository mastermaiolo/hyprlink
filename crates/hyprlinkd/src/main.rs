mod action;
mod active;
mod art;
mod audio;
mod battery;
mod bridge;
mod clip;
mod config;
mod ctl;
mod envinfo;
mod gesture;
mod hub;
mod hypr;
mod identity;
mod input;
mod ipc;
mod media;
mod mic;
mod notif;
mod pairing;
mod phone;
mod phone_audio;
mod protocol;
mod server;
mod share;
mod shortcuts;
mod speaker;
mod state;
mod tap;
mod telemetry;
mod tls_verifier;
mod webcam;

use std::sync::{Arc, Mutex};

use qrcode::QrCode;
use qrcode::render::unicode;

const PORT: u16 = 7443;

fn main() -> anyhow::Result<()> {
    let identity = identity::load_or_generate()?;
    let pairing = Arc::new(Mutex::new(pairing::PairingStore::load()?));
    let local_ip = local_ip_guess();

    let token_hex = pairing.lock().unwrap().current_token_hex.clone();
    let hud = state::HudState::new(token_hex.clone());
    let config = config::load();
    // Ambiente detetado (shell, Hyprland, GPU, áudio…) e opções de correção do
    // config.json — o mesmo código do `hyprlinkctl doctor`.
    for line in envinfo::init(&config) {
        state::push_log(&hud, line);
    }
    // Sessão anterior morreu com o modo coluna ativo? Devolve o som do PC
    // ao sink original antes de qualquer outra coisa — sem isto, o PC
    // ficaria mudo (apps a tocar no sink virtual que já ninguém consome).
    speaker::cleanup_orphans(&config);

    print_terminal_qr(&identity.fingerprint_hex, local_ip, &token_hex)?;

    // O daemon não tem janela: a interface é o hyprlink-gui (e o
    // hyprlinkctl), que falam com ele pelo socket local da bridge.
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_daemon(identity, pairing, hud, config, local_ip));
    Ok(())
}

async fn run_daemon(
    identity: identity::ServerIdentity,
    pairing: Arc<Mutex<pairing::PairingStore>>,
    hud: Arc<Mutex<state::HudState>>,
    config: config::SharedConfig,
    local_ip: std::net::IpAddr,
) {
    let active = active::new_registry();
    let pending_webcam = webcam::new_pending();
    let tap_handle = tap::new_handle();
    let speaker_handle = speaker::new_handle();

    let addr: std::net::SocketAddr = format!("0.0.0.0:{PORT}")
        .parse()
        .expect("porta fixa válida");
    let endpoint = match server::build_endpoint(&identity, addr) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[fatal] não foi possível subir o servidor QUIC: {e}");
            return;
        }
    };
    // Uma sessão D-Bus só, reusada por notif.rs/media.rs (ver `Ctx::dbus`) —
    // criada aqui (não em Ctx::new, que é síncrona) porque conectar é async.
    let dbus = zbus::Connection::session().await.ok();
    let ctx = server::Ctx::new(
        hud,
        config,
        active,
        pending_webcam,
        dbus,
        tap_handle,
        speaker_handle,
    );
    server::spawn_background_tasks(ctx.clone());
    // Socket local para a GUI nova e o hyprlinkctl (hyprlink-proto).
    bridge::spawn(
        ctx.clone(),
        pairing.clone(),
        identity.fingerprint_hex.clone(),
        format!("{local_ip}:{PORT}"),
    )
    .await;
    server::run(endpoint, pairing, ctx).await;
}

fn print_terminal_qr(
    fingerprint_hex: &str,
    local_ip: std::net::IpAddr,
    token_hex: &str,
) -> anyhow::Result<()> {
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
    println!(
        "\n[+] Aponte a câmara do app HyprLink para o QR Code acima, ou abra a GUI: hyprlink-gui\n"
    );
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
