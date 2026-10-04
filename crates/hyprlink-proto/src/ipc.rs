//! O socket local entre o `hyprlinkd` e os clientes (GUI, `hyprlinkctl`).
//!
//! - Caminho: `$XDG_RUNTIME_DIR/hyprlink.sock`, permissões 0600.
//! - Frame: comprimento `u32` big-endian + corpo CBOR. Máximo [`MAX_FRAME`].
//! - Ao ligar, o daemon envia [`ServerMsg::Hello`], depois o estado completo
//!   (um `Event` por cada tipo de estado conhecido), [`ServerMsg::Ready`] e a
//!   seguir os `Event`s à medida que acontecem. O cliente só envia [`ClientMsg::Command`].
//! - Eventos de estado de alta frequência (níveis, sensores, telemetria) são
//!   coalescidos por cliente: um cliente lento recebe o valor mais recente,
//!   nunca uma fila atrasada.
//!
//! Sem dependências de runtime: os dois lados usam [`encode`] e [`decode`] e
//! fazem o I/O com o que tiverem (tokio no daemon, iced/std nos clientes).

use crate::link::{Command, Event};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// Sobe quando o envelope ou o contrato mudam de forma incompatível.
pub const PROTO_VERSION: u32 = 1;

/// Igual ao limite do protocolo QUIC (`hyprlinkd::protocol::MAX_FRAME_SIZE`).
pub const MAX_FRAME: u32 = 16 * 1024 * 1024;

pub const SOCKET_NAME: &str = "hyprlink.sock";

/// `$XDG_RUNTIME_DIR/hyprlink.sock`. `None` sem `XDG_RUNTIME_DIR`.
pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join(SOCKET_NAME))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerMsg {
    Hello {
        proto_version: u32,
        /// `CARGO_PKG_VERSION` do daemon.
        daemon_version: String,
    },
    Event(Event),
    /// Fim do estado completo inicial: daqui em diante só há novidades.
    Ready,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientMsg {
    Command(Command),
}

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    TooLarge(u32),
    Decode(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "io: {e}"),
            FrameError::TooLarge(n) => write!(f, "frame de {n} B acima do limite de {MAX_FRAME} B"),
            FrameError::Decode(e) => write!(f, "cbor: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        FrameError::Io(e)
    }
}

/// Frame completo (prefixo + corpo), pronto a escrever.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut buf = vec![0u8; 4];
    ciborium::into_writer(msg, &mut buf).expect("escrever CBOR para Vec não falha");
    let len = (buf.len() - 4) as u32;
    buf[..4].copy_from_slice(&len.to_be_bytes());
    buf
}

/// Comprimento do corpo a partir do prefixo de 4 bytes.
pub fn body_len(prefix: [u8; 4]) -> Result<usize, FrameError> {
    let n = u32::from_be_bytes(prefix);
    if n > MAX_FRAME {
        return Err(FrameError::TooLarge(n));
    }
    Ok(n as usize)
}

/// Descodifica só o corpo (sem o prefixo).
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, FrameError> {
    ciborium::from_reader(body).map_err(|e| FrameError::Decode(e.to_string()))
}

/// Leitura bloqueante de um frame (clientes `std`).
pub fn read<T: DeserializeOwned>(r: &mut impl Read) -> Result<T, FrameError> {
    let mut prefix = [0u8; 4];
    r.read_exact(&mut prefix)?;
    let mut body = vec![0u8; body_len(prefix)?];
    r.read_exact(&mut body)?;
    decode(&body)
}

/// Escrita bloqueante de um frame (clientes `std`).
pub fn write<T: Serialize>(w: &mut impl Write, msg: &T) -> Result<(), FrameError> {
    w.write_all(&encode(msg))?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Command, Event, Notice};

    #[test]
    fn frame_round_trip() {
        let msgs = vec![
            ServerMsg::Hello {
                proto_version: PROTO_VERSION,
                daemon_version: "0.1.0".into(),
            },
            ServerMsg::Event(Event::Rssi(-53)),
            ServerMsg::Event(Event::Notice(Notice::MirrorStarted)),
        ];
        let mut wire = Vec::new();
        for m in &msgs {
            write(&mut wire, m).unwrap();
        }
        let mut r = wire.as_slice();
        for _ in &msgs {
            let back: ServerMsg = read(&mut r).unwrap();
            assert!(matches!(
                back,
                ServerMsg::Hello { .. } | ServerMsg::Event(_)
            ));
        }
        assert!(r.is_empty());

        let c = encode(&ClientMsg::Command(Command::SetMic(true)));
        let back: ClientMsg = decode(&c[4..]).unwrap();
        assert!(matches!(back, ClientMsg::Command(Command::SetMic(true))));
    }

    #[test]
    fn rejects_oversized_prefix() {
        let prefix = (MAX_FRAME + 1).to_be_bytes();
        assert!(matches!(body_len(prefix), Err(FrameError::TooLarge(_))));
        let mut r: &[u8] = &prefix;
        assert!(matches!(
            read::<ServerMsg>(&mut r),
            Err(FrameError::TooLarge(_))
        ));
    }

    #[test]
    fn truncated_frame_is_io_error() {
        let full = encode(&ServerMsg::Event(Event::Rssi(-1)));
        let mut r = &full[..full.len() - 1];
        assert!(matches!(read::<ServerMsg>(&mut r), Err(FrameError::Io(_))));
    }
}
