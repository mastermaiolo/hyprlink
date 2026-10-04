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

use crate::active::{push, ActiveConn};
use crate::state::{self, push_log, HudState};

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
    push_log(hud, "[i] webcam: carregando v4l2loopback (pode pedir sua senha)...".to_string());
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
pub async fn request_start(active: &ActiveConn, pending: &PendingWebcam, width: i64, height: i64, fps: i64, codec: &str) -> bool {
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
    let Some(pipeline) = handle.lock().unwrap().clone() else { return };
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
pub async fn feed(mut recv: quinn::RecvStream, handle: WebcamHandle, hud: Arc<Mutex<HudState>>, width: i64, height: i64) {
    if !ensure_v4l2loopback_loaded(&hud) {
        return;
    }

    let mut codec_byte = [0u8; 1];
    if recv.read_exact(&mut codec_byte).await.is_err() {
        push_log(&hud, "[!] webcam: stream fechou antes do byte de codec".to_string());
        return;
    }
    let (decoder, label) = match codec_byte[0] {
        0x02 => ("avdec_h265", "H.265"),
        _ => ("avdec_h264", "H.264"),
    };
    let parser = if decoder == "avdec_h265" { "h265parse" } else { "h264parse" };

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
    let Some(appsrc) = pipeline.by_name("src").and_then(|e| e.downcast::<AppSrc>().ok()) else {
        push_log(&hud, "[!] webcam: appsrc não encontrado no pipeline".to_string());
        return;
    };
    if pipeline.set_state(gst::State::Playing).is_err() {
        push_log(&hud, "[!] webcam: não foi possível iniciar o pipeline".to_string());
        return;
    }
    *handle.lock().unwrap() = Some(pipeline.clone());
    state::set_webcam_active(&hud, true);
    push_log(&hud, format!("[+] webcam: stream iniciado ({label}) · {V4L2_DEVICE_PATH}"));

    if let Some(bus) = pipeline.bus() {
        let hud_bus = hud.clone();
        std::thread::spawn(move || {
            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                match msg.view() {
                    gst::MessageView::Error(err) => {
                        push_log(&hud_bus, format!("[!] webcam: erro no pipeline GStreamer: {} ({:?})", err.error(), err.debug()));
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
                push_log(&hud, "[!] webcam: sem dados há 15s, encerrando stream travado".to_string());
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
    push_log(&hud, format!("[i] webcam: stream encerrado ({} KB recebidos)", total / 1024));
    let _ = appsrc.end_of_stream();
    stop(&handle, &hud);
}
