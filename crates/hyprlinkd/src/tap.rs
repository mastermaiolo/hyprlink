//! Audio tap: ouvir o áudio do PC no telemóvel. Pipeline GStreamer
//! (`pipewiresrc` no monitor) — sem bindings C do libpipewire, o GStreamer
//! já faz esse trabalho.
//!
//! Como o tap se adapta ao roteamento (robustez, 2026-10-05):
//! - **Modo normal** (`sink: None`): captura SEM `target-object`, só com
//!   `stream.capture.sink=true` — o WirePlumber liga a captura ao monitor
//!   da saída padrão E SEGUE-A sozinha: ligar auscultadores, Bluetooth a
//!   entrar/sair ou trocar de saída a meio não exige parar o tap.
//! - **Modo coluna** (`sink: Some(name)`, ver `speaker.rs`): nome explícito
//!   pro sink virtual — determinístico mesmo que o default mude a meio.
//! - **Recuperação**: se o pipeline morrer (saída desapareceu, PipeWire
//!   reiniciou), a supervisão volta a construir depois de um backoff
//!   crescente (1s → 15s) — em vez de parar de vez como antes. Backoff
//!   renova se a sessão anterior durou >30s (falha nova, não falha crónica).
//! - **Diagnóstico**: no arranque, uma linha diz o servidor de áudio
//!   (pactl info) e se o GStreamer tem `pipewiresrc` — as falhas de ambiente
//!   (sem pipewire-pulse, sem gst-plugin-pipewire) deixam de ser silêncio.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;

use crate::state::{self, HudState, push_audio_vu, push_log};

pub type TapHandle = Arc<Mutex<Option<gst::Pipeline>>>;

pub fn new_handle() -> TapHandle {
    Arc::new(Mutex::new(None))
}

/// Pico de amplitude num bloco S16LE — mesmo cálculo que o Android já faz em
/// `AudioStreamPlayer.kt` do lado da reprodução, aqui do lado da captura, pro
/// VU meter do AUDIO na GUI.
fn peak_percent(bytes: &[u8]) -> u8 {
    let mut peak: i16 = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        let sample = i16::from_le_bytes([bytes[i], bytes[i + 1]]);
        peak = peak.max(sample.saturating_abs());
        i += 2;
    }
    ((peak as u32 * 100) / 32767) as u8
}

/// Saída padrão atual (só pro log/diagnóstico — o pipeline normal já não
/// precisa do nome, segue o default sozinho). Filtra vazia: o pactl pode
/// responder sucesso com corpo vazio quando o servidor não respondeu a
/// tempo, e `target-object=""` já fez o tap "funcionar por acaso" no errado.
fn default_sink_name() -> Option<String> {
    std::process::Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Uma linha de diagnóstico do ambiente de áudio — as falhas de ambiente
/// (sem pipewire-pulse, sem gst-plugin-pipewire) deixam de ser silêncio.
fn env_line() -> String {
    let server = std::process::Command::new("pactl")
        .args(["info"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .and_then(|s| {
            s.lines()
                .find(|l| l.trim_start().starts_with("Server Name"))
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_else(|| "pactl indisponível (falta pipewire-pulse?)".to_string());
    let src = if gst::ElementFactory::find("pipewiresrc").is_some() {
        "pipewiresrc ✓"
    } else {
        "pipewiresrc AUSENTE (gst-plugin-pipewire)"
    };
    let sink = default_sink_name().unwrap_or_else(|| "—".to_string());
    format!("audio: {server} · {src} · saída padrão: {sink}")
}

/// Pipeline do tap. `None` = seguir a saída padrão (sem `target-object`: o
/// WirePlumber religa ao monitor do default a cada mudança); `Some(nome)` =
/// capturar esse sink em concreto (modo coluna).
///
/// ponytail: "target-object=<sink>.monitor" NÃO existe como nó nativo do
/// PipeWire (é convenção do PulseAudio) — sem correspondência, o WirePlumber
/// liga a captura à fonte padrão (o MICROFONE físico). A forma certa de
/// "escutar" um sink: apontar `target-object` pro próprio nó do sink e marcar
/// a stream com `stream.capture.sink=true` (liga nas portas de monitor). No
/// modo normal NÃO se põe target-object nenhum — só o
/// `stream.capture.sink=true` — e é o próprio WirePlumber que escolhe o
/// monitor do default e o segue quando o default muda.
fn pipeline_str(sink: Option<&str>) -> String {
    let target = sink
        .map(|s| format!(" target-object=\"{s}\""))
        .unwrap_or_default();
    format!(
        "pipewiresrc{target} stream-properties=\"props,stream.capture.sink=true\" \
         ! audioconvert ! audioresample \
         ! audio/x-raw,format=S16LE,rate=48000,channels=2,layout=interleaved \
         ! appsink name=hyprlink_tap sync=false max-buffers=8 drop=true"
    )
}

/// Para o tap em andamento, se houver (chamado por `audio.tap_stop` e
/// também antes de iniciar um novo tap, pra nunca ter dois ao mesmo tempo).
pub fn stop(handle: &TapHandle, hud: &Arc<Mutex<HudState>>) {
    // Nova geração: qualquer supervisão em curso (inclusive uma que esteja
    // em backoff, com o handle vazio) vê que já não é a dona e sai.
    GERACAO.fetch_add(1, Ordering::SeqCst);
    if let Some(pipeline) = handle.lock().unwrap().take() {
        let _ = pipeline.set_state(gst::State::Null);
    }
    if hud.lock().unwrap().modules.audio_tap_active {
        state::set_audio_tap_active(hud, false);
        push_log(hud, "[i] audio tap parado".to_string());
    }
}

/// Geração do tap: sobe a cada `stop()`. Uma supervisão só reconstrói o
/// pipeline enquanto a geração for a mesma com que arrancou — sem isto, um
/// `stop()` durante o backoff (handle vazio) não parava nada e o tap voltava
/// sozinho, e um `start()` nesse intervalo deixava duas supervisões vivas.
static GERACAO: AtomicU64 = AtomicU64::new(0);

fn ainda_dono(geracao: u64) -> bool {
    GERACAO.load(Ordering::SeqCst) == geracao
}

/// Como `stop()`, mas só mata o pipeline se ele ainda for exatamente o
/// mesmo desta sessão — a limpeza de uma sessão morta não pode matar o
/// pipeline novo de um restart rápido (tap_stop + tap_start).
fn stop_if_current(handle: &TapHandle, pipeline: &gst::Pipeline, hud: &Arc<Mutex<HudState>>) {
    let mut guard = handle.lock().unwrap();
    if guard.as_ref() == Some(pipeline) {
        let old = guard.take().unwrap();
        drop(guard);
        let _ = old.set_state(gst::State::Null);
        state::set_audio_tap_active(hud, false);
    }
}

/// Como uma sessão de captura terminou — a supervisão de `start()` decide
/// o que fazer com cada desfecho.
enum TapEnd {
    /// O utilizador parou (ou outra sessão tomou o lugar) — estado já limpo.
    Stopped,
    /// O telemóvel foi-se (datagram falhou) — não vale tentar de novo.
    ConnectionLost,
    /// O pipeline morreu (saída sumiu, PipeWire reiniciou) — tentar de novo.
    PipelineDied(String),
    /// Erro de ambiente que não passa sozinho (falta o `pipewiresrc`, por
    /// exemplo) — tentar de novo de 15 em 15 s só enchia o log.
    Fatal(String),
}

/// Inicia o tap: envia PCM cru (16-bit LE, 48kHz, estéreo) via QUIC DATAGRAM
/// (RFC 9221, sem stream). Cada datagrama carrega um contador de sequência
/// de 2 bytes (BE) — suficiente pra detectar perda/desordem sem o overhead
/// de RTP, já que só existe um tap ativo por conexão.
///
/// `sink`: `None` = espelho da saída padrão (segue-a sozinha); `Some(name)`
/// = modo coluna (`speaker.rs`), captura determinística do sink virtual.
pub async fn start(
    connection: quinn::Connection,
    sink: Option<String>,
    handle: TapHandle,
    hud: Arc<Mutex<HudState>>,
) {
    stop(&handle, &hud);
    let geracao = GERACAO.load(Ordering::SeqCst);

    if let Err(e) = gst::init() {
        push_log(
            &hud,
            format!("[!] audio tap: GStreamer não inicializou: {e}"),
        );
        return;
    }

    push_log(&hud, format!("[i] {}", env_line()));

    let Some(max_dgram) = connection.max_datagram_size() else {
        push_log(&hud, "[!] audio tap: telemóvel não suporta QUIC DATAGRAM (RFC 9221) — precisa da atualização do app".to_string());
        return;
    };
    let chunk_size = max_dgram.saturating_sub(2); // 2 bytes de seq na frente
    if chunk_size == 0 {
        push_log(
            &hud,
            "[!] audio tap: tamanho de datagrama insuficiente pra qualquer payload".to_string(),
        );
        return;
    }

    match sink.as_deref() {
        Some(name) => push_log(
            &hud,
            format!("[+] audio tap iniciado · captura fixa de {name}"),
        ),
        None => {
            let atual = default_sink_name()
                .unwrap_or_else(|| "desconhecida (a seguir a padrão)".to_string());
            push_log(
                &hud,
                format!("[+] audio tap iniciado · a seguir a saída padrão (agora: {atual})"),
            );
        }
    }
    state::set_audio_tap_active(&hud, true);

    // Supervisão: pipeline morto volta a nascer com backoff crescente.
    let mut backoff = std::time::Duration::from_secs(1);
    loop {
        let sessao = std::time::Instant::now();
        let fim = run_once(
            &connection,
            sink.as_deref(),
            &handle,
            &hud,
            chunk_size,
            geracao,
        )
        .await;
        match fim {
            TapEnd::Stopped => return,
            TapEnd::Fatal(motivo) => {
                push_log(&hud, format!("[!] audio tap: {motivo} — tap parado"));
                if ainda_dono(geracao) {
                    stop(&handle, &hud);
                }
                return;
            }
            TapEnd::ConnectionLost => {
                push_log(
                    &hud,
                    "[i] audio tap: conexão terminada, a encerrar o tap".to_string(),
                );
                if ainda_dono(geracao) {
                    stop(&handle, &hud);
                }
                return;
            }
            TapEnd::PipelineDied(motivo) => {
                // Um stop() (ou um start() novo) entretanto: esta supervisão
                // já não é a dona — não se volta a construir nada.
                if !ainda_dono(geracao) {
                    return;
                }
                if sessao.elapsed() > std::time::Duration::from_secs(30) {
                    backoff = std::time::Duration::from_secs(1); // durou: falha nova, não crónica
                }
                push_log(
                    &hud,
                    format!(
                        "[i] audio tap: pipeline morreu ({motivo}) — a retomar em {:?}",
                        backoff
                    ),
                );
                tokio::time::sleep(backoff).await;
                if !ainda_dono(geracao) {
                    return;
                }
                backoff = (backoff * 2).min(std::time::Duration::from_secs(15));
            }
        }
    }
}

/// Uma sessão de captura: monta o pipeline, envia até morrer e devolve o
/// desfecho pra supervisão decidir.
async fn run_once(
    connection: &quinn::Connection,
    sink: Option<&str>,
    handle: &TapHandle,
    hud: &Arc<Mutex<HudState>>,
    chunk_size: usize,
    geracao: u64,
) -> TapEnd {
    // Montar o pipeline só falha por ambiente (elemento em falta) — não
    // passa com o tempo, por isso é fatal e não entra no backoff.
    let pipeline = match gst::parse::launch(&pipeline_str(sink)) {
        Ok(el) => match el.downcast::<gst::Pipeline>() {
            Ok(p) => p,
            Err(_) => return TapEnd::Fatal("pipeline inesperado".into()),
        },
        Err(e) => return TapEnd::Fatal(format!("falha ao montar o pipeline: {e}")),
    };

    let Some(sink_el) = pipeline.by_name("hyprlink_tap") else {
        return TapEnd::Fatal("appsink não encontrado".into());
    };
    let Ok(appsink) = sink_el.downcast::<AppSink>() else {
        return TapEnd::Fatal("appsink com tipo inesperado".into());
    };

    // Mesmo fix que o mic.rs já precisou (GstSystemClock é singleton do
    // processo — calibração de uma pipeline anterior podia vazar pra esta).
    pipeline.use_clock(None::<&gst::Clock>);

    if pipeline.set_state(gst::State::Playing).is_err() {
        return TapEnd::PipelineDied("não foi possível iniciar o pipeline".into());
    }
    // A sessão morta anterior (se houver) sai do handle; esta entra — mas só
    // se ninguém chamou stop() enquanto o pipeline arrancava.
    {
        let mut guard = handle.lock().unwrap();
        if !ainda_dono(geracao) {
            drop(guard);
            let _ = pipeline.set_state(gst::State::Null);
            return TapEnd::Stopped;
        }
        *guard = Some(pipeline.clone());
    }

    // Bus: logar erros e DERRUBAR o pipeline (Null desbloqueia o
    // pull_sample da thread de captura, que decide o desfecho pelo handle).
    // Não é o bus que para o tap — quem decide é a supervisão.
    if let Some(bus) = pipeline.bus() {
        let pipeline_weak = pipeline.downgrade();
        let hud_bus = hud.clone();
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
                                "[!] audio tap: erro no pipeline GStreamer: {} ({:?})",
                                err.error(),
                                err.debug()
                            ),
                        );
                        if let Some(p) = pipeline_weak.upgrade() {
                            let _ = p.set_state(gst::State::Null);
                        }
                        break;
                    }
                    gst::MessageView::Eos(_) => {
                        if let Some(p) = pipeline_weak.upgrade() {
                            let _ = p.set_state(gst::State::Null);
                        }
                        break;
                    }
                    _ => {}
                }
            }
        });
    }

    // pull_sample() bloqueia — roda numa thread própria. `send_datagram` do
    // quinn é síncrono (só enfileira), então a thread de captura manda direto.
    let hud_thread = hud.clone();
    let handle_thread = handle.clone();
    let pipeline_thread = pipeline.clone();
    let connection = connection.clone();
    let (tx, rx) = std::sync::mpsc::channel::<TapEnd>();
    std::thread::spawn(move || {
        let mut seq: u16 = 0;
        let mut total: u64 = 0;
        let mut last_logged: u64 = 0;
        let fim = 'capture: loop {
            match appsink.pull_sample() {
                Ok(sample) => {
                    let Some(buffer) = sample.buffer() else {
                        continue;
                    };
                    let Ok(map) = buffer.map_readable() else {
                        continue;
                    };
                    let data = map.as_slice();
                    push_audio_vu(&hud_thread, peak_percent(data));

                    for chunk in data.chunks(chunk_size) {
                        let mut datagram = Vec::with_capacity(2 + chunk.len());
                        datagram.extend_from_slice(&seq.to_be_bytes());
                        datagram.extend_from_slice(chunk);
                        seq = seq.wrapping_add(1);

                        if let Err(e) = connection.send_datagram(Bytes::from(datagram)) {
                            push_log(
                                &hud_thread,
                                format!("[!] audio tap: envio de datagram falhou: {e}"),
                            );
                            break 'capture TapEnd::ConnectionLost;
                        }
                        total += chunk.len() as u64;
                    }
                    state::set_audio_tap_bytes(&hud_thread, total);
                    if total.saturating_sub(last_logged) >= 1_000_000 {
                        last_logged = total;
                        push_log(
                            &hud_thread,
                            format!("[i] audio tap: {} KB enviados", total / 1024),
                        );
                    }
                }
                Err(e) => {
                    // Quem é o dono atual do handle? Se já não somos nós, foi
                    // o stop() do utilizador (ou outra sessão) — desfecho
                    // limpo. Se somos nós, o pipeline morreu sozinho.
                    let nosso = handle_thread
                        .lock()
                        .unwrap()
                        .as_ref()
                        .is_some_and(|p| p == &pipeline_thread);
                    if nosso {
                        break 'capture TapEnd::PipelineDied(format!("pull_sample: {e}"));
                    }
                    break 'capture TapEnd::Stopped;
                }
            }
        };
        // A captura acabou: limpa o handle se ainda for desta sessão (a
        // supervisão reconstrói; o stop() do utilizador já limpou).
        {
            let mut guard = handle_thread.lock().unwrap();
            if guard.as_ref() == Some(&pipeline_thread) {
                let dead = guard.take().unwrap();
                drop(guard);
                let _ = dead.set_state(gst::State::Null);
            }
        }
        push_log(
            &hud_thread,
            format!(
                "[i] audio tap: sessão encerrada ({} KB no total)",
                total / 1024
            ),
        );
        let _ = tx.send(fim);
    });

    // Espera a thread de captura entregar o desfecho (spawn_blocking pra
    // não bloquear o executor à espera de um canal std).
    match tokio::task::spawn_blocking(move || rx.recv()).await {
        Ok(Ok(fim)) => fim,
        _ => {
            // A thread morreu sem desfecho — encerrar por segurança.
            stop_if_current(handle, &pipeline, hud);
            TapEnd::PipelineDied("thread de captura desapareceu".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os dois modos têm de diferir EXATAMENTE no target-object: o normal
    /// não pode fixar nome nenhum (é o que faz o WirePlumber seguir o
    /// default), o coluna tem de fixar o sink virtual.
    #[test]
    fn pipeline_variants() {
        let normal = pipeline_str(None);
        assert!(
            !normal.contains("target-object"),
            "normal não pode fixar: {normal}"
        );
        assert!(normal.contains("stream.capture.sink=true"));
        assert!(normal.contains("channels=2"));

        let coluna = pipeline_str(Some("hyprlink-speaker"));
        assert!(coluna.contains("target-object=\"hyprlink-speaker\""));
        assert!(coluna.contains("stream.capture.sink=true"));
    }

    /// O bug do backoff: com o pipeline morto o handle está vazio, e o
    /// stop() antigo não fazia nada — o estado ficava "ativo" e a supervisão
    /// voltava a ligar o tap. Agora o stop() muda a geração e limpa o estado
    /// mesmo com o handle vazio.
    #[test]
    fn stop_com_handle_vazio_desliga_e_tira_a_dona() {
        let hud = HudState::new("00".into());
        let handle: TapHandle = Arc::new(Mutex::new(None));
        state::set_audio_tap_active(&hud, true);
        let geracao = GERACAO.load(Ordering::SeqCst);
        assert!(ainda_dono(geracao));

        stop(&handle, &hud);

        assert!(!ainda_dono(geracao));
        assert!(!hud.lock().unwrap().modules.audio_tap_active);
    }

    /// Não roda em CI — precisa de PipeWire real. `--ignored`: confirma que
    /// pelo menos um buffer sai do monitor do sink padrão.
    #[test]
    #[ignore]
    fn manual_pipeline() {
        gst::init().expect("GStreamer deveria inicializar");
        println!("{}", env_line());
        let pipeline = gst::parse::launch(&pipeline_str(default_sink_name().as_deref()))
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        let appsink = pipeline
            .by_name("hyprlink_tap")
            .unwrap()
            .downcast::<AppSink>()
            .unwrap();
        pipeline
            .set_state(gst::State::Playing)
            .expect("pipeline deveria iniciar");

        let sample = appsink
            .pull_sample()
            .expect("deveria sair pelo menos uma amostra");
        let buffer = sample.buffer().expect("amostra deveria ter buffer");
        let map = buffer.map_readable().expect("buffer deveria ser legível");
        println!("recebido: {} bytes", map.len());
        assert!(!map.is_empty(), "esperava bytes de PCM reais");

        pipeline.set_state(gst::State::Null).ok();
    }

    /// Diagnóstico: pula em loop 30x seguidas pra ver se `pull_sample()`
    /// continua entregando amostras quando chamado repetidamente.
    #[test]
    #[ignore]
    fn manual_pipeline_loop() {
        gst::init().expect("GStreamer deveria inicializar");
        let pipeline = gst::parse::launch(&pipeline_str(default_sink_name().as_deref()))
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        let appsink = pipeline
            .by_name("hyprlink_tap")
            .unwrap()
            .downcast::<AppSink>()
            .unwrap();
        pipeline
            .set_state(gst::State::Playing)
            .expect("pipeline deveria iniciar");

        let start = std::time::Instant::now();
        for i in 0..30 {
            let t0 = std::time::Instant::now();
            let sample = appsink
                .pull_sample()
                .unwrap_or_else(|_| panic!("pull #{i} deveria funcionar"));
            let buffer = sample.buffer().expect("amostra deveria ter buffer");
            let map = buffer.map_readable().expect("buffer deveria ser legível");
            println!(
                "pull #{i}: {} bytes em {:?} (total decorrido: {:?})",
                map.len(),
                t0.elapsed(),
                start.elapsed()
            );
        }

        pipeline.set_state(gst::State::Null).ok();
    }
}
