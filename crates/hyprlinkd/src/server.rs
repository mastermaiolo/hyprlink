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
use crate::config::{self, SharedConfig};
use crate::ctl;
use crate::identity::{ServerIdentity, fingerprint_der};
use crate::input::InputDevice;
use crate::notif;
use crate::pairing::PairingStore;
use crate::protocol::{Packet, body_get_bytes, body_get_str, read_frame, write_frame};
use crate::share;
use crate::state::{self, HudState};
use crate::telemetry;
use crate::tls_verifier::AcceptAnyClientCert;
use crate::webcam;
use crate::{audio, battery, hypr, media};

/// Estado partilhado entre todas as streams/tarefas de fundo de uma sessão.
#[derive(Clone)]
pub struct Ctx {
    pub hud: Arc<Mutex<HudState>>,
    pub active: ActiveConn,
    pub clip_guard: LastLocalSet,
    pub input: Arc<InputDevice>,
    pub notif: notif::Registry,
    pub incoming_files: share::IncomingRegistry,
    pub config: SharedConfig,
    /// Clipboard: guard de imagem (evita eco no poll) + id pendente do
    /// uni-stream de imagem anunciado por `clipboard.set` has_payload.
    pub clip_img: clip::LastLocalImage,
    pub pending_clip: clip::PendingClipImage,
    /// Tap e coluna vêm de FORA (criados no `main`) porque os dois lados
    /// precisam deles: o servidor (handlers `audio.*`) e a GUI (toggle do
    /// modo coluna) — tem de ser o MESMO handle, senão o tap iniciado pela
    /// GUI e o iniciado pelo telemóvel não partilham o slot de pipeline
    /// atual e a paginação stop/start entre eles quebra.
    pub tap: crate::tap::TapHandle,
    pub speaker: crate::speaker::SpeakerHandle,
    pub webcam: webcam::WebcamHandle,
    pub pending_webcam: webcam::PendingWebcam,
    pub mic: crate::mic::MicHandle,
    pub pending_mic: crate::mic::PendingMic,
    /// Uma sessão D-Bus só, reusada por `notif.rs`/`media.rs` — antes cada
    /// notificação e cada poll de mídia (a cada 2s, pra sempre) abriam uma
    /// conexão nova, cada uma é uma negociação de socket completa.
    pub dbus: Option<zbus::Connection>,
}

impl Ctx {
    pub fn new(
        hud: Arc<Mutex<HudState>>,
        config: SharedConfig,
        active: ActiveConn,
        pending_webcam: webcam::PendingWebcam,
        dbus: Option<zbus::Connection>,
        tap: crate::tap::TapHandle,
        speaker: crate::speaker::SpeakerHandle,
    ) -> Self {
        Self {
            hud,
            active,
            clip_guard: clip::new_guard(),
            input: Arc::new(InputDevice::open()),
            notif: notif::new_registry(),
            incoming_files: share::new_incoming_registry(),
            config,
            clip_img: clip::new_image_guard(),
            pending_clip: clip::new_pending_clip(),
            tap,
            speaker,
            webcam: webcam::new_handle(),
            pending_webcam,
            mic: crate::mic::new_handle(),
            pending_mic: crate::mic::new_pending(),
            dbus,
        }
    }
}

pub fn build_endpoint(
    identity: &ServerIdentity,
    addr: SocketAddr,
) -> anyhow::Result<quinn::Endpoint> {
    let provider = rustls::crypto::ring::default_provider();
    let _ = provider.clone().install_default(); // ok se outro código já instalou

    let verifier = AcceptAnyClientCert::new(&provider);

    let mut tls_config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3 é sempre suportado pelo provider ring")
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![identity.cert_der.clone()],
            identity.key_der.clone_key(),
        )?;
    tls_config.alpn_protocols = vec![b"hyprlink/1".to_vec()];
    // ponytail: 0-RTT (max_early_data_size = MAX) permite ao cliente retomar
    // sessão sem reapresentar o certificado — mTLS é obrigatório aqui, e o
    // fingerprint do peer (peer_cert_fingerprint) some numa retomada, fazendo
    // toda reconexão após a primeira falhar em silêncio (bail antes do
    // primeiro log). 0 força handshake completo com cert sempre.
    tls_config.max_early_data_size = 0;
    // Isso sozinho não basta: em TLS 1.3 (o único usado no QUIC) a retomada
    // é via NewSessionTicket/PSK, controlada por `send_tls13_tickets`, não
    // por `session_storage` (esse é o mecanismo de sessão à moda TLS 1.2).
    // Zerar aqui impede o servidor de emitir qualquer ticket resumível.
    tls_config.send_tls13_tickets = 0;

    let quic_tls_config = QuicServerConfig::try_from(tls_config)?;
    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_tls_config));
    if let Some(transport) = Arc::get_mut(&mut server_config.transport) {
        transport
            .max_concurrent_bidi_streams(100u32.into())
            .max_concurrent_uni_streams(100u32.into())
            // ponytail: o app manda ping de keepalive a cada 30s (mesmo valor do
            // max_idle_timeout padrão do quinn) — sem margem, qualquer jitter faz o
            // QUIC fechar a conexão por "ociosa" bem na hora que o ping ia renová-la.
            // keep_alive_interval manda PING de verdade a nível de transporte, sem
            // depender do app; max_idle_timeout maior dá folga extra.
            .keep_alive_interval(Some(std::time::Duration::from_secs(5)))
            .max_idle_timeout(Some(quinn::VarInt::from_u32(60_000).into()));
    }

    let endpoint = quinn::Endpoint::server(server_config, addr)?;
    Ok(endpoint)
}

/// Sobe as tarefas de fundo que empurram eventos pro telemóvel independente
/// de estar ou não conectado no momento (elas checam `ctx.active` sozinhas) —
/// chamado uma vez, na subida do daemon.
pub fn spawn_background_tasks(ctx: Ctx) {
    tokio::spawn(clip::watch(
        ctx.active.clone(),
        ctx.clip_guard.clone(),
        ctx.hud.clone(),
    ));
    tokio::spawn(clip::watch_images(
        ctx.active.clone(),
        ctx.clip_img.clone(),
        ctx.hud.clone(),
    ));
    tokio::spawn(hypr::watch_events(ctx.active.clone(), ctx.hud.clone()));
    tokio::spawn(battery::poll_and_push(ctx.active.clone(), ctx.hud.clone()));
    tokio::spawn(media::poll_and_push(
        ctx.active.clone(),
        ctx.hud.clone(),
        ctx.dbus.clone(),
    ));
    tokio::spawn(notif::watch(
        ctx.active.clone(),
        ctx.notif.clone(),
        ctx.hud.clone(),
    ));
    // `pc.status` pro telemóvel a cada 2s (CPU/RAM/temp/uptime/RTT) — o
    // dashboard da app deixa de mostrar valores inventados.
    tokio::spawn(telemetry::poll_and_push(
        ctx.active.clone(),
        ctx.hud.clone(),
    ));
    // Camada de ecossistema (hyprlinkctl/Waybar): socket de comandos +
    // status.json no $XDG_RUNTIME_DIR (ver ctl.rs).
    tokio::spawn(ctl::serve(ctl::Ctl {
        hud: ctx.hud.clone(),
        config: ctx.config.clone(),
        active: ctx.active.clone(),
        tap: ctx.tap.clone(),
        speaker: ctx.speaker.clone(),
    }));
    tokio::spawn(ctl::publish_status_forever(ctx.hud.clone()));
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
                crate::tap::stop(&ctx.tap, &ctx.hud);
                webcam::stop(&ctx.webcam, &ctx.hud);
                crate::mic::stop(&ctx.mic, &ctx.hud);
                // O estado que o telemóvel reportou de si mesmo morre com a
                // conexão — não fica a informação velha na GUI/ctl.
                state::set_phone_status(&ctx.hud, Default::default());
                // Modo coluna sem telemóvel = PC mudo: devolve o som às
                // colunas na hora (não espera pelo próximo arranque).
                crate::speaker::disable(&ctx.tap, &ctx.hud, &ctx.config, &ctx.speaker).await;
            }
        });
    }
}

async fn handle_connection(
    incoming: quinn::Incoming,
    pairing: Arc<Mutex<PairingStore>>,
    ctx: Ctx,
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
        anyhow::bail!(
            "primeiro pacote não foi core.hello (recebido: {})",
            hello.kind
        );
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

    let (authorized, newly_paired) = {
        let mut store = pairing.lock().unwrap();
        if store.is_paired(&peer_fingerprint) {
            (true, false)
        } else if let Some(token_hex) = &token_hex {
            let ok = store.try_pair_with_token(&peer_fingerprint, token_hex)?;
            if ok {
                // O token roda depois de um pareamento: o QR mostrado tem de
                // passar a ter o novo, senão o próximo pareamento falha.
                ctx.hud.lock().unwrap().pairing_token_hex = store.current_token_hex.clone();
            }
            (ok, ok)
        } else {
            (false, false)
        }
    };

    if !authorized {
        anyhow::bail!("dispositivo não pareado e sem pairing_token válido — conexão rejeitada");
    }
    {
        let text = |k: &str| {
            hello
                .body
                .as_ref()
                .and_then(|b| body_get_str(b, k))
                .map(str::to_string)
        };
        let capabilities = hello
            .body
            .as_ref()
            .and_then(|b| crate::protocol::body_get(b, "capabilities"))
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_text().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let meta = crate::pairing::DeviceMeta {
            name: device_name.clone(),
            manufacturer: text("manufacturer"),
            model: text("model"),
            android: text("android"),
            app_version: text("app_version"),
            capabilities,
            paired_since: None,
            last_seen: None,
        };
        pairing
            .lock()
            .unwrap()
            .record_hello(&peer_fingerprint, meta, state::now_unix());
    }
    if newly_paired {
        crate::bridge::on_paired(&peer_fingerprint, &device_name);
    }

    println!("[+] core.hello de '{device_name}' — dispositivo autorizado");
    state::set_connected(&ctx.hud, device_name.clone(), peer_fingerprint.clone());
    state::push_log(
        &ctx.hud,
        format!("[+] core.hello autorizado · {device_name}"),
    );
    active::set(&ctx.active, connection.clone());

    let reply_body = {
        let mut pairs = vec![(Value::Text("device_name".into()), Value::Text(hostname()))];
        // MAC da interface de saída — a app guarda no pareamento pra poder
        // acordar este PC com um magic packet (Wake-on-LAN) quando o daemon
        // nem sequer está a correr. Campo opcional: sem ele, o botão "acordar
        // PC" fica desativado na app.
        if let Some(mac) = default_iface_mac() {
            pairs.push((Value::Text("pc_mac".into()), Value::Text(mac)));
        }
        Value::Map(pairs)
    };
    let reply = Packet::new(hello.id, "core.hello", Some(reply_body), false);
    write_frame(&mut send, &reply.encode()).await?;
    send.finish()?;

    // Streams seguintes (incluindo mais chamadas na mesma stream de controlo,
    // se o app reabrir uma nova) são despachadas por tipo.
    // Streams unidirecionais (ficheiros, por ora) chegam numa tarefa à
    // parte — não competem com as streams de controlo bidirecionais.
    {
        let connection = connection.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            loop {
                let recv = match connection.accept_uni().await {
                    Ok(recv) => recv,
                    Err(_) => break,
                };
                tokio::spawn(route_uni_stream(recv, ctx.clone()));
            }
        });
    }

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
    crate::tap::stop(&ctx.tap, &ctx.hud);
    webcam::stop(&ctx.webcam, &ctx.hud);
    crate::mic::stop(&ctx.mic, &ctx.hud);
    // O estado que o telemóvel reportou de si mesmo morre com a conexão —
    // não fica a informação velha na GUI/ctl.
    state::set_phone_status(&ctx.hud, Default::default());
    // Modo coluna sem telemóvel = PC mudo: devolve o som às colunas na
    // hora (não espera pelo próximo arranque).
    crate::speaker::disable(&ctx.tap, &ctx.hud, &ctx.config, &ctx.speaker).await;
    Ok(())
}

/// Lê o id de correlação (8 bytes, comum a todo uni-stream) e decide se é
/// vídeo de webcam (id bate com um `webcam.start` pendente), microfone (id
/// bate com um `webcam.mic_start` pendente) ou ficheiro — única leitura do
/// cabeçalho, pra não competir com `share::receive_uni_stream`.
async fn route_uni_stream(mut recv: quinn::RecvStream, ctx: Ctx) {
    let mut id_buf = [0u8; 8];
    if recv.read_exact(&mut id_buf).await.is_err() {
        return;
    }
    let id = u64::from_be_bytes(id_buf);

    let webcam_res = {
        let mut pending = ctx.pending_webcam.lock().unwrap();
        match *pending {
            Some((pending_id, width, height)) if pending_id == id => {
                *pending = None;
                Some((width, height))
            }
            _ => None,
        }
    };
    let mic_res = {
        let mut pending = ctx.pending_mic.lock().unwrap();
        match *pending {
            Some(pending_id) if pending_id == id => {
                *pending = None;
                true
            }
            _ => false,
        }
    };
    let clip_img_res = {
        let mut pending = ctx.pending_clip.lock().unwrap();
        match *pending {
            Some(pending_id) if pending_id == id => {
                *pending = None;
                true
            }
            _ => false,
        }
    };

    if let Some((width, height)) = webcam_res {
        webcam::feed(recv, ctx.webcam.clone(), ctx.hud.clone(), width, height).await;
    } else if mic_res {
        crate::mic::feed(recv, ctx.mic.clone(), ctx.hud.clone()).await;
    } else if clip_img_res {
        clip::receive_image_stream(recv, ctx.clip_img.clone(), ctx.hud.clone()).await;
    } else {
        share::receive_uni_stream(
            recv,
            id,
            ctx.incoming_files.clone(),
            ctx.active.clone(),
            ctx.hud.clone(),
            ctx.config.clone(),
        )
        .await;
    }
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
    crate::bridge::log_packet_rx(&packet.kind, frame.len(), packet.id);
    let body = packet.body.as_ref();
    let hud = &ctx.hud;

    match packet.kind.as_str() {
        "core.ping" => {
            reply(&mut send, packet.id, "core.pong", None).await;
        }

        "clipboard.set" => {
            // Imagem: body traz mime image/png e um uni-stream segue com os
            // bytes (id correlacionado) — registar o pendente e deixar o
            // route_uni_stream entregar ao clip::receive_image_stream.
            // NÃO fazer return aqui: o finish() no fim do handler é o que
            // destrava o app (ele bloqueia lendo a bidi até EOF —
            // PROTOCOL.md §4).
            if body.and_then(|b| body_get_str(b, "mime")) == Some("image/png") && packet.has_payload
            {
                *ctx.pending_clip.lock().unwrap() = Some(packet.id);
                state::push_log(
                    hud,
                    "[i] clipboard.set · telemóvel → PC · imagem anunciada".to_string(),
                );
            } else if let Some(text) = body.and_then(|b| body_get_str(b, "text")) {
                clip::set_from_remote(text, &ctx.clip_guard).await;
                state::push_clip_entry(hud, state::DIR_PHONE_TO_PC, text.to_string());
                state::push_log(hud, "[i] clipboard.set · telemóvel → PC".to_string());
            }
        }

        "hypr.workspaces" => {
            let data = hypr::workspaces_json();
            reply(
                &mut send,
                packet.id,
                "hypr.workspaces_state",
                Some(ok_data(data)),
            )
            .await;
        }
        "hypr.clients" => {
            let data = hypr::clients_json();
            reply(
                &mut send,
                packet.id,
                "hypr.clients_state",
                Some(ok_data(data)),
            )
            .await;
        }
        "hypr.dispatch" => {
            let cmd = body
                .and_then(|b| body_get_str(b, "cmd"))
                .unwrap_or("")
                .to_string();
            let data = hypr::dispatch(&cmd);
            state::push_log(hud, format!("[i] hypr.dispatch {cmd}"));
            reply(
                &mut send,
                packet.id,
                "hypr.dispatch_result",
                Some(ok_data(data)),
            )
            .await;
        }

        "audio.state" => {
            reply(
                &mut send,
                packet.id,
                "audio.state_reply",
                Some(audio::state_body()),
            )
            .await;
        }
        "audio.set_volume" => {
            if let (Some(kind), Some(id), Some(volume)) = (
                body.and_then(|b| body_get_str(b, "kind")),
                body.and_then(|b| crate::protocol::body_get_i64(b, "id")),
                body.and_then(|b| crate::protocol::body_get_i64(b, "volume")),
            ) {
                audio::set_volume(kind, id, volume);
            }
            reply(&mut send, packet.id, "audio.ack", Some(ok_bool(true))).await;
        }
        "audio.set_mute" => {
            if let (Some(kind), Some(id), Some(muted)) = (
                body.and_then(|b| body_get_str(b, "kind")),
                body.and_then(|b| crate::protocol::body_get_i64(b, "id")),
                body.and_then(|b| crate::protocol::body_get(b, "muted"))
                    .and_then(|v| v.as_bool()),
            ) {
                audio::set_mute(kind, id, muted);
            }
            reply(&mut send, packet.id, "audio.ack", Some(ok_bool(true))).await;
        }
        "audio.set_default_sink" => {
            if let Some(name) = body.and_then(|b| body_get_str(b, "name")) {
                audio::set_default_sink(name);
            }
            reply(&mut send, packet.id, "audio.ack", Some(ok_bool(true))).await;
        }
        "audio.tap_start" => {
            let ready_body = Value::Map(vec![
                (Value::Text("id".into()), Value::Integer(packet.id.into())),
                (Value::Text("rate".into()), Value::Integer(48000.into())),
                (Value::Text("channels".into()), Value::Integer(2.into())),
            ]);
            reply(&mut send, packet.id, "audio.tap_ready", Some(ready_body)).await;
            let _ = send.finish();
            if let Some(connection) = ctx.active.lock().unwrap().clone() {
                // No modo coluna, o "ouvir PC" do telemóvel tem de continuar
                // a capturar o sink virtual (o default mudou pra ele quando
                // o modo ligou — mas se o usuário mexer no default a meio, o
                // modo coluna não desanda por causa disso).
                let sink = crate::bridge::tap_sink(&ctx.speaker);
                tokio::spawn(crate::tap::start(
                    connection,
                    sink,
                    ctx.tap.clone(),
                    ctx.hud.clone(),
                ));
            }
            return; // já fechou o stream de controlo acima
        }
        "audio.tap_stop" => {
            crate::tap::stop(&ctx.tap, &ctx.hud);
            reply(&mut send, packet.id, "audio.ack", Some(ok_bool(true))).await;
        }

        "webcam.error" => {
            let message = body
                .and_then(|b| body_get_str(b, "message"))
                .unwrap_or("erro desconhecido");
            state::push_log(hud, format!("[!] webcam (telemóvel): {message}"));
            webcam::stop(&ctx.webcam, hud);
        }
        "webcam.transform" => {
            if let (Some(rotation), Some(mirror)) = (
                body.and_then(|b| crate::protocol::body_get_i64(b, "rotation")),
                body.and_then(|b| crate::protocol::body_get(b, "mirror"))
                    .and_then(|v| v.as_bool()),
            ) {
                webcam::apply_transform(&ctx.webcam, rotation, mirror);
            }
        }
        "webcam.mic_start" => {
            *ctx.pending_mic.lock().unwrap() = Some(packet.id);
            state::push_log(
                hud,
                "[i] microfone: telemóvel anunciou stream de áudio".to_string(),
            );
        }
        "webcam.mic_stop" => {
            crate::mic::stop(&ctx.mic, hud);
        }

        "battery.request" => {
            reply(
                &mut send,
                packet.id,
                "battery.state",
                Some(battery::state_body()),
            )
            .await;
        }
        "battery.state" => {
            if let (Some(level), Some(charging)) = (
                body.and_then(|b| crate::protocol::body_get_i64(b, "level")),
                body.and_then(|b| crate::protocol::body_get(b, "charging"))
                    .and_then(|v| v.as_bool()),
            ) {
                let previous = state::phone_battery_pct(hud);
                state::set_phone_battery(hud, level, charging);
                state::push_log(
                    hud,
                    format!("[i] bateria do telemóvel: {level}% · carregando={charging}"),
                );

                // Alertas do BATT (CONFIG do usuário) — dispara só na transição
                // pro limiar, não a cada battery.state enquanto já está lá.
                let alerts = config::battery_alerts(&ctx.config);
                let low = i64::from(alerts.low_pct);
                if alerts.low && level <= low && previous.is_none_or(|p| p > low) {
                    notif::post(
                        "HyprLink",
                        "Bateria do telemóvel baixa",
                        &format!("{level}% restantes"),
                        "hyprlink-batt-low",
                        &[],
                        &ctx.notif,
                        &ctx.dbus,
                    )
                    .await;
                }
                if alerts.full && level >= 100 && previous.is_none_or(|p| p < 100) {
                    notif::post(
                        "HyprLink",
                        "Telemóvel carregado",
                        "Bateria a 100%",
                        "hyprlink-batt-full",
                        &[],
                        &ctx.notif,
                        &ctx.dbus,
                    )
                    .await;
                }
            }
        }

        // Estado que o telemóvel reporta de si mesmo (rede, armazenamento,
        // RAM, ecrã, notificações, now-playing) — chega a cada 30 s e em
        // cada mudança; aqui é "o mais recente ganha", sem log (senão eram
        // duas linhas por minuto para sempre). A bridge polia e publica
        // `Event::Phone` pro hub (GUI/hyprlinkctl).
        "phone.status" => {
            if let Some(body) = body {
                state::set_phone_status(hud, crate::phone::parse(body));
            }
        }

        "media.command" => {
            let cmd = body
                .and_then(|b| body_get_str(b, "command"))
                .unwrap_or("")
                .to_string();
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
                                let label = crate::protocol::body_get(item, "label")?
                                    .as_text()?
                                    .to_string();
                                Some((idx, label))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                state::push_notif_entry(hud, key.clone(), app.clone(), title.clone(), text.clone());
                state::push_log(hud, format!("[i] notification.post · {app}: {title}"));
                notif::post(&app, &title, &text, &key, &actions, &ctx.notif, &ctx.dbus).await;
            }
        }
        "notification.dismissed" => {
            if let Some(key) = body.and_then(|b| body_get_str(b, "key")) {
                state::remove_active_notif(hud, key);
                notif::dismiss_local(key, &ctx.notif, &ctx.dbus).await;
            }
        }

        "input.move" => {
            if let (Some(dx), Some(dy)) = (
                body.and_then(|b| crate::protocol::body_get_i64(b, "dx")),
                body.and_then(|b| crate::protocol::body_get_i64(b, "dy")),
            ) {
                let t = config::track_settings(&ctx.config);
                // ponytail: "aceleração" é só um multiplicador extra fixo, não
                // uma curva real (ver TrackSettings) — bom o bastante por ora.
                let mult = t.sensitivity * if t.acceleration { 1.6 } else { 1.0 };
                ctx.input
                    .move_relative((dx as f32 * mult) as i32, (dy as f32 * mult) as i32);
            }
        }
        "input.scroll" => {
            if let (Some(dx), Some(dy)) = (
                body.and_then(|b| crate::protocol::body_get_i64(b, "dx")),
                body.and_then(|b| crate::protocol::body_get_i64(b, "dy")),
            ) {
                let t = config::track_settings(&ctx.config);
                let sign = if t.invert_scroll { -1.0 } else { 1.0 };
                ctx.input.scroll(
                    (dx as f32 * t.scroll_speed * sign) as i32,
                    (dy as f32 * t.scroll_speed * sign) as i32,
                );
            }
        }
        "input.click" => {
            let button = body
                .and_then(|b| body_get_str(b, "button"))
                .unwrap_or("left");
            ctx.input.click(button);
        }
        "input.type" => {
            if let Some(text) = body.and_then(|b| body_get_str(b, "text"))
                && config::track_settings(&ctx.config).virtual_keyboard
            {
                ctx.input.type_text(text);
            }
        }
        "input.key" => {
            if let Some(key) = body.and_then(|b| body_get_str(b, "key"))
                && config::track_settings(&ctx.config).virtual_keyboard
            {
                ctx.input.key(key);
            }
        }

        "share.file" => {
            if let Some(b) = body {
                let name = body_get_str(b, "name").unwrap_or("arquivo").to_string();
                let size = crate::protocol::body_get_i64(b, "size").unwrap_or(0).max(0) as u64;
                state::push_log(
                    hud,
                    format!("[i] share.file · recebendo {name} ({size} bytes)"),
                );
                share::announce_incoming(packet.id, name, size, &ctx.incoming_files);
            }
        }

        // Respostas do telemóvel a um envio PC → telemóvel (ele é o
        // recetor). O progresso local do envio já chega para a UI; o
        // `share.done` é a verificação final do outro lado.
        "share.progress" => {}
        "share.done" => {
            let id = body.and_then(|b| crate::protocol::body_get_i64(b, "id"));
            let ok = body
                .and_then(|b| crate::protocol::body_get_bool(b, "ok"))
                .unwrap_or(false);
            let error = body
                .and_then(|b| body_get_str(b, "error"))
                .map(str::to_string);
            match (id.and_then(|i| u64::try_from(i).ok()), ok) {
                (Some(_), true) => {
                    state::push_log(hud, "[+] share.done · o telemóvel confirmou o ficheiro");
                }
                (Some(id), false) => {
                    let known = state::mark_transfer_rejected(hud, id, error.clone());
                    state::push_log(
                        hud,
                        format!(
                            "[!] share.done · o telemóvel rejeitou o envio #{id}{}: {}",
                            if known { "" } else { " (desconhecido)" },
                            error.as_deref().unwrap_or("sem motivo")
                        ),
                    );
                }
                (None, _) => {}
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
    let frame = packet.encode();
    crate::bridge::log_packet_tx(kind, frame.len(), id);
    let _ = write_frame(send, &frame).await;
}

fn ok_data(data: String) -> Value {
    Value::Map(vec![
        (Value::Text("ok".into()), Value::Bool(true)),
        (Value::Text("data".into()), Value::Text(data)),
    ])
}

fn ok_bool(ok: bool) -> Value {
    Value::Map(vec![(Value::Text("ok".into()), Value::Bool(ok))])
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
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "HyprLink-Desktop".to_string())
}

/// MAC da interface que o SO escolheria pra sair pra internet — a mesma
/// heurística do `local_ip_guess` do main. Best-effort: falha silenciosa
/// vira campo ausente (a app só oferece WoL quando tem MAC gravado).
fn default_iface_mac() -> Option<String> {
    let out = std::process::Command::new("ip")
        .args(["-o", "route", "get", "10.255.255.255"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let mut tokens = line.split_whitespace();
    let iface = tokens
        .position(|t| t == "dev")
        .and_then(|_| tokens.next())?;
    let mac = std::fs::read_to_string(format!("/sys/class/net/{iface}/address")).ok()?;
    let mac = mac.trim().to_string();
    (mac.len() == 17).then_some(mac)
}
