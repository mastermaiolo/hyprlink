//! `hyprlinkctl` — CLI do HyprLink. Fala com o daemon pelo socket Unix
//! `$XDG_RUNTIME_DIR/hyprlink/cmd.sock` (ver `src/ctl.rs`). Zero deps de
//! rede: std puro, para o binário ser instantâneo em qualquer script,
//! atalho de gestor de ficheiros, wofi/rofi ou Makefile.
//!
//! Uso:
//!   hyprlinkctl status                          # estado completo (JSON)
//!   hyprlinkctl send ~/fotos/foto.jpg            # ficheiro → telemóvel
//!   hyprlinkctl dispatch "workspace 2"          # hyprctl dispatch remoto
//!   hyprlinkctl lock                            # exec hyprlock
//!   hyprlinkctl tap on|off                       # ouvir o PC no telemóvel
//!   hyprlinkctl speaker on|off                   # telemóvel como coluna
//!   hyprlinkctl headset on|off                    # headset completo: coluna + mic
//!   hyprlinkctl mic on|off                       # microfone do telemóvel
//!   hyprlinkctl notif "Título" "corpo opcional"  # notificação no telemóvel
//!   hyprlinkctl url https://exemplo.com         # abrir URL no PC
//!   hyprlinkctl phone-url https://exemplo.com   # abrir URL no telemóvel
//!   hyprlinkctl phone-app org.example.app       # lançar app no telemóvel
//!   hyprlinkctl phone-media prev|pause|next     # controlar o que toca no telemóvel
//!   hyprlinkctl ping                            # o daemon responde?
//!
//! Exit codes: 0 = ok · 1 = erro reportado pelo daemon/socket · 2 = uso.
//!
//! É a metade "texto" do `hyprlinkctl` (sem `--json`): mantém as respostas
//! `ok …` / `erro: …` de que o `contrib/` depende. `main.rs` decide quando
//! encaminhar para aqui.

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

/// Subcomandos que só existem no protocolo de texto do `cmd.sock`.
pub const ONLY_HERE: &[&str] = &[
    "send",
    "dispatch",
    "lock",
    "notif",
    "url",
    "phone-url",
    "phone-app",
    "phone-media",
];

/// Subcomandos que existem nas duas metades: sem `--json` vêm para aqui.
pub const SHARED: &[&str] = &["status", "ping", "mic", "tap", "speaker", "headset"];

pub fn run(args: Vec<String>) -> ! {
    if args.is_empty() {
        eprintln!(
            "uso: hyprlinkctl <status|send|dispatch|lock|tap|speaker|mic|notif|url|phone-url|phone-app|phone-media|ping> [argumentos]"
        );
        std::process::exit(2);
    }

    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") else {
        eprintln!("erro: XDG_RUNTIME_DIR não definido");
        std::process::exit(1);
    };
    let socket = std::path::Path::new(&dir).join("hyprlink").join("cmd.sock");

    let mut stream = match UnixStream::connect(&socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "erro: não foi possível falar com o daemon em {}: {e}",
                socket.display()
            );
            eprintln!("       (o daemon está a correr? `cargo run -p hyprlinkd`)");
            std::process::exit(1);
        }
    };

    // Uma linha por conexão (contrato v1 do ctl.rs): envia tudo, fecha a
    // escrita e lê a resposta até EOF.
    let line = if args[0] == "send" {
        let cwd = std::env::current_dir().unwrap_or_default();
        match send_line(&args[1..], &cwd) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("erro: {e}");
                std::process::exit(2);
            }
        }
    } else {
        args.join(" ")
    };
    if let Err(e) = stream.write_all(line.as_bytes()) {
        eprintln!("erro: {e}");
        std::process::exit(1);
    }
    let _ = stream.shutdown(Shutdown::Write);

    let mut reply = String::new();
    if let Err(e) = stream.read_to_string(&mut reply) {
        eprintln!("erro: {e}");
        std::process::exit(1);
    }
    let reply = reply.trim();
    println!("{reply}");
    if reply.starts_with("erro") {
        std::process::exit(1);
    }
    std::process::exit(0);
}

/// A linha do `send`: `send <caminho>` com o caminho **inteiro** (o daemon
/// usa o resto da linha tal e qual, ver `ctl.rs` `send_path`). Um `file://`
/// passa a caminho (percent-decoding) e um caminho relativo passa a
/// absoluto, porque o daemon corre noutro diretório.
fn send_line(args: &[String], cwd: &std::path::Path) -> Result<String, String> {
    if args.is_empty() {
        return Err("uso: hyprlinkctl send <ficheiro>".into());
    }
    // Normalmente é um só argumento (`%f`, ou aspas na shell); várias
    // palavras soltas são o mesmo caminho partido pela shell.
    let raw = args.join(" ");
    let path = match raw.strip_prefix("file://") {
        Some(rest) => file_uri_path(rest)?,
        None => raw,
    };
    if path.contains(['\n', '\r', '\0']) {
        return Err("caminho com mudança de linha ou NUL".into());
    }
    let p = std::path::Path::new(&path);
    let abs = if p.is_absolute() {
        path
    } else {
        cwd.join(p).to_string_lossy().into_owned()
    };
    Ok(format!("send {abs}"))
}

/// `file://[localhost]/caminho%20com%20espaços` → `/caminho com espaços`.
fn file_uri_path(rest: &str) -> Result<String, String> {
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return Err("só file:// local (sem anfitrião)".into());
    }
    let b = path.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = path.get(i + 1..i + 3).ok_or("file:// com % incompleto")?;
            let v = u8::from_str_radix(hex, 16).map_err(|_| "file:// com % inválido")?;
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "file:// com bytes que não são UTF-8".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn line(args: &[&str]) -> Result<String, String> {
        let v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        send_line(&v, Path::new("/tmp/x/Transferências"))
    }

    #[test]
    fn send_passa_o_caminho_inteiro() {
        for p in [
            "/tmp/As minhas fotos/a.jpg",
            "/tmp/dois  espaços.txt",
            "/tmp/acentuação.pdf",
            "/tmp/it's.txt",
            "/tmp/\"aspas\".txt",
            "/tmp/#1.txt",
            "/tmp/literal%20.txt",
        ] {
            assert_eq!(line(&[p]), Ok(format!("send {p}")), "{p:?}");
        }
    }

    #[test]
    fn send_relativo_passa_a_absoluto() {
        assert_eq!(
            line(&["foto 1.jpg"]),
            Ok("send /tmp/x/Transferências/foto 1.jpg".into())
        );
    }

    #[test]
    fn send_file_uri_e_descodificado() {
        assert_eq!(
            line(&["file:///tmp/As%20minhas%20fotos/a%C3%A7%C3%A3o%25.jpg"]),
            Ok("send /tmp/As minhas fotos/ação%.jpg".into())
        );
        assert_eq!(
            line(&["file://localhost/tmp/a.txt"]),
            Ok("send /tmp/a.txt".into())
        );
        for mau in [
            "file://outro-pc/tmp/a",
            "file:///tmp/a%2",
            "file:///tmp/a%zz",
            "file:///tmp/a%0Ab",
            "file:///tmp/%FF",
        ] {
            assert!(line(&[mau]).is_err(), "devia recusar {mau:?}");
        }
    }

    #[test]
    fn send_recusa_mudanca_de_linha_e_vazio() {
        assert!(line(&["/tmp/a\nb"]).is_err());
        assert!(line(&[]).is_err());
    }
}
