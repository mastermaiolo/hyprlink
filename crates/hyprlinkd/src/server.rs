//! Servidor QUIC: aceita conexões mTLS, valida o pareamento por fingerprint e
//! despacha os pacotes de controlo. Fase 3: clipboard, hyprland, bateria e
//! mídia — ver PROTOCOL.md para o catálogo completo.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ciborium::Value;
use hyprlink_proto::link::NotifAction;
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::CertificateDer;

use crate::active::{self, ActiveConn};
use crate::clip::{self, LastLocalSet};
use crate::config::{self, SharedConfig};
use crate::ctl;
use crate::gate;
use crate::identity::{ServerIdentity, fingerprint_der};
use crate::input::InputDevice;
use crate::notif;
use crate::pairing::PairingStore;
use crate::protocol::{
    Packet, body_get_bytes, body_get_str, read_frame, read_frame_limited, write_frame,
};
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
    /// A última cópia do PC feita sem telemóvel ligado (texto ou imagem).
    pub clip_outbox: clip::Outbox,
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
    /// Limite de frequência do `shortcut.run` (por ligação; recomeça ao ligar).
    pub shortcut_limiter: crate::shortcuts::SharedLimiter,
    /// Limite do `phone.find` (por ligação; recomeça ao ligar).
    pub find_limiter: crate::find::SharedLimiter,
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
            clip_outbox: clip::new_outbox(),
            tap,
            speaker,
            webcam: webcam::new_handle(),
            pending_webcam,
            mic: crate::mic::new_handle(),
            pending_mic: crate::mic::new_pending(),
            dbus,
            shortcut_limiter: crate::shortcuts::new_limiter(),
            find_limiter: crate::find::new_limiter(),
        }
    }
}

/// Streams por ligação depois de autenticada. Antes: 1 bidi (a do hello) e
/// nenhuma uni — ver `build_endpoint` e `authenticate`.
const AUTH_MAX_BIDI: u32 = 100;
const AUTH_MAX_UNI: u32 = 100;

/// Janela de receção da ligação até autenticar: o hello (64 KiB no máximo)
/// cabe com folga e um peer anónimo não põe mais do que isto em memória.
/// Depois de autorizado passa a ilimitada (o valor por omissão do quinn,
/// como antes); cada stream continua com a sua janela.
const PRE_AUTH_RECEIVE_WINDOW: u32 = 128 * 1024;

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
            // Até autenticar (sobem em `authenticate`).
            .max_concurrent_bidi_streams(1u32.into())
            .max_concurrent_uni_streams(0u32.into())
            .receive_window(PRE_AUTH_RECEIVE_WINDOW.into())
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
        ctx.clip_outbox.clone(),
        ctx.hud.clone(),
    ));
    tokio::spawn(clip::watch_images(
        ctx.active.clone(),
        ctx.clip_img.clone(),
        ctx.clip_outbox.clone(),
        ctx.hud.clone(),
    ));
    // Botão do rato preso (telemóvel adormeceu a meio de um arrastar): solta-o.
    {
        let (input, hud) = (ctx.input.clone(), ctx.hud.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tick.tick().await;
                let (input, hud) = (input.clone(), hud.clone());
                // O dispositivo é um Mutex síncrono: fora do executor.
                let released = tokio::task::spawn_blocking(move || {
                    let r = input.watchdog_tick();
                    (r, hud)
                })
                .await;
                if let Ok((true, hud)) = released {
                    state::push_log(
                        &hud,
                        "[!] input: botão do rato solto (30 s sem atividade)".to_string(),
                    );
                }
            }
        });
    }
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
        ctx.config.clone(),
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
    // Endereços do PC (Wi-Fi/cabo/USB) → `pc.addrs` + `link_path`.
    tokio::spawn(crate::netaddrs::watch(ctx.active.clone(), ctx.hud.clone()));
}

pub async fn run(endpoint: quinn::Endpoint, pairing: Arc<Mutex<PairingStore>>, ctx: Ctx) {
    let gate = gate::new(gate::Limits::DEFAULT);
    while let Some(incoming) = endpoint.accept().await {
        let Some((incoming, slot)) = admit_incoming(incoming, &gate, &ctx.hud) else {
            continue;
        };
        let (pairing, ctx, gate) = (pairing.clone(), ctx.clone(), gate.clone());
        // Com um telemóvel já ligado, um peer por autenticar não mexe no
        // estado mostrado na GUI.
        if ctx.active.lock().unwrap().is_none() {
            state::set_connecting(&ctx.hud);
        }
        tokio::spawn(async move {
            let authed = match authenticate(incoming, slot, &gate, &pairing, &ctx.hud).await {
                Ok(authed) => authed,
                // Recusado antes de autenticar: a sessão em curso (se
                // houver) não é tocada — só se volta a «à espera» se não
                // houver nenhuma.
                Err(_) => {
                    if ctx.active.lock().unwrap().is_none() {
                        state::set_pairing(&ctx.hud);
                    }
                    return;
                }
            };
            let connection = authed.connection.clone();
            let device_name = authed.device_name.clone();
            let result = serve(authed, pairing, ctx.clone()).await;
            // Uma ligação velha que morre depois de o telemóvel já ter
            // ligado outra vez não mexe na sessão nova.
            if !active::clear_if(&ctx.active, &connection) {
                state::push_log(
                    &ctx.hud,
                    format!("[i] {device_name}: ligação antiga fechada; a nova continua"),
                );
                return;
            }
            match result {
                Ok(()) => state::push_log(&ctx.hud, format!("[!] {device_name} desconectou")),
                Err(e) => state::push_log(&ctx.hud, format!("[!] conexão encerrada: {e}")),
            }
            teardown(&ctx).await;
        });
    }
}

/// Reserva um lugar de ligação por autenticar, ou recusa o `Incoming` logo à
/// entrada (antes do handshake) se já houver demasiadas — no total ou deste IP.
fn admit_incoming(
    incoming: quinn::Incoming,
    gate: &gate::Gate,
    hud: &Arc<Mutex<HudState>>,
) -> Option<(quinn::Incoming, gate::PendingSlot)> {
    let ip = incoming.remote_address().ip();
    match gate::admit(gate, ip) {
        Ok(slot) => Some((incoming, slot)),
        Err(refusal) => {
            incoming.refuse();
            log_refused(gate, hud, refusal.reason(), ip, None);
            None
        }
    }
}

/// Uma linha no HUD por minuto, no máximo, com quantas ligações por
/// autenticar foram recusadas e o motivo da última (sem token, fingerprint
/// abreviada).
fn log_refused(
    gate: &gate::Gate,
    hud: &Arc<Mutex<HudState>>,
    reason: &str,
    ip: std::net::IpAddr,
    fingerprint: Option<&str>,
) {
    let Some(n) = gate.lock().unwrap().note_refused(Instant::now()) else {
        return;
    };
    let fp = fingerprint.map_or_else(|| "sem certificado".to_string(), gate::short_fp);
    state::push_log(
        hud,
        format!(
            "[!] ligação não autenticada recusada ({n} desde a última linha) · {reason} · {} · {fp}",
            gate::IpKey::of(ip)
        ),
    );
}

/// Ligação que passou o gate: o `core.hello` foi lido e o peer é um
/// dispositivo emparelhado (ou acabou de emparelhar com o token certo).
struct Authed {
    connection: quinn::Connection,
    send: quinn::SendStream,
    /// A metade de leitura da stream do hello vive tanto quanto a ligação
    /// (largá-la mandava STOP_SENDING ao telemóvel).
    recv: quinn::RecvStream,
    hello: Packet,
    device_name: String,
    fingerprint: String,
    newly_paired: bool,
}

/// Porque é que uma ligação não passou o gate.
#[derive(Debug)]
enum PreAuthError {
    HandshakeTimeout,
    Handshake(String),
    NoCert,
    HelloTimeout,
    BadHello(String),
    Unauthorized,
    Blocked(Duration),
    Store(std::io::Error),
}

impl std::fmt::Display for PreAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreAuthError::HandshakeTimeout => write!(f, "handshake sem resposta"),
            PreAuthError::Handshake(e) => write!(f, "handshake falhou: {e}"),
            PreAuthError::NoCert => write!(f, "sem certificado de cliente"),
            PreAuthError::HelloTimeout => write!(f, "core.hello não chegou a tempo"),
            PreAuthError::BadHello(e) => write!(f, "core.hello inválido: {e}"),
            PreAuthError::Unauthorized => {
                write!(f, "dispositivo não emparelhado e sem token válido")
            }
            PreAuthError::Blocked(left) => {
                write!(f, "IP bloqueado por tokens errados (faltam {left:.0?})")
            }
            PreAuthError::Store(e) => write!(f, "não foi possível guardar o emparelhamento: {e}"),
        }
    }
}

/// Códigos de fecho (de aplicação) para quem não passa o gate.
const CLOSE_HELLO_TIMEOUT: u32 = 0x10;
const CLOSE_BAD_HELLO: u32 = 0x11;
const CLOSE_UNAUTHORIZED: u32 = 0x12;
const CLOSE_BLOCKED: u32 = 0x13;

impl PreAuthError {
    fn close_code(&self) -> u32 {
        match self {
            PreAuthError::HelloTimeout => CLOSE_HELLO_TIMEOUT,
            PreAuthError::BadHello(_) => CLOSE_BAD_HELLO,
            PreAuthError::Blocked(_) => CLOSE_BLOCKED,
            _ => CLOSE_UNAUTHORIZED,
        }
    }
}

/// O gate: handshake, `core.hello` e decisão de emparelhamento, tudo com os
/// limites de [`gate::Limits`]. Até aqui o peer é anónimo — o TLS aceita
/// qualquer certificado (`tls_verifier.rs`) e a confiança vem só da
/// fingerprint fixada, verificada abaixo. Por isso, antes desta decisão:
/// uma stream bidi, nenhuma uni, 128 KiB de janela (ver `build_endpoint`),
/// um hello de 64 KiB no máximo e 10 s para o mandar. Só depois de
/// autorizado se sobe para os limites normais.
async fn authenticate(
    incoming: quinn::Incoming,
    slot: gate::PendingSlot,
    gate: &gate::Gate,
    pairing: &Arc<Mutex<PairingStore>>,
    hud: &Arc<Mutex<HudState>>,
) -> Result<Authed, PreAuthError> {
    let ip = incoming.remote_address().ip();
    let mut fingerprint = None;
    let result = authenticate_inner(incoming, ip, gate, pairing, hud, &mut fingerprint).await;
    // O lugar de «por autenticar» liberta-se já, passe ou não.
    drop(slot);
    if let Err(e) = &result {
        log_refused(gate, hud, &e.to_string(), ip, fingerprint.as_deref());
    }
    result
}

async fn authenticate_inner(
    incoming: quinn::Incoming,
    ip: std::net::IpAddr,
    gate: &gate::Gate,
    pairing: &Arc<Mutex<PairingStore>>,
    hud: &Arc<Mutex<HudState>>,
    fingerprint: &mut Option<String>,
) -> Result<Authed, PreAuthError> {
    let limits = gate.lock().unwrap().limits;
    let connection = match tokio::time::timeout(limits.handshake_timeout, incoming).await {
        Err(_) => return Err(PreAuthError::HandshakeTimeout),
        Ok(Err(e)) => return Err(PreAuthError::Handshake(e.to_string())),
        Ok(Ok(c)) => c,
    };
    let refuse = |e: PreAuthError| {
        connection.close(e.close_code().into(), b"");
        e
    };
    let peer_fingerprint =
        peer_cert_fingerprint(&connection).ok_or_else(|| refuse(PreAuthError::NoCert))?;
    *fingerprint = Some(peer_fingerprint.clone());

    // O primeiro stream bidirecional deve trazer o core.hello — é ali que a
    // autenticação por pareamento acontece antes de qualquer outro pacote.
    let (send, recv, hello) = match tokio::time::timeout(
        limits.hello_timeout,
        read_hello(&connection, limits.hello_max),
    )
    .await
    {
        Err(_) => return Err(refuse(PreAuthError::HelloTimeout)),
        Ok(Err(e)) => return Err(refuse(PreAuthError::BadHello(e))),
        Ok(Ok(h)) => h,
    };

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

    // Um hello sem token de um dispositivo não emparelhado (ex.: um
    // telemóvel revogado a tentar religar) não testa o token e não conta
    // como tentativa falhada; só um token errado conta.
    let now = Instant::now();
    let newly_paired = {
        let mut store = pairing.lock().unwrap();
        if store.is_paired(&peer_fingerprint) {
            false
        } else if let Some(token_hex) = &token_hex {
            let mut g = gate.lock().unwrap();
            if let Some(left) = g.blocked_for(ip, now) {
                // Bloqueado: o token nem é comparado, certo ou errado.
                return Err(refuse(PreAuthError::Blocked(left)));
            }
            match store.try_pair_with_token(&peer_fingerprint, token_hex) {
                Ok(true) => {
                    g.record_success(ip);
                    // O token roda depois de um pareamento: o QR mostrado tem de
                    // passar a ter o novo, senão o próximo pareamento falha.
                    hud.lock().unwrap().pairing_token_hex = store.current_token_hex.clone();
                    true
                }
                Ok(false) => {
                    let out = g.record_failure(ip, now);
                    if out.rotate {
                        store.rotate_token();
                        hud.lock().unwrap().pairing_token_hex = store.current_token_hex.clone();
                    }
                    let limits = g.limits;
                    drop(g);
                    drop(store);
                    if let Some(block) = out.blocked {
                        state::push_log(
                            hud,
                            format!(
                                "[!] emparelhamento: {} tokens errados de {} — bloqueado {block:?}",
                                limits.fails_per_ip,
                                gate::IpKey::of(ip),
                            ),
                        );
                    }
                    if out.rotate {
                        state::push_log(
                            hud,
                            format!(
                                "[!] emparelhamento: {} tokens errados no total — token novo (o QR na GUI já mudou)",
                                limits.fails_global
                            ),
                        );
                    }
                    return Err(refuse(PreAuthError::Unauthorized));
                }
                Err(e) => return Err(refuse(PreAuthError::Store(e))),
            }
        } else {
            return Err(refuse(PreAuthError::Unauthorized));
        }
    };

    // Autorizado: abre os limites normais da ligação.
    connection.set_max_concurrent_bi_streams(AUTH_MAX_BIDI.into());
    connection.set_max_concurrent_uni_streams(AUTH_MAX_UNI.into());
    connection.set_receive_window(quinn::VarInt::MAX);

    Ok(Authed {
        connection,
        send,
        recv,
        hello,
        device_name,
        fingerprint: peer_fingerprint,
        newly_paired,
    })
}

/// `accept_bi` + o primeiro frame, limitado a `max` bytes. O tempo-limite
/// fica com quem chama.
async fn read_hello(
    connection: &quinn::Connection,
    max: u32,
) -> Result<(quinn::SendStream, quinn::RecvStream, Packet), String> {
    let (send, mut recv) = connection.accept_bi().await.map_err(|e| e.to_string())?;
    let hello = read_hello_frame(&mut recv, max).await?;
    Ok((send, recv, hello))
}

async fn read_hello_frame<R: tokio::io::AsyncRead + Unpin>(
    recv: &mut R,
    max: u32,
) -> Result<Packet, String> {
    let frame = read_frame_limited(recv, max)
        .await
        .map_err(|e| e.to_string())?;
    let hello =
        Packet::decode(&frame).map_err(|_| format!("CBOR inválido ({} bytes)", frame.len()))?;
    if hello.kind != "core.hello" {
        let kind: String = hello.kind.chars().take(40).collect();
        return Err(format!("primeiro pacote não foi core.hello: {kind:?}"));
    }
    Ok(hello)
}

/// A sessão de um dispositivo autorizado: resposta ao hello e despacho das
/// streams até a ligação fechar.
async fn serve(authed: Authed, pairing: Arc<Mutex<PairingStore>>, ctx: Ctx) -> anyhow::Result<()> {
    let Authed {
        connection,
        mut send,
        recv: _hello_recv,
        hello,
        device_name,
        fingerprint: peer_fingerprint,
        newly_paired,
    } = authed;
    println!("[+] handshake QUIC/mTLS completo, fingerprint do peer: {peer_fingerprint}");
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
            alias: None,
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
    // O nome do estado é o que se mostra (alias › nome › «fabricante modelo»):
    // é também o que dá nome ao nó PipeWire da webcam.
    let shown_name = pairing
        .lock()
        .unwrap()
        .meta(&peer_fingerprint)
        .map_or_else(|| device_name.clone(), |m| m.display_name());
    state::set_connected(&ctx.hud, shown_name, peer_fingerprint.clone());
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
        // Os IPs do PC por onde o telemóvel pode ligar (Wi-Fi, cabo, USB):
        // aditivo, as apps antigas ignoram. O `ip` corre fora do executor.
        let addrs = tokio::task::spawn_blocking(crate::netaddrs::current)
            .await
            .unwrap_or_default();
        pairs.push((
            Value::Text("pc_addrs".into()),
            crate::netaddrs::addrs_value(&addrs),
        ));
        Value::Map(pairs)
    };
    let reply = Packet::new(hello.id, "core.hello", Some(reply_body), false);
    write_frame(&mut send, &reply.encode()).await?;
    send.finish()?;

    // Atalhos da GUI no telemóvel: a lista (sem comandos) logo a seguir ao
    // handshake; o limite de frequência recomeça com a ligação.
    ctx.shortcut_limiter.lock().unwrap().reset();
    ctx.find_limiter.lock().unwrap().reset();
    crate::shortcuts::spawn_push_on_connect(ctx.active.clone(), ctx.config.clone());
    // A última cópia feita no PC enquanto o telemóvel estava desligado.
    tokio::spawn(clip::flush_pending(
        ctx.active.clone(),
        ctx.clip_outbox.clone(),
        ctx.hud.clone(),
    ));

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

    Ok(())
}

/// O que morre com a sessão do telemóvel. Só para a ligação que estava
/// registada (ver `active::clear_if`).
async fn teardown(ctx: &Ctx) {
    state::set_pairing(&ctx.hud);
    crate::tap::stop(&ctx.tap, &ctx.hud);
    webcam::stop(&ctx.webcam, &ctx.hud);
    crate::mic::stop(&ctx.mic, &ctx.hud);
    // O estado que o telemóvel reportou de si mesmo morre com a conexão —
    // não fica a informação velha na GUI/ctl.
    state::set_phone_status(&ctx.hud, Default::default());
    state::clear_active_notifs(&ctx.hud);
    // Toque «onde está o telemóvel» a meio: fica em erro (o telemóvel para
    // sozinho pelo tempo).
    crate::find::on_disconnect(&ctx.hud);
    // Botão do rato premido pelo telemóvel: solta-se com a ligação.
    ctx.input.release_all();
    // Modo coluna sem telemóvel = PC mudo: devolve o som às colunas na
    // hora (não espera pelo próximo arranque).
    crate::speaker::disable(&ctx.tap, &ctx.hud, &ctx.config, &ctx.speaker).await;
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
                state::set_webcam_session(&ctx.hud, Some((id, width, height)));
                Some((width, height))
            }
            // Reinício local no telemóvel (resolução/codec): reabre o stream
            // com o id da sessão em curso, sem novo `webcam.start`.
            _ => match ctx.hud.lock().unwrap().modules.webcam_session {
                Some((sid, width, height)) if sid == id => Some((width, height)),
                _ => None,
            },
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
            let result = hypr::dispatch_checked(&cmd);
            match &result {
                Ok(_) => state::push_log(hud, format!("[i] hypr.dispatch {cmd}")),
                Err(e) => state::push_log(hud, format!("[!] hypr.dispatch {cmd}: {e}")),
            }
            let body = match result {
                Ok(data) => ok_data(data),
                // `ok:false` + `error` (curto, em inglês): a app já não fica
                // sem saber que o botão não fez nada.
                Err(e) => Value::Map(vec![
                    (Value::Text("ok".into()), Value::Bool(false)),
                    (
                        Value::Text("data".into()),
                        Value::Text(format!("erro: {e}")),
                    ),
                    (Value::Text("error".into()), Value::Text(e)),
                ]),
            };
            reply(&mut send, packet.id, "hypr.dispatch_result", Some(body)).await;
        }
        // Ações rápidas do início (bloquear, suspender, captura, volume, media):
        // a app diz o nome, o daemon escolhe o comando (ver `action.rs`).
        "pc.action" => {
            let name = body
                .and_then(|b| body_get_str(b, "name"))
                .unwrap_or("")
                .to_string();
            let result = crate::action::run(&name).await;
            let body = match &result {
                Ok(()) => {
                    state::push_log(hud, format!("[i] pc.action {name}"));
                    ok_bool(true)
                }
                Err(e) => {
                    state::push_log(hud, format!("[!] pc.action {name}: {e}"));
                    Value::Map(vec![
                        (Value::Text("ok".into()), Value::Bool(false)),
                        (Value::Text("error".into()), Value::Text(e.clone())),
                    ])
                }
            };
            reply(&mut send, packet.id, "pc.action_result", Some(body)).await;
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
        // O telemóvel pede a câmara (pedido/resposta): reutiliza o caminho do
        // `Command2::StartWebcam`; em caso de sucesso o `webcam.start` normal
        // segue por outra stream.
        "webcam.request" => {
            let connected = ctx.active.lock().unwrap().is_some();
            let result = webcam::check_request(body, hud, &ctx.pending_webcam, connected);
            let line = match &result {
                Ok(_) => "[i] webcam.request · câmara pedida pelo telemóvel".to_string(),
                Err(e) => format!("[!] webcam.request recusado · {e}"),
            };
            state::push_log(hud, line);
            reply(
                &mut send,
                packet.id,
                "webcam.request_result",
                Some(webcam::request_result_body(&result)),
            )
            .await;
            let _ = send.finish();
            if let Ok(p) = result
                && !webcam::request_start(
                    &ctx.active,
                    &ctx.pending_webcam,
                    p.width,
                    p.height,
                    p.fps,
                    p.codec,
                )
                .await
            {
                state::push_log(hud, "[!] webcam.request · telemóvel desligou".to_string());
            }
        }
        // Opcional (telemóvel recente): formato efetivo. Um telemóvel antigo
        // nunca o envia e a GUI cai para o pedido.
        "webcam.state" => {
            if let Some(fmt) = webcam::parse_state(body) {
                webcam::apply_state(fmt, &ctx.webcam, &ctx.pending_webcam, hud);
            }
        }
        "webcam.transform" => {
            if let (Some(rotation), Some(mirror)) = (
                body.and_then(|b| crate::protocol::body_get_i64(b, "rotation")),
                body.and_then(|b| crate::protocol::body_get(b, "mirror"))
                    .and_then(|v| v.as_bool()),
            ) {
                webcam::apply_transform(&ctx.webcam, hud, rotation, mirror);
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

        // «Onde está o telemóvel?»: o telemóvel conta se está a tocar e quem
        // parou (ver find.rs).
        "phone.find.state" => {
            if let Some(body) = body {
                crate::find::on_state(hud, body);
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
                let parsed: Vec<(i64, String, bool)> = crate::protocol::body_get(b, "actions")
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
                                let is_reply = crate::protocol::body_get_bool(item, "is_reply")
                                    .unwrap_or(false);
                                Some((idx, label, is_reply))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let actions: Vec<(i64, String)> = parsed
                    .iter()
                    .map(|(idx, label, _)| (*idx, label.clone()))
                    .collect();
                let known: Vec<NotifAction> = parsed
                    .into_iter()
                    .filter_map(|(idx, label, is_reply)| {
                        Some(NotifAction {
                            idx: u32::try_from(idx).ok()?,
                            label,
                            is_reply,
                        })
                    })
                    .collect();
                // `replay: true` = já estava na barra do telemóvel quando ele ligou:
                // entra na lista de ativas, mas não gera balão no PC nem conta como nova.
                if crate::protocol::body_get_bool(b, "replay").unwrap_or(false) {
                    state::register_active_notif(hud, key, app, title, text, known);
                } else {
                    state::push_notif_entry(
                        hud,
                        key.clone(),
                        app.clone(),
                        title.clone(),
                        text.clone(),
                        known,
                    );
                    state::push_log(hud, format!("[i] notification.post · {app}: {title}"));
                    notif::post(&app, &title, &text, &key, &actions, &ctx.notif, &ctx.dbus).await;
                }
            }
        }
        // O telemóvel fez um gesto (one-way): ver `gesture.rs` e PROTOCOL.md.
        "gesture" => {
            if let Some(name) = body.and_then(|b| body_get_str(b, "name")) {
                let dir = body.and_then(|b| body_get_str(b, "dir"));
                let line = crate::gesture::handle(&ctx.config, name, dir).await;
                state::push_log(hud, line);
                // O «último gesto» vai logo para a GUI.
                crate::hub::global().publish(crate::bridge::gestures_event(&ctx.config));
            }
        }
        // Atalho da GUI pedido pelo telemóvel (só a identificação, nunca o
        // comando): ver `shortcuts.rs`. Responde `shortcut.run_result`.
        "shortcut.run" => {
            let id = body
                .and_then(|b| body_get_str(b, "id"))
                .unwrap_or("")
                .to_string();
            let (config, limiter, hud_c) = (
                ctx.config.clone(),
                ctx.shortcut_limiter.clone(),
                hud.clone(),
            );
            let id_c = id.clone();
            let result = tokio::task::spawn_blocking(move || {
                crate::shortcuts::run(
                    &config,
                    &limiter,
                    &hud_c,
                    &id_c,
                    std::time::Instant::now(),
                    &crate::hypr::dispatch_checked,
                )
            })
            .await
            .unwrap_or(Err(crate::shortcuts::ERR_FAILED));
            reply(
                &mut send,
                packet.id,
                "shortcut.run_result",
                Some(crate::shortcuts::result_body(&id, result)),
            )
            .await;
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
        // Premir/largar um botão (arrastar, selecionar uma área): ver `input.rs`.
        "input.button" => {
            if let (Some(button), Some(st)) = (
                body.and_then(|b| body_get_str(b, "button")),
                body.and_then(|b| body_get_str(b, "state")),
            ) && !ctx.input.button(button, st)
            {
                state::push_log(hud, format!("[!] input.button inválido: {button}/{st}"));
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
                // Teto e espaço livre verificados já no anúncio, antes de se
                // criar o ficheiro (o stream recusado leva share.done ok=false).
                let max = config::max_incoming_file_bytes(&ctx.config);
                let free = share::free_space_for(&config::download_dir(&ctx.config));
                match share::announce_incoming(
                    packet.id,
                    name.clone(),
                    size,
                    &ctx.incoming_files,
                    max,
                    free,
                ) {
                    Ok(()) => state::push_log(
                        hud,
                        format!("[i] share.file · recebendo {name} ({size} bytes)"),
                    ),
                    Err(motivo) => {
                        state::push_log(
                            hud,
                            format!("[!] share.file recusado · {name} ({size} bytes): {motivo}"),
                        );
                        crate::hub::global().publish(hyprlink_proto::link::Event::Notice(
                            hyprlink_proto::link::Notice::Failed {
                                op: hyprlink_proto::link::Op::Files,
                                error: hyprlink_proto::link::ErrorKind::Refused,
                            },
                        ));
                    }
                }
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
                // Só http(s): ver open_url.rs. Recusado → log sem o URL
                // completo e aviso na GUI.
                match crate::open_url::safe_open_url(url) {
                    Ok(()) => state::push_log(hud, format!("[i] share.url {url}")),
                    Err(motivo) => {
                        state::push_log(hud, format!("[!] share.url recusado ({motivo})"));
                        crate::hub::global().publish(hyprlink_proto::link::Event::Notice(
                            hyprlink_proto::link::Notice::Failed {
                                op: hyprlink_proto::link::Op::Phone,
                                error: hyprlink_proto::link::ErrorKind::Refused,
                            },
                        ));
                    }
                }
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

#[cfg(test)]
mod tests {
    //! O gate antes da autenticação com QUIC a sério em loopback: um servidor
    //! com `build_endpoint` + `admit_incoming` + `authenticate` (o mesmo
    //! caminho do `run`, sem a sessão) e clientes com certificados próprios.
    //! O token é fictício.

    use super::*;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, SignatureScheme};
    use tokio::sync::mpsc;

    const TOKEN: &str = "00112233445566778899aabbccddeeff";
    const WRONG: &str = "ffeeddccbbaa99887766554433221100";

    #[derive(Debug)]
    struct AnyServer(rustls::crypto::WebPkiSupportedAlgorithms);

    impl ServerCertVerifier for AnyServer {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
        }
        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.supported_schemes()
        }
    }

    fn new_cert() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["teste".to_string()]).unwrap();
        (
            cert.der().clone(),
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der())),
        )
    }

    /// Um «telemóvel»: endpoint cliente num IP de loopback, com certificado próprio.
    struct Peer {
        endpoint: quinn::Endpoint,
        fingerprint: String,
    }

    fn peer(ip: &str) -> Peer {
        let provider = rustls::crypto::ring::default_provider();
        let (cert, key) = new_cert();
        let fingerprint = fingerprint_der(cert.as_ref());
        let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(provider.clone()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyServer(
                provider.signature_verification_algorithms,
            )))
            .with_client_auth_cert(vec![cert], key)
            .unwrap();
        tls.alpn_protocols = vec![b"hyprlink/1".to_vec()];
        let quic = quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap();
        let mut endpoint = quinn::Endpoint::client(format!("{ip}:0").parse().unwrap()).unwrap();
        endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(quic)));
        Peer {
            endpoint,
            fingerprint,
        }
    }

    impl Peer {
        async fn connect(
            &self,
            addr: SocketAddr,
        ) -> Result<quinn::Connection, quinn::ConnectionError> {
            self.endpoint
                .connect(addr, "HyprLink-Desktop")
                .unwrap()
                .await
        }
    }

    struct Server {
        addr: SocketAddr,
        pairing: Arc<Mutex<PairingStore>>,
        hud: Arc<Mutex<HudState>>,
        results: mpsc::UnboundedReceiver<Result<Authed, PreAuthError>>,
        _endpoint: quinn::Endpoint,
    }

    fn server(limits: gate::Limits) -> Server {
        let (cert_der, key_der) = new_cert();
        let identity = ServerIdentity {
            fingerprint_hex: fingerprint_der(cert_der.as_ref()),
            cert_der,
            key_der,
        };
        let endpoint = build_endpoint(&identity, "127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = endpoint.local_addr().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "hyprlinkd-gate-{}-{}",
            std::process::id(),
            addr.port()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pairing = Arc::new(Mutex::new(PairingStore::in_dir(&dir, TOKEN)));
        let hud = HudState::new(TOKEN.to_string());
        let gate = gate::new(limits);
        let (tx, results) = mpsc::unbounded_channel();
        {
            let (endpoint, pairing, hud) = (endpoint.clone(), pairing.clone(), hud.clone());
            tokio::spawn(async move {
                while let Some(incoming) = endpoint.accept().await {
                    let Some((incoming, slot)) = admit_incoming(incoming, &gate, &hud) else {
                        continue;
                    };
                    let (gate, pairing, hud, tx) =
                        (gate.clone(), pairing.clone(), hud.clone(), tx.clone());
                    tokio::spawn(async move {
                        let r = authenticate(incoming, slot, &gate, &pairing, &hud).await;
                        let _ = tx.send(r);
                    });
                }
            });
        }
        Server {
            addr,
            pairing,
            hud,
            results,
            _endpoint: endpoint,
        }
    }

    fn quick() -> gate::Limits {
        gate::Limits {
            hello_timeout: Duration::from_millis(300),
            block_base: Duration::from_millis(400),
            block_max: Duration::from_secs(2),
            ..gate::Limits::DEFAULT
        }
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Manda um `core.hello` como a app (com ou sem token). Devolve a stream
    /// para não a largar antes do fim do teste.
    async fn send_hello(
        conn: &quinn::Connection,
        token: Option<&str>,
    ) -> (quinn::SendStream, quinn::RecvStream) {
        let mut body = vec![(
            Value::Text("device_name".into()),
            Value::Text("Teste".into()),
        )];
        if let Some(t) = token {
            body.push((
                Value::Text("pairing_token".into()),
                Value::Bytes(hex_bytes(t)),
            ));
        }
        let hello = Packet::new(1, "core.hello", Some(Value::Map(body)), false);
        let (mut send, recv) = conn.open_bi().await.unwrap();
        write_frame(&mut send, &hello.encode()).await.unwrap();
        (send, recv)
    }

    async fn close_code(conn: &quinn::Connection) -> u64 {
        match tokio::time::timeout(Duration::from_secs(5), conn.closed())
            .await
            .expect("a ligação devia ter sido fechada")
        {
            quinn::ConnectionError::ApplicationClosed(c) => c.error_code.into_inner(),
            other => panic!("fecho inesperado: {other:?}"),
        }
    }

    async fn next(s: &mut Server) -> Result<Authed, PreAuthError> {
        tokio::time::timeout(Duration::from_secs(5), s.results.recv())
            .await
            .expect("o servidor devia ter decidido")
            .unwrap()
    }

    /// Hello real da app (campos do `buildHelloCbor`): ~350 bytes, muito
    /// abaixo dos 64 KiB do gate.
    #[test]
    fn hello_da_app_cabe_com_folga() {
        let text = |k: &str, v: &str| (Value::Text(k.into()), Value::Text(v.into()));
        let caps = [
            "core",
            "clipboard",
            "notification",
            "media",
            "battery",
            "share",
            "webcam",
            "audio",
            "input",
            "hypr",
            "phone",
            "phone_status",
            "media_session",
        ];
        let body = Value::Map(vec![
            text("device_name", &"N".repeat(64)),
            text("manufacturer", "Xiaomi"),
            text("model", "22021211RG"),
            text("android", "14"),
            text("app_version", "0.1.0"),
            (
                Value::Text("capabilities".into()),
                Value::Array(caps.iter().map(|c| Value::Text((*c).into())).collect()),
            ),
            (
                Value::Text("pairing_token".into()),
                Value::Bytes(vec![0; 16]),
            ),
        ]);
        let len = Packet::new(1, "core.hello", Some(body), false)
            .encode()
            .len();
        assert!(len < 512, "{len} bytes");
        assert!(len as u32 * 64 < gate::Limits::DEFAULT.hello_max);
    }

    #[tokio::test(start_paused = true)]
    async fn hello_que_nao_chega_expira_aos_10_s() {
        let limits = gate::Limits::DEFAULT;
        // Ainda à espera aos 9,9 s.
        let (mut reader, _writer) = tokio::io::duplex(64);
        let early = tokio::time::timeout(
            Duration::from_millis(9_900),
            read_hello_frame(&mut reader, limits.hello_max),
        )
        .await;
        assert!(early.is_err());
        // O mesmo `timeout` do `authenticate_inner`: desiste aos 10 s certos
        // (o relógio pausado avança sozinho).
        let (mut reader, _writer) = tokio::io::duplex(64);
        let start = tokio::time::Instant::now();
        let r = tokio::time::timeout(
            limits.hello_timeout,
            read_hello_frame(&mut reader, limits.hello_max),
        )
        .await;
        assert!(r.is_err());
        assert_eq!(start.elapsed(), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn prefixo_de_16_mib_fecha_sem_ler_mais() {
        let mut s = server(gate::Limits::DEFAULT);
        let p = peer("127.0.0.1");
        let conn = p.connect(s.addr).await.unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        send.write_all(&crate::protocol::MAX_FRAME_SIZE.to_be_bytes())
            .await
            .unwrap();
        send.write_all(&[0u8; 1024]).await.unwrap();
        let started = std::time::Instant::now();
        assert_eq!(close_code(&conn).await, u64::from(CLOSE_BAD_HELLO));
        // Não esperou pelo tempo-limite (10 s): recusou logo pelo tamanho.
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(matches!(next(&mut s).await, Err(PreAuthError::BadHello(_))));
    }

    #[tokio::test]
    async fn outro_pacote_antes_do_hello_fecha() {
        let mut s = server(gate::Limits::DEFAULT);
        let p = peer("127.0.0.1");
        let conn = p.connect(s.addr).await.unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        let ping = Packet::new(1, "core.ping", None, false);
        write_frame(&mut send, &ping.encode()).await.unwrap();
        assert_eq!(close_code(&conn).await, u64::from(CLOSE_BAD_HELLO));
        assert!(matches!(next(&mut s).await, Err(PreAuthError::BadHello(_))));
    }

    #[tokio::test]
    async fn peer_calado_fecha_no_tempo_limite() {
        let mut s = server(quick());
        let p = peer("127.0.0.1");
        let conn = p.connect(s.addr).await.unwrap();
        assert_eq!(close_code(&conn).await, u64::from(CLOSE_HELLO_TIMEOUT));
        assert!(matches!(
            next(&mut s).await,
            Err(PreAuthError::HelloTimeout)
        ));
    }

    #[tokio::test]
    async fn excesso_de_ligacoes_por_autenticar_e_recusado() {
        // Por IP: 2.
        let s = server(gate::Limits::DEFAULT);
        let a = peer("127.0.0.1");
        let _c1 = a.connect(s.addr).await.unwrap();
        let _c2 = a.connect(s.addr).await.unwrap();
        assert!(matches!(
            a.connect(s.addr).await,
            Err(quinn::ConnectionError::ConnectionClosed(_))
        ));
        // Outro IP ainda entra.
        let b = peer("127.0.0.2");
        let _c3 = b.connect(s.addr).await.unwrap();

        // No total: aqui 3.
        let s = server(gate::Limits {
            max_pending: 3,
            ..gate::Limits::DEFAULT
        });
        let _d1 = a.connect(s.addr).await.unwrap();
        let _d2 = a.connect(s.addr).await.unwrap();
        let _d3 = b.connect(s.addr).await.unwrap();
        let c = peer("127.0.0.3");
        assert!(matches!(
            c.connect(s.addr).await,
            Err(quinn::ConnectionError::ConnectionClosed(_))
        ));
    }

    #[tokio::test]
    async fn lugar_liberta_se_depois_de_decidir() {
        let mut s = server(quick());
        let a = peer("127.0.0.1");
        for _ in 0..4 {
            let conn = a.connect(s.addr).await.unwrap();
            let _st = send_hello(&conn, None).await;
            assert_eq!(close_code(&conn).await, u64::from(CLOSE_UNAUTHORIZED));
            assert!(matches!(
                next(&mut s).await,
                Err(PreAuthError::Unauthorized)
            ));
        }
    }

    #[tokio::test]
    async fn forca_bruta_bloqueia_o_ip_ate_ao_fim_do_recuo() {
        let mut s = server(quick());
        let a = peer("127.0.0.1");
        for _ in 0..5 {
            let conn = a.connect(s.addr).await.unwrap();
            let _st = send_hello(&conn, Some(WRONG)).await;
            assert_eq!(close_code(&conn).await, u64::from(CLOSE_UNAUTHORIZED));
            assert!(matches!(
                next(&mut s).await,
                Err(PreAuthError::Unauthorized)
            ));
        }
        // Bloqueado: nem o token certo passa.
        let conn = a.connect(s.addr).await.unwrap();
        let _st = send_hello(&conn, Some(TOKEN)).await;
        assert_eq!(close_code(&conn).await, u64::from(CLOSE_BLOCKED));
        assert!(matches!(next(&mut s).await, Err(PreAuthError::Blocked(_))));
        assert!(!s.pairing.lock().unwrap().is_paired(&a.fingerprint));
        // O bloqueio não gasta o token.
        assert_eq!(s.pairing.lock().unwrap().current_token_hex, TOKEN);

        // Outro IP não está bloqueado.
        let b = peer("127.0.0.2");
        let conn = b.connect(s.addr).await.unwrap();
        let _st = send_hello(&conn, Some(WRONG)).await;
        assert_eq!(close_code(&conn).await, u64::from(CLOSE_UNAUTHORIZED));
        let _ = next(&mut s).await;

        // Passado o recuo, o token certo emparelha.
        tokio::time::sleep(Duration::from_millis(450)).await;
        let conn = a.connect(s.addr).await.unwrap();
        let _st = send_hello(&conn, Some(TOKEN)).await;
        let authed = next(&mut s)
            .await
            .expect("devia emparelhar depois do recuo");
        assert!(authed.newly_paired);
        assert!(s.pairing.lock().unwrap().is_paired(&a.fingerprint));
    }

    #[tokio::test]
    async fn demasiados_tokens_errados_no_total_rodam_o_token() {
        let mut s = server(gate::Limits {
            fails_global: 6,
            ..quick()
        });
        let ips = ["127.0.0.1", "127.0.0.2"];
        for (i, n) in [(0, 5), (1, 1)] {
            let p = peer(ips[i]);
            for _ in 0..n {
                let conn = p.connect(s.addr).await.unwrap();
                let _st = send_hello(&conn, Some(WRONG)).await;
                assert_eq!(close_code(&conn).await, u64::from(CLOSE_UNAUTHORIZED));
                let _ = next(&mut s).await;
            }
        }
        let novo = s.pairing.lock().unwrap().current_token_hex.clone();
        assert_ne!(novo, TOKEN);
        assert_eq!(s.hud.lock().unwrap().pairing_token_hex, novo);

        // O token antigo já não serve, o novo sim.
        let c = peer("127.0.0.3");
        let conn = c.connect(s.addr).await.unwrap();
        let _st = send_hello(&conn, Some(TOKEN)).await;
        assert_eq!(close_code(&conn).await, u64::from(CLOSE_UNAUTHORIZED));
        let _ = next(&mut s).await;
        let conn = c.connect(s.addr).await.unwrap();
        let _st = send_hello(&conn, Some(&novo)).await;
        assert!(next(&mut s).await.unwrap().newly_paired);
    }

    #[tokio::test]
    async fn emparelhamento_novo_com_o_token_certo() {
        let mut s = server(gate::Limits::DEFAULT);
        let p = peer("127.0.0.1");
        let conn = p.connect(s.addr).await.unwrap();
        let _st = send_hello(&conn, Some(TOKEN)).await;
        let authed = next(&mut s).await.expect("token certo devia emparelhar");
        assert!(authed.newly_paired);
        assert_eq!(authed.fingerprint, p.fingerprint);
        assert_eq!(authed.device_name, "Teste");
        let store = s.pairing.lock().unwrap();
        assert!(store.is_paired(&p.fingerprint));
        // Uso único: o token rodou e o HUD (QR) acompanha.
        assert_ne!(store.current_token_hex, TOKEN);
        assert_eq!(
            s.hud.lock().unwrap().pairing_token_hex,
            store.current_token_hex
        );
    }

    /// Dispositivo já emparelhado: entra sem token, mesmo vindo de um IP
    /// bloqueado (NAT partilhado), e só depois de autorizado pode abrir mais
    /// streams.
    #[tokio::test]
    async fn emparelhado_entra_e_so_depois_os_limites_sobem() {
        // Tempo para o hello maior do que as esperas abaixo.
        let mut s = server(gate::Limits {
            hello_timeout: Duration::from_secs(3),
            ..quick()
        });
        let p = peer("127.0.0.1");
        s.pairing.lock().unwrap().insert_paired(&p.fingerprint);
        // Outro peer bloqueia o IP.
        let intruso = peer("127.0.0.1");
        for _ in 0..5 {
            let conn = intruso.connect(s.addr).await.unwrap();
            let _st = send_hello(&conn, Some(WRONG)).await;
            close_code(&conn).await;
            let _ = next(&mut s).await;
        }

        let conn = p.connect(s.addr).await.unwrap();
        let (mut send, recv) = conn.open_bi().await.unwrap();
        // Antes do hello: nenhuma uni e só a bidi do hello.
        let short = Duration::from_millis(200);
        assert!(tokio::time::timeout(short, conn.open_uni()).await.is_err());
        assert!(tokio::time::timeout(short, conn.open_bi()).await.is_err());

        let hello = Packet::new(
            1,
            "core.hello",
            Some(Value::Map(vec![(
                Value::Text("device_name".into()),
                Value::Text("Teste".into()),
            )])),
            false,
        );
        write_frame(&mut send, &hello.encode()).await.unwrap();
        let authed = next(&mut s).await.expect("emparelhado devia entrar");
        assert!(!authed.newly_paired);

        // Depois: várias streams de cada tipo, como antes do gate.
        let long = Duration::from_secs(2);
        let mut keep = Vec::new();
        for _ in 0..3 {
            let mut uni = tokio::time::timeout(long, conn.open_uni())
                .await
                .expect("uni devia abrir depois de autorizado")
                .unwrap();
            uni.write_all(b"x").await.unwrap();
            keep.push(uni);
            let (mut bi, r) = tokio::time::timeout(long, conn.open_bi())
                .await
                .expect("bidi devia abrir depois de autorizado")
                .unwrap();
            bi.write_all(b"x").await.unwrap();
            keep.push(bi);
            drop(r);
        }
        drop((recv, authed));
    }
}
