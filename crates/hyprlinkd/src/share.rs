//! Transferência de ficheiros (Fase 5): telemóvel→PC e PC→telemóvel.

use std::collections::HashMap;
use std::future::Future;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ciborium::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::active::{ActiveConn, announce, push};
use crate::config;
use crate::state::{HudState, push_log};

pub(crate) struct PendingIncoming {
    name: String,
    size: u64,
    /// Pasta de destino (por `kind`), já usada na verificação de espaço.
    dir: PathBuf,
    /// Quando foi anunciado — os que ficam sem stream expiram.
    at: Instant,
    /// `Some(motivo)` = anúncio recusado (teto ou espaço): o stream que
    /// vier com este id é parado e o telemóvel recebe `share.done` ok=false.
    refused: Option<String>,
}

/// packet id do anúncio `share.file` -> nome/tamanho esperado, até o
/// uni-stream correspondente chegar.
pub type IncomingRegistry = Arc<Mutex<HashMap<u64, PendingIncoming>>>;

/// Anúncios à espera do stream ao mesmo tempo; acima disto sai o mais antigo.
const MAX_PENDING: usize = 32;
/// Um anúncio sem stream durante isto expira.
const PENDING_TTL: Duration = Duration::from_secs(5 * 60);
/// Folga de espaço livre exigida além do tamanho anunciado.
const FREE_MARGIN: u64 = 256 * 1024 * 1024;

pub fn new_incoming_registry() -> IncomingRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Aceita ou recusa um anúncio de `size` bytes. `max` 0 = sem teto; `free`
/// `None` = espaço livre desconhecido (não se recusa por isso).
fn admit(size: u64, max: u64, free: Option<u64>) -> Result<(), String> {
    if max != 0 && size > max {
        return Err(format!("excede o limite de {max} bytes"));
    }
    if let Some(free) = free
        && free < size.saturating_add(FREE_MARGIN)
    {
        return Err(format!("sem espaço no destino ({free} bytes livres)"));
    }
    Ok(())
}

/// Tira os expirados e, se ainda estiver cheio, os mais antigos, até caber
/// mais um.
fn make_room(map: &mut HashMap<u64, PendingIncoming>, now: Instant) {
    map.retain(|_, p| now.saturating_duration_since(p.at) < PENDING_TTL);
    while map.len() >= MAX_PENDING {
        let Some(oldest) = map.iter().min_by_key(|(_, p)| p.at).map(|(id, _)| *id) else {
            break;
        };
        map.remove(&oldest);
    }
}

/// Regista um anúncio `share.file`. `Err(motivo)` = recusado antes de se
/// criar o ficheiro (fica marcado para o stream ser parado).
pub fn announce_incoming(
    id: u64,
    name: String,
    size: u64,
    dir: PathBuf,
    registry: &IncomingRegistry,
    max: u64,
    free: Option<u64>,
) -> Result<(), String> {
    let verdict = admit(size, max, free);
    let now = Instant::now();
    let mut map = registry.lock().unwrap();
    make_room(&mut map, now);
    map.insert(
        id,
        PendingIncoming {
            name,
            size,
            dir,
            at: now,
            refused: verdict.clone().err(),
        },
    );
    verdict
}

/// Bytes livres (para utilizador normal) no sistema de ficheiros de `dir`;
/// se `dir` ainda não existe, o do primeiro antecessor que exista.
pub fn free_space_for(dir: &Path) -> Option<u64> {
    let existing = dir.ancestors().find(|a| a.exists())?;
    let st = rustix::fs::statvfs(existing).ok()?;
    Some(st.f_bavail.saturating_mul(st.f_frsize))
}

/// Só o último componente do caminho, rejeitando `.`/`..`/vazio — mesma
/// cautela que o app aplica do lado dele. Caracteres de controlo viram `_`;
/// nomes longos são cortados a [`MAX_NAME_BYTES`] (mantendo a extensão); um
/// nome que pareça um parcial (`….hyprlink-part`) leva `_` no fim, para a
/// limpeza de órfãos nunca lhe tocar.
fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    if base.is_empty() || base == "." || base == ".." {
        return "arquivo_recebido".to_string();
    }
    let mut out: String = base
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    if out.len() > MAX_NAME_BYTES {
        let ext = match out.rfind('.') {
            Some(i) if i > 0 && out.len() - i <= 17 => out[i..].to_string(),
            _ => String::new(),
        };
        let mut cut = MAX_NAME_BYTES - ext.len();
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str(&ext);
    }
    if out.ends_with(PART_SUFFIX) {
        out.push('_');
    }
    out
}

/// Sufixo dos ficheiros a meio da receção (`.<nome>.hyprlink-part`).
const PART_SUFFIX: &str = ".hyprlink-part";
/// Parciais órfãos (daemon morto a meio) mais velhos do que isto são
/// apagados ao arrancar.
pub const ORPHAN_PART_AGE: Duration = Duration::from_secs(60 * 60);
/// Sem bytes novos durante isto, a receção expira.
const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// `nome (1).ext` … `nome (N).ext` antes de recorrer à hora.
const MAX_NAME_SUFFIX: u32 = 999;
/// Comprimento máximo (bytes) do nome guardado, para caberem o prefixo/sufixo
/// do parcial e o ` (999)` dentro dos 255 bytes do Linux.
const MAX_NAME_BYTES: usize = 200;

/// Como acabou uma receção que não chegou ao fim. Em todos os casos o
/// `.hyprlink-part` já foi apagado.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// Não deu para criar o parcial (permissões, nome…).
    Create(String),
    Cancelled,
    /// O telemóvel mandou mais do que anunciou.
    Exceeded,
    /// A stream acabou antes do tamanho anunciado.
    Short {
        got: u64,
        expected: u64,
    },
    Read,
    Write,
    /// Sem bytes durante [`READ_IDLE_TIMEOUT`].
    Expired,
    /// O `rename` final falhou.
    Finish(String),
}

impl Failure {
    /// Motivo curto, para o HUD e para o `error` do `share.done`.
    fn reason(&self) -> String {
        match self {
            Failure::Create(e) => format!("não foi possível criar o ficheiro: {e}"),
            Failure::Cancelled => "cancelado pelo utilizador".into(),
            Failure::Exceeded => "excedeu o tamanho anunciado".into(),
            Failure::Short { got, expected } => {
                format!("incompleto: {got} de {expected} bytes")
            }
            Failure::Read => "a ligação falhou a meio".into(),
            Failure::Write => "erro ao escrever no disco".into(),
            Failure::Expired => "sem dados há demasiado tempo".into(),
            Failure::Finish(e) => format!("não foi possível gravar: {e}"),
        }
    }
}

/// Receção que acabou bem: nome final (na pasta de destino), bytes e SHA-256.
#[derive(Debug)]
struct Received {
    name: String,
    bytes: u64,
    sha_hex: String,
}

/// Nomes finais a tentar, por ordem: `nome.ext`, `nome (1).ext` … `nome
/// (999).ext` e, depois, com a hora (`nome (20261014-191500).ext`) e a hora
/// mais o id da transferência.
fn candidate_names(name: &str, id: u64) -> impl Iterator<Item = String> {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 17 && i + 1 < name.len() => name.split_at(i),
        _ => (name, ""),
    };
    let (stem, ext) = (stem.to_string(), ext.to_string());
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    std::iter::once(name.to_string())
        .chain((1..=MAX_NAME_SUFFIX).map({
            let (stem, ext) = (stem.clone(), ext.clone());
            move |i| format!("{stem} ({i}){ext}")
        }))
        .chain([
            format!("{stem} ({ts}){ext}"),
            format!("{stem} ({ts}-{id}){ext}"),
        ])
}

fn errno_is(e: &std::io::Error, errno: rustix::io::Errno) -> bool {
    e.raw_os_error() == Some(errno.raw_os_error())
}

/// `rename` que nunca substitui `to`: `AlreadyExists` se já existir. Usa
/// `renameat2(RENAME_NOREPLACE)`; em sistemas de ficheiros que não o têm,
/// `hard_link` + `remove` (o link também falha se `to` existir); sem links
/// (ex. FAT), reserva `to` com `create_new` e só então renomeia por cima
/// desse marcador, que é nosso.
fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    use rustix::io::Errno;
    let unsupported = |e: &std::io::Error| {
        [Errno::INVAL, Errno::NOSYS, Errno::OPNOTSUPP, Errno::PERM]
            .into_iter()
            .any(|n| errno_is(e, n))
    };
    let e: std::io::Error = match rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        Ok(()) => return Ok(()),
        Err(e) => e.into(),
    };
    if !unsupported(&e) {
        return Err(e);
    }
    match std::fs::hard_link(from, to) {
        Ok(()) => {
            let _ = std::fs::remove_file(from);
            return Ok(());
        }
        Err(e) if !unsupported(&e) => return Err(e),
        Err(_) => {}
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(to)?;
    std::fs::rename(from, to)
}

/// Dá ao parcial `part` o primeiro nome livre de [`candidate_names`] em
/// `dir`, sem nunca substituir um ficheiro existente (nem numa corrida com
/// outro processo). Devolve o nome final.
fn place_without_replacing(
    part: &Path,
    dir: &Path,
    name: &str,
    id: u64,
) -> std::io::Result<String> {
    for candidate in candidate_names(name, id) {
        match rename_noreplace(part, &dir.join(&candidate)) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "sem nome livre",
    ))
}

/// Cria `.<nome>.hyprlink-part` em `dir` (0600, sem substituir nada). Se já
/// existir (outra receção com o mesmo nome em curso), acrescenta `-<id>`.
async fn create_part(
    dir: &Path,
    name: &str,
    id: u64,
) -> std::io::Result<(PathBuf, tokio::fs::File)> {
    let mut last = None;
    for candidate in [
        format!(".{name}{PART_SUFFIX}"),
        format!(".{name}-{id}{PART_SUFFIX}"),
    ] {
        let path = dir.join(candidate);
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .await
        {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("sem nome livre")))
}

/// `umask` do processo, lido de `/proc/self/status` (mudá-lo para o ler,
/// com `umask(2)`, seria uma corrida com as outras threads). Sem o campo,
/// assume o habitual `022`.
fn current_umask() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Umask:"))
                .and_then(|v| u32::from_str_radix(v.trim(), 8).ok())
        })
        .unwrap_or(0o022)
}

/// Modo do ficheiro final: o de um ficheiro novo do utilizador
/// (`0666 & !umask`, normalmente `0644`), como um descarregado pelo browser.
/// O parcial fica `0600` enquanto está incompleto.
fn final_mode(umask: u32) -> u32 {
    0o666 & !umask
}

/// O que o anúncio `share.file` prometeu: nome (já sanitizado), id e tamanho.
struct Expected<'a> {
    name: &'a str,
    id: u64,
    size: u64,
}

/// Recebe `size` bytes de `reader` para `dir`, primeiro num `.hyprlink-part`
/// e só no fim com o nome final. Qualquer falha apaga o parcial. `progress`
/// é chamado a cada ~512 KiB com o total já escrito.
async fn receive_to_dir<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    dir: &Path,
    Expected { name, id, size }: Expected<'_>,
    cancel: &std::sync::atomic::AtomicBool,
    idle: Duration,
    umask: u32,
    mut progress: impl FnMut(u64),
) -> Result<Received, Failure> {
    let (part, mut file) = create_part(dir, name, id)
        .await
        .map_err(|e| Failure::Create(e.to_string()))?;
    let outcome = async {
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 32 * 1024];
        let mut total: u64 = 0;
        let mut last_progress = 0u64;
        loop {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(Failure::Cancelled);
            }
            let n = match tokio::time::timeout(idle, reader.read(&mut buf)).await {
                Err(_) => return Err(Failure::Expired),
                Ok(Err(_)) => return Err(Failure::Read),
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => n,
            };
            // O peer anunciou `size` de antemão (share.file) — se mandar
            // mais do que isso, é ou um bug ou alguém a tentar encher o disco;
            // corta logo em vez de escrever até a stream fechar sozinha.
            if total + n as u64 > size {
                return Err(Failure::Exceeded);
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .await
                .map_err(|_| Failure::Write)?;
            total += n as u64;
            if total.saturating_sub(last_progress) >= 512 * 1024 {
                last_progress = total;
                progress(total);
            }
        }
        // EOF antes do anunciado: o telemóvel desistiu ou a ligação caiu —
        // não é um ficheiro completo, não se grava.
        if total != size {
            return Err(Failure::Short {
                got: total,
                expected: size,
            });
        }
        file.flush().await.map_err(|_| Failure::Write)?;
        {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(std::fs::Permissions::from_mode(final_mode(umask)))
                .await
                .map_err(|_| Failure::Write)?;
        }
        file.sync_all().await.map_err(|_| Failure::Write)?;
        let sha_hex: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok((total, sha_hex))
    }
    .await;
    drop(file);
    let finished = match outcome {
        Ok((bytes, sha_hex)) => {
            let (part, dir, name) = (part.clone(), dir.to_path_buf(), name.to_string());
            tokio::task::spawn_blocking(move || place_without_replacing(&part, &dir, &name, id))
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))
                .flatten()
                .map(|name| Received {
                    name,
                    bytes,
                    sha_hex,
                })
                .map_err(|e| Failure::Finish(e.to_string()))
        }
        Err(f) => Err(f),
    };
    if finished.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
    }
    finished
}

/// Apaga os `.<nome>.hyprlink-part` com mais de `max_age` (por `mtime`,
/// relativo a `now`) em `base` e nas suas [`config::INCOMING_SUBDIRS`] (sem
/// descer mais). Só ficheiros normais; ligações simbólicas e pastas ficam.
/// Devolve quantos apagou.
pub fn clean_orphan_parts(base: &Path, max_age: Duration, now: std::time::SystemTime) -> usize {
    std::iter::once(base.to_path_buf())
        .chain(config::INCOMING_SUBDIRS.map(|s| base.join(s)))
        .map(|dir| clean_orphan_parts_in(&dir, max_age, now))
        .sum()
}

fn clean_orphan_parts_in(dir: &Path, max_age: Duration, now: std::time::SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.starts_with('.') && name.ends_with(PART_SUFFIX)) {
            continue;
        }
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > max_age);
        if meta.is_file() && old && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Trata um stream unidirecional recebido: `id` é o id de correlação com um
/// `share.file` já anunciado (já lido pelo chamador — o roteador em
/// `server.rs` precisa decidir entre ficheiro/webcam antes de despachar),
/// com uma pequena espera — o anúncio e o stream de bytes podem chegar em
/// tarefas concorrentes.
pub async fn receive_uni_stream(
    mut recv: quinn::RecvStream,
    id: u64,
    registry: IncomingRegistry,
    active: ActiveConn,
    hud: Arc<Mutex<HudState>>,
) {
    let mut pending = None;
    for _ in 0..20 {
        if let Some(p) = registry.lock().unwrap().remove(&id) {
            pending = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let Some(pending) = pending else {
        push_log(
            &hud,
            format!("[!] share.file: stream sem anúncio correspondente (id={id})"),
        );
        return;
    };
    if let Some(reason) = pending.refused {
        // Recusado no anúncio: não se cria nada; para o envio e diz porquê.
        let _ = recv.stop(0u32.into());
        push(&active, "share.done", Some(done_failed(id, reason))).await;
        return;
    }

    let dir = pending.dir;
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        push_log(
            &hud,
            format!(
                "[!] share.file: não foi possível criar {}: {e}",
                dir.display()
            ),
        );
        push(
            &active,
            "share.done",
            Some(done_failed(id, "pasta de destino indisponível".into())),
        )
        .await;
        return;
    }
    let filename = sanitize_filename(&pending.name);
    let cancel =
        crate::state::start_file_transfer(&hud, filename.clone(), "recebendo", pending.size);

    // O progresso sai por um canal para uma tarefa só, que manda os
    // `share.progress` por ordem; acaba antes do `share.done`.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
    let forwarder = {
        let active = active.clone();
        let hud = hud.clone();
        let size = pending.size;
        tokio::spawn(async move {
            while let Some(total) = rx.recv().await {
                crate::state::update_file_transfer_progress(&hud, total);
                let body = Value::Map(vec![
                    (Value::Text("id".into()), Value::Integer(id.into())),
                    (Value::Text("bytes".into()), Value::Integer(total.into())),
                    (Value::Text("total".into()), Value::Integer(size.into())),
                ]);
                push(&active, "share.progress", Some(body)).await;
            }
        })
    };
    let result = receive_to_dir(
        &mut recv,
        &dir,
        Expected {
            name: &filename,
            id,
            size: pending.size,
        },
        &cancel,
        READ_IDLE_TIMEOUT,
        current_umask(),
        |total| {
            let _ = tx.send(total);
        },
    )
    .await;
    drop(tx);
    let _ = forwarder.await;

    match result {
        Ok(got) => {
            crate::state::set_file_transfer_name(&hud, got.name.clone());
            crate::state::finish_file_transfer(&hud, true, None);
            push_log(
                &hud,
                format!(
                    "[+] ficheiro recebido: {} ({} bytes) · sha256 {}",
                    got.name, got.bytes, got.sha_hex
                ),
            );
            let body = Value::Map(vec![
                (Value::Text("id".into()), Value::Integer(id.into())),
                (Value::Text("ok".into()), Value::Bool(true)),
                (Value::Text("sha256".into()), Value::Text(got.sha_hex)),
                (
                    Value::Text("bytes".into()),
                    Value::Integer(got.bytes.into()),
                ),
                (Value::Text("error".into()), Value::Null),
                // Nome com que ficou gravado (pode ser `nome (1).ext`).
                (Value::Text("name".into()), Value::Text(got.name)),
            ]);
            push(&active, "share.done", Some(body)).await;
        }
        Err(failure) => {
            if matches!(
                failure,
                Failure::Cancelled | Failure::Exceeded | Failure::Expired
            ) {
                let _ = recv.stop(0u32.into());
            }
            let reason = failure.reason();
            crate::state::finish_file_transfer(&hud, false, Some(reason.clone()));
            push_log(
                &hud,
                format!("[!] ficheiro recebido falhou: {filename} · {reason} (parcial apagado)"),
            );
            push(&active, "share.done", Some(done_failed(id, reason))).await;
        }
    }
}

/// `share.done` de uma receção que não se gravou (mesmo formato do 45).
fn done_failed(id: u64, reason: String) -> Value {
    Value::Map(vec![
        (Value::Text("id".into()), Value::Integer(id.into())),
        (Value::Text("ok".into()), Value::Bool(false)),
        (Value::Text("sha256".into()), Value::Text(String::new())),
        (Value::Text("bytes".into()), Value::Integer(0.into())),
        (Value::Text("error".into()), Value::Text(reason)),
    ])
}

/// Fila dos envios PC→telemóvel: **um ficheiro de cada vez**. O estado só tem
/// um espaço de transferência ativa (`HudState::file_transfer`/`file_cancel`);
/// com N envios em paralelo o progresso trocava-se, só o último se podia
/// cancelar e o histórico ficava com uma só entrada. A `Semaphore` do tokio é
/// justa (FIFO): os ficheiros saem pela ordem em que chegaram. A **receção**
/// (telemóvel→PC) não passa por aqui — as capturas automáticas do telemóvel não
/// podem esperar por um envio longo (a app dá uma transferência a 0 % > 30 s
/// por falhada).
static SEND_QUEUE: Semaphore = Semaphore::const_new(1);

/// Espera a vez na fila `queue`. `None` se `closed` terminar primeiro (a
/// ligação caiu enquanto esperava): o envio desiste sem erro e **sem ficar
/// com a permissão** — nunca foi adquirida. A permissão devolvida larga-se
/// sozinha (RAII) em qualquer saída do envio: fim, erro de rede, cancelado.
async fn acquire_slot<'a>(
    queue: &'a Semaphore,
    closed: impl Future<Output = ()>,
    on_queued: impl FnOnce(),
) -> Option<SemaphorePermit<'a>> {
    match queue.try_acquire() {
        Ok(p) => Some(p),
        Err(_) => {
            on_queued();
            tokio::select! {
                p = queue.acquire() => p.ok(),
                _ = closed => None,
            }
        }
    }
}

/// Envia um ficheiro do PC pro telemóvel: anuncia (`share.file`, D→P) pra
/// ganhar um `id`, depois abre um uni-stream próprio (8 bytes de id + bytes
/// crus, mesmo formato que o telemóvel usa na direção inversa) e transmite
/// em chunks de 32 KiB. Não espera pelo `share.done` de volta — isso chega
/// como qualquer outro pacote em `server.rs` e só é logado.
pub async fn send_file(active: &ActiveConn, hud: &Arc<Mutex<HudState>>, path: &std::path::Path) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("arquivo")
        .to_string();
    // Um de cada vez, ANTES de anunciar: o telemóvel só espera uns 10 s pelo
    // uni-stream depois do `share.file`, e um anúncio na fila expirava.
    let Some(connection) = active.lock().unwrap().clone() else {
        push_log(
            hud,
            "[!] share.file: sem conexão ativa pra enviar".to_string(),
        );
        return;
    };
    let Some(_slot) = acquire_slot(
        &SEND_QUEUE,
        async {
            let _ = connection.closed().await;
        },
        || push_log(hud, format!("[i] share.file · {name} em fila")),
    )
    .await
    else {
        push_log(
            hud,
            format!("[i] share.file · {name} desistiu da fila (a ligação caiu)"),
        );
        return;
    };
    let size = match tokio::fs::metadata(path).await {
        Ok(m) => m.len(),
        Err(e) => {
            push_log(
                hud,
                format!(
                    "[!] share.file: não foi possível ler {}: {e}",
                    path.display()
                ),
            );
            return;
        }
    };

    let body = Value::Map(vec![
        (Value::Text("name".into()), Value::Text(name.clone())),
        (Value::Text("size".into()), Value::Integer(size.into())),
    ]);
    let Some(id) = announce(active, "share.file", Some(body)).await else {
        push_log(
            hud,
            "[!] share.file: sem conexão ativa pra enviar".to_string(),
        );
        return;
    };

    let connection = { active.lock().unwrap().clone() };
    let Some(connection) = connection else { return };
    let mut send = match connection.open_uni().await {
        Ok(s) => s,
        Err(e) => {
            push_log(
                hud,
                format!("[!] share.file: não foi possível abrir o uni-stream: {e}"),
            );
            return;
        }
    };
    if send.write_all(&id.to_be_bytes()).await.is_err() {
        return;
    }

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            push_log(
                hud,
                format!(
                    "[!] share.file: não foi possível abrir {}: {e}",
                    path.display()
                ),
            );
            return;
        }
    };

    push_log(
        hud,
        format!("[i] share.file · enviando {name} ({size} bytes)"),
    );
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 32 * 1024];
    let mut total: u64 = 0;
    let mut last_progress = 0u64;
    let cancel = crate::state::start_file_transfer(hud, name.clone(), "enviando", size);
    crate::state::set_file_transfer_wire_id(hud, id);
    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = send.reset(0u32.into());
            crate::state::finish_file_transfer(
                hud,
                false,
                Some("cancelado pelo usuário".to_string()),
            );
            push_log(
                hud,
                format!("[!] envio cancelado: {name} ({total} de {size} bytes)"),
            );
            return;
        }
        let n = match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if send.write_all(&buf[..n]).await.is_err() {
            crate::state::finish_file_transfer(hud, false, Some("falha de rede".to_string()));
            push_log(
                hud,
                format!("[!] share.file: falha ao enviar {name} em {total} bytes"),
            );
            return;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
        if total.saturating_sub(last_progress) >= 512 * 1024 {
            last_progress = total;
            crate::state::update_file_transfer_progress(hud, total);
        }
    }
    let _ = send.finish();
    crate::state::finish_file_transfer(hud, true, None);

    // O telemóvel (receptor nessa direção) espera um `share.done` do
    // remetente pra confirmar/verificar — mesmo papel que o daemon já faz
    // pra ficheiros telemóvel→PC, só invertido (ver `receive_uni_stream`).
    let sha_hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let done_body = Value::Map(vec![
        (Value::Text("id".into()), Value::Integer(id.into())),
        (Value::Text("ok".into()), Value::Bool(true)),
        (Value::Text("sha256".into()), Value::Text(sha_hex.clone())),
        (Value::Text("bytes".into()), Value::Integer(total.into())),
        (Value::Text("error".into()), Value::Null),
    ]);
    push(active, "share.done", Some(done_body)).await;
    push_log(
        hud,
        format!("[+] ficheiro enviado: {name} ({total} bytes) · sha256 {sha_hex}"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    type Log = Arc<Mutex<Vec<String>>>;

    /// Um «envio» de teste: espera a vez, regista início e fim, e demora `ms`.
    /// `fail` = sai a meio (como um erro de rede) sem chegar ao fim.
    async fn job(q: Arc<Semaphore>, log: Log, id: usize, ms: u64, fail: bool) {
        let Some(_slot) = acquire_slot(&q, pending::<()>(), || {}).await else {
            return;
        };
        log.lock().unwrap().push(format!("start {id}"));
        tokio::time::sleep(Duration::from_millis(ms)).await;
        if fail {
            log.lock().unwrap().push(format!("fail {id}"));
            return; // a permissão larga-se aqui, como num `return` antecipado
        }
        log.lock().unwrap().push(format!("end {id}"));
    }

    #[tokio::test]
    async fn dois_envios_em_simultaneo_correm_um_depois_do_outro() {
        let q = Arc::new(Semaphore::new(1));
        let log: Log = Arc::default();
        let a = tokio::spawn(job(q.clone(), log.clone(), 0, 40, false));
        let b = tokio::spawn(job(q.clone(), log.clone(), 1, 10, false));
        a.await.unwrap();
        b.await.unwrap();
        // O segundo só começa depois de o primeiro acabar (sem sobreposição),
        // mesmo sendo mais curto.
        assert_eq!(
            *log.lock().unwrap(),
            ["start 0", "end 0", "start 1", "end 1"]
        );
    }

    #[tokio::test]
    async fn a_permissao_volta_quando_o_envio_falha() {
        let q = Arc::new(Semaphore::new(1));
        let log: Log = Arc::default();
        job(q.clone(), log.clone(), 0, 1, true).await;
        assert_eq!(q.available_permits(), 1, "a falha não prende a fila");
        // E o seguinte corre normalmente.
        job(q.clone(), log.clone(), 1, 1, false).await;
        assert_eq!(
            *log.lock().unwrap(),
            ["start 0", "fail 0", "start 1", "end 1"]
        );
    }

    #[tokio::test]
    async fn a_ordem_de_chegada_e_respeitada() {
        let q = Arc::new(Semaphore::new(1));
        let log: Log = Arc::default();
        // O 0 segura a fila enquanto os outros se enfileiram, um a um.
        let mut hs = vec![tokio::spawn(job(q.clone(), log.clone(), 0, 60, false))];
        tokio::time::sleep(Duration::from_millis(10)).await;
        for id in 1..6 {
            hs.push(tokio::spawn(job(q.clone(), log.clone(), id, 1, false)));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        for h in hs {
            h.await.unwrap();
        }
        let starts: Vec<String> = log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("start"))
            .cloned()
            .collect();
        assert_eq!(
            starts,
            [
                "start 0", "start 1", "start 2", "start 3", "start 4", "start 5"
            ]
        );
    }

    #[tokio::test]
    async fn ligacao_a_cair_na_fila_desiste_sem_prender_a_permissao() {
        let q = Arc::new(Semaphore::new(1));
        // Alguém segura a vez.
        let holder = q.clone().acquire_owned().await.unwrap();
        let (drop_conn, closed) = tokio::sync::oneshot::channel::<()>();
        let queued = Arc::new(AtomicUsize::new(0));
        let waiter = {
            let (q, queued) = (q.clone(), queued.clone());
            tokio::spawn(async move {
                acquire_slot(
                    &q,
                    async {
                        let _ = closed.await;
                    },
                    || {
                        queued.fetch_add(1, Ordering::SeqCst);
                    },
                )
                .await
                .is_none()
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(queued.load(Ordering::SeqCst), 1, "avisou que ficou em fila");
        assert!(!waiter.is_finished(), "ainda à espera da vez");
        let _ = drop_conn.send(()); // a ligação caiu
        assert!(waiter.await.unwrap(), "desistiu (None)");
        // O dono larga a vez: está livre, nada ficou preso pelo que desistiu.
        drop(holder);
        assert_eq!(q.available_permits(), 1);
        assert!(q.try_acquire().is_ok());
    }

    #[tokio::test]
    async fn fila_vazia_nao_avisa_nem_espera() {
        let q = Semaphore::new(1);
        let avisou = AtomicUsize::new(0);
        let slot = acquire_slot(&q, pending::<()>(), || {
            avisou.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        assert!(slot.is_some());
        assert_eq!(avisou.load(Ordering::SeqCst), 0);
        assert_eq!(q.available_permits(), 0, "a permissão está em uso");
        drop(slot);
        assert_eq!(q.available_permits(), 1);
    }

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn anuncio_acima_do_teto_e_recusado_e_marcado() {
        let reg = new_incoming_registry();
        let r = announce_incoming(
            1,
            "a.iso".into(),
            5 * GIB,
            PathBuf::new(),
            &reg,
            4 * GIB,
            Some(100 * GIB),
        );
        assert!(r.unwrap_err().contains("limite"));
        assert!(reg.lock().unwrap()[&1].refused.is_some());
        // No teto exato passa; 0 = sem teto.
        assert!(
            announce_incoming(2, "b".into(), 4 * GIB, PathBuf::new(), &reg, 4 * GIB, None).is_ok()
        );
        assert!(
            announce_incoming(
                3,
                "c".into(),
                50 * GIB,
                PathBuf::new(),
                &reg,
                0,
                Some(100 * GIB)
            )
            .is_ok()
        );
        assert!(reg.lock().unwrap()[&2].refused.is_none());
    }

    #[test]
    fn sem_espaco_livre_com_margem_e_recusado() {
        let mb = 1024 * 1024;
        assert!(admit(GIB, 0, Some(GIB + FREE_MARGIN)).is_ok());
        let e = admit(GIB, 0, Some(GIB + FREE_MARGIN - 1)).unwrap_err();
        assert!(e.contains("espaço"));
        assert!(admit(10 * mb, 4 * GIB, Some(0)).is_err());
        // Espaço desconhecido não recusa; tamanho enorme não transborda.
        assert!(admit(GIB, 0, None).is_ok());
        assert!(admit(u64::MAX - 1, 0, Some(u64::MAX - 2)).is_err());
    }

    #[test]
    fn espaco_livre_de_pasta_que_ainda_nao_existe_usa_o_antecessor() {
        let dir = std::env::temp_dir().join("hyprlink-teste-nao-existe/a/b");
        assert!(free_space_for(&dir).is_some());
    }

    #[test]
    fn excesso_de_anuncios_pendentes_tira_os_mais_antigos() {
        let reg = new_incoming_registry();
        for id in 0..(MAX_PENDING as u64 + 10) {
            announce_incoming(id, format!("f{id}"), 1, PathBuf::new(), &reg, 0, None).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        let map = reg.lock().unwrap();
        assert_eq!(map.len(), MAX_PENDING);
        assert!(!map.contains_key(&0) && !map.contains_key(&9));
        assert!(map.contains_key(&10) && map.contains_key(&(MAX_PENDING as u64 + 9)));
    }

    #[test]
    fn anuncios_sem_stream_expiram() {
        let t0 = Instant::now();
        let mut map = HashMap::new();
        for (id, at) in [(1, t0), (2, t0 + Duration::from_secs(4 * 60))] {
            map.insert(
                id,
                PendingIncoming {
                    name: String::new(),
                    size: 1,
                    dir: PathBuf::new(),
                    at,
                    refused: None,
                },
            );
        }
        make_room(&mut map, t0 + PENDING_TTL + Duration::from_secs(1));
        assert!(!map.contains_key(&1), "o de há mais de 5 min expirou");
        assert!(map.contains_key(&2));
    }

    /// Pasta temporária própria de cada teste; apagada no início e no fim
    /// (`Drop`).
    struct TmpDir(PathBuf);
    impl std::ops::Deref for TmpDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmp_dir(tag: &str) -> TmpDir {
        let d = std::env::temp_dir().join(format!("hyprlink-teste-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TmpDir(d)
    }

    /// Nomes dos ficheiros em `dir`, ordenados.
    fn listing(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    async fn recv_bytes_umask(
        dir: &Path,
        name: &str,
        data: &[u8],
        size: u64,
        cancel: &std::sync::atomic::AtomicBool,
        umask: u32,
    ) -> Result<Received, Failure> {
        let mut reader = data;
        receive_to_dir(
            &mut reader,
            dir,
            Expected { name, id: 7, size },
            cancel,
            READ_IDLE_TIMEOUT,
            umask,
            |_| {},
        )
        .await
    }

    async fn recv_bytes(
        dir: &Path,
        name: &str,
        data: &[u8],
        size: u64,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Received, Failure> {
        recv_bytes_umask(dir, name, data, size, cancel, 0o022).await
    }

    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[tokio::test]
    async fn rececao_completa_fica_so_com_o_nome_final() {
        let dir = tmp_dir("completa");
        let no = std::sync::atomic::AtomicBool::new(false);
        let got = recv_bytes(&dir, "a.txt", b"ola", 3, &no).await.unwrap();
        assert_eq!((got.name.as_str(), got.bytes), ("a.txt", 3));
        assert_eq!(listing(&dir), ["a.txt"]);
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"ola");
    }

    /// O parcial nasce `0600`; o final fica `0666 & !umask`, como um
    /// ficheiro descarregado pelo browser.
    #[tokio::test]
    async fn ficheiro_final_segue_o_umask_do_utilizador() {
        let no = std::sync::atomic::AtomicBool::new(false);
        for (umask, esperado) in [(0o022, 0o644), (0o077, 0o600), (0o002, 0o664)] {
            let dir = tmp_dir(&format!("umask-{umask:o}"));
            recv_bytes_umask(&dir, "a.txt", b"ola", 3, &no, umask)
                .await
                .unwrap();
            assert_eq!(mode_of(&dir.join("a.txt")), esperado, "umask {umask:o}");
        }
        assert_eq!(final_mode(0o022), 0o644);
        assert_eq!(final_mode(0o077), 0o600);
    }

    #[tokio::test]
    async fn parcial_nasce_em_0600() {
        let dir = tmp_dir("parcial-0600");
        let (path, _f) = create_part(&dir, "a.txt", 7).await.unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    #[test]
    fn umask_do_processo_le_se_do_proc() {
        // Não se muda o umask (é do processo todo e os testes correm em
        // paralelo): só se confirma que o valor lido é plausível.
        let u = current_umask();
        assert!(u <= 0o777, "{u:o}");
    }

    #[tokio::test]
    async fn cancelamento_nao_deixa_ficheiros() {
        let dir = tmp_dir("cancelado");
        let cancel = std::sync::atomic::AtomicBool::new(true);
        let r = recv_bytes(&dir, "a.txt", b"ola", 3, &cancel).await;
        assert_eq!(r.unwrap_err(), Failure::Cancelled);
        assert!(listing(&dir).is_empty());

        // A meio: o progresso liga a flag.
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let data = vec![1u8; 2 * 1024 * 1024];
        let mut reader = &data[..];
        let r = receive_to_dir(
            &mut reader,
            &dir,
            Expected {
                name: "b.bin",
                id: 7,
                size: data.len() as u64,
            },
            &cancel,
            READ_IDLE_TIMEOUT,
            0o022,
            |_| cancel.store(true, std::sync::atomic::Ordering::Relaxed),
        )
        .await;
        assert_eq!(r.unwrap_err(), Failure::Cancelled);
        assert!(listing(&dir).is_empty());
    }

    #[tokio::test]
    async fn tamanho_excedido_nao_deixa_ficheiros() {
        let dir = tmp_dir("excedido");
        let no = std::sync::atomic::AtomicBool::new(false);
        let r = recv_bytes(&dir, "a.txt", b"ola mundo", 3, &no).await;
        assert_eq!(r.unwrap_err(), Failure::Exceeded);
        assert!(listing(&dir).is_empty());
    }

    #[test]
    fn parcial_orfao_antigo_e_limpo_e_recente_nao() {
        let dir = tmp_dir("orfaos");
        let velho = dir.join(".velho.txt.hyprlink-part");
        let novo = dir.join(".novo.txt.hyprlink-part");
        let normal = dir.join("normal.txt");
        let normal_velho = dir.join("normal-velho.hyprlink-part");
        for p in [&velho, &novo, &normal, &normal_velho] {
            std::fs::write(p, b"x").unwrap();
        }
        let ha_2h = std::time::SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        for p in [&velho, &normal, &normal_velho] {
            std::fs::File::options()
                .write(true)
                .open(p)
                .unwrap()
                .set_modified(ha_2h)
                .unwrap();
        }
        let n = clean_orphan_parts(&dir, ORPHAN_PART_AGE, std::time::SystemTime::now());
        assert_eq!(n, 1);
        assert_eq!(
            listing(&dir),
            [
                ".novo.txt.hyprlink-part",
                "normal-velho.hyprlink-part",
                "normal.txt"
            ]
        );
    }

    fn shared_config(base: &Path) -> config::SharedConfig {
        let json = serde_json::json!({ "download_dir": base });
        Arc::new(Mutex::new(serde_json::from_value(json).unwrap()))
    }

    /// Anúncio → registo → receção, como `server.rs` + `receive_uni_stream`:
    /// screenshot vai para `Screenshot`, o resto (e app antiga sem `kind`)
    /// para `Transferidos`; o mesmo nome repetido ganha `(1)` só na sua pasta.
    #[tokio::test]
    async fn rececao_cai_na_pasta_do_kind() {
        let base = tmp_dir("kind");
        let config = shared_config(&base);
        let reg = new_incoming_registry();
        let no = std::sync::atomic::AtomicBool::new(false);
        for (id, kind) in [
            (1, Some("screenshot")),
            (2, None),
            (3, Some("../x")),
            (4, Some("screenshot")),
        ] {
            let dir = config::incoming_dir(&config, kind);
            let free = free_space_for(&dir);
            announce_incoming(id, "s.png".into(), 2, dir, &reg, 0, free).unwrap();
            let pending = reg.lock().unwrap().remove(&id).unwrap();
            std::fs::create_dir_all(&pending.dir).unwrap();
            recv_bytes(&pending.dir, &pending.name, b"ok", 2, &no)
                .await
                .unwrap();
        }
        assert_eq!(listing(&base), ["Screenshot", "Transferidos"]);
        assert_eq!(listing(&base.join("Screenshot")), ["s (1).png", "s.png"]);
        assert_eq!(listing(&base.join("Transferidos")), ["s (1).png", "s.png"]);
    }

    /// O espaço livre é o da pasta de destino real (que ainda pode não
    /// existir), e é essa pasta que fica no registo para a receção.
    #[test]
    fn espaco_livre_e_destino_sao_a_mesma_pasta() {
        let base = tmp_dir("espaco");
        let config = shared_config(&base);
        let reg = new_incoming_registry();
        let dir = config::incoming_dir(&config, Some("screenshot"));
        assert!(!dir.exists());
        let free = free_space_for(&dir);
        assert_eq!(free, free_space_for(&base));
        announce_incoming(1, "a".into(), 1, dir.clone(), &reg, 0, free).unwrap();
        assert_eq!(reg.lock().unwrap()[&1].dir, base.join("Screenshot"));
        let r = announce_incoming(2, "b".into(), 1, dir, &reg, 0, Some(0));
        assert!(r.unwrap_err().contains("espaço"));
    }

    #[test]
    fn parciais_orfaos_limpos_na_base_e_nas_duas_subpastas() {
        let base = tmp_dir("orfaos-sub");
        let fundo = base.join("Screenshot/fundo");
        std::fs::create_dir_all(base.join("Transferidos")).unwrap();
        std::fs::create_dir_all(&fundo).unwrap();
        let ha_2h = std::time::SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        for d in [
            &*base,
            &base.join("Screenshot"),
            &base.join("Transferidos"),
            &fundo,
        ] {
            let p = d.join(".v.png.hyprlink-part");
            std::fs::write(&p, b"x").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(ha_2h)
                .unwrap();
        }
        let n = clean_orphan_parts(&base, ORPHAN_PART_AGE, std::time::SystemTime::now());
        assert_eq!(n, 3, "base + Screenshot + Transferidos; sem recursão");
        assert_eq!(listing(&fundo), [".v.png.hyprlink-part"]);
    }

    #[tokio::test]
    async fn stream_curta_e_falha_e_nao_deixa_ficheiros() {
        let dir = tmp_dir("curta");
        let no = std::sync::atomic::AtomicBool::new(false);
        let r = recv_bytes(&dir, "a.txt", b"ola", 10, &no).await;
        assert_eq!(
            r.unwrap_err(),
            Failure::Short {
                got: 3,
                expected: 10
            }
        );
        let r = recv_bytes(&dir, "vazio.txt", b"", 1, &no).await;
        assert_eq!(
            r.unwrap_err(),
            Failure::Short {
                got: 0,
                expected: 1
            }
        );
        assert!(listing(&dir).is_empty());
        // Ficheiro vazio anunciado com 0 é válido.
        assert!(recv_bytes(&dir, "zero.txt", b"", 0, &no).await.is_ok());
        assert_eq!(listing(&dir), ["zero.txt"]);
    }

    #[tokio::test]
    async fn nome_repetido_vira_1_e_2_e_existente_nunca_muda() {
        let dir = tmp_dir("repetido");
        std::fs::write(dir.join("foto.jpg"), b"original").unwrap();
        let no = std::sync::atomic::AtomicBool::new(false);
        let a = recv_bytes(&dir, "foto.jpg", b"um", 2, &no).await.unwrap();
        let b = recv_bytes(&dir, "foto.jpg", b"dois", 4, &no).await.unwrap();
        assert_eq!(
            (a.name.as_str(), b.name.as_str()),
            ("foto (1).jpg", "foto (2).jpg")
        );
        assert_eq!(std::fs::read(dir.join("foto.jpg")).unwrap(), b"original");
        assert_eq!(std::fs::read(dir.join("foto (1).jpg")).unwrap(), b"um");
        assert_eq!(std::fs::read(dir.join("foto (2).jpg")).unwrap(), b"dois");
        // Sem extensão e ficheiro escondido.
        std::fs::write(dir.join("LEIA"), b"x").unwrap();
        let c = recv_bytes(&dir, "LEIA", b"y", 1, &no).await.unwrap();
        assert_eq!(c.name, "LEIA (1)");
        std::fs::write(dir.join(".bashrc"), b"x").unwrap();
        let d = recv_bytes(&dir, ".bashrc", b"y", 1, &no).await.unwrap();
        assert_eq!(d.name, ".bashrc (1)");
        assert_eq!(std::fs::read(dir.join(".bashrc")).unwrap(), b"x");
        assert!(!listing(&dir).iter().any(|n| n.ends_with(PART_SUFFIX)));
    }

    #[test]
    fn rename_sem_substituir_recusa_destino_existente() {
        let dir = tmp_dir("noreplace");
        let (from, to) = (dir.join("de"), dir.join("para"));
        std::fs::write(&from, b"novo").unwrap();
        std::fs::write(&to, b"antigo").unwrap();
        let e = rename_noreplace(&from, &to).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&to).unwrap(), b"antigo");
        assert_eq!(std::fs::read(&from).unwrap(), b"novo");
        // Uma pasta com o nome final também nunca é substituída.
        std::fs::create_dir(dir.join("pasta")).unwrap();
        assert_eq!(
            place_without_replacing(&from, &dir, "pasta", 1).unwrap(),
            "pasta (1)"
        );
    }

    #[test]
    fn depois_de_999_usa_a_hora_e_depois_a_hora_com_id() {
        let names: Vec<String> = candidate_names("a.txt", 42).collect();
        assert_eq!(names.len(), MAX_NAME_SUFFIX as usize + 3);
        assert_eq!(names[0], "a.txt");
        assert_eq!(names[999], "a (999).txt");
        assert!(names[1000].starts_with("a (2") && names[1000].ends_with(").txt"));
        assert!(names[1001].ends_with("-42).txt"));
    }

    #[test]
    fn nomes_sanitizados() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("a\nb.txt"), "a_b.txt");
        assert_eq!(sanitize_filename(".."), "arquivo_recebido");
        assert_eq!(sanitize_filename(".x.hyprlink-part"), ".x.hyprlink-part_");
        let longo = format!("{}.jpeg", "é".repeat(300));
        let s = sanitize_filename(&longo);
        assert!(s.len() <= MAX_NAME_BYTES && s.ends_with(".jpeg"));
    }

    /// Leitor que falha logo (como uma stream QUIC reiniciada).
    struct Broken;
    impl tokio::io::AsyncRead for Broken {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::ErrorKind::ConnectionReset.into()))
        }
    }

    fn exp(name: &str, id: u64) -> Expected<'_> {
        Expected { name, id, size: 3 }
    }

    #[tokio::test]
    async fn erro_de_leitura_e_expiracao_nao_deixam_ficheiros() {
        let dir = tmp_dir("leitura");
        let no = std::sync::atomic::AtomicBool::new(false);
        let idle = Duration::from_millis(50);
        let r = receive_to_dir(&mut Broken, &dir, exp("a", 1), &no, idle, 0o022, |_| {}).await;
        assert_eq!(r.unwrap_err(), Failure::Read);
        // O outro lado fica aberto sem mandar nada: expira.
        let (mut reader, _writer) = tokio::io::duplex(64);
        let r = receive_to_dir(&mut reader, &dir, exp("b", 2), &no, idle, 0o022, |_| {}).await;
        assert_eq!(r.unwrap_err(), Failure::Expired);
        assert!(listing(&dir).is_empty());
    }
}
