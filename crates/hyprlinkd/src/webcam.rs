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

// ponytail: número fixo em vez de descobrir dinamicamente qual /dev/videoN
// o v4l2loopback criou — simples e determinístico; só um risco real, que é
// colidir com outro dispositivo físico nesse índice (improvável, 42 é bem
// acima de qualquer webcam/capturadora comum).
const V4L2_DEVICE_NR: u32 = 42;
const V4L2_DEVICE_PATH: &str = "/dev/video42";

fn ensure_v4l2loopback_loaded(hud: &Arc<Mutex<HudState>>) -> bool {
    if std::path::Path::new(V4L2_DEVICE_PATH).exists() {
        return true;
    }
    push_log(
        hud,
        "[i] webcam: carregando v4l2loopback (pode pedir sua senha)...".to_string(),
    );
    let status = std::process::Command::new("pkexec")
        .args([
            "modprobe",
            "v4l2loopback",
            &format!("video_nr={V4L2_DEVICE_NR}"),
            "card_label=HyprLink Webcam",
            "exclusive_caps=1",
        ])
        .status();
    match status {
        Ok(s) if s.success() && std::path::Path::new(V4L2_DEVICE_PATH).exists() => true,
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
/// (decodifica H.264/H.265 → escreve no `/dev/video42` via v4l2loopback) e
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
    let pipeline_str = format!(
        "appsrc name=src is-live=true format=time do-timestamp=true block=true \
         ! {parser} ! {decoder} ! videoconvert \
         ! videoflip name=rotate method=none ! videoflip name=mirror method=none \
         ! videoscale ! capsfilter caps=\"video/x-raw,width={width},height={height}\" \
         ! v4l2sink device={V4L2_DEVICE_PATH} sync=false"
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
    *handle.lock().unwrap() = Some(pipeline.clone());
    state::set_webcam_active(&hud, true);
    push_log(
        &hud,
        format!("[+] webcam: stream iniciado ({label}) · {V4L2_DEVICE_PATH}"),
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
    stop(&handle, &hud);
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
pub fn parse_request(body: Option<&Value>) -> Result<StartParams, &'static str> {
    let mut p = StartParams::default();
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
    let params = parse_request(body)?;
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
}
