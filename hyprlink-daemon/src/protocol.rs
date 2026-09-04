//! Enquadramento e codificação CBOR do protocolo HyprLink. Ver PROTOCOL.md
//! para a especificação completa extraída do app Android real.

use ciborium::Value;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Limite generoso de tamanho de frame (o app usa 5-10 MiB conforme o contexto).
pub const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub id: u64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub has_payload: bool,
}

impl Packet {
    pub fn new(id: u64, kind: impl Into<String>, body: Option<Value>, has_payload: bool) -> Self {
        Self {
            id,
            kind: kind.into(),
            body,
            has_payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        ciborium::into_writer(self, &mut buf).expect("serialização de Packet é infalível");
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ciborium::de::Error<std::io::Error>> {
        ciborium::from_reader(bytes)
    }
}

/// Lê um frame de um stream bidirecional: 4 bytes BE (tamanho) + payload CBOR cru.
pub async fn read_frame<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len == 0 || len > MAX_FRAME_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("tamanho de frame inválido: {len}"),
        ));
    }
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}

/// Escreve um frame num stream bidirecional (4 bytes BE + payload cru).
pub async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    payload: &[u8],
) -> std::io::Result<()> {
    writer.write_all(&(payload.len() as u32).to_be_bytes()).await?;
    writer.write_all(payload).await?;
    Ok(())
}

/// Helpers pra ler campos de um `body` (mapa CBOR dinâmico, possivelmente indefinite-length).
pub fn body_get<'a>(body: &'a Value, key: &str) -> Option<&'a Value> {
    body.as_map()?
        .iter()
        .find(|(k, _)| k.as_text() == Some(key))
        .map(|(_, v)| v)
}

pub fn body_get_str<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body_get(body, key)?.as_text()
}

pub fn body_get_bytes<'a>(body: &'a Value, key: &str) -> Option<&'a [u8]> {
    body_get(body, key)?.as_bytes().map(|b| b.as_slice())
}

pub fn body_get_i64(body: &Value, key: &str) -> Option<i64> {
    body_get(body, key)?.as_integer().and_then(|i| i64::try_from(i).ok())
}

pub fn body_get_bool(body: &Value, key: &str) -> Option<bool> {
    body_get(body, key)?.as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O app Android (`co.nstant.in:cbor:0.9` com `setChunked(true)`) codifica o
    /// `body` como um mapa CBOR de comprimento indefinido (major type 5, header
    /// 0xBF, terminado em 0xFF) em vez de comprimento definido. Este pacote foi
    /// montado byte a byte pra reproduzir exatamente esse formato — envelope
    /// definite-length normal, mas com um `body` indefinite-length dentro —
    /// e confirmar que o decoder do daemon aceita isso sem exigir nenhum hack
    /// (ver PROTOCOL.md §3).
    #[test]
    fn decodes_packet_with_indefinite_length_body() {
        #[rustfmt::skip]
        let raw: Vec<u8> = vec![
            0xA4, // map definido, 4 pares (envelope)
              0x62, b'i', b'd',                 // key "id"
              0x01,                              // value: uint 1
              0x64, b't', b'y', b'p', b'e',      // key "type"
              0x6D, b'c', b'l', b'i', b'p', b'b', b'o', b'a', b'r', b'd', b'.', b's', b'e', b't', // "clipboard.set"
              0x64, b'b', b'o', b'd', b'y',       // key "body"
                0xBF,                             // map INDEFINIDO (o que o app manda)
                  0x64, b't', b'e', b'x', b't',    // key "text"
                  0x63, b'a', b'b', b'c',          // value "abc"
                0xFF,                              // break
              0x6B, b'h', b'a', b's', b'_', b'p', b'a', b'y', b'l', b'o', b'a', b'd', // key "has_payload"
              0xF4, // value: false
        ];

        let packet = Packet::decode(&raw).expect("deve decodificar body indefinite-length");
        assert_eq!(packet.id, 1);
        assert_eq!(packet.kind, "clipboard.set");
        assert!(!packet.has_payload);
        let body = packet.body.expect("body deveria estar presente");
        assert_eq!(body_get_str(&body, "text"), Some("abc"));
    }

    #[test]
    fn round_trips_encode_decode() {
        let body = Value::Map(vec![(Value::Text("text".into()), Value::Text("olá".into()))]);
        let original = Packet::new(42, "clipboard.set", Some(body), false);
        let bytes = original.encode();
        let decoded = Packet::decode(&bytes).unwrap();
        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.kind, "clipboard.set");
        assert_eq!(
            body_get_str(&decoded.body.unwrap(), "text"),
            Some("olá")
        );
    }
}
