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
mod gate;
mod gesture;
mod hub;
mod hypr;
mod identity;
mod input;
mod ipc;
mod media;
mod mic;
mod notif;
mod open_url;
mod pairing;
mod phone;
mod phone_audio;
mod protocol;
mod server;
mod session;
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
    // Receções que o daemon deixou a meio (crash/kill): o parcial nunca
    // chega a ter o nome final; apaga os que já têm mais de 1 h.
    let orphans = share::clean_orphan_parts(
        &config::download_dir(&config),
        share::ORPHAN_PART_AGE,
        std::time::SystemTime::now(),
    );
    if orphans > 0 {
        state::push_log(
            &hud,
            format!("[i] apagados {orphans} ficheiros parciais órfãos"),
        );
    }

    {
        use std::io::IsTerminal;
        let tty = std::io::stdout().is_terminal();
        print!(
            "{}",
            terminal_banner(&identity.fingerprint_hex, local_ip, &token_hex, tty)?
        );
    }

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

/// Cabeçalho do arranque. O token e o QR (que o contém) só saem quando o
/// stdout é um terminal: sem TTY o stdout costuma ir para um ficheiro
/// (`hyprlink-start.sh` → `daemon.log`) e o segredo ficaria em disco. Nesse
/// caso só se indica onde ver o QR (GUI/hyprlinkctl, pelo socket local).
fn terminal_banner(
    fingerprint_hex: &str,
    local_ip: std::net::IpAddr,
    token_hex: &str,
    tty: bool,
) -> anyhow::Result<String> {
    use std::fmt::Write as _;
    let rule = "=".repeat(60);
    let mut out = String::new();
    writeln!(out, "{rule}")?;
    writeln!(out, "           HYPRLINK DESKTOP DAEMON (Rust)")?;
    writeln!(out, "{rule}")?;
    writeln!(out, "IP Local:     {local_ip}:{PORT}")?;
    writeln!(out, "Fingerprint:  {fingerprint_hex}")?;
    if !tty {
        writeln!(out, "{rule}")?;
        writeln!(
            out,
            "[+] emparelhamento: abre a GUI (hyprlink-gui) ou `hyprlinkctl pair` para ver o QR"
        )?;
        return Ok(out);
    }
    writeln!(out, "Token:        {token_hex}")?;
    writeln!(out, "{rule}")?;

    let qr_payload = format!("{fingerprint_hex}|{local_ip}:{PORT}|{token_hex}");
    let code = QrCode::new(qr_payload.as_bytes())?;
    let qr_ascii = code.render::<unicode::Dense1x2>().quiet_zone(true).build();
    writeln!(out, "{qr_ascii}")?;
    writeln!(
        out,
        "\n[+] Aponte a câmara do app HyprLink para o QR Code acima, ou abra a GUI: hyprlink-gui\n"
    )?;
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "ab12cd34";
    const TOKEN: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn sem_tty_o_banner_nao_tem_token_nem_qr() {
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 2));
        let out = terminal_banner(FP, ip, TOKEN, false).unwrap();
        assert!(!out.contains(TOKEN), "token no stdout sem TTY");
        assert!(!out.contains("Token:"));
        assert!(!out.contains('█') && !out.contains('▀') && !out.contains('▄'));
        assert!(out.contains("hyprlinkctl pair"));
        assert!(out.contains(FP));
    }

    #[test]
    fn com_tty_mostra_token_e_qr() {
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let out = terminal_banner(FP, ip, TOKEN, true).unwrap();
        assert!(out.contains(TOKEN));
        assert!(out.contains('█') || out.contains('▀') || out.contains('▄'));
    }
}
