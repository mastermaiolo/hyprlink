//! One HYPRLINK window per session.
//!
//! The running GUI listens on `$XDG_RUNTIME_DIR/hyprlink-gui.sock`. A second
//! `hyprlink-gui` (or `hyprlinkctl open`) connects, asks it to show its window
//! and exits — the window comes back focused instead of a second process.

use iced::Subscription;
use iced::futures::SinkExt;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("hyprlink-gui.sock"))
}

/// If a HYPRLINK GUI is already running, ask it to open its window (unless
/// `open` is false, e.g. `--hidden`) and return `true`: the caller exits.
pub fn forward_to_running(open: bool) -> bool {
    use std::io::Write;
    let Some(path) = socket_path() else {
        return false;
    };
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(mut s) => {
            if open {
                let _ = s.write_all(b"open\n");
            }
            true
        }
        Err(_) => false,
    }
}

/// Emits `()` each time another process asks for the window.
pub fn subscription() -> Subscription<()> {
    Subscription::run(listen)
}

fn listen() -> impl iced::futures::Stream<Item = ()> {
    iced::stream::channel(
        4,
        async |mut out: iced::futures::channel::mpsc::Sender<()>| {
            let Some(path) = socket_path() else {
                return;
            };
            // A stale socket from a crashed GUI: nobody answers, so take it over.
            if path.exists() && std::os::unix::net::UnixStream::connect(&path).is_err() {
                let _ = std::fs::remove_file(&path);
            }
            let listener = match tokio::net::UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("hyprlink: instância única indisponível ({e})");
                    return;
                }
            };
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            while let Ok((mut s, _)) = listener.accept().await {
                let mut buf = [0u8; 16];
                let n = s.read(&mut buf).await.unwrap_or(0);
                if buf[..n].starts_with(b"open") {
                    let _ = out.send(()).await;
                }
            }
        },
    )
}
