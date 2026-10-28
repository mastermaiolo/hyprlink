//! Clipboard bidirecional via `wl-copy`/`wl-paste` (wl-clipboard).
//!
//! Duas cargas: texto (watcher `wl-paste --watch`, instantâneo) e imagens
//! PNG (poll de 1,5s — o `--watch` só serve UMA flag `--type` e não existe
//! evento "tipos mudaram"; ver `watch_images`).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::session::tokio_command;
use ciborium::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::active::{ActiveConn, announce, push};
use crate::state::{self, HudState, push_log};

/// Teto pra aceitar uma imagem do clipboard (32 MiB) — maior que isso é
/// recusado com log; imagem de clipboard tem de ser "uma foto/screenshot",
/// não "um PSD de 2 GB".
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

/// Um eco só conta durante este tempo. Se o `wl-paste` nunca chegar a ver a
/// escrita do daemon (o `wl-copy` falhou, o watcher estava a reiniciar), o
/// guard não pode ficar à espera e engolir a próxima cópia igual do
/// utilizador.
const ECHO_TTL: Duration = Duration::from_secs(5);

/// Uma cópia feita sem telemóvel ligado só segue se ele ligar dentro disto.
const PENDING_TTL: Duration = Duration::from_secs(60);

/// O que o próprio daemon acabou de escrever no clipboard local (texto, ou o
/// hash de uma imagem) e quando.
#[derive(Debug)]
pub struct Echo {
    value: String,
    at: Instant,
}

/// Último texto que o próprio daemon escreveu no clipboard local (via pedido
/// do telemóvel) — evita reenviar o mesmo conteúdo de volta assim que o
/// watcher do wl-paste perceber a mudança que nós mesmos causamos.
pub type LastLocalSet = Arc<Mutex<Option<Echo>>>;

pub fn new_guard() -> LastLocalSet {
    Arc::new(Mutex::new(None))
}

fn mark_echo(guard: &LastLocalSet, value: String) {
    *guard.lock().unwrap() = Some(Echo {
        value,
        at: Instant::now(),
    });
}

/// `value` é o eco da última escrita do daemon? Se for, consome o guard. Um
/// guard vencido cai sem engolir nada; um válido mas diferente fica (a
/// escrita do daemon ainda vem a caminho).
fn take_echo(guard: &mut Option<Echo>, value: &str, now: Instant) -> bool {
    match guard.take() {
        Some(e) if now.duration_since(e.at) > ECHO_TTL => false,
        Some(e) if e.value == value => true,
        other => {
            *guard = other;
            false
        }
    }
}

/// Hash SHA-256 (hex) da última imagem que o próprio daemon escreveu no
/// clipboard local — mesma função do `LastLocalSet`, pro poll de imagens
/// não ecoar de volta o que acabou de chegar do telemóvel.
pub type LastLocalImage = LastLocalSet;

pub fn new_image_guard() -> LastLocalImage {
    Arc::new(Mutex::new(None))
}

/// Id do `clipboard.set` (imagem) esperado no próximo uni-stream — mesmo
/// padrão do `pending_mic`.
pub type PendingClipImage = Arc<Mutex<Option<u64>>>;

pub fn new_pending_clip() -> PendingClipImage {
    Arc::new(Mutex::new(None))
}

/// Uma cópia do PC à espera de telemóvel.
#[derive(Debug, PartialEq)]
pub enum Outgoing {
    Text(String),
    Image(Vec<u8>),
}

/// Só a **última** cópia feita sem telemóvel ligado, e quando foi feita
/// (sem histórico).
pub type Outbox = Arc<Mutex<Option<(Outgoing, Instant)>>>;

pub fn new_outbox() -> Outbox {
    Arc::new(Mutex::new(None))
}

fn keep_for_later(outbox: &Outbox, item: Outgoing) {
    *outbox.lock().unwrap() = Some((item, Instant::now()));
}

/// Tira a cópia guardada; só a devolve se ainda tiver menos de `PENDING_TTL`.
fn take_fresh(slot: &mut Option<(Outgoing, Instant)>, now: Instant) -> Option<Outgoing> {
    let (item, at) = slot.take()?;
    (now.duration_since(at) < PENDING_TTL).then_some(item)
}

/// Chamado quando um telemóvel acaba de ligar: envia a última cópia feita
/// sem ele, se for recente.
pub async fn flush_pending(active: ActiveConn, outbox: Outbox, hud: Arc<Mutex<HudState>>) {
    let item = take_fresh(&mut outbox.lock().unwrap(), Instant::now());
    match item {
        Some(Outgoing::Text(text)) => {
            if send_text_to_phone(&active, &hud, text.clone()).await {
                push_log(&hud, "[i] clipboard: enviada a cópia feita antes de ligar");
            } else {
                keep_for_later(&outbox, Outgoing::Text(text));
            }
        }
        Some(Outgoing::Image(bytes)) => {
            if send_image_to_phone(&active, &hud, bytes.clone()).await {
                push_log(
                    &hud,
                    "[i] clipboard: enviada a imagem copiada antes de ligar",
                );
            } else {
                keep_for_later(&outbox, Outgoing::Image(bytes));
            }
        }
        None => {}
    }
}

/// O texto que já estava no clipboard quando o `wl-paste --watch` arrancou
/// (ele emite-o logo à entrada) não é uma cópia nova: não vai ao telemóvel —
/// nem no arranque, nem quando o watcher volta a ligar depois de o Hyprland
/// reiniciar. `baseline` é o texto lido logo antes de arrancar o watcher; só
/// a **primeira** emissão é comparada (se o clipboard não tinha texto, a
/// primeira cópia do utilizador segue normalmente).
fn is_startup_echo(baseline: &mut Option<Option<String>>, text: &str) -> bool {
    match baseline.take() {
        Some(Some(b)) => b == text,
        _ => false,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `clipboard.set` recebido do telemóvel: escreve no clipboard do Wayland.
pub async fn set_from_remote(text: &str, guard: &LastLocalSet) {
    mark_echo(guard, text.to_string());
    let mut child = match tokio_command("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(text.as_bytes()).await;
    }
    let _ = child.wait().await;
}

/// Texto do PC → telemóvel. Sem telemóvel ligado (ou com o envio a falhar),
/// devolve `false` e quem chamou guarda-o para quando ligar.
async fn send_text_to_phone(active: &ActiveConn, hud: &Arc<Mutex<HudState>>, text: String) -> bool {
    let chars = text.chars().count();
    let body = Value::Map(vec![(Value::Text("text".into()), Value::Text(text))]);
    if push(active, "clipboard.set", Some(body)).await.is_none() {
        return false;
    }
    push_log(
        hud,
        format!("[i] clipboard.set · PC → telemóvel · texto ({chars} caracteres)"),
    );
    true
}

/// Lê o texto atual do clipboard (uma vez, sem `--watch`). `None` se não há
/// texto ou não há sessão Wayland.
async fn current_text() -> Option<String> {
    let out = tokio_command("wl-paste")
        .args(["--type", "text", "--no-newline"])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// Observa o clipboard local (`wl-paste --watch`) e empurra `clipboard.set`
/// pro telemóvel a cada mudança que não veio dele mesmo.
///
/// O ambiente da sessão (`tokio_command`) é recalculado a cada arranque do
/// watcher: sem Hyprland ainda (daemon arrancado antes da sessão), o
/// `wl-paste` sai logo e tenta-se de novo 2 s depois; se o Hyprland
/// reiniciar, o `wl-paste` morre com o compositor e volta a ligar ao novo.
pub async fn watch(
    active: ActiveConn,
    guard: LastLocalSet,
    outbox: Outbox,
    hud: Arc<Mutex<HudState>>,
) {
    let mut announced_down = false;
    loop {
        let mut baseline = Some(current_text().await);
        let mut child = match tokio_command("wl-paste")
            .args(["--type", "text", "--no-newline", "--watch", "cat"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                push_log(&hud, format!("[!] wl-paste indisponível: {e}"));
                return;
            }
        };
        let Some(mut stdout) = child.stdout.take() else {
            return;
        };
        let started = Instant::now();
        let mut buf = vec![0u8; 64 * 1024];
        // Um clipboard maior que 64 KiB chega em mais de um `read()` — sem
        // isso, cada pedaço virava um `clipboard.set` próprio e corrompia o
        // conteúdo do lado do telemóvel. Acumula enquanto os bytes ainda
        // estiverem chegando; só manda quando a stream fica ~150ms quieta
        // (fim natural de uma cópia — o `wl-paste --watch` só volta a
        // escrever de novo quando o usuário copiar outra coisa).
        const IDLE_FLUSH: std::time::Duration = std::time::Duration::from_millis(150);
        let mut acc: Vec<u8> = Vec::new();
        loop {
            match tokio::time::timeout(IDLE_FLUSH, stdout.read(&mut buf)).await {
                Ok(Ok(0)) | Ok(Err(_)) => break, // wl-paste morreu, tenta reiniciar
                Ok(Ok(n)) => {
                    acc.extend_from_slice(&buf[..n]);
                    continue;
                }
                Err(_) => {} // timeout: nada novo há IDLE_FLUSH, cai pro flush abaixo
            }
            if acc.is_empty() {
                continue;
            }
            let text = String::from_utf8_lossy(&acc).to_string();
            acc.clear();
            if announced_down {
                announced_down = false;
                push_log(&hud, "[i] clipboard: a observar o Wayland outra vez");
            }
            if is_startup_echo(&mut baseline, &text) {
                continue;
            }
            if take_echo(&mut guard.lock().unwrap(), &text, Instant::now()) {
                continue;
            }
            state::push_clip_entry(&hud, state::DIR_PC_TO_PHONE, text.clone());
            if !send_text_to_phone(&active, &hud, text.clone()).await {
                keep_for_later(&outbox, Outgoing::Text(text));
                push_log(
                    &hud,
                    "[i] clipboard: sem telemóvel ligado; a cópia segue se ele ligar dentro de 60 s",
                );
            }
        }
        let _ = child.wait().await;
        // Uma linha só por queda (ex.: sem sessão Wayland no arranque), não
        // uma a cada 2 s.
        if !announced_down && started.elapsed() < Duration::from_secs(2) {
            announced_down = true;
            push_log(
                &hud,
                "[!] clipboard: wl-paste sem sessão Wayland; tento de novo a cada 2 s",
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// `clipboard.set` de IMAGEM recebido do telemóvel: escreve PNG cru no
/// clipboard do Wayland oferecendo `image/png`. O hash fica no guard pra o
/// poll não devolver a mesma imagem ao remetente.
pub async fn set_image_from_remote(
    bytes: Vec<u8>,
    guard: &LastLocalImage,
    hud: &Arc<Mutex<HudState>>,
) {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        push_log(
            hud,
            format!("[!] clipboard: imagem recusada ({} bytes)", bytes.len()),
        );
        return;
    }
    mark_echo(guard, sha256_hex(&bytes));
    let mut child = match tokio_command("wl-copy")
        .args(["--type", "image/png"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            push_log(hud, format!("[!] clipboard: wl-copy falhou: {e}"));
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        if stdin.write_all(&bytes).await.is_err() {
            let _ = child.kill().await;
            return;
        }
    }
    let _ = child.wait().await;
    state::push_clip_image(hud, state::DIR_PHONE_TO_PC, bytes.len() as u64);
    push_log(
        hud,
        format!(
            "[i] clipboard.set · telemóvel → PC · imagem {} KB",
            bytes.len() / 1024
        ),
    );
}

/// Uni-stream de imagem anunciada por `clipboard.set` (has_payload=true):
/// lê os bytes até EOF e entrega ao `wl-copy`.
pub async fn receive_image_stream(
    mut recv: quinn::RecvStream,
    guard: LastLocalImage,
    hud: Arc<Mutex<HudState>>,
) {
    let mut bytes: Vec<u8> = Vec::with_capacity(256 * 1024);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match recv.read(&mut buf).await {
            Ok(Some(n)) => n,
            Ok(None) | Err(_) => break,
        };
        if bytes.len() + n > MAX_IMAGE_BYTES {
            push_log(
                &hud,
                "[!] clipboard: imagem do telemóvel excede 32 MiB, descartada".to_string(),
            );
            let _ = recv.stop(0u32.into());
            return;
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    set_image_from_remote(bytes, &guard, &hud).await;
}

/// Envia uma imagem do clipboard local pro telemóvel — announce + uni-stream,
/// o mesmo par `share.file`/`webcam.mic_start` já usa (PROTOCOL.md §4).
async fn send_image_to_phone(
    active: &ActiveConn,
    hud: &Arc<Mutex<HudState>>,
    bytes: Vec<u8>,
) -> bool {
    let body = Value::Map(vec![
        (Value::Text("mime".into()), Value::Text("image/png".into())),
        (
            Value::Text("size".into()),
            Value::Integer((bytes.len() as i64).into()),
        ),
    ]);
    let Some(id) = announce(active, "clipboard.set", Some(body)).await else {
        return false;
    };
    let Some(connection) = active.lock().unwrap().clone() else {
        return false;
    };
    let Ok(mut send) = connection.open_uni().await else {
        return false;
    };
    if send.write_all(&id.to_be_bytes()).await.is_err() || send.write_all(&bytes).await.is_err() {
        return false;
    }
    let _ = send.finish();
    state::push_clip_image(hud, state::DIR_PC_TO_PHONE, bytes.len() as u64);
    push_log(
        hud,
        format!(
            "[i] clipboard.set · PC → telemóvel · imagem {} KB",
            bytes.len() / 1024
        ),
    );
    true
}

/// A imagem PNG que está agora no clipboard (se houver e couber no teto).
async fn current_image() -> Option<Vec<u8>> {
    let types = tokio_command("wl-paste")
        .args(["--list-types"])
        .output()
        .await
        .ok()?;
    if !String::from_utf8_lossy(&types.stdout)
        .split_whitespace()
        .any(|t| t == "image/png")
    {
        return None;
    }
    let bytes = tokio_command("wl-paste")
        .args(["--type", "image/png"])
        .output()
        .await
        .ok()?
        .stdout;
    (!bytes.is_empty() && bytes.len() <= MAX_IMAGE_BYTES).then_some(bytes)
}

/// `hash` é uma imagem diferente da última vista? Marca-a como vista.
fn is_new_image(last_seen: &mut Option<String>, hash: &str) -> bool {
    if last_seen.as_deref() == Some(hash) {
        return false;
    }
    *last_seen = Some(hash.to_string());
    true
}

/// Poll de imagens do clipboard local → telemóvel. Por que poll e não
/// `--watch` como o texto: o `wl-paste --watch` serve exatamente UM `--type`
/// e não tem evento de "a lista de tipos mudou" — um watcher `--type text`
/// nem sequer vê cópias só-imagem direito. Um poll de 1,5s lendo `wl-paste
/// -l` é barato (roundtrip wayland pequeno) e o custo real (ler os bytes)
/// só acontece quando há `image/png` na lista e o hash mudou.
pub async fn watch_images(
    active: ActiveConn,
    guard: LastLocalImage,
    outbox: Outbox,
    hud: Arc<Mutex<HudState>>,
) {
    // Baseline: a imagem que já está no clipboard quando o daemon sobe não
    // vai ao telemóvel. Lida uma vez aqui — antes, a «primeira imagem vista»
    // era a baseline, e se o clipboard só tinha texto no arranque, a
    // primeira captura do utilizador era engolida.
    let mut last_seen: Option<String> = current_image().await.map(|b| sha256_hex(&b));
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(1500));
    loop {
        interval.tick().await;
        let Some(bytes) = current_image().await else {
            continue;
        };
        let hash = sha256_hex(&bytes);
        if !is_new_image(&mut last_seen, &hash) {
            continue;
        }
        // Echo do que o próprio daemon escreveu (veio do telemóvel agora
        // mesmo) — consome o guard e segue.
        if take_echo(&mut guard.lock().unwrap(), &hash, Instant::now()) {
            continue;
        }
        if !send_image_to_phone(&active, &hud, bytes.clone()).await {
            keep_for_later(&outbox, Outgoing::Image(bytes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo(value: &str, at: Instant) -> Option<Echo> {
        Some(Echo {
            value: value.to_string(),
            at,
        })
    }

    #[test]
    fn sem_eco_registado_a_primeira_copia_segue() {
        let t0 = Instant::now();
        let mut guard = None;
        assert!(!take_echo(&mut guard, "olá", t0));
    }

    #[test]
    fn eco_do_telemovel_e_consumido_uma_vez() {
        let t0 = Instant::now();
        let mut guard = echo("do telemóvel", t0);
        assert!(take_echo(
            &mut guard,
            "do telemóvel",
            t0 + Duration::from_millis(200)
        ));
        assert!(guard.is_none());
        // O utilizador copia o mesmo texto outra vez: segue.
        assert!(!take_echo(
            &mut guard,
            "do telemóvel",
            t0 + Duration::from_secs(1)
        ));
    }

    #[test]
    fn eco_vencido_nao_engole_a_copia_do_utilizador() {
        let t0 = Instant::now();
        let mut guard = echo("x", t0);
        assert!(!take_echo(
            &mut guard,
            "x",
            t0 + ECHO_TTL + Duration::from_millis(1)
        ));
        assert!(guard.is_none());
    }

    #[test]
    fn eco_diferente_fica_a_espera() {
        let t0 = Instant::now();
        let mut guard = echo("a", t0);
        assert!(!take_echo(&mut guard, "b", t0));
        assert!(take_echo(&mut guard, "a", t0 + Duration::from_millis(100)));
    }

    #[test]
    fn texto_do_arranque_nao_vai_mas_a_copia_seguinte_vai() {
        let mut baseline = Some(Some("antigo".to_string()));
        assert!(is_startup_echo(&mut baseline, "antigo"));
        // Copiar o mesmo texto depois já é uma cópia a sério.
        assert!(!is_startup_echo(&mut baseline, "antigo"));
    }

    #[test]
    fn clipboard_sem_texto_no_arranque_a_primeira_copia_segue() {
        // Clipboard vazio ou com imagem: o `wl-paste --watch` não emite nada
        // ao arrancar; a primeira emissão já é a cópia do utilizador.
        let mut baseline = Some(None);
        assert!(!is_startup_echo(&mut baseline, "primeira"));
        let mut baseline = Some(Some("antigo".to_string()));
        assert!(!is_startup_echo(&mut baseline, "novo"));
        assert!(!is_startup_echo(&mut baseline, "antigo"));
    }

    #[test]
    fn copia_sem_telemovel_segue_so_se_recente() {
        let t0 = Instant::now();
        let mut slot = Some((Outgoing::Text("a".into()), t0));
        assert_eq!(
            take_fresh(&mut slot, t0 + Duration::from_secs(59)),
            Some(Outgoing::Text("a".into()))
        );
        assert!(slot.is_none());
        let mut slot = Some((Outgoing::Text("a".into()), t0));
        assert_eq!(take_fresh(&mut slot, t0 + PENDING_TTL), None);
        assert!(slot.is_none(), "uma cópia velha também sai da caixa");
    }

    #[test]
    fn so_a_ultima_copia_fica_guardada() {
        let outbox = new_outbox();
        keep_for_later(&outbox, Outgoing::Text("primeira".into()));
        keep_for_later(&outbox, Outgoing::Image(vec![1, 2, 3]));
        keep_for_later(&outbox, Outgoing::Text("última".into()));
        let item = take_fresh(&mut outbox.lock().unwrap(), Instant::now());
        assert_eq!(item, Some(Outgoing::Text("última".into())));
        assert!(take_fresh(&mut outbox.lock().unwrap(), Instant::now()).is_none());
    }

    #[test]
    fn primeira_imagem_depois_de_arrancar_sem_imagem_segue() {
        // Antes, a primeira imagem vista era a baseline e não seguia.
        let mut last_seen = None;
        assert!(is_new_image(&mut last_seen, "h1"));
        assert!(!is_new_image(&mut last_seen, "h1"));
        assert!(is_new_image(&mut last_seen, "h2"));
    }

    #[test]
    fn imagem_que_ja_estava_no_arranque_nao_segue() {
        let mut last_seen = Some("h0".to_string());
        assert!(!is_new_image(&mut last_seen, "h0"));
        assert!(is_new_image(&mut last_seen, "h1"));
    }
}
