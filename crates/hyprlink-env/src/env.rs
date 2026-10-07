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

/// Liga a `$XDG_RUNTIME_DIR/<nome>`, envia `cmd` + `\n` e lê a resposta, tudo
/// com tempo-limite de 400 ms. Nunca bloqueia nem falha: sem socket, `None`.
fn real_sock_query(name: &str, cmd: &str) -> Option<String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let path = std::path::Path::new(&dir).join(name);
    let mut s = UnixStream::connect(path).ok()?;
    let t = Some(Duration::from_millis(400));
    s.set_read_timeout(t).ok()?;
    s.set_write_timeout(t).ok()?;
    s.write_all(format!("{cmd}\n").as_bytes()).ok()?;
    let mut buf = [0u8; 256];
    let n = s.read(&mut buf).ok()?;
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n]).trim().to_string())
}
