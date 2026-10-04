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
//!   hyprlinkctl mic on|off                       # microfone do telemóvel
//!   hyprlinkctl notif "Título" "corpo opcional"  # notificação no telemóvel
//!   hyprlinkctl url https://exemplo.com         # abrir URL no PC
//!   hyprlinkctl ping                            # o daemon responde?
//!
//! Exit codes: 0 = ok · 1 = erro reportado pelo daemon/socket · 2 = uso.

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("uso: hyprlinkctl <status|send|dispatch|lock|tap|speaker|mic|notif|url|ping> [argumentos]");
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
            eprintln!("erro: não foi possível falar com o daemon em {}: {e}", socket.display());
            eprintln!("       (o daemon está a correr? `cargo run` na pasta hyprlink-daemon)");
            std::process::exit(1);
        }
    };

    // Uma linha por conexão (contrato v1 do ctl.rs): envia tudo, fecha a
    // escrita e lê a resposta até EOF.
    let line = args.join(" ");
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
}
