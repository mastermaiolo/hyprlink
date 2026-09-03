//! Audio tap: ouvir o áudio do PC no telemóvel. Pipeline GStreamer
//! (`pipewiresrc` no monitor do sink padrão) — sem bindings C do
//! libpipewire, o GStreamer já faz esse trabalho.
//!
//! ponytail: pipeline reconstruído do zero a cada `audio.tap_start` (não
//! acompanha troca de sink padrão em tempo real); trocar de saída durante um
//! tap ativo exige parar e começar de novo. Suficiente pro uso normal
//! (ligar o tap, ouvir, desligar).

use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;

use crate::state::{push_log, HudState};

pub type TapHandle = Arc<Mutex<Option<gst::Pipeline>>>;

pub fn new_handle() -> TapHandle {
    Arc::new(Mutex::new(None))
}

fn default_sink_name() -> Option<String> {
    std::process::Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Para o tap em andamento, se houver (chamado por `audio.tap_stop` e
/// também antes de iniciar um novo tap, pra nunca ter dois ao mesmo tempo).
pub fn stop(handle: &TapHandle) {
    if let Some(pipeline) = handle.lock().unwrap().take() {
        let _ = pipeline.set_state(gst::State::Null);
    }
}

/// Como `stop()`, mas só mata o pipeline se ele ainda for exatamente o
/// mesmo `pipeline` desta chamada de `start()` — usado na limpeza de fim de
/// task (falha de escrita/EOF). Sem isso, um restart rápido (tap_stop
/// seguido de tap_start) corre risco de a limpeza atrasada da sessão antiga
/// matar o pipeline novo que já está tocando.
fn stop_if_current(handle: &TapHandle, pipeline: &gst::Pipeline) {
    let mut guard = handle.lock().unwrap();
    if guard.as_ref() == Some(pipeline) {
        let old = guard.take().unwrap();
        drop(guard);
        let _ = old.set_state(gst::State::Null);
    }
}

/// Inicia o tap: abre um stream unidirecional novo (`id` já combinado com o
/// telemóvel via `audio.tap_ready`), escreve o id de 8 bytes e então PCM cru
/// (16-bit LE, 48kHz, estéreo) indefinidamente até `stop()` ser chamado.
pub async fn start(connection: quinn::Connection, id: u64, handle: TapHandle, hud: Arc<Mutex<HudState>>) {
    stop(&handle);

    if let Err(e) = gst::init() {
        push_log(&hud, format!("[!] audio tap: GStreamer não inicializou: {e}"));
        return;
    }

    let Some(sink) = default_sink_name() else {
        push_log(&hud, "[!] audio tap: sem sink padrão detetado".to_string());
        return;
    };

    // ponytail: "target-object=<sink>.monitor" NÃO existe como nó nativo do
    // PipeWire (é convenção do PulseAudio) — sem correspondência, o
    // WirePlumber liga a captura à fonte padrão (o MICROFONE físico), não ao
    // monitor do sink. A forma certa de "escutar" um sink no PipeWire nativo:
    // apontar target-object pro próprio nó do sink e marcar a stream com
    // `stream.capture.sink=true`, que instrui o session manager a ligar nas
    // portas de monitor dele em vez das portas de entrada normais.
    let pipeline_str = format!(
        "pipewiresrc target-object=\"{sink}\" stream-properties=\"props,stream.capture.sink=true\" \
         ! audioconvert ! audioresample \
         ! audio/x-raw,format=S16LE,rate=48000,channels=2,layout=interleaved \
         ! appsink name=hyprlink_tap sync=false max-buffers=8 drop=true"
    );

    let pipeline = match gst::parse::launch(&pipeline_str) {
        Ok(el) => match el.downcast::<gst::Pipeline>() {
            Ok(p) => p,
            Err(_) => {
                push_log(&hud, "[!] audio tap: pipeline inesperado".to_string());
                return;
            }
        },
        Err(e) => {
            push_log(&hud, format!("[!] audio tap: falha ao montar o pipeline: {e}"));
            return;
        }
    };

    let Some(sink_el) = pipeline.by_name("hyprlink_tap") else {
        push_log(&hud, "[!] audio tap: appsink não encontrado no pipeline".to_string());
        return;
    };
    let appsink = match sink_el.downcast::<AppSink>() {
        Ok(s) => s,
        Err(_) => {
            push_log(&hud, "[!] audio tap: appsink com tipo inesperado".to_string());
            return;
        }
    };

    if pipeline.set_state(gst::State::Playing).is_err() {
        push_log(&hud, "[!] audio tap: não foi possível iniciar o pipeline".to_string());
        return;
    }
    *handle.lock().unwrap() = Some(pipeline.clone());
    push_log(&hud, format!("[+] audio tap iniciado · monitor de {sink}"));

    // ponytail: set_state(Playing) só reporta falha síncrona — se o
    // pipewiresrc falhar ao linkar (ex: sink sumiu, PipeWire reiniciou), o
    // erro chega de forma assíncrona pelo bus e ficava mudo, sem log nenhum.
    if let Some(bus) = pipeline.bus() {
        let hud_bus = hud.clone();
        std::thread::spawn(move || {
            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                match msg.view() {
                    gst::MessageView::Error(err) => {
                        push_log(
                            &hud_bus,
                            format!("[!] audio tap: erro no pipeline GStreamer: {} ({:?})", err.error(), err.debug()),
                        );
                        break;
                    }
                    gst::MessageView::Eos(_) => break,
                    _ => {}
                }
            }
        });
    }

    let Ok(mut send) = connection.open_uni().await else {
        push_log(&hud, "[!] audio tap: não foi possível abrir o stream".to_string());
        stop(&handle);
        return;
    };
    if send.write_all(&id.to_be_bytes()).await.is_err() {
        push_log(&hud, "[!] audio tap: falha ao escrever o id no stream".to_string());
        stop(&handle);
        return;
    }
    push_log(&hud, format!("[i] audio tap: stream aberto (id={id})"));

    // pull_sample() bloqueia — roda numa thread própria, encaminha os
    // buffers pro stream QUIC via canal assíncrono.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
    let hud_thread = hud.clone();
    std::thread::spawn(move || {
        loop {
            match appsink.pull_sample() {
                Ok(sample) => {
                    let Some(buffer) = sample.buffer() else { continue };
                    let Ok(map) = buffer.map_readable() else { continue };
                    if tx.blocking_send(map.as_slice().to_vec()).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    push_log(&hud_thread, format!("[!] audio tap: pull_sample parou: {e}"));
                    break;
                }
            }
        }
    });

    let handle_cleanup = handle.clone();
    let pipeline_cleanup = pipeline.clone();
    tokio::spawn(async move {
        let mut total: u64 = 0;
        let mut last_logged: u64 = 0;
        while let Some(chunk) = rx.recv().await {
            if let Err(e) = send.write_all(&chunk).await {
                push_log(&hud, format!("[!] audio tap: escrita no stream falhou: {e}"));
                break;
            }
            total += chunk.len() as u64;
            if total.saturating_sub(last_logged) >= 1_000_000 {
                last_logged = total;
                push_log(&hud, format!("[i] audio tap: {} KB enviados", total / 1024));
            }
        }
        push_log(&hud, format!("[i] audio tap: encerrado ({} KB no total)", total / 1024));
        let _ = send.finish();
        // ponytail: sem isso o pipeline GStreamer fica "zumbi" — rodando em
        // Playing mesmo depois da escrita QUIC falhar (peer desconectou). Só
        // mata se ainda for ESTE pipeline — um restart rápido (tap_stop +
        // tap_start) já pode ter posto um pipeline novo no handle.
        stop_if_current(&handle_cleanup, &pipeline_cleanup);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Não roda em CI — precisa de PipeWire real com um sink padrão.
    /// `cargo test -- --ignored manual_pipeline`: monta o pipeline sozinho
    /// (sem QUIC) e confirma que pelo menos um buffer de áudio real sai do
    /// monitor do sink padrão.
    #[test]
    #[ignore]
    fn manual_pipeline() {
        gst::init().expect("GStreamer deveria inicializar");
        let sink = default_sink_name().expect("deveria haver um sink padrão");
        println!("sink: {sink}");

        let pipeline_str = format!(
            "pipewiresrc target-object=\"{sink}\" stream-properties=\"props,stream.capture.sink=true\" \
             ! audioconvert ! audioresample \
             ! audio/x-raw,format=S16LE,rate=48000,channels=2,layout=interleaved \
             ! appsink name=hyprlink_tap sync=false max-buffers=8 drop=true"
        );
        let pipeline =
            gst::parse::launch(&pipeline_str).unwrap().downcast::<gst::Pipeline>().unwrap();
        let appsink = pipeline.by_name("hyprlink_tap").unwrap().downcast::<AppSink>().unwrap();
        pipeline.set_state(gst::State::Playing).expect("pipeline deveria iniciar");

        let sample = appsink.pull_sample().expect("deveria sair pelo menos uma amostra");
        let buffer = sample.buffer().expect("amostra deveria ter buffer");
        let map = buffer.map_readable().expect("buffer deveria ser legível");
        println!("recebido: {} bytes", map.len());
        assert!(!map.is_empty(), "esperava bytes de PCM reais");

        pipeline.set_state(gst::State::Null).ok();
    }
}
