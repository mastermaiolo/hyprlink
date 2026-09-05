//! Microfone do telemóvel como entrada virtual do PC (PROTOCOL.md
//! §webcam, `webcam.mic_start`/`webcam.mic_stop`): o próprio `pipewiresink`
//! do GStreamer se registra no PipeWire com `media.class=Audio/Source` —
//! aparece como microfone de verdade em qualquer app (Chrome, OBS,
//! pavucontrol), confirmado ao vivo.
//!
//! ponytail: tentei duas abordagens mais complicadas antes desta pro nó
//! virtual, ambas erradas — não repetir:
//! 1. Sink nulo comum (`pactl load-module module-null-sink`): o
//!    "microfone" resultante seria o *monitor* do sink, e navegadores/apps
//!    de gravação escondem fontes do tipo monitor de propósito (proteção
//!    contra escutar a reprodução do sistema escondido) — nunca aparecia
//!    no Chrome/OBS.
//! 2. `pw-loopback` com capture-props marcado `Audio/Source`: os dois nós
//!    que ele cria (capture E playback) só expõem portas de SAÍDA — não
//!    existe porta de entrada nenhuma pra alimentar com os bytes do
//!    telemóvel. Também instável: crasha (segfault confirmado) se
//!    `media.class=Audio/Sink` for passado no lado de playback.
//! A solução de verdade (`pipewiresink` com `media.class=Audio/Source`
//! direto no `stream-properties`) dispensa qualquer processo externo.
//!
//! ponytail: o `appsrc`/`pipewiresink` PRECISA ser alimentado sempre pela
//! MESMA thread do SO — o loop de leitura da rede é `async` (tokio
//! multi-thread), e a task pode migrar de thread a cada `.await`; empurrar
//! pro PipeWire a partir de threads que ficam trocando corrompia o
//! áudio (saía quase todo esmagado em 80-300Hz, confirmado comparando o
//! PCM cru real com a saída do pipeline — isolei testando cada outra
//! hipótese, uma por uma: alinhamento de bytes ímpar, PTS por relógio de
//! chegada vs contagem de amostras, elemento `volume`, `sync` do
//! `pipewiresink`, disputa de CPU com a GUI, duas pipelines "ao vivo"
//! concorrentes no mesmo processo — só migrar a interação com o GStreamer
//! pra uma thread dedicada via canal resolveu). Mesmo padrão que `tap.rs`
//! já usa (só que invertido: lá é o GStreamer que produz e o tokio
//! consome; aqui o tokio consome a rede e uma thread dedicada alimenta o
//! GStreamer).

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSrc;

use crate::active::{push, ActiveConn};
use crate::state::{self, push_log, HudState};

/// Pede pro telemóvel ligar/desligar o mic dele (botão na GUI do PC) —
/// equivalente a tocar o botão por lá; o telemóvel decide se aceita (ex:
/// permissão concedida) e o `webcam.mic_start`/uni-stream normal segue
/// depois por conta dele, sem resposta direta a este pedido.
pub async fn request_start(active: &ActiveConn) -> bool {
    push(active, "webcam.mic_start_request", None).await.is_some()
}

pub async fn request_stop(active: &ActiveConn) -> bool {
    push(active, "webcam.mic_stop_request", None).await.is_some()
}

pub type MicHandle = Arc<Mutex<Option<gst::Pipeline>>>;

pub fn new_handle() -> MicHandle {
    Arc::new(Mutex::new(None))
}

/// Id do `webcam.mic_start` esperado no próximo uni-stream.
pub type PendingMic = Arc<Mutex<Option<u64>>>;

pub fn new_pending() -> PendingMic {
    Arc::new(Mutex::new(None))
}

pub fn stop(handle: &MicHandle, hud: &Arc<Mutex<HudState>>) {
    if let Some(pipeline) = handle.lock().unwrap().take() {
        let _ = pipeline.set_state(gst::State::Null);
        state::set_mic_active(hud, false);
        push_log(hud, "[i] microfone: stream encerrado".to_string());
    }
}

/// Trata o uni-stream de PCM já reconhecido pelo roteador (id bateu com
/// `PendingMic`): o loop de leitura da rede (aqui, tokio) só encaminha os
/// bytes crus por um canal pra `run_gst_thread`, que faz tudo relacionado
/// ao GStreamer/PipeWire numa única OS thread fixa.
pub async fn feed(mut recv: quinn::RecvStream, handle: MicHandle, hud: Arc<Mutex<HudState>>) {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let gst_thread = {
        let handle = handle.clone();
        let hud = hud.clone();
        std::thread::spawn(move || run_gst_thread(rx, handle, hud))
    };

    const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
    let mut buf = vec![0u8; 16 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = match tokio::time::timeout(STALL_TIMEOUT, recv.read(&mut buf)).await {
            Ok(Ok(Some(n))) => n,
            Ok(Ok(None)) => break,
            Ok(Err(_)) => break,
            Err(_) => {
                push_log(&hud, "[!] microfone: sem dados há 15s, encerrando stream travado".to_string());
                break;
            }
        };
        total += n as u64;
        if tx.send(buf[..n].to_vec()).is_err() {
            break; // a thread do GStreamer já encerrou (erro no pipeline)
        }
    }
    drop(tx); // sinaliza fim pra thread do GStreamer
    push_log(&hud, format!("[i] microfone: stream encerrado ({} KB recebidos)", total / 1024));
    let _ = gst_thread.join();
}

/// Roda inteiramente numa OS thread própria e fixa (ver nota do módulo) —
/// monta o pipeline, alimenta o `appsrc` conforme os chunks chegam pelo
/// canal, e limpa tudo quando o canal fecha (rede encerrou).
fn run_gst_thread(rx: Receiver<Vec<u8>>, handle: MicHandle, hud: Arc<Mutex<HudState>>) {
    if let Err(e) = gst::init() {
        push_log(&hud, format!("[!] microfone: GStreamer não inicializou: {e}"));
        return;
    }

    // ponytail: 1.0 pressupõe que o app manda PCM já com ganho de hardware
    // (AudioSource.CAMCORDER/UNPROCESSED, não MIC) — se "abafado" voltar depois
    // da troca no Android, ajustar aqui primeiro em vez de mexer no lado do telemóvel.
    let pipeline_str = "appsrc name=src is-live=true format=time block=true \
         ! audio/x-raw,format=S16LE,rate=48000,channels=1,layout=interleaved \
         ! audioconvert ! audioresample \
         ! volume volume=1.0 \
         ! pipewiresink sync=false stream-properties=\"props,media.class=Audio/Source,node.name=hyprlink-mic,node.description=HyprLink-Mic\"";
    let pipeline = match gst::parse::launch(pipeline_str) {
        Ok(el) => match el.downcast::<gst::Pipeline>() {
            Ok(p) => p,
            Err(_) => {
                push_log(&hud, "[!] microfone: pipeline inesperado".to_string());
                return;
            }
        },
        Err(e) => {
            push_log(&hud, format!("[!] microfone: falha ao montar o pipeline: {e}"));
            return;
        }
    };
    let Some(appsrc) = pipeline.by_name("src").and_then(|e| e.downcast::<AppSrc>().ok()) else {
        push_log(&hud, "[!] microfone: appsrc não encontrado no pipeline".to_string());
        return;
    };

    // ponytail: o `GstSystemClock` é um singleton do PROCESSO INTEIRO — se
    // webcam/tap já rodaram (e rodaram, várias vezes, nesta mesma sessão do
    // daemon), a calibração dele pode não estar mais confiável pra um
    // pipeline novo. `use_clock(None)` tira essa dependência de vez —
    // ninguém aqui precisa de sincronismo entre pipelines mesmo.
    pipeline.use_clock(None::<&gst::Clock>);

    if pipeline.set_state(gst::State::Playing).is_err() {
        push_log(&hud, "[!] microfone: não foi possível iniciar o pipeline".to_string());
        return;
    }
    *handle.lock().unwrap() = Some(pipeline.clone());
    state::set_mic_active(&hud, true);
    push_log(&hud, "[+] microfone: stream iniciado — selecione \"HyprLink-Mic\" como entrada de áudio".to_string());

    if let Some(bus) = pipeline.bus() {
        let hud_bus = hud.clone();
        std::thread::spawn(move || {
            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                match msg.view() {
                    gst::MessageView::Error(err) => {
                        push_log(&hud_bus, format!("[!] microfone: erro no pipeline GStreamer: {} ({:?})", err.error(), err.debug()));
                        break;
                    }
                    gst::MessageView::Eos(_) => break,
                    _ => {}
                }
            }
        });
    }

    // ponytail: um `read()` de rede não respeita limite de amostra — pode
    // entregar uma quantidade ÍMPAR de bytes, o que desalinha todo par de
    // 16 bits daí em diante (mesma classe de bug que o app Android já teve
    // do lado dele, "pendingLowByte", recebendo áudio do PC).
    let mut pending_byte: Option<u8> = None;
    // PTS calculado pela contagem de amostras (taxa fixa 48kHz), não pelo
    // relógio de chegada — evita o `pipewiresink` reajustar a taxa achando
    // que o áudio está acelerando/atrasando por causa de rajadas de rede.
    const SAMPLE_RATE: u64 = 48_000;
    let mut running_ns: u64 = 0;
    while let Ok(piece) = rx.recv() {
        let mut chunk = Vec::with_capacity(piece.len() + 1);
        if let Some(b) = pending_byte.take() {
            chunk.push(b);
        }
        chunk.extend_from_slice(&piece);
        if chunk.len() % 2 != 0 {
            pending_byte = chunk.pop();
        }
        if chunk.is_empty() {
            continue;
        }
        let samples = (chunk.len() / 2) as u64;
        let duration_ns = samples * 1_000_000_000 / SAMPLE_RATE;
        let mut gst_buf = gst::Buffer::from_mut_slice(chunk);
        if let Some(buf_ref) = gst_buf.get_mut() {
            buf_ref.set_pts(gst::ClockTime::from_nseconds(running_ns));
            buf_ref.set_duration(gst::ClockTime::from_nseconds(duration_ns));
        }
        running_ns += duration_ns;
        if appsrc.push_buffer(gst_buf).is_err() {
            break;
        }
    }
    let _ = appsrc.end_of_stream();
    stop(&handle, &hud);
}
