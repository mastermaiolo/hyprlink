//! Qual é a instância do Hyprland desta sessão — **uma** descoberta para o
//! daemon (`hyprctl`, socket2 dos eventos), o `hyprlinkctl` e a GUI.
//!
//! O daemon pode arrancar sem `HYPRLAND_INSTANCE_SIGNATURE` (pelo
//! `systemd --user`, ou num `exec-once` cedo) ou com uma assinatura velha (o
//! Hyprland reiniciou e o daemon não). Por isso:
//!
//! 1. a assinatura da variável, se essa instância estiver viva;
//! 2. senão, a instância viva mais recente em `$XDG_RUNTIME_DIR/hypr/`;
//! 3. senão, a mais recente em `/tmp/hypr/` (Hyprland antigo).
//!
//! «Viva» = tem `.socket.sock` e, se houver `hyprland.lock` (PID na 1.ª
//! linha, `WAYLAND_DISPLAY` na 2.ª), esse processo existe em `/proc`. Não se
//! liga a nenhum socket do compositor para decidir.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Uma instância do Hyprland encontrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub signature: String,
    /// `…/hypr/<assinatura>`.
    pub dir: PathBuf,
    /// O `WAYLAND_DISPLAY` desta instância (2.ª linha do `hyprland.lock`).
    pub wayland_display: Option<String>,
}

impl Instance {
    /// Socket de comandos (`hyprctl`).
    pub fn socket(&self) -> PathBuf {
        self.dir.join(".socket.sock")
    }

    /// Socket de eventos.
    pub fn socket2(&self) -> PathBuf {
        self.dir.join(".socket2.sock")
    }
}

/// Onde procurar. [`Roots::real`] é o sistema; os testes usam diretórios
/// temporários.
#[derive(Debug, Clone)]
pub struct Roots {
    /// `$XDG_RUNTIME_DIR` (as instâncias ficam em `<isto>/hypr/`).
    pub runtime_dir: Option<PathBuf>,
    /// `/tmp/hypr`.
    pub legacy: PathBuf,
    /// `/proc`.
    pub proc_dir: PathBuf,
}

impl Roots {
    pub fn real() -> Self {
        Self {
            runtime_dir: std::env::var_os("XDG_RUNTIME_DIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            legacy: PathBuf::from("/tmp/hypr"),
            proc_dir: PathBuf::from("/proc"),
        }
    }

    fn hypr_dirs(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.runtime_dir.iter().map(|d| d.join("hypr")).collect();
        v.push(self.legacy.clone());
        v
    }
}

/// Uma assinatura só serve de nome de diretório se não tiver separadores.
fn valid_signature(sig: &str) -> bool {
    !sig.is_empty() && sig != "." && sig != ".." && !sig.contains('/') && !sig.contains('\0')
}

/// Lê o `hyprland.lock`: `(pid, wayland_display)`.
fn read_lock(dir: &Path) -> Option<(Option<u32>, Option<String>)> {
    let s = std::fs::read_to_string(dir.join("hyprland.lock")).ok()?;
    let mut lines = s.lines().map(str::trim);
    let pid = lines.next().and_then(|l| l.parse().ok());
    let display = lines
        .next()
        .filter(|l| !l.is_empty() && !l.contains('/'))
        .map(str::to_string);
    Some((pid, display))
}

/// A instância em `dir`, se estiver viva.
fn alive(dir: &Path, proc_dir: &Path) -> Option<Instance> {
    let signature = dir.file_name()?.to_str()?.to_string();
    if !valid_signature(&signature) || !dir.join(".socket.sock").exists() {
        return None;
    }
    let lock = read_lock(dir);
    if let Some((Some(pid), _)) = &lock
        && !proc_dir.join(pid.to_string()).exists()
    {
        return None;
    }
    Some(Instance {
        signature,
        dir: dir.to_path_buf(),
        wayland_display: lock.and_then(|(_, d)| d),
    })
}

/// A instância a usar (ver o topo do módulo). `env_sig` é o valor de
/// `HYPRLAND_INSTANCE_SIGNATURE`, se houver.
pub fn find(roots: &Roots, env_sig: Option<&str>) -> Option<Instance> {
    let dirs = roots.hypr_dirs();
    if let Some(sig) = env_sig.map(str::trim).filter(|s| valid_signature(s))
        && let Some(i) = dirs
            .iter()
            .find_map(|d| alive(&d.join(sig), &roots.proc_dir))
    {
        return Some(i);
    }
    // A mais recente (pela data do diretório; empate pelo nome, que acaba
    // em `_<timestamp>_<n>`), primeiro no runtime dir.
    dirs.iter().find_map(|hypr| {
        let mut found: Vec<(SystemTime, Instance)> = std::fs::read_dir(hypr)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let i = alive(&e.path(), &roots.proc_dir)?;
                let t = e.metadata().ok()?.modified().ok()?;
                Some((t, i))
            })
            .collect();
        found.sort_by(|a, b| (a.0, &a.1.signature).cmp(&(b.0, &b.1.signature)));
        found.pop().map(|(_, i)| i)
    })
}

/// De onde veio a instância usada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A de `HYPRLAND_INSTANCE_SIGNATURE`, viva.
    Variable,
    /// Encontrada nos diretórios (sem a variável, ou a variável aponta
    /// para uma instância que já não existe).
    Discovered,
}

/// [`find`] e de onde veio.
pub fn find_with_source(roots: &Roots, env_sig: Option<&str>) -> Option<(Instance, Source)> {
    let i = find(roots, env_sig)?;
    let src = if env_sig.map(str::trim) == Some(i.signature.as_str()) {
        Source::Variable
    } else {
        Source::Discovered
    };
    Some((i, src))
}

/// [`find`] no sistema real.
pub fn current() -> Option<Instance> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    find(&Roots::real(), sig.as_deref())
}

/// Há sessão Hyprland? A variável definida **ou** uma instância viva (o
/// processo pode ter arrancado fora do ambiente do Hyprland).
pub fn is_hyprland_session() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|v| !v.is_empty())
        || current().is_some()
}

/// Variáveis de sessão a passar aos comandos lançados (`hyprctl`, ações):
/// as que faltam **ou** apontam para algo que já não existe, com o valor
/// descoberto. As que estão bem não aparecem.
///
/// - `HYPRLAND_INSTANCE_SIGNATURE`: a de [`find`], se for outra.
/// - `WAYLAND_DISPLAY`: se faltar ou o socket não existir, o da instância
///   (`hyprland.lock`); sem isso, o primeiro `wayland-*` do runtime dir.
pub fn session_env(roots: &Roots, get: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let env_sig = get("HYPRLAND_INSTANCE_SIGNATURE").filter(|s| !s.is_empty());
    let inst = find(roots, env_sig.as_deref());
    if let Some(i) = &inst
        && env_sig.as_deref() != Some(i.signature.as_str())
    {
        out.push((
            "HYPRLAND_INSTANCE_SIGNATURE".to_string(),
            i.signature.clone(),
        ));
    }

    if let Some(rt) = &roots.runtime_dir {
        let current = get("WAYLAND_DISPLAY").filter(|s| !s.is_empty());
        let ok = current.as_deref().is_some_and(|d| rt.join(d).exists());
        if !ok {
            let from_lock = inst
                .as_ref()
                .and_then(|i| i.wayland_display.clone())
                .filter(|d| rt.join(d).exists());
            let pick = from_lock.or_else(|| {
                let mut socks: Vec<String> = std::fs::read_dir(rt)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("wayland-") && !n.ends_with(".lock"))
                    .collect();
                socks.sort();
                socks.into_iter().next()
            });
            if let Some(d) = pick
                && current.as_deref() != Some(d.as_str())
            {
                out.push(("WAYLAND_DISPLAY".to_string(), d));
            }
        }
    }
    out
}

/// [`session_env`] com o ambiente real do processo.
pub fn real_session_env() -> Vec<(String, String)> {
    session_env(&Roots::real(), &|k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Um `$XDG_RUNTIME_DIR`, um `/tmp/hypr` e um `/proc` falsos.
    struct Fake {
        root: PathBuf,
    }

    impl Fake {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("hyprlink-inst-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            for d in ["run/hypr", "tmp-hypr", "proc"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Self { root }
        }

        fn roots(&self) -> Roots {
            Roots {
                runtime_dir: Some(self.root.join("run")),
                legacy: self.root.join("tmp-hypr"),
                proc_dir: self.root.join("proc"),
            }
        }

        /// Uma instância com socket; `pid` vivo se `alive`.
        fn instance(&self, base: &str, sig: &str, pid: u32, alive: bool, display: &str) -> PathBuf {
            let dir = self.root.join(base).join(sig);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(".socket.sock"), b"").unwrap();
            std::fs::write(dir.join(".socket2.sock"), b"").unwrap();
            std::fs::write(dir.join("hyprland.lock"), format!("{pid}\n{display}\n")).unwrap();
            if alive {
                std::fs::create_dir_all(self.root.join("proc").join(pid.to_string())).unwrap();
            }
            dir
        }

        fn wayland(&self, name: &str) {
            std::fs::write(self.root.join("run").join(name), b"").unwrap();
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn sig(i: Option<Instance>) -> Option<String> {
        i.map(|i| i.signature)
    }

    #[test]
    fn de_onde_veio_a_instancia() {
        let f = Fake::new("fonte");
        f.instance("run/hypr", "viva_1_1", 100, true, "wayland-1");
        let src = |sig| find_with_source(&f.roots(), sig).map(|(_, s)| s);
        assert_eq!(src(Some("viva_1_1")), Some(Source::Variable));
        assert_eq!(src(None), Some(Source::Discovered));
        assert_eq!(src(Some("morta_0_0")), Some(Source::Discovered));
    }

    #[test]
    fn sem_instancias_nada() {
        let f = Fake::new("zero");
        assert_eq!(find(&f.roots(), None), None);
        assert_eq!(find(&f.roots(), Some("abc_1_2")), None);
        assert!(session_env(&f.roots(), &|_| None).is_empty());
    }

    #[test]
    fn uma_instancia_viva() {
        let f = Fake::new("uma");
        let dir = f.instance("run/hypr", "abc_1_2", 100, true, "wayland-1");
        let i = find(&f.roots(), None).unwrap();
        assert_eq!(i.signature, "abc_1_2");
        assert_eq!(i.socket2(), dir.join(".socket2.sock"));
        assert_eq!(i.wayland_display.as_deref(), Some("wayland-1"));
    }

    #[test]
    fn duas_instancias_escolhe_a_mais_recente() {
        let f = Fake::new("duas");
        f.instance("run/hypr", "velha_1_1", 100, true, "wayland-1");
        std::thread::sleep(Duration::from_millis(20));
        f.instance("run/hypr", "nova_2_1", 200, true, "wayland-2");
        assert_eq!(sig(find(&f.roots(), None)).as_deref(), Some("nova_2_1"));
        // A variável manda, se essa estiver viva.
        assert_eq!(
            sig(find(&f.roots(), Some("velha_1_1"))).as_deref(),
            Some("velha_1_1")
        );
    }

    #[test]
    fn variavel_a_apontar_para_instancia_morta_escolhe_a_viva() {
        let f = Fake::new("morta");
        // Morta: o processo do lock já não existe.
        f.instance("run/hypr", "morta_1_1", 100, false, "wayland-1");
        std::thread::sleep(Duration::from_millis(20));
        f.instance("run/hypr", "viva_2_1", 200, true, "wayland-1");
        assert_eq!(
            sig(find(&f.roots(), Some("morta_1_1"))).as_deref(),
            Some("viva_2_1")
        );
        // E uma morta mais recente do que a viva não ganha.
        std::thread::sleep(Duration::from_millis(20));
        f.instance("run/hypr", "zombie_3_1", 300, false, "wayland-1");
        assert_eq!(sig(find(&f.roots(), None)).as_deref(), Some("viva_2_1"));
        // Variável com uma assinatura que nem existe.
        assert_eq!(
            sig(find(&f.roots(), Some("naoexiste"))).as_deref(),
            Some("viva_2_1")
        );
    }

    #[test]
    fn sem_socket_ou_sem_lock() {
        let f = Fake::new("socket");
        // Diretório sem socket: não conta.
        std::fs::create_dir_all(f.root.join("run/hypr/vazia_0_0")).unwrap();
        assert_eq!(find(&f.roots(), None), None);
        // Socket sem lock (Hyprland antigo): conta.
        let d = f.root.join("run/hypr/antiga_1_1");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(".socket.sock"), b"").unwrap();
        let i = find(&f.roots(), None).unwrap();
        assert_eq!(
            (i.signature.as_str(), i.wayland_display),
            ("antiga_1_1", None)
        );
    }

    #[test]
    fn fallback_para_tmp_hypr() {
        let f = Fake::new("tmp");
        f.instance("tmp-hypr", "legado_1_1", 100, true, "wayland-0");
        assert_eq!(sig(find(&f.roots(), None)).as_deref(), Some("legado_1_1"));
        assert_eq!(
            sig(find(&f.roots(), Some("legado_1_1"))).as_deref(),
            Some("legado_1_1")
        );
        // No runtime dir, ganha a do runtime dir.
        f.instance("run/hypr", "atual_2_1", 200, true, "wayland-1");
        assert_eq!(sig(find(&f.roots(), None)).as_deref(), Some("atual_2_1"));
    }

    #[test]
    fn assinatura_com_barras_e_ignorada() {
        let f = Fake::new("barras");
        f.instance("run/hypr", "boa_1_1", 100, true, "wayland-1");
        assert_eq!(
            sig(find(&f.roots(), Some("../../etc"))).as_deref(),
            Some("boa_1_1")
        );
    }

    #[test]
    fn ambiente_de_sessao_so_o_que_falta_ou_esta_morto() {
        let f = Fake::new("env");
        f.instance("run/hypr", "abc_1_2", 100, true, "wayland-1");
        f.wayland("wayland-1");
        f.wayland("wayland-1.lock");
        f.wayland("wayland-0");

        let nada = |_: &str| None;
        let v = session_env(&f.roots(), &nada);
        assert!(
            v.contains(&("WAYLAND_DISPLAY".into(), "wayland-1".into())),
            "{v:?}"
        );
        assert!(
            v.contains(&("HYPRLAND_INSTANCE_SIGNATURE".into(), "abc_1_2".into())),
            "{v:?}"
        );

        // Já definidas e vivas: não se mexe.
        let certas = |k: &str| match k {
            "HYPRLAND_INSTANCE_SIGNATURE" => Some("abc_1_2".to_string()),
            "WAYLAND_DISPLAY" => Some("wayland-0".to_string()),
            _ => None,
        };
        assert!(session_env(&f.roots(), &certas).is_empty());

        // Definidas mas mortas (o Hyprland reiniciou): trocam-se.
        let velhas = |k: &str| match k {
            "HYPRLAND_INSTANCE_SIGNATURE" => Some("velha_0_0".to_string()),
            "WAYLAND_DISPLAY" => Some("wayland-9".to_string()),
            _ => None,
        };
        let v = session_env(&f.roots(), &velhas);
        assert_eq!(
            v,
            vec![
                ("HYPRLAND_INSTANCE_SIGNATURE".into(), "abc_1_2".into()),
                ("WAYLAND_DISPLAY".into(), "wayland-1".into()),
            ]
        );
    }

    #[test]
    fn sem_lock_o_display_e_o_primeiro_wayland() {
        let f = Fake::new("display");
        f.wayland("wayland-2");
        f.wayland("wayland-1");
        let v = session_env(&f.roots(), &|_| None);
        assert_eq!(v, vec![("WAYLAND_DISPLAY".into(), "wayland-1".into())]);
    }
}
