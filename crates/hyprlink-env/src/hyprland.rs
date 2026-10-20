//! Hyprland: sintaxe do `hyprctl dispatch` (clássica ou Lua).
//!
//! Com configuração Lua, `hyprctl dispatch` é «a shorthand for
//! `eval 'hl.dispatch(...)'`» e recebe uma expressão Lua; a sintaxe clássica
//! deixa de ser aceite (`TRAY/hyprland-wiki/.../using-hyprctl.md`, issue
//! end-4/dots-hyprland #3320). O modo deteta-se **uma vez** com um dispatcher
//! sem efeito, `hl.dsp.no_op()` («Does nothing», `dispatchers.md`): se o
//! `hyprctl` o aceita, é Lua. Escolha de desenho; a confirmar num Hyprland
//! hyprlang real, onde se espera que falhe.

use std::sync::Mutex;

use serde::Serialize;

use crate::Runner;
use crate::overrides::DispatchChoice;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchMode {
    Classic,
    Lua,
}

/// A expressão do teste.
pub const PROBE_EXPR: &str = "hl.dsp.no_op()";

/// Corre o teste. `Some(modo)` se o `hyprctl` respondeu; `None` se não
/// arrancou (sem `hyprctl`, sem Hyprland): não se guarda.
pub fn probe(hyprctl: &dyn Runner) -> Option<DispatchMode> {
    let out = hyprctl.run("hyprctl", &["dispatch", PROBE_EXPR])?;
    Some(if out.ok {
        DispatchMode::Lua
    } else {
        DispatchMode::Classic
    })
}

/// Primeira versão com configuração Lua (e `hyprctl dispatch` em Lua).
pub const LUA_SINCE: (u32, u32) = (0, 55);

/// `Hyprland 0.56.2` (ou `0.56.2`) → `(0, 56)`.
pub fn parse_version(v: &str) -> Option<(u32, u32)> {
    let num = v
        .split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut it = num.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    Some((major, minor))
}

/// O modo **sem** dispatch (`hyprlinkctl doctor --no-dispatch`): pela versão
/// e pelo ficheiro de configuração em `~/.config/hypr`.
///
/// - versão < 0.55 → clássico (ainda não havia Lua);
/// - `hyprland.lua` → Lua; só `hyprland.conf` → clássico (um Hyprland
///   recente com config hyprlang continua clássico);
/// - versão ≥ 0.55 sem nenhum dos dois → Lua; sem versão nem ficheiro →
///   clássico.
pub fn infer_mode(version: Option<&str>, config_file: Option<&str>) -> DispatchMode {
    infer(version, config_file).mode
}

/// O modo inferido e se é seguro usá-lo sem confirmar por dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inferred {
    pub mode: DispatchMode,
    /// `true`: versão antiga, ou um `hyprland.lua`/`hyprland.conf` encontrado.
    /// `false` (sem ficheiro de configuração): é um palpite, a confirmar no
    /// primeiro dispatch a sério.
    pub certain: bool,
}

/// Como [`infer_mode`], com o grau de certeza.
pub fn infer(version: Option<&str>, config_file: Option<&str>) -> Inferred {
    let v = version.and_then(parse_version);
    let (mode, certain) = if v.is_some_and(|v| v < LUA_SINCE) {
        (DispatchMode::Classic, true)
    } else {
        match config_file {
            Some("hyprland.lua") => (DispatchMode::Lua, true),
            Some(_) => (DispatchMode::Classic, true),
            None if v.is_some() => (DispatchMode::Lua, false),
            None => (DispatchMode::Classic, false),
        }
    };
    Inferred { mode, certain }
}

/// Guarda o modo detetado (uma vez); `reset` obriga a testar de novo.
#[derive(Default)]
pub struct ModeCache(Mutex<Option<DispatchMode>>);

impl ModeCache {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn get(&self) -> Option<DispatchMode> {
        *self.0.lock().unwrap()
    }

    pub fn set(&self, m: DispatchMode) {
        *self.0.lock().unwrap() = Some(m);
    }

    pub fn reset(&self) {
        *self.0.lock().unwrap() = None;
    }

    /// O modo a usar: o forçado, o guardado, ou o detetado agora (e guardado).
    /// Sem resposta do `hyprctl`, assume o clássico **sem guardar**.
    pub fn resolve(&self, choice: DispatchChoice, hyprctl: &dyn Runner) -> DispatchMode {
        match choice {
            DispatchChoice::Classic => DispatchMode::Classic,
            DispatchChoice::Lua => DispatchMode::Lua,
            DispatchChoice::Auto => {
                if let Some(m) = self.get() {
                    return m;
                }
                match probe(hyprctl) {
                    Some(m) => {
                        self.set(m);
                        m
                    }
                    None => DispatchMode::Classic,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunOut;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        lua: bool,
        calls: AtomicUsize,
        missing: bool,
    }

    impl Runner for Fake {
        fn run(&self, _p: &str, args: &[&str]) -> Option<RunOut> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.missing {
                return None;
            }
            assert_eq!(args, ["dispatch", PROBE_EXPR]);
            Some(RunOut {
                ok: self.lua,
                stdout: String::new(),
            })
        }
    }

    #[test]
    fn inferencia_diz_quando_e_palpite() {
        let i = infer(Some("0.56.2"), Some("hyprland.lua"));
        assert_eq!((i.mode, i.certain), (DispatchMode::Lua, true));
        let i = infer(Some("0.56.2"), Some("hyprland.conf"));
        assert_eq!((i.mode, i.certain), (DispatchMode::Classic, true));
        let i = infer(Some("0.54.0"), None);
        assert_eq!((i.mode, i.certain), (DispatchMode::Classic, true));
        let i = infer(Some("0.56.2"), None);
        assert_eq!((i.mode, i.certain), (DispatchMode::Lua, false));
        let i = infer(None, None);
        assert_eq!((i.mode, i.certain), (DispatchMode::Classic, false));
    }

    #[test]
    fn modo_inferido_sem_dispatch() {
        use DispatchMode::{Classic, Lua};
        assert_eq!(parse_version("Hyprland 0.56.2"), Some((0, 56)));
        assert_eq!(parse_version("0.54.0-dev"), Some((0, 54)));
        assert_eq!(parse_version("Hyprland"), None);
        let m = |v, f| infer_mode(v, f);
        assert_eq!(m(Some("Hyprland 0.54.3"), Some("hyprland.lua")), Classic);
        assert_eq!(m(Some("Hyprland 0.55.0"), Some("hyprland.lua")), Lua);
        assert_eq!(m(Some("Hyprland 0.56.2"), Some("hyprland.conf")), Classic);
        assert_eq!(m(Some("Hyprland 0.56.2"), None), Lua);
        assert_eq!(m(None, Some("hyprland.lua")), Lua);
        assert_eq!(m(None, None), Classic);
    }

    fn fake(lua: bool, missing: bool) -> Fake {
        Fake {
            lua,
            calls: AtomicUsize::new(0),
            missing,
        }
    }

    #[test]
    fn deteta_uma_vez_e_guarda() {
        let c = ModeCache::new();
        let f = fake(true, false);
        assert_eq!(c.resolve(DispatchChoice::Auto, &f), DispatchMode::Lua);
        assert_eq!(c.resolve(DispatchChoice::Auto, &f), DispatchMode::Lua);
        assert_eq!(f.calls.load(Ordering::Relaxed), 1);
        c.reset();
        let g = fake(false, false);
        assert_eq!(c.resolve(DispatchChoice::Auto, &g), DispatchMode::Classic);
        assert_eq!(g.calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn opcao_forca_sem_testar() {
        let c = ModeCache::new();
        let f = fake(true, false);
        assert_eq!(
            c.resolve(DispatchChoice::Classic, &f),
            DispatchMode::Classic
        );
        assert_eq!(c.resolve(DispatchChoice::Lua, &f), DispatchMode::Lua);
        assert_eq!(f.calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn sem_hyprctl_assume_classico_sem_guardar() {
        let c = ModeCache::new();
        let f = fake(true, true);
        assert_eq!(c.resolve(DispatchChoice::Auto, &f), DispatchMode::Classic);
        assert_eq!(c.get(), None);
    }
}
