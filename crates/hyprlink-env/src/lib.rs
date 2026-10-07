//! Deteção do ambiente do HyprLink: hardware (CPU, GPU, temperatura,
//! baterias), sessão (Hyprland, shell), áudio e ferramentas.
//!
//! **Uma única fonte de verdade:** o daemon e o `hyprlinkctl doctor` usam as
//! mesmas funções, para o relatório mostrar o que o daemon vai mesmo fazer.
//! Tudo lê de uma raiz parametrizável (`Env`), por isso os testes usam
//! árvores `/sys`, `/proc` e `/etc` falsas (ver `crates/hyprlinkd/tests`).

pub mod battery;
pub mod camera;
pub mod env;
pub mod gpu;
pub mod hyprland;
pub mod overrides;
pub mod pipewire;
pub mod plan;
pub mod report;
pub mod sensors;
pub mod shell;

pub use env::{Env, RealRunner, RunOut, Runner};
pub use overrides::{Overrides, Resolved};

/// Lê um ficheiro pequeno do sysfs/procfs e apara-o.
pub(crate) fn read_trim(path: &std::path::Path) -> Option<String> {
    Some(std::fs::read_to_string(path).ok()?.trim().to_string())
}

/// Como `read_trim`, para fora do crate.
pub fn read_trim_pub(path: &std::path::Path) -> Option<String> {
    read_trim(path)
}
