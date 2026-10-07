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
    let status = std::process::Command::new("pkexec")
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
}

/// Como `stop`, mas só se o pipeline em `handle` for o `mine` — um stream
/// substituído (reinício local no telemóvel, mesmo id) não pode derrubar o
/// pipeline que o substituiu quando o seu `feed()` antigo acaba.
fn stop_if_current(handle: &WebcamHandle, hud: &Arc<Mutex<HudState>>, mine: &gst::Pipeline) {
    let current = handle.lock().unwrap().as_ref() == Some(mine);
    if current {
        stop(handle, hud);
    } else {
        let _ = mine.set_state(gst::State::Null);
    }
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

/// Aplica um `webcam.state`: guarda o formato efetivo e, se a resolução
/// mudou a meio, atualiza o pedido pendente e os caps do pipeline vivo —
/// sem pedir a câmara de novo.
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
    let changed = {
        let mut s = hud.lock().unwrap();
        match s.modules.webcam_session.as_mut() {
            Some(sess) if (sess.1, sess.2) != (w, h) => {
                sess.1 = w;
                sess.2 = h;
                true
            }
            _ => false,
        }
    };
    if changed && let Some(pipeline) = handle.lock().unwrap().clone() {
        if let Some(el) = pipeline.by_name("outcaps") {
            el.set_property("caps", output_caps(w, h));
        }
        push_log(
            hud,
            format!("[i] webcam: formato do telemóvel agora {w}×{h}"),
        );
    }
}

fn output_caps(width: i64, height: i64) -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("width", width as i32)
        .field("height", height as i32)
        .build()
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

/// Aplica uma rotação/espelho vindos de `webcam.transform` (P→D) ao
/// pipeline em andamento, se houver — elementos nomeados `rotate`/`mirror`
/// cuidam de cada eixo separadamente (evita a matemática de combinar os 8
/// métodos diagonais do `videoflip` numa propriedade só).
pub fn apply_transform(handle: &WebcamHandle, rotation: i64, mirror: bool) {
    let Some(pipeline) = handle.lock().unwrap().clone() else {
        return;
    };
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

/// Trata o uni-stream de vídeo já reconhecido pelo roteador (id bateu com
/// `PendingWebcam`): lê o byte de codec, monta o pipeline GStreamer
/// (decodifica H.264/H.265 → escreve no `/dev/video{N}` via v4l2loopback; N = `v4l2_device_nr`, 42 por omissão) e
/// alimenta o `appsrc` com os bytes crus conforme chegam.
pub async fn feed(
    mut recv: quinn::RecvStream,
    handle: WebcamHandle,
    hud: Arc<Mutex<HudState>>,
    width: i64,
    height: i64,
) {
    if !ensure_v4l2loopback_loaded(&hud) {
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
    let (decoder, label) = match codec_byte[0] {
        0x02 => ("avdec_h265", "H.265"),
        _ => ("avdec_h264", "H.264"),
    };
    state::set_webcam_codec(&hud, label);
    let parser = if decoder == "avdec_h265" {
        "h265parse"
    } else {
        "h264parse"
    };

    if let Err(e) = gst::init() {
        push_log(&hud, format!("[!] webcam: GStreamer não inicializou: {e}"));
        return;
    }

    // ponytail: escala pro tamanho pedido sem preservar proporção em
    // rotações de 90/270° (a imagem fica esticada em vez de com barras) —
    // aceitável pra primeira versão; upgrade seria `videobox` calculando
    // padding a partir da resolução real pós-`rotate`.
    let dev_path = device_path();
    let pipeline_str = format!(
        "appsrc name=src is-live=true format=time do-timestamp=true block=true \
         ! {parser} ! {decoder} ! videoconvert \
         ! videoflip name=rotate method=none ! videoflip name=mirror method=none \
         ! videoscale ! capsfilter name=outcaps caps=\"video/x-raw,width={width},height={height}\" \
         ! v4l2sink device={dev_path} sync=false"
    );
    let pipeline = match gst::parse::launch(&pipeline_str) {
        Ok(el) => match el.downcast::<gst::Pipeline>() {
            Ok(p) => p,
            Err(_) => {
                push_log(&hud, "[!] webcam: pipeline inesperado".to_string());
                return;
            }
        },
        Err(e) => {
            push_log(&hud, format!("[!] webcam: falha ao montar o pipeline: {e}"));
            return;
        }
    };
    let Some(appsrc) = pipeline
        .by_name("src")
        .and_then(|e| e.downcast::<AppSrc>().ok())
    else {
        push_log(
            &hud,
            "[!] webcam: appsrc não encontrado no pipeline".to_string(),
        );
        return;
    };
    if pipeline.set_state(gst::State::Playing).is_err() {
        push_log(
            &hud,
            "[!] webcam: não foi possível iniciar o pipeline".to_string(),
        );
        return;
    }
    // Reinício local no telemóvel (mesmo id): o stream novo substitui o
    // pipeline anterior; o `feed()` antigo vê o `push_buffer` falhar e sai.
    let previous = handle.lock().unwrap().replace(pipeline.clone());
    let restarted = previous.is_some();
    if let Some(old) = previous {
        let _ = old.set_state(gst::State::Null);
    }
    if !hud.lock().unwrap().modules.webcam_active {
        state::set_webcam_active(&hud, true);
    }
    push_log(
        &hud,
        format!(
            "[+] webcam: stream {} ({label}) · {dev_path}",
            if restarted { "reiniciado" } else { "iniciado" }
        ),
    );

    // timed_pop + Weak, como em mic.rs/tap.rs: um stop limpo não posta
    // Eos/Error, e iter_timed(NONE) deixava esta thread presa para sempre
    // (uma por cada stream de câmara já terminado).
    if let Some(bus) = pipeline.bus() {
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

    // ponytail: timeout de leitura, não só EOF/erro — se o app crashar no
    // telemóvel mas o serviço de conexão sobreviver (visto ao vivo: uma
    // rotação derrubou a câmara mas não a conexão QUIC), o stream de vídeo
    // fica preso pra sempre sem EOF nem erro, e o pipeline (+ sua thread)
    // vaza rodando até o daemon reiniciar. 15s sem nenhum frame é folga de
    // sobra pra qualquer engasgo real de rede.
    const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    let mut window_bytes: u64 = 0;
    let mut window_start = std::time::Instant::now();
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
        let gst_buf = gst::Buffer::from_mut_slice(buf[..n].to_vec());
        if appsrc.push_buffer(gst_buf).is_err() {
            break;
        }
        total += n as u64;
        window_bytes += n as u64;
        let elapsed = window_start.elapsed();
        if elapsed >= std::time::Duration::from_secs(1) {
            let mbps = (window_bytes as f64 * 8.0) / elapsed.as_secs_f64() / 1_000_000.0;
            state::set_webcam_mbps(&hud, mbps);
            window_bytes = 0;
            window_start = std::time::Instant::now();
        }
    }
    push_log(
        &hud,
        format!(
            "[i] webcam: stream encerrado ({} KB recebidos)",
            total / 1024
        ),
    );
    let _ = appsrc.end_of_stream();
    stop_if_current(&handle, &hud, &pipeline);
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
