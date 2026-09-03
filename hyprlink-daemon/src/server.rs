//! Servidor QUIC: aceita conexões mTLS, valida o pareamento por fingerprint e
//! despacha os pacotes de controlo. Fase 3: clipboard, hyprland, bateria e
//! mídia — ver PROTOCOL.md para o catálogo completo.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use ciborium::Value;
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::CertificateDer;

use crate::active::{self, ActiveConn};
use crate::clip::{self, LastLocalSet};
use crate::identity::{fingerprint_der, ServerIdentity};
use crate::input::InputDevice;
use crate::notif;
use crate::pairing::PairingStore;
use crate::protocol::{body_get_bytes, body_get_str, read_frame, write_frame, Packet};
use crate::state::{self, HudState};
use crate::tls_verifier::AcceptAnyClientCert;
use crate::{battery, hypr, media};

/// Estado partilhado entre todas as streams/tarefas de fundo de uma sessão.
#[derive(Clone)]
pub struct Ctx {
    pub hud: Arc<Mutex<HudState>>,
    pub active: ActiveConn,
    pub clip_guard: LastLocalSet,
    pub input: Arc<InputDevice>,
    pub notif: notif::Registry,
}

impl Ctx {
    pub fn new(hud: Arc<Mutex<HudState>>) -> Self {
        Self {
            hud,
            active: active::new_registry(),
            clip_guard: clip::new_guard(),
            input: Arc::new(InputDevice::open()),
            notif: notif::new_registry(),
        }
    }
}

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

/// Sobe as tarefas de fundo que empurram eventos pro telemóvel independente
/// de estar ou não conectado no momento (elas checam `ctx.active` sozinhas) —
/// chamado uma vez, na subida do daemon.
pub fn spawn_background_tasks(ctx: Ctx) {
    tokio::spawn(clip::watch(ctx.active.clone(), ctx.clip_guard.clone(), ctx.hud.clone()));
    tokio::spawn(hypr::watch_events(ctx.active.clone(), ctx.hud.clone()));
    tokio::spawn(battery::poll_and_push(ctx.active.clone()));
    tokio::spawn(media::poll_and_push(ctx.active.clone(), ctx.hud.clone()));
    tokio::spawn(notif::watch(ctx.active.clone(), ctx.notif.clone(), ctx.hud.clone()));
}

pub async fn run(endpoint: quinn::Endpoint, pairing: Arc<Mutex<PairingStore>>, ctx: Ctx) {
    while let Some(incoming) = endpoint.accept().await {
        let pairing = pairing.clone();
        let ctx = ctx.clone();
        state::set_connecting(&ctx.hud);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(incoming, pairing, ctx.clone()).await {
                state::push_log(&ctx.hud, format!("[!] conexão encerrada: {e}"));
                state::set_pairing(&ctx.hud);
                active::clear(&ctx.active);
            }
        });
    }
}

async fn handle_connection(incoming: quinn::Incoming, pairing: Arc<Mutex<PairingStore>>, ctx: Ctx) -> anyhow::Result<()> {
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
    state::set_connected(&ctx.hud, device_name.clone(), peer_fingerprint.clone());
    state::push_log(&ctx.hud, format!("[+] core.hello autorizado · {device_name}"));
    active::set(&ctx.active, connection.clone());

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
        tokio::spawn(handle_control_stream(send, recv, ctx.clone()));
    }

    state::push_log(&ctx.hud, format!("[!] {device_name} desconectou"));
    state::set_pairing(&ctx.hud);
    active::clear(&ctx.active);
    Ok(())
}

async fn handle_control_stream(mut send: quinn::SendStream, mut recv: quinn::RecvStream, ctx: Ctx) {
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
            eprintln!(
                "[stream] erro decodificando CBOR: {e} · {} bytes · hex: {}",
                frame.len(),
                frame.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            return;
        }
    };
    let body = packet.body.as_ref();
    let hud = &ctx.hud;

    match packet.kind.as_str() {
        "core.ping" => {
            reply(&mut send, packet.id, "core.pong", None).await;
        }

        "clipboard.set" => {
            if let Some(text) = body.and_then(|b| body_get_str(b, "text")) {
                clip::set_from_remote(text, &ctx.clip_guard).await;
                state::push_log(hud, "[i] clipboard.set · telemóvel → PC".to_string());
            }
        }

        "hypr.workspaces" => {
            let data = hypr::workspaces_json();
            reply(&mut send, packet.id, "hypr.workspaces_state", Some(ok_data(data))).await;
        }
        "hypr.clients" => {
            let data = hypr::clients_json();
            reply(&mut send, packet.id, "hypr.clients_state", Some(ok_data(data))).await;
        }
        "hypr.dispatch" => {
            let cmd = body.and_then(|b| body_get_str(b, "cmd")).unwrap_or("").to_string();
            let data = hypr::dispatch(&cmd);
            state::push_log(hud, format!("[i] hypr.dispatch {cmd}"));
            reply(&mut send, packet.id, "hypr.dispatch_result", Some(ok_data(data))).await;
        }

        "battery.request" => {
            reply(&mut send, packet.id, "battery.state", Some(battery::state_body())).await;
        }
        "battery.state" => {
            // bateria do telemóvel — só log por enquanto (ver Fase 8 pra UI).
            if let (Some(level), Some(charging)) = (
                body.and_then(|b| crate::protocol::body_get_i64(b, "level")),
                body.and_then(|b| crate::protocol::body_get(b, "charging")).and_then(|v| v.as_bool()),
            ) {
                state::push_log(hud, format!("[i] bateria do telemóvel: {level}% · carregando={charging}"));
            }
        }

        "media.command" => {
            let cmd = body.and_then(|b| body_get_str(b, "command")).unwrap_or("").to_string();
            media::handle_command(&cmd).await;
        }

        "notification.post" => {
            if let Some(b) = body {
                let key = body_get_str(b, "key").unwrap_or("").to_string();
                let app = body_get_str(b, "app").unwrap_or("HyprLink").to_string();
                let title = body_get_str(b, "title").unwrap_or("").to_string();
                let text = body_get_str(b, "text").unwrap_or("").to_string();
                let actions: Vec<(i64, String)> = crate::protocol::body_get(b, "actions")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                let idx = crate::protocol::body_get(item, "idx")?
                                    .as_integer()
                                    .and_then(|i| i64::try_from(i).ok())?;
                                let label = crate::protocol::body_get(item, "label")?.as_text()?.to_string();
                                Some((idx, label))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                state::push_log(hud, format!("[i] notification.post · {app}: {title}"));
                notif::post(&app, &title, &text, &key, &actions, &ctx.notif).await;
            }
        }
        "notification.dismissed" => {
            if let Some(key) = body.and_then(|b| body_get_str(b, "key")) {
                notif::dismiss_local(key, &ctx.notif).await;
            }
        }

        "input.move" => {
            if let (Some(dx), Some(dy)) = (
                body.and_then(|b| crate::protocol::body_get_i64(b, "dx")),
                body.and_then(|b| crate::protocol::body_get_i64(b, "dy")),
            ) {
                ctx.input.move_relative(dx as i32, dy as i32);
            }
        }
        "input.scroll" => {
            if let (Some(dx), Some(dy)) = (
                body.and_then(|b| crate::protocol::body_get_i64(b, "dx")),
                body.and_then(|b| crate::protocol::body_get_i64(b, "dy")),
            ) {
                ctx.input.scroll(dx as i32, dy as i32);
            }
        }
        "input.click" => {
            let button = body.and_then(|b| body_get_str(b, "button")).unwrap_or("left");
            ctx.input.click(button);
        }
        "input.type" => {
            if let Some(text) = body.and_then(|b| body_get_str(b, "text")) {
                ctx.input.type_text(text);
            }
        }
        "input.key" => {
            if let Some(key) = body.and_then(|b| body_get_str(b, "key")) {
                ctx.input.key(key);
            }
        }

        "share.url" => {
            if let Some(url) = body.and_then(|b| body_get_str(b, "url")) {
                state::push_log(hud, format!("[i] share.url {url}"));
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            }
        }

        other => {
            state::push_log(hud, format!("[!] tipo ainda não implementado: {other}"));
        }
    }
    // Fecha a stream de escrita prontamente — o app bloqueia lendo até EOF
    // nas respostas tipadas (ver PROTOCOL.md §4).
    let _ = send.finish();
}

async fn reply(send: &mut quinn::SendStream, id: u64, kind: &str, body: Option<Value>) {
    let packet = Packet::new(id, kind, body, false);
    let _ = write_frame(send, &packet.encode()).await;
}

fn ok_data(data: String) -> Value {
    Value::Map(vec![(Value::Text("ok".into()), Value::Bool(true)), (Value::Text("data".into()), Value::Text(data))])
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
