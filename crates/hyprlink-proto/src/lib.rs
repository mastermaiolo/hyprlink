//! `hyprlink-proto` — o contrato entre o `hyprlinkd`, a GUI e o `hyprlinkctl`.
//!
//! - [`link`]: `Command` / `Event` / `Notice` e os tipos de dados (CBOR no
//!   socket local, serde em tudo). Regra: o daemon envia **dados**, nunca texto.
//! - [`link::packets`]: os nomes de pacote `módulo.ação`, num só sítio.
//! - [`client`]: cliente do socket local (threads `std`), para a GUI e o
//!   `hyprlinkctl`.
//! - [`ipc`]: o envelope do socket local `hyprlink.sock` (u32 BE + CBOR).
//! - [`snapshot`]: o JSON v1 estável para plugins de shell.
//! - [`host`]: "Este PC" lido de `/proc` e `/sys`.
//! - `fmt` (feature `fmt`): os textos pt-PT — só para quem desenha.
//! - `link::mock` (feature `mock`): o daemon simulado.
//!
//! Desde 2026-10-06 este crate e o `hyprlink-gui` são a única fonte do
//! contrato e da GUI (o antigo repositório de design está arquivado).

pub mod client;
#[cfg(feature = "fmt")]
pub mod fmt;
pub mod host;
pub mod ipc;
pub mod link;
pub mod snapshot;
