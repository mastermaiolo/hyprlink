//! O «mundo» que a deteção lê: raízes do sistema de ficheiros, variáveis,
//! ferramentas no PATH e um executor de comandos. Tudo injetável.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Resultado de um comando que arrancou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOut {
    pub ok: bool,
    pub stdout: String,
}

/// Executa programas (sem shell). `None` = não arrancou ou passou o tempo.
pub trait Runner: Send + Sync {
    fn run(&self, prog: &str, args: &[&str]) -> Option<RunOut>;
}

/// Executor real, com tempo limite e variáveis extra de sessão.
pub struct RealRunner {
    pub timeout: Duration,
    pub envs: Vec<(String, String)>,
}

impl Default for RealRunner {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(3),
            envs: Vec::new(),
        }
    }
}

impl Runner for RealRunner {
    fn run(&self, prog: &str, args: &[&str]) -> Option<RunOut> {
        let mut child = Command::new(prog)
            .args(args)
            .envs(self.envs.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        // A saída lê-se numa thread à parte: um `pw-dump` enche o pipe (64 KiB)
        // e bloquearia à espera de quem lê enquanto aqui só se espera.
        let mut pipe = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = pipe.read_to_string(&mut s);
            s
        });
        let t0 = Instant::now();
        let ok = loop {
            match child.try_wait().ok()? {
                Some(st) => break st.success(),
                None if t0.elapsed() > self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return None;
                }
                None => std::thread::sleep(Duration::from_millis(15)),
            }
        };
        let stdout = reader.join().ok()?;
        Some(RunOut { ok, stdout })
    }
}

/// Variáveis de ambiente que a deteção usa (nunca se imprimem todas).
pub const VARS: &[&str] = &[
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "XDG_CONFIG_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "HYPRLAND_INSTANCE_SIGNATURE",
    "HOME",
    "USER",
];

/// `(nome do socket, comando)` → resposta.
pub type SockQuery = Box<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

pub struct Env {
    pub sys: PathBuf,
    pub proc_dir: PathBuf,
    pub etc: PathBuf,
    pub dev: PathBuf,
    pub vars: BTreeMap<String, String>,
    pub have: Box<dyn Fn(&str) -> bool + Send + Sync>,
    pub runner: Box<dyn Runner>,
    /// Pergunta (só leitura) a um socket local do utilizador, por nome
    /// (`EasyEffectsServer`), com tempo-limite curto. `None` = sem socket ou
    /// sem resposta.
    pub sock_query: SockQuery,
}

impl Env {
    /// O sistema real.
    pub fn real() -> Self {
        let vars = VARS
            .iter()
            .filter_map(|k| Some((k.to_string(), std::env::var(k).ok()?)))
            .collect();
        Self {
            sys: "/sys".into(),
            proc_dir: "/proc".into(),
            etc: "/etc".into(),
            dev: "/dev".into(),
            vars,
            have: Box::new(on_path),
            runner: Box::new(RealRunner::default()),
            sock_query: Box::new(real_sock_query),
        }
    }

    pub fn var(&self, key: &str) -> Option<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    pub fn home(&self) -> Option<PathBuf> {
        self.var("HOME").map(PathBuf::from)
    }

    pub fn have(&self, tool: &str) -> bool {
        (self.have)(tool)
    }

    /// `$XDG_CONFIG_HOME` ou `~/.config`.
    pub fn config_dir(&self) -> Option<PathBuf> {
        self.var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(self.home()?.join(".config")))
    }

    /// `$XDG_STATE_HOME` ou `~/.local/state`.
    pub fn state_dir(&self) -> Option<PathBuf> {
        self.var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(self.home()?.join(".local/state")))
    }

    /// `~/.config/hyprlink/config.json`.
    pub fn hyprlink_config_path(&self) -> Option<PathBuf> {
        Some(self.config_dir()?.join("hyprlink").join("config.json"))
    }

    pub fn run(&self, prog: &str, args: &[&str]) -> Option<RunOut> {
        self.runner.run(prog, args)
    }
}

/// Há um executável com este nome no `PATH`?
pub fn on_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| is_exe(&d.join(exe))))
}

fn is_exe(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Teto da resposta de um socket local.
const SOCK_REPLY_MAX: usize = 64 * 1024;
/// Prazo total de uma pergunta (ligar + escrever + ler).
const SOCK_DEADLINE: Duration = Duration::from_millis(400);
/// Depois do primeiro bloco: sem mais nada durante isto, a resposta acabou
/// (há servidores que respondem e não fecham a ligação).
const SOCK_IDLE: Duration = Duration::from_millis(50);

/// Liga a `$XDG_RUNTIME_DIR/<nome>` e pergunta: ver [`sock_query_at`].
fn real_sock_query(name: &str, cmd: &str) -> Option<String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    sock_query_at(&Path::new(&dir).join(name), cmd)
}

/// Envia `cmd` + `\n` a um socket Unix e lê a resposta até EOF, até
/// [`SOCK_REPLY_MAX`] bytes ou até o prazo de 400 ms acabar (depois do
/// primeiro bloco, 50 ms sem mais dados também a dão por acabada). Nunca
/// bloqueia nem falha: sem socket ou sem resposta, `None`.
fn sock_query_at(path: &Path, cmd: &str) -> Option<String> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let deadline = Instant::now() + SOCK_DEADLINE;
    let mut s = UnixStream::connect(path).ok()?;
    s.set_write_timeout(Some(SOCK_DEADLINE)).ok()?;
    s.write_all(format!("{cmd}\n").as_bytes()).ok()?;
    let mut reply = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let wait = if reply.is_empty() {
            left
        } else {
            left.min(SOCK_IDLE)
        };
        if wait.is_zero() {
            break;
        }
        s.set_read_timeout(Some(wait)).ok()?;
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let room = SOCK_REPLY_MAX - reply.len();
                reply.extend_from_slice(&buf[..n.min(room)]);
                if reply.len() >= SOCK_REPLY_MAX {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // Prazo (WouldBlock/TimedOut) ou erro: fica o que chegou.
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&reply).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;

    fn sock(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("hyprlink-sq-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// Servidor falso: lê o pedido e responde com `chunks`, um de cada vez.
    fn serve(path: &Path, chunks: Vec<Vec<u8>>, close: bool) -> std::thread::JoinHandle<()> {
        let l = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut req = [0u8; 64];
            let _ = c.read(&mut req);
            for ch in chunks {
                // O cliente pode fechar antes (teto): o erro aqui não conta.
                let _ = c.write_all(&ch);
                std::thread::sleep(Duration::from_millis(10));
            }
            if !close {
                std::thread::sleep(Duration::from_millis(600));
            }
        })
    }

    #[test]
    fn resposta_maior_que_256_bytes_em_varias_escritas() {
        let p = sock("grande");
        let parts: Vec<Vec<u8>> = (0..4).map(|i| vec![b'a' + i; 200]).collect();
        let expected: String = parts.iter().map(|c| String::from_utf8_lossy(c)).collect();
        let t = serve(&p, parts, true);
        assert_eq!(sock_query_at(&p, "get").as_deref(), Some(expected.as_str()));
        t.join().unwrap();
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn servidor_que_nao_fecha_nao_prende_os_400_ms() {
        let p = sock("aberto");
        let t = serve(&p, vec![b"1\n".to_vec()], false);
        let t0 = Instant::now();
        assert_eq!(sock_query_at(&p, "get").as_deref(), Some("1"));
        assert!(t0.elapsed() < SOCK_DEADLINE, "{:?}", t0.elapsed());
        t.join().unwrap();
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn sem_resposta_desiste_no_prazo() {
        let p = sock("calado");
        let t = serve(&p, vec![], false);
        let t0 = Instant::now();
        assert_eq!(sock_query_at(&p, "get"), None);
        let e = t0.elapsed();
        assert!(e >= SOCK_DEADLINE && e < SOCK_DEADLINE * 2, "{e:?}");
        t.join().unwrap();
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn resposta_tem_teto() {
        let p = sock("teto");
        let t = serve(&p, vec![vec![b'x'; SOCK_REPLY_MAX + 5000]], true);
        assert_eq!(
            sock_query_at(&p, "get").map(|r| r.len()),
            Some(SOCK_REPLY_MAX)
        );
        t.join().unwrap();
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn sem_socket_e_none() {
        assert_eq!(sock_query_at(&sock("nada"), "get"), None);
    }
}
