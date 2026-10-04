//! Telemóvel como coluna do PC: cria um sink virtual `hyprlink-speaker`
//! (`module-null-sink` via `pactl`), torna-o a saída padrão — movendo as
//! streams que já estão a tocar junto (ver `audio::set_default_sink`) — e
//! aponta o audio tap pro monitor dele. O som do PC deixa de sair nas
//! colunas físicas e passa a tocar no telemóvel, como se fosse um
//! auricular Bluetooth.
//!
//! Restauro garantido (o PC nunca fica mudo por engano): o sink padrão
//! ANTERIOR é persistido em config (`speaker_prev_sink`) antes de qualquer
//! mudança, e é devolvido no OFF, na desconexão do telemóvel e no arranque
//! do daemon — se o daemon morrer com o modo ativo, o arranque seguinte
//! remove o módulo órfão e devolve o default ao dono original
//! (`cleanup_orphans`). Limite conhecido: entre o crash e o próximo
//! arranque, o default continua apontando pro sink virtual (apps tocam
//! "para lugar nenhum") — aceitável pra um daemon que corre com a sessão.
//!
//! ponytail: por que `module-null-sink` do pactl e não um nó nativo do
//! PipeWire/GStreamer como o `mic.rs` faz? O mic precisa que apps de
//! terceiros VEJAM uma fonte nova (por isso `pipewiresink` com
//! `media.class=Audio/Source`); aqui o requisito é diferente — o sink
//! precisa ser o ALVO do roteamento do sistema (default + `move-sink-input`,
//! operações do pactl que o session manager honra) e o seu monitor é a fonte
//! de captura que já sabemos consumir (`tap.rs`). O null-sink é exatamente
//! isso, e é o mesmo mecanismo que o mic.rs descartou por outros motivos
//! (lá o problema era o monitor ser fonte "escondida" pros apps de
//! gravação — aqui a captura somos nós, via `stream.capture.sink=true`).

use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::active::ActiveConn;
use crate::audio::PHONE_SINK_NAME;
use crate::config::SharedConfig;
use crate::state::{self, push_log, HudState};
use crate::tap::TapHandle;

/// Índice do `module-null-sink` carregado (`pactl unload-module <idx>` pra
/// derrubar) — `None` = modo coluna desligado.
pub type SpeakerHandle = Arc<Mutex<Option<u32>>>;

pub fn new_handle() -> SpeakerHandle {
    Arc::new(Mutex::new(None))
}

pub fn is_active(handle: &SpeakerHandle) -> bool {
    handle.lock().unwrap().is_some()
}

fn pactl(args: &[&str]) -> Option<String> {
    let out = Command::new("pactl").args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn default_sink_name() -> Option<String> {
    pactl(&["get-default-sink"]).filter(|s| !s.is_empty())
}

fn load_speaker_module() -> Option<u32> {
    let out = pactl(&[
        "load-module",
        "module-null-sink",
        &format!("sink_name={PHONE_SINK_NAME}"),
        "sink_properties=device.description=HyprLink-Phone",
    ])?;
    out.trim().parse::<u32>().ok()
}

fn unload_module(index: u32) -> bool {
    pactl(&["unload-module", &index.to_string()]).is_some()
}

/// Módulos `module-null-sink` órfãos com o nosso `sink_name` — restos de um
/// daemon que morreu com o modo ativo (o módulo vive no servidor
/// pipewire-pulse, não morre junto com o daemon).
fn orphan_module_indexes() -> Vec<u32> {
    let Some(listing) = pactl(&["list", "short", "modules"]) else { return Vec::new() };
    listing
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let idx = parts.next()?.parse::<u32>().ok()?;
            let name = parts.next()?;
            let args = parts.next().unwrap_or("");
            (name == "module-null-sink" && args.contains(&format!("sink_name={PHONE_SINK_NAME}")))
                .then_some(idx)
        })
        .collect()
}

/// Liga o modo coluna. Ordem deliberada: persiste o sink atual ANTES de
/// mexer em qualquer coisa (se algo falhar no meio, o restauro no próximo
/// arranque ainda sabe pra onde voltar), carrega o módulo, promove o sink
/// virtual a default e só então liga o tap apontado pra ele.
pub async fn enable(
    active: &ActiveConn,
    tap: &TapHandle,
    hud: &Arc<Mutex<HudState>>,
    config: &SharedConfig,
    handle: &SpeakerHandle,
) -> bool {
    if is_active(handle) {
        return true;
    }

    let Some(connection) = active.lock().unwrap().clone() else {
        push_log(hud, "[!] coluna: sem telemóvel conectado pra ouvir o som do PC".to_string());
        return false;
    };

    // Restos de uma sessão anterior que morreu no meio? Limpa antes de
    // criar outro (o load com o mesmo sink_name falharia, ou pior, criaria
    // um segundo módulo com nome alternativo).
    for idx in tokio::task::spawn_blocking(orphan_module_indexes).await.unwrap_or_default() {
        let _ = tokio::task::spawn_blocking(move || unload_module(idx)).await;
    }

    let Some(prev) = tokio::task::spawn_blocking(default_sink_name).await.unwrap_or(None) else {
        push_log(hud, "[!] coluna: não foi possível descobrir o sink padrão atual".to_string());
        return false;
    };
    if prev == PHONE_SINK_NAME {
        push_log(hud, "[!] coluna: o sink do telemóvel já é o padrão (estado inconsistente), recusando".to_string());
        return false;
    }

    crate::config::set_speaker_prev_sink(config, Some(&prev));

    let Some(index) = tokio::task::spawn_blocking(load_speaker_module).await.unwrap_or(None) else {
        crate::config::set_speaker_prev_sink(config, None);
        push_log(hud, "[!] coluna: o PipeWire não aceitou criar o sink virtual".to_string());
        return false;
    };
    *handle.lock().unwrap() = Some(index);

    let moved = tokio::task::spawn_blocking(move || {
        crate::audio::set_default_sink(PHONE_SINK_NAME);
    })
    .await;
    if moved.is_err() {
        // spawn_blocking pânico — improvável; limpa pra não deixar meio-ligado.
        let _ = disable(tap, hud, config, handle).await;
        return false;
    }

    state::set_speaker_active(hud, true);
    push_log(
        hud,
        format!("[+] coluna: telemóvel é agora a saída de som do PC (antes: {prev})"),
    );

    crate::tap::start(connection, Some(PHONE_SINK_NAME.to_string()), tap.clone(), hud.clone()).await;
    true
}

/// Desliga o modo coluna e devolve o sink padrão ao dono original. Ordem
/// deliberada: restaura o default PRIMEIRO (as streams em curso voltam pras
/// colunas de uma vez), para o tap, e só então descarrega o módulo —
/// descarregar antes mataria as streams em vez de movê-las.
pub async fn disable(tap: &TapHandle, hud: &Arc<Mutex<HudState>>, config: &SharedConfig, handle: &SpeakerHandle) -> bool {
    let index = handle.lock().unwrap().take();
    if index.is_none() {
        return false; // já estava desligado
    }
    state::set_speaker_active(hud, false);

    let prev = crate::config::speaker_prev_sink(config);
    if let Some(prev_name) = prev.clone() {
        // Log ANTES do move pra closure (spawn_blocking exige posse).
        push_log(hud, format!("[i] coluna: a devolver a saída de som a {prev_name}"));
        let _ = tokio::task::spawn_blocking(move || crate::audio::set_default_sink(&prev_name)).await;
        crate::config::set_speaker_prev_sink(config, None);
    }

    crate::tap::stop(tap, hud);

    if let Some(index) = index {
        let _ = tokio::task::spawn_blocking(move || unload_module(index)).await;
    }
    true
}

/// Arranque do daemon: limpa o que uma sessão anterior pode ter deixado
/// (crash com o modo ativo) — módulo órfão fora, default devolvido ao dono
/// salvo em config. Síncrono e antes da GUI/subir o servidor: tem de
/// terminar antes de qualquer coisa poder ligar o modo de novo.
pub fn cleanup_orphans(config: &SharedConfig) {
    let orphans = orphan_module_indexes();
    if let Some(prev) = crate::config::speaker_prev_sink(config) {
        if !orphans.is_empty() {
            crate::audio::set_default_sink(&prev);
            println!("[i] coluna: sessão anterior terminou com o telemóvel como saída — a devolver pra {prev}");
        }
        crate::config::set_speaker_prev_sink(config, None);
    }
    for idx in orphans {
        if unload_module(idx) {
            println!("[i] coluna: módulo órfão hyprlink-speaker ({idx}) removido");
        }
    }
}
