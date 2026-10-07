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
