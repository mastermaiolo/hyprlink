//! Webcam do telemóvel como câmara virtual do PC, via v4l2loopback.
//!
//! Fluxo (PROTOCOL.md §webcam): o daemon empurra `webcam.start` (push
//! D→P, corpo com resolução/fps/codec pedidos); o telemóvel responde
//! abrindo um stream unidirecional P→D com 8 bytes de id + 1 byte de codec
//! efetivo (0x01=H.264, 0x02=H.265) + NALUs Annex-B contínuos. O
//! `webcam.transform` (P→D one-way) chega quando o usuário gira/espelha no
//! telemóvel — ajustamos o pipeline ao vivo, sem reiniciar o stream.

use std::sync::{Arc, Mutex};

use ciborium::Value;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSrc;

use crate::active::{ActiveConn, push};
use crate::state::{self, HudState, push_log};
use hyprlink_env::overrides::DecoderChoice;
use hyprlink_proto::link::{CamDecoder, CamOutput};

pub type WebcamHandle = Arc<Mutex<Option<gst::Pipeline>>>;

pub fn new_handle() -> WebcamHandle {
    Arc::new(Mutex::new(None))
}

/// Id do `webcam.start` esperado no próximo uni-stream (+ a resolução
/// pedida, pra `feed()` saber com que caps montar o `v4l2sink`) — um só por
/// vez (design "um telemóvel por vez", igual ao resto do daemon).
pub type PendingWebcam = Arc<Mutex<Option<(u64, i64, i64)>>>;

pub fn new_pending() -> PendingWebcam {
    Arc::new(Mutex::new(None))
}

// O número do `/dev/videoN` é uma opção (`v4l2_device_nr`, por omissão 42):
// fixo em vez de descoberto, simples e determinístico; o risco é colidir com
// um dispositivo real nesse índice (o `doctor` avisa).
fn device_nr() -> u32 {
    crate::envinfo::resolved().v4l2_device_nr
}

fn device_path() -> String {
    format!("/dev/video{}", device_nr())
}

/// O que fazer para ter a câmara virtual pronta.
#[derive(Debug, PartialEq, Eq)]
enum Prepare {
    /// Já há um dispositivo utilizável.
    Ready,
    /// Carregar o módulo (`pkexec modprobe`).
    Load,
    /// Não dá: a frase (pt-PT) para o Diário.
    Refuse(String),
}

/// Decisão pura (testável): estado do dispositivo, se o módulo está carregado
/// e se existe para o kernel em uso (`modinfo`; `None` = não se sabe).
fn prepare(
    state: hyprlink_env::camera::DeviceState,
    module_loaded: bool,
    available: Option<bool>,
    nr: u32,
    kernel: &str,
) -> Prepare {
    use hyprlink_env::camera::DeviceState;
    match state {
        DeviceState::Ours => Prepare::Ready,
        DeviceState::Busy => Prepare::Refuse(format!(
            "o /dev/video{nr} é outro dispositivo (não o v4l2loopback): escolhe outro número em v4l2_device_nr no config.json"
        )),
        DeviceState::Free if module_loaded => Prepare::Refuse(format!(
            "o v4l2loopback já está carregado mas sem /dev/video{nr}: põe em v4l2_device_nr o número do dispositivo que ele criou (ou descarrega o módulo)"
        )),
        DeviceState::Free if available == Some(false) => Prepare::Refuse(format!(
            "o módulo v4l2loopback não existe para o kernel {kernel}: instala-o para este kernel (v4l2loopback-dkms com os cabeçalhos do kernel, ou o pacote do módulo do teu kernel) e reinicia a câmara"
        )),
        DeviceState::Free => Prepare::Load,
    }
}

/// O descodificador de um codec: o elemento GStreamer, o parser que o
/// antecede e se corre na GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Decoder {
    element: &'static str,
    parser: &'static str,
    h265: bool,
    gpu: bool,
}

impl Decoder {
    fn cpu(h265: bool) -> Self {
        Self {
            element: if h265 { "avdec_h265" } else { "avdec_h264" },
            parser: if h265 { "h265parse" } else { "h264parse" },
            h265,
            gpu: false,
        }
    }

    fn va(h265: bool) -> Self {
        Self {
            element: if h265 { "vah265dec" } else { "vah264dec" },
            gpu: true,
            ..Self::cpu(h265)
        }
    }

    fn kind(self) -> CamDecoder {
        if self.gpu {
            CamDecoder::Gpu
        } else {
            CamDecoder::Cpu
        }
    }
}

/// Escolha pura do descodificador: `va_present` diz se o elemento `va*dec`
/// deste codec existe. O segundo valor é `true` quando o utilizador forçou
/// `gpu` mas o VA-API não está disponível (recua para o CPU com aviso).
fn pick_decoder(choice: DecoderChoice, h265: bool, va_present: bool) -> (Decoder, bool) {
    match (choice, va_present) {
        (DecoderChoice::Cpu, _) => (Decoder::cpu(h265), false),
        (DecoderChoice::Gpu, false) => (Decoder::cpu(h265), true),
        (_, true) => (Decoder::va(h265), false),
        (DecoderChoice::Auto, false) => (Decoder::cpu(h265), false),
    }
}

/// Para onde o pipeline escreve.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Sink {
    V4l2(String),
    Pipewire,
}

impl Sink {
    fn output(&self) -> CamOutput {
        match self {
            Sink::V4l2(_) => CamOutput::V4l2(device_nr()),
            Sink::Pipewire => CamOutput::Pipewire,
        }
    }

    fn description(&self) -> String {
        match self {
            Sink::V4l2(dev) => format!("v4l2sink device={dev} sync=false"),
            // Sem programa a ler, o pool do `pipewiresink` não liberta o
            // primeiro buffer e bloquearia a cadeia até à fonte. O `tee` com
            // um ramo que descarta (`fakesink`) mantém o pipeline a correr;
            // a `queue` leaky larga frames se o sink não acompanhar.
            Sink::Pipewire => format!(
                "tee name=camtee ! {LEAKY_QUEUE} \
                 ! pipewiresink name=camsink mode=provide sync=false \
                 camtee. ! queue leaky=downstream max-size-buffers=1 max-size-bytes=0 \
                 max-size-time=0 ! fakesink sync=false async=false"
            ),
        }
    }
}

/// Propriedades do nó PipeWire que fazem dele uma câmara para o WirePlumber
/// e para o portal.
fn camera_properties(phone: &str) -> gst::Structure {
    gst::Structure::builder("props")
        .field("media.class", "Video/Source")
        .field("media.role", "Camera")
        .field("node.name", "hyprlink-camera")
        .field("node.description", format!("HyprLink · {phone}"))
        .build()
}

/// Formatos de vídeo bruto que a saída PipeWire oferece, por ordem de
/// tentativa. O `pipewiresink` só anuncia **um** formato (o negociado no
/// arranque): se o programa pedir outro, o nó dá erro («no more input
/// formats»). Começa-se pelo I420 (o nativo do WebRTC) e, se um cliente o
/// recusar, o pipeline refaz-se com o seguinte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PwFormat {
    I420,
    Nv12,
    Yuy2,
}

impl PwFormat {
    fn name(self) -> &'static str {
        match self {
            PwFormat::I420 => "I420",
            PwFormat::Nv12 => "NV12",
            PwFormat::Yuy2 => "YUY2",
        }
    }

    fn next(self) -> Self {
        match self {
            PwFormat::I420 => PwFormat::Nv12,
            PwFormat::Nv12 => PwFormat::Yuy2,
            PwFormat::Yuy2 => PwFormat::I420,
        }
    }
}

/// Caps fixas da saída PipeWire: formato, tamanho **e `framerate`** (os
/// navegadores não aceitam um nó sem frame rate).
fn pipewire_caps(width: i64, height: i64, fps: u32, format: PwFormat) -> String {
    format!(
        "video/x-raw,format={},width={width},height={height},framerate={}/1,pixel-aspect-ratio=1/1",
        format.name(),
        fps.max(1)
    )
}

/// `queue` que larga as frames antigas em vez de bloquear.
const LEAKY_QUEUE: &str =
    "queue leaky=downstream max-size-buffers=2 max-size-bytes=0 max-size-time=0";

/// A saída a usar. `Err` = a frase (pt-PT) para o Diário.
fn pick_output(
    choice: hyprlink_env::overrides::OutputChoice,
    module_loaded: bool,
    module_available: Option<bool>,
    pipewiresink: bool,
) -> Result<hyprlink_env::camera::Output, String> {
    use hyprlink_env::camera::{Output, resolve_output};
    let out = resolve_output(choice, module_loaded, module_available);
    if out == Output::Pipewire && !pipewiresink {
        return Err(
            "falta o plugin pipewiresink (instala gst-plugin-pipewire) para a saída PipeWire"
                .into(),
        );
    }
    Ok(out)
}

fn sink_for_config() -> Result<Sink, String> {
    use hyprlink_env::camera::{self, Output};
    let choice = crate::envinfo::resolved().webcam_output;
    let env = crate::envinfo::daemon_env();
    let loaded = camera::module_loaded(&env.sys);
    let available = (choice == hyprlink_env::overrides::OutputChoice::Auto && !loaded)
        .then(|| camera::module_available(&env))
        .flatten();
    let pw = gst::ElementFactory::find("pipewiresink").is_some();
    Ok(match pick_output(choice, loaded, available, pw)? {
        Output::V4l2loopback => Sink::V4l2(device_path()),
        Output::Pipewire => Sink::Pipewire,
    })
}

/// Tudo o que define como o pipeline se monta.
#[derive(Debug, Clone)]
struct Plan {
    decoder: Decoder,
    sink: Sink,
    phone: String,
    /// Frame rate fixo dos caps da saída PipeWire.
    fps: u32,
    /// Formato oferecido pela saída PipeWire.
    format: PwFormat,
}

fn ensure_v4l2loopback_loaded(hud: &Arc<Mutex<HudState>>) -> bool {
    let env = crate::envinfo::daemon_env();
    let nr = device_nr();
    let kernel = hyprlink_env::read_trim_pub(&env.proc_dir.join("sys/kernel/osrelease"))
        .unwrap_or_else(|| "atual".into());
    let state = hyprlink_env::camera::device_state(&env.sys, &env.dev, nr);
    let loaded = hyprlink_env::camera::module_loaded(&env.sys);
    let available = (state == hyprlink_env::camera::DeviceState::Free && !loaded)
        .then(|| hyprlink_env::camera::module_available(&env))
        .flatten();
    match prepare(state, loaded, available, nr, &kernel) {
        Prepare::Ready => return true,
        Prepare::Refuse(why) => {
            push_log(hud, format!("[!] webcam: {why}"));
            return false;
        }
        Prepare::Load => {}
    }
    push_log(
        hud,
        "[i] webcam: carregando v4l2loopback (pode pedir sua senha)...".to_string(),
    );
    let status = crate::session::command("pkexec")
        .args([
            "modprobe",
            "v4l2loopback",
            &format!("video_nr={nr}"),
            &format!("card_label={}", hyprlink_env::camera::CARD_LABEL),
            "exclusive_caps=1",
        ])
        .status();
    match status {
        Ok(s) if s.success() && std::path::Path::new(&device_path()).exists() => true,
        _ => {
            push_log(hud, "[!] webcam: não foi possível carregar o v4l2loopback (pkexec cancelado ou módulo indisponível)".to_string());
            false
        }
    }
}

pub fn stop(handle: &WebcamHandle, hud: &Arc<Mutex<HudState>>) {
    if let Some(pipeline) = handle.lock().unwrap().take() {
        let _ = pipeline.set_state(gst::State::Null);
        state::set_webcam_active(hud, false);
        push_log(hud, "[i] webcam: stream encerrado".to_string());
    }
    // Um `webcam.state` pode ter chegado antes do vídeo: não deixar o
    // formato/sessão de uma câmara já parada para a próxima.
    state::set_webcam_format(hud, None);
    state::set_webcam_session(hud, None);
    hud.lock().unwrap().modules.webcam_transform = (0, false);
}

/// Quanto tempo uma sessão espera pelo stream novo depois de o antigo
/// fechar. O telemóvel fecha o stream e abre outro com o mesmo id quando muda
/// de resolução, codec ou lente; parar logo apagava a sessão e o stream novo
/// já não era reconhecido como vídeo.
const RESTART_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// O stream de `mine` acabou. Se ainda é o pipeline em uso, a sessão fica
/// viva [`RESTART_GRACE`] à espera de um reinício com o mesmo id; só depois
/// pára. Um pipeline já substituído (o reinício chegou primeiro) só é
/// desligado.
fn finish(handle: WebcamHandle, hud: Arc<Mutex<HudState>>, mine: gst::Pipeline) {
    let current = handle.lock().unwrap().as_ref() == Some(&mine);
    if !current {
        let _ = mine.set_state(gst::State::Null);
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(RESTART_GRACE).await;
        if handle.lock().unwrap().as_ref() == Some(&mine) {
            stop(&handle, &hud);
        }
    });
}

/// Dimensões à saída (`/dev/video42`) para um vídeo `w`×`h` rodado
/// `rotation` graus no PC: 90/270 dão a imagem ao alto. Aceita `w`×`h` em
/// qualquer orientação (o `webcam.state` já as manda rodadas; o
/// `webcam.start` não), porque o encoder manda sempre o que a câmara capta.
pub fn oriented(w: i64, h: i64, rotation: u16) -> (i64, i64) {
    let (long, short) = (w.max(h), w.min(h));
    if matches!(rotation, 90 | 270) {
        (short, long)
    } else {
        (long, short)
    }
}

/// O que fazer quando a saída pedida difere da atual.
#[derive(Debug, PartialEq, Eq)]
enum Resize {
    /// Igual: nada a fazer.
    Keep,
    /// Ninguém tem a câmara aberta: refaz o pipeline com o tamanho novo.
    Rebuild,
    /// Um programa (browser, OBS) tem a câmara aberta: a v4l2loopback recusa
    /// mudar o formato enquanto alguém lê. Fica o tamanho atual, com barras.
    KeepForConsumer,
}

fn resize_plan(current: (i64, i64), wanted: (i64, i64), consumer_open: bool) -> Resize {
    if current == wanted {
        Resize::Keep
    } else if consumer_open {
        Resize::KeepForConsumer
    } else {
        Resize::Rebuild
    }
}

/// Algum processo **além deste** tem `dev` aberto? Lê os `/proc/*/fd` (só os
/// do mesmo utilizador são legíveis, que é o caso do browser e do OBS).
fn opened_by_others(proc_dir: &std::path::Path, dev: &std::path::Path, me: u32) -> bool {
    let Ok(entries) = std::fs::read_dir(proc_dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
            return false;
        };
        if pid == me {
            return false;
        }
        std::fs::read_dir(e.path().join("fd")).is_ok_and(|fds| {
            fds.flatten()
                .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|t| t == dev))
        })
    })
}

fn consumer_open(dev_path: &str) -> bool {
    opened_by_others(
        std::path::Path::new("/proc"),
        std::path::Path::new(dev_path),
        std::process::id(),
    )
}

/// Corpo do `webcam.configure` (D→P): `{width, height, fps, codec}`.
pub fn configure_body(width: i64, height: i64, fps: i64, codec: &str) -> Value {
    Value::Map(vec![
        (Value::Text("width".into()), Value::Integer(width.into())),
        (Value::Text("height".into()), Value::Integer(height.into())),
        (Value::Text("fps".into()), Value::Integer(fps.into())),
        (Value::Text("codec".into()), Value::Text(codec.to_string())),
    ])
}

/// Pede ao telemóvel que mude o formato com a câmara ligada. Pacote opcional:
/// uma app antiga ignora-o. `false` = sem telemóvel ligado.
pub async fn request_configure(
    active: &ActiveConn,
    width: i64,
    height: i64,
    fps: i64,
    codec: &str,
) -> bool {
    push(
        active,
        "webcam.configure",
        Some(configure_body(width, height, fps, codec)),
    )
    .await
    .is_some()
}

/// Lê o corpo de um `webcam.state` (P→D): `{width, height, fps, codec,
/// lens?, rotation?, mirror?}`. `None` se faltar/for inválido um campo
/// obrigatório (o pacote é ignorado, nunca derruba a ligação).
pub fn parse_state(body: Option<&Value>) -> Option<hyprlink_proto::link::WebcamFormat> {
    use hyprlink_proto::link::{CamCodec, CamLens, WebcamFormat};
    let b = body?;
    let num = |key: &str, max: i64| -> Option<i64> {
        crate::protocol::body_get(b, key)?
            .as_integer()
            .and_then(|i| i64::try_from(i).ok())
            .filter(|n| (1..=max).contains(n))
    };
    let codec = match crate::protocol::body_get(b, "codec")?.as_text()? {
        "h264" => CamCodec::H264,
        "h265" => CamCodec::H265,
        _ => return None,
    };
    let lens = match crate::protocol::body_get(b, "lens").and_then(|v| v.as_text()) {
        Some("back") => Some(CamLens::Back),
        Some("front") => Some(CamLens::Front),
        Some(_) => Some(CamLens::Other),
        None => None,
    };
    let rotation = crate::protocol::body_get_i64(b, "rotation")
        .filter(|r| matches!(r, 0 | 90 | 180 | 270))
        .unwrap_or(0) as u16;
    let mirror = crate::protocol::body_get(b, "mirror")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(WebcamFormat {
        width: num("width", 7680)? as u32,
        height: num("height", 4320)? as u32,
        fps: num("fps", 240)? as u32,
        codec,
        lens,
        rotation,
        mirror,
    })
}

/// Aplica um `webcam.state`: guarda o formato efetivo, a rotação/espelho e,
/// se a resolução mudou a meio, atualiza o pedido pendente e a sessão. O
/// `feed()` em curso vê a sessão nova e refaz a saída (não dá para mudar os
/// caps de um `v4l2sink` a correr: o `S_FMT` falha com «Device busy»).
pub fn apply_state(
    fmt: hyprlink_proto::link::WebcamFormat,
    handle: &WebcamHandle,
    pending: &PendingWebcam,
    hud: &Arc<Mutex<HudState>>,
) {
    state::set_webcam_format(hud, Some(fmt));
    let (w, h) = (i64::from(fmt.width), i64::from(fmt.height));
    if let Some(p) = pending.lock().unwrap().as_mut() {
        p.1 = w;
        p.2 = h;
    }
    if let Some(sess) = hud.lock().unwrap().modules.webcam_session.as_mut() {
        sess.1 = w;
        sess.2 = h;
    }
    apply_transform(handle, hud, i64::from(fmt.rotation), fmt.mirror);
}

/// Píxeis quadrados: com a imagem rodada para um tamanho de saída que não é
/// o dela, o `videoscale` põe barras (`add-borders`) em vez de a esticar.
fn output_caps(width: i64, height: i64) -> String {
    format!("video/x-raw,width={width},height={height},pixel-aspect-ratio=1/1")
}

/// Chamado pela GUI ("iniciar stream"): manda `webcam.start` pro telemóvel
/// e guarda o id retornado — o uni-stream de vídeo que chegar com esse id
/// é reconhecido pelo roteador em `server.rs` e despachado pra `feed()`.
pub async fn request_start(
    active: &ActiveConn,
    pending: &PendingWebcam,
    width: i64,
    height: i64,
    fps: i64,
    codec: &str,
) -> bool {
    let body = Value::Map(vec![
        (Value::Text("width".into()), Value::Integer(width.into())),
        (Value::Text("height".into()), Value::Integer(height.into())),
        (Value::Text("fps".into()), Value::Integer(fps.into())),
        (Value::Text("codec".into()), Value::Text(codec.to_string())),
    ]);
    let Some(id) = push(active, "webcam.start", Some(body)).await else {
        return false;
    };
    *pending.lock().unwrap() = Some((id, width, height));
    true
}

/// Rotação/espelho vindos de `webcam.transform` ou `webcam.state` (P→D).
/// Ficam guardados (a app manda-os logo ao ligar, muitas vezes antes do
/// vídeo; o pipeline nasce já com eles) e aplicam-se ao pipeline em curso.
/// O encoder do telemóvel manda sempre a imagem como a câmara a capta: esta
/// é a única rotação, feita aqui (`videoflip`). Elementos nomeados
/// `rotate`/`mirror` cuidam de cada eixo (evita a matemática de combinar os
/// 8 métodos diagonais do `videoflip` numa propriedade só).
pub fn apply_transform(
    handle: &WebcamHandle,
    hud: &Arc<Mutex<HudState>>,
    rotation: i64,
    mirror: bool,
) {
    let rotation = match rotation {
        90 | 180 | 270 => rotation as u16,
        _ => 0,
    };
    hud.lock().unwrap().modules.webcam_transform = (rotation, mirror);
    if let Some(pipeline) = handle.lock().unwrap().clone() {
        set_flip(&pipeline, rotation, mirror);
    }
}

fn set_flip(pipeline: &gst::Pipeline, rotation: u16, mirror: bool) {
    let rotate_method = match rotation {
        90 => "clockwise",
        180 => "rotate-180",
        270 => "counterclockwise",
        _ => "none",
    };
    if let Some(el) = pipeline.by_name("rotate") {
        el.set_property_from_str("method", rotate_method);
    }
    if let Some(el) = pipeline.by_name("mirror") {
        el.set_property_from_str("method", if mirror { "horizontal-flip" } else { "none" });
    }
}

/// Tamanho de saída que a sessão pede agora: as dimensões da sessão
/// orientadas pela rotação guardada.
fn wanted_output(hud: &Arc<Mutex<HudState>>, fallback: (i64, i64)) -> (i64, i64) {
    let m = &hud.lock().unwrap().modules;
    let (w, h) = m.webcam_session.map_or(fallback, |s| (s.1, s.2));
    oriented(w, h, m.webcam_transform.0)
}

/// Frame rate efetivo da sessão (o do último `webcam.state`; 30 sem ele).
fn wanted_fps(hud: &Arc<Mutex<HudState>>) -> u32 {
    hud.lock()
        .unwrap()
        .modules
        .webcam_format
        .as_ref()
        .map_or(30, |f| f.fps)
        .clamp(1, 240)
}

/// Um pipeline a correr: o `appsrc` onde se empurram os bytes e a ranhura
/// onde o monitor do bus guarda a falha de um elemento `va*`.
struct Built {
    pipeline: gst::Pipeline,
    appsrc: AppSrc,
    va_failure: VaFailure,
    /// Erro do `pipewiresink` (um programa recusou o formato…).
    sink_failure: VaFailure,
}

type VaFailure = Arc<Mutex<Option<String>>>;

/// Monta e põe a tocar um pipeline novo (decodifica → roda/espelha →
/// escala para `out` com barras → saída). Um pipeline com VA-API que não
/// passa a PLAYING devolve `Err`.
fn build_pipeline(plan: &Plan, out: (i64, i64), transform: (u16, bool)) -> Result<Built, String> {
    let pipeline_str = pipeline_description(plan, out);
    let pipeline = gst::parse::launch(&pipeline_str)
        .map_err(|e| format!("falha ao montar o pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "pipeline inesperado".to_string())?;
    let appsrc = pipeline
        .by_name("src")
        .and_then(|e| e.downcast::<AppSrc>().ok())
        .ok_or("appsrc não encontrado no pipeline")?;
    if let Some(sink) = pipeline.by_name("camsink") {
        sink.set_property("stream-properties", camera_properties(&plan.phone));
    }
    set_flip(&pipeline, transform.0, transform.1);
    if pipeline.set_state(gst::State::Playing).is_err() {
        let _ = pipeline.set_state(gst::State::Null);
        return Err("não foi possível iniciar o pipeline".into());
    }
    Ok(Built {
        pipeline,
        appsrc,
        va_failure: Arc::default(),
        sink_failure: Arc::default(),
    })
}

fn pipeline_description(plan: &Plan, out: (i64, i64)) -> String {
    let (parser, decoder) = (plan.decoder.parser, plan.decoder.element);
    // Na saída PipeWire o `appsrc` não bloqueia (larga o mais antigo se o
    // pipeline não acompanhar) e o `videorate` fixa o frame rate dos caps.
    let (src_opts, rate, caps) = match plan.sink {
        Sink::V4l2(_) => ("block=true", "", output_caps(out.0, out.1)),
        Sink::Pipewire => (
            "block=false leaky-type=downstream max-bytes=2097152",
            "! videorate ",
            pipewire_caps(out.0, out.1, plan.fps, plan.format),
        ),
    };
    // O `videoconvert` lê a memória `VAMemory` do VA-API e entrega memória
    // de sistema, que é o que o `v4l2sink`/`pipewiresink` precisam.
    format!(
        "appsrc name=src is-live=true format=time do-timestamp=true {src_opts} \
         ! {parser} ! {decoder} ! videoconvert \
         ! videoflip name=rotate method=none ! videoflip name=mirror method=none \
         ! videoscale add-borders=true {rate}! capsfilter name=outcaps caps=\"{caps}\" \
         ! {}",
        plan.sink.description()
    )
}

/// Largura×altura nos caps de saída de um pipeline (`outcaps`).
fn pipeline_output(pipeline: &gst::Pipeline) -> Option<(i64, i64)> {
    let caps = pipeline
        .by_name("outcaps")?
        .property::<Option<gst::Caps>>("caps")?;
    let st = caps.structure(0)?;
    Some((
        i64::from(st.get::<i32>("width").ok()?),
        i64::from(st.get::<i32>("height").ok()?),
    ))
}

/// O elemento `vah264dec0`, `vapostproc1`… é do plugin VA-API?
fn is_va_element(name: &str) -> bool {
    name.starts_with("va") && name.chars().any(|c| c.is_ascii_digit())
}

/// Erros do pipeline no Diário. timed_pop + Weak, como em mic.rs/tap.rs: um
/// stop limpo não posta Eos/Error, e iter_timed(NONE) deixava esta thread
/// presa para sempre (uma por cada stream de câmara já terminado).
///
/// Com VA-API (`va_failure` presente) o erro de um elemento `va*` não vai
/// para o Diário: fica na ranhura, e o `feed()` refaz o pipeline com o CPU.
/// O mesmo para o `pipewiresink` (`sink_failure`): um programa que recusa o
/// formato faz o nó dar erro e o `feed()` refaz o pipeline com outro.
fn watch_bus(
    pipeline: &gst::Pipeline,
    hud: &Arc<Mutex<HudState>>,
    va_failure: Option<VaFailure>,
    sink_failure: Option<VaFailure>,
) {
    let Some(bus) = pipeline.bus() else {
        return;
    };
    let hud_bus = hud.clone();
    let pipeline_weak = pipeline.downgrade();
    std::thread::spawn(move || {
        loop {
            let Some(msg) = bus.timed_pop(gst::ClockTime::from_seconds(1)) else {
                if pipeline_weak.upgrade().is_none() {
                    break;
                }
                continue;
            };
            match msg.view() {
                gst::MessageView::Error(err) => {
                    let from_va = err.src().is_some_and(|o| is_va_element(o.name().as_str()));
                    if let (true, Some(slot)) = (from_va, &va_failure) {
                        *slot.lock().unwrap() = Some(err.error().to_string());
                        break;
                    }
                    let from_sink = err.src().is_some_and(|o| o.name().as_str() == "camsink");
                    if let (true, Some(slot)) = (from_sink, &sink_failure) {
                        *slot.lock().unwrap() = Some(err.error().to_string());
                        break;
                    }
                    push_log(
                        &hud_bus,
                        format!(
                            "[!] webcam: erro no pipeline GStreamer: {} ({:?})",
                            err.error(),
                            err.debug()
                        ),
                    );
                    break;
                }
                gst::MessageView::Eos(_) => break,
                _ => {}
            }
        }
    });
}

/// Descodificador de recurso: o VA-API falhou (`why`), o CPU assume.
fn fall_back_to_cpu(
    plan: &mut Plan,
    hud: &Arc<Mutex<HudState>>,
    why: &str,
    out: (i64, i64),
) -> Result<Built, String> {
    push_log(
        hud,
        format!("[!] webcam: VA-API falhou ({why}), a usar CPU"),
    );
    plan.decoder = Decoder::cpu(plan.decoder.h265);
    let transform = hud.lock().unwrap().modules.webcam_transform;
    build_pipeline(plan, out, transform)
}

/// Um programa recusou o formato da saída PipeWire (`why`): refaz o
/// pipeline com o formato seguinte.
fn retry_format(
    plan: &mut Plan,
    hud: &Arc<Mutex<HudState>>,
    why: &str,
    out: (i64, i64),
) -> Result<Built, String> {
    let next = plan.format.next();
    push_log(
        hud,
        format!(
            "[!] webcam (PipeWire): cliente recusou o formato ({}): {why}; a tentar {}",
            plan.format.name(),
            next.name()
        ),
    );
    plan.format = next;
    build_resilient(plan, hud, out)
}

/// Monta o pipeline; se for com VA-API e não arrancar, refaz-o com o CPU sem
/// cortar a sessão. Liga o monitor do bus.
fn build_resilient(
    plan: &mut Plan,
    hud: &Arc<Mutex<HudState>>,
    out: (i64, i64),
) -> Result<Built, String> {
    let transform = hud.lock().unwrap().modules.webcam_transform;
    let built = match build_pipeline(plan, out, transform) {
        Err(e) if plan.decoder.gpu => fall_back_to_cpu(plan, hud, &e, out)?,
        other => other?,
    };
    watch_bus(
        &built.pipeline,
        hud,
        plan.decoder.gpu.then(|| built.va_failure.clone()),
        (plan.sink == Sink::Pipewire).then(|| built.sink_failure.clone()),
    );
    state::set_webcam_decoder(hud, plan.decoder.kind());
    Ok(built)
}

/// O texto «GPU (VA-API)» / «CPU» para o Diário.
fn decoder_label(d: Decoder) -> &'static str {
    if d.gpu { "GPU (VA-API)" } else { "CPU" }
}

fn phone_name(hud: &Arc<Mutex<HudState>>) -> String {
    match &hud.lock().unwrap().conn {
        state::ConnState::Connected { device_name, .. } => device_name.clone(),
        _ => "telemóvel".to_string(),
    }
}

/// Trata o uni-stream de vídeo já reconhecido pelo roteador (id bateu com
/// `PendingWebcam` ou com a sessão em curso): lê o byte de codec, monta o
/// pipeline GStreamer (descodifica H.264/H.265 na GPU por VA-API, com recuo
/// para o CPU → escreve no `/dev/video{N}` via v4l2loopback, N =
/// `v4l2_device_nr`, 42 por omissão, ou num nó de câmara PipeWire conforme
/// `webcam_output`) e alimenta o `appsrc` com os bytes crus conforme chegam.
///
/// Tamanho de saída: as dimensões da sessão orientadas pela rotação. Se
/// mudarem a meio (o telemóvel rodou, mudou de resolução), o pipeline é
/// refeito — o encoder repete VPS/SPS/PPS em cada keyframe (1 por segundo),
/// a imagem volta em ≤ 1 s. Com um programa a usar a câmara (só no
/// v4l2loopback), o tamanho fica (a v4l2loopback recusa mudá-lo) e o Diário
/// diz para a reabrir.
pub async fn feed(
    mut recv: quinn::RecvStream,
    handle: WebcamHandle,
    hud: Arc<Mutex<HudState>>,
    width: i64,
    height: i64,
) {
    if let Err(e) = gst::init() {
        push_log(&hud, format!("[!] webcam: GStreamer não inicializou: {e}"));
        return;
    }
    let sink = match sink_for_config() {
        Ok(s) => s,
        Err(e) => {
            push_log(&hud, format!("[!] webcam: {e}"));
            return;
        }
    };
    if matches!(sink, Sink::V4l2(_)) && !ensure_v4l2loopback_loaded(&hud) {
        return;
    }

    let mut codec_byte = [0u8; 1];
    if recv.read_exact(&mut codec_byte).await.is_err() {
        push_log(
            &hud,
            "[!] webcam: stream fechou antes do byte de codec".to_string(),
        );
        return;
    }
    let (h265, label) = match codec_byte[0] {
        0x02 => (true, "H.265"),
        _ => (false, "H.264"),
    };
    state::set_webcam_codec(&hud, label);
    let va_present = gst::ElementFactory::find(Decoder::va(h265).element).is_some();
    let (decoder, gpu_missing) =
        pick_decoder(crate::envinfo::resolved().webcam_decoder, h265, va_present);
    if gpu_missing {
        push_log(
            &hud,
            format!(
                "[!] webcam: webcam_decoder=gpu mas o VA-API não está disponível para {label} (falta libva-mesa-driver?), a usar CPU"
            ),
        );
    }
    let mut plan = Plan {
        decoder,
        sink,
        phone: phone_name(&hud),
        fps: wanted_fps(&hud),
        format: PwFormat::I420,
    };
    let dest = match &plan.sink {
        Sink::V4l2(dev) => dev.clone(),
        Sink::Pipewire => format!("PipeWire: HyprLink · {}", plan.phone),
    };
    let consumer_open = |plan: &Plan| match &plan.sink {
        Sink::V4l2(dev) => consumer_open(dev),
        Sink::Pipewire => false,
    };

    const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
    let mut buf = vec![0u8; 64 * 1024];

    // A app manda `webcam.state`/`webcam.transform` mais ou menos ao mesmo
    // tempo que abre o vídeo. Guarda os primeiros bytes até ~1 s à espera
    // deles, para o pipeline já nascer com o tamanho e a rotação certos.
    let mut early: Vec<u8> = Vec::new();
    let waited = std::time::Instant::now();
    while hud.lock().unwrap().modules.webcam_format.is_none()
        && waited.elapsed() < std::time::Duration::from_secs(1)
    {
        match tokio::time::timeout(std::time::Duration::from_millis(100), recv.read(&mut buf)).await
        {
            Ok(Ok(Some(n))) => early.extend_from_slice(&buf[..n]),
            Ok(Ok(None)) | Ok(Err(_)) => return,
            Err(_) => {}
        }
    }

    // Reinício (mesmo id): o pipeline antigo larga o dispositivo antes de o
    // novo o abrir. Com um programa a ler a câmara, o novo fica no tamanho
    // do antigo (a v4l2loopback não deixa mudar com um leitor aberto).
    let previous = handle.lock().unwrap().take();
    let restarted = previous.is_some();
    let mut out = wanted_output(&hud, (width, height));
    if let Some(old) = previous {
        if let Some(old_out) = pipeline_output(&old)
            && old_out != out
            && consumer_open(&plan)
        {
            push_log(&hud, consumer_note(old_out, out));
            out = old_out;
        }
        let _ = old.set_state(gst::State::Null);
    }
    let mut built = match build_resilient(&mut plan, &hud, out) {
        Ok(b) => b,
        Err(e) => {
            push_log(&hud, format!("[!] webcam: {e}"));
            if restarted {
                stop(&handle, &hud);
            }
            return;
        }
    };
    *handle.lock().unwrap() = Some(built.pipeline.clone());
    state::set_webcam_output(&hud, plan.sink.output());
    if !hud.lock().unwrap().modules.webcam_active {
        state::set_webcam_active(&hud, true);
    }
    push_log(
        &hud,
        format!(
            "[+] webcam: stream {} ({label}) · {} · {dest} · {}×{}",
            if restarted { "reiniciado" } else { "iniciado" },
            decoder_label(plan.decoder),
            out.0,
            out.1
        ),
    );
    if !early.is_empty()
        && built
            .appsrc
            .push_buffer(gst::Buffer::from_mut_slice(early))
            .is_err()
    {
        finish(handle, hud, built.pipeline);
        return;
    }

    // ponytail: timeout de leitura, não só EOF/erro — se o app crashar no
    // telemóvel mas o serviço de conexão sobreviver (visto ao vivo: uma
    // rotação derrubou a câmara mas não a conexão QUIC), o stream de vídeo
    // fica preso pra sempre sem EOF nem erro, e o pipeline (+ sua thread)
    // vaza rodando até o daemon reiniciar. 15s sem nenhum frame é folga de
    // sobra pra qualquer engasgo real de rede.
    let mut total: u64 = 0;
    let mut window_bytes: u64 = 0;
    let mut window_start = std::time::Instant::now();
    // Tamanho pedido que já foi avisado como «reabre a câmara no programa».
    let mut warned: Option<(i64, i64)> = None;
    loop {
        let n = match tokio::time::timeout(STALL_TIMEOUT, recv.read(&mut buf)).await {
            Ok(Ok(Some(n))) => n,
            Ok(Ok(None)) => break,
            Ok(Err(_)) => break,
            Err(_) => {
                push_log(
                    &hud,
                    "[!] webcam: sem dados há 15s, encerrando stream travado".to_string(),
                );
                break;
            }
        };
        // Um elemento VA-API falhou (no arranque ou a meio): o pipeline está
        // morto, refaz-se com o CPU e a sessão segue.
        let va_error = built.va_failure.lock().unwrap().take();
        if let Some(why) = va_error {
            if handle.lock().unwrap().as_ref() != Some(&built.pipeline) {
                break;
            }
            let _ = built.pipeline.set_state(gst::State::Null);
            match fall_back_to_cpu(&mut plan, &hud, &why, out) {
                Ok(b) => {
                    watch_bus(
                        &b.pipeline,
                        &hud,
                        None,
                        (plan.sink == Sink::Pipewire).then(|| b.sink_failure.clone()),
                    );
                    state::set_webcam_decoder(&hud, plan.decoder.kind());
                    *handle.lock().unwrap() = Some(b.pipeline.clone());
                    built = b;
                }
                Err(e) => {
                    push_log(&hud, format!("[!] webcam: {e}"));
                    break;
                }
            }
        }
        // O `pipewiresink` deu erro (um programa recusou o formato): o
        // pipeline refaz-se com o formato seguinte e a sessão segue.
        let sink_error = built.sink_failure.lock().unwrap().take();
        if let Some(why) = sink_error {
            if handle.lock().unwrap().as_ref() != Some(&built.pipeline) {
                break;
            }
            let _ = built.pipeline.set_state(gst::State::Null);
            match retry_format(&mut plan, &hud, &why, out) {
                Ok(b) => {
                    *handle.lock().unwrap() = Some(b.pipeline.clone());
                    built = b;
                }
                Err(e) => {
                    push_log(&hud, format!("[!] webcam (PipeWire): {e}"));
                    break;
                }
            }
        }
        let gst_buf = gst::Buffer::from_mut_slice(buf[..n].to_vec());
        if built.appsrc.push_buffer(gst_buf).is_err() {
            // Falhou por causa do VA-API ou do `pipewiresink` (o monitor
            // ainda não o registou)? Dá-lhe um instante e deixa a próxima
            // volta tratar.
            if plan.decoder.gpu || plan.sink == Sink::Pipewire {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if built.va_failure.lock().unwrap().is_some()
                    || built.sink_failure.lock().unwrap().is_some()
                {
                    continue;
                }
            }
            break;
        }
        total += n as u64;
        window_bytes += n as u64;
        let elapsed = window_start.elapsed();
        if elapsed < std::time::Duration::from_secs(1) {
            continue;
        }
        let mbps = (window_bytes as f64 * 8.0) / elapsed.as_secs_f64() / 1_000_000.0;
        state::set_webcam_mbps(&hud, mbps);
        window_bytes = 0;
        window_start = std::time::Instant::now();

        // Uma vez por segundo: o tamanho pedido mudou?
        let wanted = wanted_output(&hud, out);
        let plan_resize = if wanted == out {
            Resize::Keep
        } else {
            resize_plan(out, wanted, consumer_open(&plan))
        };
        match plan_resize {
            Resize::Keep => warned = None,
            Resize::KeepForConsumer => {
                if warned != Some(wanted) {
                    warned = Some(wanted);
                    push_log(&hud, consumer_note(out, wanted));
                }
            }
            Resize::Rebuild => {
                // Outro `feed()` (reinício) já tomou conta: este sai.
                if handle.lock().unwrap().as_ref() != Some(&built.pipeline) {
                    break;
                }
                let _ = built.pipeline.set_state(gst::State::Null);
                match build_resilient(&mut plan, &hud, wanted) {
                    Ok(b) => {
                        *handle.lock().unwrap() = Some(b.pipeline.clone());
                        built = b;
                        out = wanted;
                        warned = None;
                        push_log(&hud, format!("[i] webcam: saída agora {}×{}", out.0, out.1));
                    }
                    Err(e) => {
                        push_log(&hud, format!("[!] webcam: {e}"));
                        break;
                    }
                }
            }
        }
    }
    push_log(
        &hud,
        format!(
            "[i] webcam: stream encerrado ({} KB recebidos)",
            total / 1024
        ),
    );
    finish(handle, hud, built.pipeline);
}

/// «muda a resolução: reabre a câmara no programa» — a v4l2loopback não deixa
/// mudar o formato com um leitor aberto; a imagem segue com barras.
fn consumer_note(out: (i64, i64), wanted: (i64, i64)) -> String {
    format!(
        "[!] webcam: o telemóvel passou a {}×{}, mas um programa está a usar a câmara a {}×{} \
         (a imagem segue com barras). Muda a resolução: reabre a câmara no programa.",
        wanted.0, wanted.1, out.0, out.1
    )
}

/// Parâmetros de um `webcam.start`; o mesmo conjunto serve o comando da GUI
/// (`Command2::StartWebcam`) e o pedido do telemóvel (`webcam.request`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartParams {
    pub width: i64,
    pub height: i64,
    pub fps: i64,
    pub codec: &'static str,
}

impl Default for StartParams {
    /// Os valores por omissão do `webcam.start` (PROTOCOL.md §webcam).
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 24,
            codec: "h264",
        }
    }
}

/// Lê o corpo de um `webcam.request` (`{width?, height?, fps?, codec?}`);
/// o que faltar fica com o valor por omissão. O erro é curto e em inglês,
/// para o `webcam.request_result`.
#[cfg(test)]
pub fn parse_request(body: Option<&Value>) -> Result<StartParams, &'static str> {
    parse_request_with(body, StartParams::default())
}

/// Como `parse_request`, mas o que faltar fica com `base` (a última
/// preferência definida na GUI) em vez dos valores por omissão.
pub fn parse_request_with(
    body: Option<&Value>,
    base: StartParams,
) -> Result<StartParams, &'static str> {
    let mut p = base;
    let Some(b) = body else { return Ok(p) };
    let num = |key: &str, max: i64| -> Result<Option<i64>, &'static str> {
        match crate::protocol::body_get(b, key) {
            None => Ok(None),
            Some(v) => v
                .as_integer()
                .and_then(|i| i64::try_from(i).ok())
                .filter(|n| (1..=max).contains(n))
                .map(Some)
                .ok_or("invalid parameters"),
        }
    };
    if let Some(w) = num("width", 7680)? {
        p.width = w;
    }
    if let Some(h) = num("height", 4320)? {
        p.height = h;
    }
    if let Some(f) = num("fps", 120)? {
        p.fps = f;
    }
    if let Some(c) = crate::protocol::body_get(b, "codec") {
        p.codec = match c.as_text() {
            Some("h264") => "h264",
            Some("h265") => "h265",
            _ => return Err("unsupported codec"),
        };
    }
    Ok(p)
}

/// `webcam.request` (P→D): valida o pedido e recusa se não houver telemóvel
/// ligado ou se já houver câmara ativa (ou um `webcam.start` à espera do
/// vídeo). Em caso de sucesso devolve os parâmetros; quem chama responde com
/// `webcam.request_result` e só depois arranca por `request_start` — o mesmo
/// caminho do `Command2::StartWebcam`. `Err(motivo)` vai no resultado.
pub fn check_request(
    body: Option<&Value>,
    hud: &Arc<Mutex<HudState>>,
    pending: &PendingWebcam,
    phone_connected: bool,
) -> Result<StartParams, &'static str> {
    let base = hud
        .lock()
        .unwrap()
        .modules
        .webcam_pref
        .clone()
        .unwrap_or_default();
    let params = parse_request_with(body, base)?;
    if !phone_connected {
        return Err("no phone connected");
    }
    if pending.lock().unwrap().is_some() || hud.lock().unwrap().modules.webcam_active {
        return Err("already active");
    }
    Ok(params)
}

/// Corpo do `webcam.request_result`: `{ok, error?}`.
pub fn request_result_body<T>(result: &Result<T, &str>) -> Value {
    let mut map = vec![(Value::Text("ok".into()), Value::Bool(result.is_ok()))];
    if let Err(e) = result {
        map.push((Value::Text("error".into()), Value::Text((*e).into())));
    }
    Value::Map(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{body_get, body_get_bool, body_get_str};

    fn hud() -> Arc<Mutex<HudState>> {
        HudState::new(String::new())
    }

    fn map(pairs: Vec<(&str, Value)>) -> Value {
        Value::Map(
            pairs
                .into_iter()
                .map(|(k, v)| (Value::Text(k.into()), v))
                .collect(),
        )
    }

    fn cbor_roundtrip(v: &Value) -> Value {
        let mut bytes = Vec::new();
        ciborium::into_writer(v, &mut bytes).unwrap();
        ciborium::from_reader(bytes.as_slice()).unwrap()
    }

    #[test]
    fn webcam_state_roundtrips_through_cbor() {
        use hyprlink_proto::link::{CamCodec, CamLens};
        let b = cbor_roundtrip(&map(vec![
            ("width", Value::Integer(1280.into())),
            ("height", Value::Integer(720.into())),
            ("fps", Value::Integer(30.into())),
            ("codec", Value::Text("h264".into())),
            ("lens", Value::Text("front".into())),
            ("rotation", Value::Integer(90.into())),
            ("mirror", Value::Bool(true)),
        ]));
        let f = parse_state(Some(&b)).unwrap();
        assert_eq!((f.width, f.height, f.fps), (1280, 720, 30));
        assert_eq!(f.codec, CamCodec::H264);
        assert_eq!(
            (f.lens, f.rotation, f.mirror),
            (Some(CamLens::Front), 90, true)
        );
    }

    #[test]
    fn webcam_state_optional_fields_and_garbage() {
        let b = map(vec![
            ("width", Value::Integer(640.into())),
            ("height", Value::Integer(480.into())),
            ("fps", Value::Integer(15.into())),
            ("codec", Value::Text("h265".into())),
        ]);
        let f = parse_state(Some(&b)).unwrap();
        assert_eq!((f.lens, f.rotation, f.mirror), (None, 0, false));
        let bad = map(vec![("width", Value::Integer(640.into()))]);
        assert!(parse_state(Some(&bad)).is_none());
        assert!(parse_state(None).is_none());
        let mjpeg = map(vec![
            ("width", Value::Integer(640.into())),
            ("height", Value::Integer(480.into())),
            ("fps", Value::Integer(15.into())),
            ("codec", Value::Text("mjpeg".into())),
        ]);
        assert!(parse_state(Some(&mjpeg)).is_none());
    }

    #[test]
    fn omitted_fields_follow_the_gui_preference() {
        let h = hud();
        h.lock().unwrap().modules.webcam_pref = Some(StartParams {
            width: 1920,
            height: 1080,
            fps: 60,
            codec: "h265",
        });
        let b = map(vec![("fps", Value::Integer(30.into()))]);
        let p = check_request(Some(&b), &h, &new_pending(), true).unwrap();
        assert_eq!(
            (p.width, p.height, p.fps, p.codec),
            (1920, 1080, 30, "h265")
        );
    }

    #[test]
    fn webcam_configure_body_roundtrips_through_cbor() {
        let b = cbor_roundtrip(&configure_body(1920, 1080, 60, "h265"));
        let p = parse_request(Some(&b)).unwrap();
        assert_eq!(
            (p.width, p.height, p.fps, p.codec),
            (1920, 1080, 60, "h265")
        );
    }

    #[test]
    fn state_updates_pending_dims_and_session() {
        let h = hud();
        let pending = new_pending();
        *pending.lock().unwrap() = Some((9, 1920, 1080));
        state::set_webcam_session(&h, Some((9, 1920, 1080)));
        let b = map(vec![
            ("width", Value::Integer(1280.into())),
            ("height", Value::Integer(720.into())),
            ("fps", Value::Integer(30.into())),
            ("codec", Value::Text("h264".into())),
        ]);
        apply_state(parse_state(Some(&b)).unwrap(), &new_handle(), &pending, &h);
        assert_eq!(*pending.lock().unwrap(), Some((9, 1280, 720)));
        let m = &h.lock().unwrap().modules;
        assert_eq!(m.webcam_session, Some((9, 1280, 720)));
        assert_eq!(m.webcam_format.map(|f| f.fps), Some(30));
    }

    #[test]
    fn saida_orientada_pela_rotacao() {
        // O encoder manda 1280×720; com o telemóvel ao alto a app pede 90°.
        assert_eq!(oriented(1280, 720, 0), (1280, 720));
        assert_eq!(oriented(1280, 720, 90), (720, 1280));
        assert_eq!(oriented(1280, 720, 270), (720, 1280));
        assert_eq!(oriented(1280, 720, 180), (1280, 720));
        // O `webcam.state` já vem rodado (720×1280 com rotation 90): não roda
        // outra vez.
        assert_eq!(oriented(720, 1280, 90), (720, 1280));
        assert_eq!(oriented(720, 1280, 0), (1280, 720));
    }

    #[test]
    fn mudar_de_tamanho_so_sem_programa_a_ler() {
        assert_eq!(resize_plan((1280, 720), (1280, 720), true), Resize::Keep);
        assert_eq!(
            resize_plan((1280, 720), (720, 1280), false),
            Resize::Rebuild
        );
        assert_eq!(
            resize_plan((1280, 720), (720, 1280), true),
            Resize::KeepForConsumer
        );
    }

    #[test]
    fn transform_fica_guardado_sem_pipeline() {
        // A app manda-o antes do vídeo: o pipeline tem de nascer já rodado.
        let h = hud();
        apply_transform(&new_handle(), &h, 90, true);
        assert_eq!(h.lock().unwrap().modules.webcam_transform, (90, true));
        apply_transform(&new_handle(), &h, 45, false);
        assert_eq!(h.lock().unwrap().modules.webcam_transform, (0, false));
        state::set_webcam_session(&h, Some((1, 1280, 720)));
        apply_transform(&new_handle(), &h, 270, false);
        assert_eq!(wanted_output(&h, (0, 0)), (720, 1280));
    }

    #[test]
    fn quem_tem_a_camera_aberta() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("hyprlink-webcam-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dev = std::path::Path::new("/dev/video42");
        for (pid, target) in [("10", "/dev/video42"), ("11", "/dev/null")] {
            let fd = root.join(pid).join("fd");
            std::fs::create_dir_all(&fd).unwrap();
            symlink(target, fd.join("3")).unwrap();
        }
        std::fs::create_dir_all(root.join("self")).unwrap();
        assert!(opened_by_others(&root, dev, 99));
        // Só este processo (o nosso v4l2sink) a tem aberta: ninguém mais.
        assert!(!opened_by_others(&root, dev, 10));
        assert!(!opened_by_others(&root.join("nada"), dev, 99));
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn plan(decoder: Decoder, sink: Sink) -> Plan {
        Plan {
            decoder,
            sink,
            phone: "Poco F4".into(),
            fps: 30,
            format: PwFormat::I420,
        }
    }

    fn v4l2() -> Sink {
        Sink::V4l2("/dev/null".into())
    }

    #[test]
    fn escolha_do_descodificador() {
        use DecoderChoice::*;
        // auto: VA se existir, senão CPU, sem aviso.
        assert_eq!(pick_decoder(Auto, true, true), (Decoder::va(true), false));
        assert_eq!(pick_decoder(Auto, false, true), (Decoder::va(false), false));
        assert_eq!(pick_decoder(Auto, true, false), (Decoder::cpu(true), false));
        // cpu: nunca VA.
        assert_eq!(pick_decoder(Cpu, false, true), (Decoder::cpu(false), false));
        // gpu: VA se existir; sem VA recua para o CPU e pede aviso.
        assert_eq!(pick_decoder(Gpu, true, true), (Decoder::va(true), false));
        assert_eq!(pick_decoder(Gpu, true, false), (Decoder::cpu(true), true));
        let d = Decoder::va(true);
        assert_eq!(
            (d.element, d.parser, d.gpu),
            ("vah265dec", "h265parse", true)
        );
        let d = Decoder::cpu(false);
        assert_eq!(
            (d.element, d.parser, d.gpu),
            ("avdec_h264", "h264parse", false)
        );
        assert_eq!(Decoder::va(false).kind(), CamDecoder::Gpu);
        assert_eq!(Decoder::cpu(false).kind(), CamDecoder::Cpu);
    }

    #[test]
    fn falha_do_va_recua_para_o_mesmo_codec_no_cpu() {
        let mut p = plan(Decoder::va(true), v4l2());
        p.decoder = Decoder::cpu(p.decoder.h265);
        assert_eq!(p.decoder, Decoder::cpu(true));
        assert!(is_va_element("vah265dec0"));
        assert!(is_va_element("vapostproc1"));
        assert!(!is_va_element("v4l2sink0"));
        assert!(!is_va_element("videoconvert0"));
        assert!(!is_va_element("avdec_h264-0"));
    }

    #[test]
    fn pipeline_em_cada_combinacao() {
        let combos = [
            (
                Decoder::va(false),
                v4l2(),
                "vah264dec",
                "h264parse",
                "v4l2sink device=/dev/null",
            ),
            (
                Decoder::va(true),
                Sink::Pipewire,
                "vah265dec",
                "h265parse",
                "pipewiresink name=camsink mode=provide",
            ),
            (
                Decoder::cpu(false),
                v4l2(),
                "avdec_h264",
                "h264parse",
                "v4l2sink device=/dev/null",
            ),
            (
                Decoder::cpu(true),
                Sink::Pipewire,
                "avdec_h265",
                "h265parse",
                "pipewiresink name=camsink mode=provide",
            ),
        ];
        for (dec, sink, el, parser, end) in combos {
            let d = pipeline_description(&plan(dec, sink), (720, 1280));
            assert!(
                d.contains(&format!("! {parser} ! {el} ! videoconvert")),
                "{d}"
            );
            assert!(d.contains("videoflip name=rotate") && d.contains("videoflip name=mirror"));
            assert!(d.contains("width=720,height=1280"), "{d}");
            assert!(
                d.trim_end().ends_with(&format!("{end} sync=false")) || d.contains(end),
                "{d}"
            );
            // O `videoconvert` vem antes do flip/escala (desce a memória VA).
            assert!(d.find("videoconvert").unwrap() < d.find("videoflip").unwrap());
        }
    }

    #[test]
    fn saida_auto_com_e_sem_modulo() {
        use hyprlink_env::camera::Output;
        use hyprlink_env::overrides::OutputChoice::*;
        // Módulo carregado ou carregável: v4l2loopback.
        assert_eq!(
            pick_output(Auto, true, None, true),
            Ok(Output::V4l2loopback)
        );
        assert_eq!(
            pick_output(Auto, false, Some(true), true),
            Ok(Output::V4l2loopback)
        );
        // Módulo inexistente para o kernel: PipeWire.
        assert_eq!(
            pick_output(Auto, false, Some(false), true),
            Ok(Output::Pipewire)
        );
        // Forçado.
        assert_eq!(
            pick_output(V4l2loopback, false, Some(false), true),
            Ok(Output::V4l2loopback)
        );
        assert_eq!(
            pick_output(Pipewire, true, Some(true), true),
            Ok(Output::Pipewire)
        );
        // PipeWire pedido sem o plugin: erro claro.
        let e = pick_output(Pipewire, true, None, false).unwrap_err();
        assert!(e.contains("pipewiresink") && e.contains("gst-plugin-pipewire"));
    }

    #[test]
    fn propriedades_de_camera_pipewire() {
        gst::init().unwrap();
        let p = camera_properties("Poco F4");
        assert_eq!(p.get::<&str>("media.class").unwrap(), "Video/Source");
        assert_eq!(p.get::<&str>("media.role").unwrap(), "Camera");
        assert_eq!(p.get::<&str>("node.name").unwrap(), "hyprlink-camera");
        assert_eq!(
            p.get::<&str>("node.description").unwrap(),
            "HyprLink · Poco F4"
        );
    }

    fn have(factories: &[&str]) -> bool {
        gst::init().is_ok()
            && factories
                .iter()
                .all(|f| gst::ElementFactory::find(f).is_some())
    }

    #[test]
    fn pipeline_monta_com_barras_e_le_o_tamanho_de_saida() {
        if !have(&["avdec_h264", "h264parse", "v4l2sink"]) {
            eprintln!("GStreamer sem os plugins da webcam: teste saltado");
            return;
        }
        let desc = pipeline_description(&plan(Decoder::cpu(false), v4l2()), (720, 1280));
        let p = gst::parse::launch(&desc)
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        assert_eq!(pipeline_output(&p), Some((720, 1280)));
        set_flip(&p, 90, true);
        let rot = p.by_name("rotate").unwrap();
        assert_eq!(
            rot.property_value("method")
                .serialize()
                .unwrap()
                .to_string(),
            "clockwise"
        );
    }

    #[test]
    fn pipeline_pipewire_monta_com_as_propriedades_de_camera() {
        if !have(&["avdec_h264", "h264parse", "pipewiresink"]) {
            eprintln!("sem pipewiresink: teste saltado");
            return;
        }
        let desc = pipeline_description(&plan(Decoder::cpu(false), Sink::Pipewire), (1280, 720));
        let p = gst::parse::launch(&desc)
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        // Só montado (nunca a tocar): não cria nenhum nó no PipeWire do utilizador.
        let sink = p.by_name("camsink").unwrap();
        sink.set_property("stream-properties", camera_properties("Poco F4"));
        let props = sink
            .property::<Option<gst::Structure>>("stream-properties")
            .unwrap();
        assert_eq!(props.get::<&str>("media.class").unwrap(), "Video/Source");
    }

    // --- Saída PipeWire: integração com um consumidor real (`pipewiresrc`) ---

    fn pw_ready() -> bool {
        let socket = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|d| std::path::Path::new(&d).join("pipewire-0"));
        let ok = have(&[
            "videotestsrc",
            "x264enc",
            "h264parse",
            "avdec_h264",
            "pipewiresink",
            "pipewiresrc",
        ]) && socket.is_some_and(|s| s.exists());
        if !ok {
            eprintln!("sem PipeWire/GStreamer completo: teste saltado");
        }
        ok
    }

    /// 2 s de H.264 (uma unidade de acesso por item, keyframe de 15 em 15).
    fn encoded_au(w: i32, h: i32) -> Vec<Vec<u8>> {
        let p = gst::parse::launch(&format!(
            "videotestsrc num-buffers=60 pattern=ball ! \
             video/x-raw,width={w},height={h},framerate=30/1 ! \
             x264enc tune=zerolatency key-int-max=15 bframes=0 ! \
             video/x-h264,stream-format=byte-stream,alignment=au ! \
             appsink name=out sync=false"
        ))
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let sink = p
            .by_name("out")
            .unwrap()
            .downcast::<gstreamer_app::AppSink>()
            .unwrap();
        p.set_state(gst::State::Playing).unwrap();
        let mut out = Vec::new();
        while let Ok(sample) = sink.pull_sample() {
            out.push(sample.buffer().unwrap().map_readable().unwrap().to_vec());
        }
        p.set_state(gst::State::Null).unwrap();
        assert!(out.len() >= 50, "o encoder deu {} AUs", out.len());
        out
    }

    /// O pipeline real da saída PipeWire, com um nome de nó só do teste (não
    /// colide com a câmara do daemon do utilizador).
    fn pw_producer(node: &str, format: PwFormat, size: (i64, i64)) -> Built {
        let p = Plan {
            decoder: Decoder::cpu(false),
            sink: Sink::Pipewire,
            phone: "Teste".into(),
            fps: 30,
            format,
        };
        let pipeline = gst::parse::launch(&pipeline_description(&p, size))
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        let appsrc = pipeline
            .by_name("src")
            .unwrap()
            .downcast::<AppSrc>()
            .unwrap();
        let props = gst::Structure::builder("props")
            .field("media.class", "Video/Source")
            .field("media.role", "Camera")
            .field("node.name", node)
            .field("node.description", "HyprLink teste")
            .build();
        pipeline
            .by_name("camsink")
            .unwrap()
            .set_property("stream-properties", props);
        pipeline.set_state(gst::State::Playing).unwrap();
        Built {
            pipeline,
            appsrc,
            va_failure: Arc::default(),
            sink_failure: Arc::default(),
        }
    }

    /// Empurra o vídeo em tempo real (1 AU a cada 33 ms) durante `secs`;
    /// devolve (AUs empurradas, a `push_buffer` mais lenta).
    fn push_realtime(appsrc: &AppSrc, au: &[Vec<u8>], secs: u64) -> (usize, std::time::Duration) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        let (mut n, mut slowest) = (0, std::time::Duration::ZERO);
        let mut next = std::time::Instant::now();
        'outer: loop {
            for chunk in au {
                if std::time::Instant::now() >= end {
                    break 'outer;
                }
                let t = std::time::Instant::now();
                appsrc
                    .push_buffer(gst::Buffer::from_mut_slice(chunk.clone()))
                    .unwrap();
                slowest = slowest.max(t.elapsed());
                n += 1;
                next += std::time::Duration::from_millis(33);
                std::thread::sleep(next.saturating_duration_since(std::time::Instant::now()));
            }
        }
        (n, slowest)
    }

    fn pw_consumer(node: &str, caps: &str, secs: u64) -> Result<usize, String> {
        let p = gst::parse::launch(&format!(
            "pipewiresrc target-object={node} \
             stream-properties=\"props,media.role=Camera,media.category=Capture,media.type=Video\" \
             ! {caps} ! appsink name=out sync=false"
        ))
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let sink = p
            .by_name("out")
            .unwrap()
            .downcast::<gstreamer_app::AppSink>()
            .unwrap();
        if p.set_state(gst::State::Playing).is_err() {
            let _ = p.set_state(gst::State::Null);
            return Err("não arrancou".into());
        }
        let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        let mut frames = 0;
        while std::time::Instant::now() < end {
            if sink
                .try_pull_sample(gst::ClockTime::from_mseconds(200))
                .is_some()
            {
                frames += 1;
            } else if let Some(bus) = p.bus()
                && let Some(gst::MessageView::Error(e)) = bus
                    .pop_filtered(&[gst::MessageType::Error])
                    .as_ref()
                    .map(|m| m.view())
            {
                let why = e.error().to_string();
                let _ = p.set_state(gst::State::Null);
                return Err(why);
            }
        }
        let _ = p.set_state(gst::State::Null);
        Ok(frames)
    }

    fn consumer_recebe(format: PwFormat, w: i64, h: i64) {
        if !pw_ready() {
            return;
        }
        let node = format!("hyprlink-teste-{}-{}", format.name(), std::process::id());
        let au = encoded_au(w as i32, h as i32);
        let b = pw_producer(&node, format, (w, h));
        let feeder = {
            let src = b.appsrc.clone();
            std::thread::spawn(move || push_realtime(&src, &au, 13))
        };
        std::thread::sleep(std::time::Duration::from_secs(2));
        // Como um navegador: formato pedido + framerate fixo.
        let caps = format!("video/x-raw,format={},framerate=30/1", format.name());
        let frames = pw_consumer(&node, &caps, 10);
        let (pushed, _) = feeder.join().unwrap();
        let _ = b.pipeline.set_state(gst::State::Null);
        let frames = frames.expect("o consumidor devia receber frames");
        assert!(pushed > 300, "só {pushed} AUs empurradas");
        assert!(frames >= 150, "{format:?}: só {frames} frames em 10 s");
    }

    #[test]
    fn pipewire_consumidor_recebe_frames_i420() {
        consumer_recebe(PwFormat::I420, 320, 240);
    }

    #[test]
    fn pipewire_consumidor_recebe_frames_nv12() {
        consumer_recebe(PwFormat::Nv12, 320, 240);
    }

    #[test]
    fn pipewire_sem_consumidor_nao_bloqueia() {
        if !pw_ready() {
            return;
        }
        let node = format!("hyprlink-teste-solo-{}", std::process::id());
        let au = encoded_au(320, 240);
        let b = pw_producer(&node, PwFormat::I420, (320, 240));
        // Conta as frames que passam pelos caps de saída (já no fim da cadeia).
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pad = b
            .pipeline
            .by_name("outcaps")
            .unwrap()
            .static_pad("src")
            .unwrap();
        let counter = seen.clone();
        pad.add_probe(gst::PadProbeType::BUFFER, move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            gst::PadProbeReturn::Ok
        });
        let (pushed, slowest) = push_realtime(&b.appsrc, &au, 10);
        let through = seen.load(std::sync::atomic::Ordering::Relaxed);
        let still_playing = b.pipeline.current_state() == gst::State::Playing;
        let _ = b.pipeline.set_state(gst::State::Null);
        assert!(pushed >= 250, "só {pushed} AUs em 10 s");
        assert!(
            slowest < std::time::Duration::from_millis(500),
            "push_buffer bloqueou {slowest:?}"
        );
        assert!(
            through >= 200,
            "só {through} frames chegaram ao fim da cadeia"
        );
        assert!(still_playing);
    }

    #[test]
    fn pipewire_formato_recusado_chega_a_ranhura_do_diario() {
        if !pw_ready() {
            return;
        }
        let node = format!("hyprlink-teste-recusa-{}", std::process::id());
        let au = encoded_au(320, 240);
        let b = pw_producer(&node, PwFormat::I420, (320, 240));
        let h = hud();
        watch_bus(&b.pipeline, &h, None, Some(b.sink_failure.clone()));
        let src = b.appsrc.clone();
        let feeder = std::thread::spawn(move || {
            let end = std::time::Instant::now() + std::time::Duration::from_secs(6);
            while std::time::Instant::now() < end {
                for chunk in &au {
                    let _ = src.push_buffer(gst::Buffer::from_mut_slice(chunk.clone()));
                    std::thread::sleep(std::time::Duration::from_millis(33));
                }
            }
        });
        std::thread::sleep(std::time::Duration::from_secs(2));
        // O programa pede um formato que o nó não oferece.
        let r = pw_consumer(&node, "video/x-raw,format=YUY2", 3);
        assert!(r.is_err(), "o consumidor devia falhar: {r:?}");
        let mut why = None;
        for _ in 0..40 {
            why = b.sink_failure.lock().unwrap().clone();
            if why.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = b.pipeline.set_state(gst::State::Null);
        feeder.join().unwrap();
        assert!(why.is_some(), "o erro do pipewiresink não foi registado");
    }

    #[test]
    fn formatos_pipewire_rodam_e_os_caps_levam_framerate() {
        assert_eq!(PwFormat::I420.next(), PwFormat::Nv12);
        assert_eq!(PwFormat::Nv12.next(), PwFormat::Yuy2);
        assert_eq!(PwFormat::Yuy2.next(), PwFormat::I420);
        let c = pipewire_caps(1080, 1920, 60, PwFormat::Nv12);
        assert!(
            c.contains("format=NV12") && c.contains("framerate=60/1"),
            "{c}"
        );
        assert!(c.contains("width=1080,height=1920"));
        // Descrição: fps fixo, `videorate`, tee com ramo que descarta, appsrc
        // sem bloqueio; o v4l2loopback fica como estava.
        let d = pipeline_description(&plan(Decoder::cpu(false), Sink::Pipewire), (720, 1280));
        for part in [
            "format=I420",
            "framerate=30/1",
            "videorate",
            "tee name=camtee",
            "leaky=downstream",
            "fakesink",
            "block=false",
        ] {
            assert!(d.contains(part), "falta {part}: {d}");
        }
        let v = pipeline_description(&plan(Decoder::cpu(false), v4l2()), (720, 1280));
        assert!(v.contains("block=true") && !v.contains("videorate") && !v.contains("tee"));
    }

    /// Prova que o VA-API arranca e entrega memória de sistema ao fim da cadeia
    /// real (sem telemóvel nem `/dev/video42`: acaba num `fakesink`).
    fn va_roundtrip(h265: bool) {
        let (enc, va) = if h265 {
            ("x265enc", "vah265dec")
        } else {
            ("x264enc", "vah264dec")
        };
        let parser = if h265 { "h265parse" } else { "h264parse" };
        if !have(&[enc, va, parser, "videoconvert", "videotestsrc"]) {
            eprintln!("sem {enc}/{va}: teste ignorado (sem VA-API neste ambiente)");
            return;
        }
        let caps = output_caps(720, 1280);
        let desc = format!(
            "videotestsrc num-buffers=45 ! video/x-raw,width=1280,height=720,framerate=30/1 \
             ! {enc} key-int-max=15 ! {parser} ! {va} ! videoconvert \
             ! videoflip name=rotate method=clockwise ! videoscale add-borders=true \
             ! capsfilter caps=\"{caps}\" ! fakesink name=end sync=false"
        );
        let p = gst::parse::launch(&desc)
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        p.set_state(gst::State::Playing).unwrap();
        let bus = p.bus().unwrap();
        let msg = bus.timed_pop_filtered(
            gst::ClockTime::from_seconds(30),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        let ok = matches!(
            msg.as_ref().map(|m| m.view()),
            Some(gst::MessageView::Eos(_))
        );
        let detail = format!("{msg:?}");
        // Caps à entrada do fakesink: memória de sistema, com o tamanho de saída.
        let pad = p.by_name("end").unwrap().static_pad("sink").unwrap();
        let caps = pad.current_caps();
        let _ = p.set_state(gst::State::Null);
        assert!(ok, "pipeline com {va} não chegou ao EOS: {detail}");
        let caps = caps.expect("sem caps negociados");
        assert!(!caps.to_string().contains("memory:VAMemory"), "{caps}");
        let st = caps.structure(0).unwrap();
        assert_eq!(
            (
                st.get::<i32>("width").unwrap(),
                st.get::<i32>("height").unwrap()
            ),
            (720, 1280)
        );
    }

    #[test]
    fn va_h264_arranca_e_desce_para_memoria_de_sistema() {
        va_roundtrip(false);
    }

    #[test]
    fn va_h265_arranca_e_desce_para_memoria_de_sistema() {
        va_roundtrip(true);
    }

    #[test]
    fn parse_uses_defaults_for_omitted_fields() {
        assert_eq!(parse_request(None).unwrap(), StartParams::default());
        let b = map(vec![("fps", Value::Integer(30.into()))]);
        let p = parse_request(Some(&b)).unwrap();
        assert_eq!((p.width, p.height, p.fps, p.codec), (1280, 720, 30, "h264"));
    }

    #[test]
    fn parse_rejects_bad_values() {
        let b = map(vec![("codec", Value::Text("mjpeg".into()))]);
        assert_eq!(parse_request(Some(&b)), Err("unsupported codec"));
        let b = map(vec![("width", Value::Integer(0.into()))]);
        assert_eq!(parse_request(Some(&b)), Err("invalid parameters"));
    }

    #[test]
    fn valid_request_carries_the_requested_params() {
        let b = map(vec![
            ("width", Value::Integer(1920.into())),
            ("height", Value::Integer(1080.into())),
            ("codec", Value::Text("h265".into())),
        ]);
        let r = check_request(Some(&b), &hud(), &new_pending(), true);
        let p = r.clone().unwrap();
        assert_eq!(
            (p.width, p.height, p.fps, p.codec),
            (1920, 1080, 24, "h265")
        );
        assert_eq!(body_get_bool(&request_result_body(&r), "ok"), Some(true));
        assert!(body_get(&request_result_body(&r), "error").is_none());
    }

    #[test]
    fn already_active_is_refused() {
        let h = hud();
        state::set_webcam_active(&h, true);
        let r = check_request(None, &h, &new_pending(), true);
        assert_eq!(r, Err("already active"));
        // Um `webcam.start` ainda à espera do vídeo também conta como ativa.
        let pending = new_pending();
        *pending.lock().unwrap() = Some((7, 1280, 720));
        let r = check_request(None, &hud(), &pending, true);
        assert_eq!(r, Err("already active"));
        let body = request_result_body(&r);
        assert_eq!(body_get_bool(&body, "ok"), Some(false));
        assert_eq!(body_get_str(&body, "error"), Some("already active"));
    }

    #[test]
    fn no_phone_connected_is_reported() {
        let r = check_request(None, &hud(), &new_pending(), false);
        assert_eq!(r, Err("no phone connected"));
        let body = request_result_body(&r);
        assert_eq!(body_get_str(&body, "error"), Some("no phone connected"));
    }

    #[test]
    fn preparar_a_camera_virtual() {
        use hyprlink_env::camera::DeviceState::*;
        assert_eq!(prepare(Ours, true, None, 42, "k"), Prepare::Ready);
        assert_eq!(prepare(Free, false, Some(true), 42, "k"), Prepare::Load);
        assert_eq!(prepare(Free, false, None, 42, "k"), Prepare::Load);
        let Prepare::Refuse(m) = prepare(Busy, false, None, 42, "k") else {
            panic!("devia recusar")
        };
        assert!(m.contains("v4l2_device_nr") && m.contains("video42"));
        let Prepare::Refuse(m) = prepare(Free, false, Some(false), 42, "6.12.4-lts") else {
            panic!("devia recusar")
        };
        assert!(m.contains("6.12.4-lts") && m.contains("v4l2loopback"));
        assert!(matches!(
            prepare(Free, true, None, 10, "k"),
            Prepare::Refuse(_)
        ));
    }
}
