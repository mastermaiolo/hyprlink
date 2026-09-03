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

fn default_monitor_source() -> Option<String> {
    let default_sink = std::process::Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;
    Some(format!("{default_sink}.monitor"))
}

/// Para o tap em andamento, se houver (chamado por `audio.tap_stop` e
/// também antes de iniciar um novo tap, pra nunca ter dois ao mesmo tempo).
pub fn stop(handle: &TapHandle) {
    if let Some(pipeline) = handle.lock().unwrap().take() {
        let _ = pipeline.set_state(gst::State::Null);
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

    let Some(monitor) = default_monitor_source() else {
        push_log(&hud, "[!] audio tap: sem sink padrão detetado".to_string());
        return;
    };

    let pipeline_str = format!(
        "pipewiresrc target-object=\"{monitor}\" ! audioconvert ! audioresample \
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
    push_log(&hud, format!("[+] audio tap iniciado · monitor {monitor}"));

    let Ok(mut send) = connection.open_uni().await else {
        push_log(&hud, "[!] audio tap: não foi possível abrir o stream".to_string());
        stop(&handle);
        return;
    };
    if send.write_all(&id.to_be_bytes()).await.is_err() {
        stop(&handle);
        return;
    }

    // pull_sample() bloqueia — roda numa thread própria, encaminha os
    // buffers pro stream QUIC via canal assíncrono.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
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
                Err(_) => break, // EOS ou pipeline parado
            }
        }
    });

    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            if send.write_all(&chunk).await.is_err() {
                break;
            }
        }
        let _ = send.finish();
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
        let monitor = default_monitor_source().expect("deveria haver um sink padrão");
        println!("monitor: {monitor}");

        let pipeline_str = format!(
            "pipewiresrc target-object=\"{monitor}\" ! audioconvert ! audioresample \
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
