//! Servidor QUIC: aceita conexões mTLS, valida o pareamento por fingerprint e
//! despacha os pacotes de controlo (Fase 1: só `core.*` — os outros módulos
//! chegam nas fases seguintes do plano).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use ciborium::Value;
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::CertificateDer;

use crate::identity::{fingerprint_der, ServerIdentity};
use crate::pairing::PairingStore;
use crate::protocol::{body_get_bytes, body_get_str, read_frame, write_frame, Packet};
use crate::state::{self, HudState};
use crate::tls_verifier::AcceptAnyClientCert;

pub fn build_endpoint(identity: &ServerIdentity, addr: SocketAddr) -> anyhow::Result<quinn::Endpoint> {
    let provider = rustls::crypto::ring::default_provider();
    let _ = provider.clone().install_default(); // ok se outro código já instalou

    let verifier = AcceptAnyClientCert::new(&provider);

    let mut tls_config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3 é sempre suportado pelo provider ring")
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![identity.cert_der.clone()], identity.key_der.clone_key())?;
    tls_config.alpn_protocols = vec![b"hyprlink/1".to_vec()];
    tls_config.max_early_data_size = u32::MAX;

    let quic_tls_config = QuicServerConfig::try_from(tls_config)?;
    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_tls_config));
    if let Some(transport) = Arc::get_mut(&mut server_config.transport) {
        transport
            .max_concurrent_bidi_streams(100u32.into())
            .max_concurrent_uni_streams(100u32.into());
    }

    let endpoint = quinn::Endpoint::server(server_config, addr)?;
    Ok(endpoint)
}

pub async fn run(endpoint: quinn::Endpoint, pairing: Arc<Mutex<PairingStore>>, hud: Arc<Mutex<HudState>>) {
    while let Some(incoming) = endpoint.accept().await {
        let pairing = pairing.clone();
        let hud = hud.clone();
        state::set_connecting(&hud);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(incoming, pairing, hud.clone()).await {
                state::push_log(&hud, format!("[!] conexão encerrada: {e}"));
                state::set_pairing(&hud);
            }
        });
    }
}

async fn handle_connection(
    incoming: quinn::Incoming,
    pairing: Arc<Mutex<PairingStore>>,
    hud: Arc<Mutex<HudState>>,
) -> anyhow::Result<()> {
    let connection = incoming.await?;
    let peer_fingerprint = peer_cert_fingerprint(&connection)
        .ok_or_else(|| anyhow::anyhow!("cliente não apresentou certificado (mTLS obrigatório)"))?;

    println!("[+] handshake QUIC/mTLS completo, fingerprint do peer: {peer_fingerprint}");

    // O primeiro stream bidirecional deve trazer o core.hello — é ali que a
    // autenticação por pareamento acontece antes de qualquer outro pacote.
    let (mut send, mut recv) = connection.accept_bi().await?;
    let frame = read_frame(&mut recv).await?;
    let hello = Packet::decode(&frame)?;
    if hello.kind != "core.hello" {
        anyhow::bail!("primeiro pacote não foi core.hello (recebido: {})", hello.kind);
    }

    let device_name = hello
        .body
        .as_ref()
        .and_then(|b| body_get_str(b, "device_name"))
        .unwrap_or("Desconhecido")
        .to_string();
    let token_hex = hello
        .body
        .as_ref()
        .and_then(|b| body_get_bytes(b, "pairing_token"))
        .map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());

    let authorized = {
        let mut store = pairing.lock().unwrap();
        if store.is_paired(&peer_fingerprint) {
            true
        } else if let Some(token_hex) = &token_hex {
            store.try_pair_with_token(&peer_fingerprint, token_hex)?
        } else {
            false
        }
    };

    if !authorized {
        anyhow::bail!("dispositivo não pareado e sem pairing_token válido — conexão rejeitada");
    }

    println!("[+] core.hello de '{device_name}' — dispositivo autorizado");
    state::set_connected(&hud, device_name.clone(), peer_fingerprint.clone());
    state::push_log(&hud, format!("[+] core.hello autorizado · {device_name}"));

    let reply_body = Value::Map(vec![(
        Value::Text("device_name".into()),
        Value::Text(hostname()),
    )]);
    let reply = Packet::new(hello.id, "core.hello", Some(reply_body), false);
    write_frame(&mut send, &reply.encode()).await?;
    send.finish()?;

    // Streams seguintes (incluindo mais chamadas na mesma stream de controlo,
    // se o app reabrir uma nova) são despachadas por tipo.
    loop {
        let (send, recv) = match connection.accept_bi().await {
            Ok(pair) => pair,
            Err(_) => break, // conexão fechada pelo peer
        };
        tokio::spawn(handle_control_stream(send, recv, hud.clone()));
    }

    state::push_log(&hud, format!("[!] {device_name} desconectou"));
    state::set_pairing(&hud);
    Ok(())
}

async fn handle_control_stream(mut send: quinn::SendStream, mut recv: quinn::RecvStream, hud: Arc<Mutex<HudState>>) {
    let frame = match read_frame(&mut recv).await {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[stream] erro lendo frame: {e}");
            return;
        }
    };
    let packet = match Packet::decode(&frame) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[stream] erro decodificando CBOR: {e}");
            return;
        }
    };

    match packet.kind.as_str() {
        "core.ping" => {
            let reply = Packet::new(packet.id, "core.pong", None, false);
            let _ = write_frame(&mut send, &reply.encode()).await;
            state::push_log(&hud, "[i] core.ping".to_string());
        }
        other => {
            state::push_log(&hud, format!("[!] tipo ainda não implementado: {other}"));
        }
    }
    // Fecha a stream de escrita prontamente — o app bloqueia lendo até EOF
    // nas respostas tipadas (ver PROTOCOL.md §4).
    let _ = send.finish();
}

fn peer_cert_fingerprint(connection: &quinn::Connection) -> Option<String> {
    let chain = connection
        .peer_identity()?
        .downcast::<Vec<CertificateDer<'static>>>()
        .ok()?;
    let leaf = chain.first()?;
    Some(fingerprint_der(leaf.as_ref()))
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .unwrap_or_else(|| "HyprLink-Desktop".to_string())
}
