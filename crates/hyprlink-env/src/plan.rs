//! O que o daemon faz em cada função, dado o ambiente: bloqueio, captura,
//! volume. O daemon (`action.rs`) e o `doctor` usam as mesmas funções.

use serde::Serialize;

use crate::overrides::{AudioChoice, ScreenshotChoice};
use crate::shell::Shell;

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn push_unique(c: &mut Vec<Vec<String>>, a: Vec<String>) {
    if !c.contains(&a) {
        c.push(a);
    }
}

/// Comandos de bloqueio por ordem (o primeiro que servir ganha).
///
/// - `lock_command` da config → só esse.
/// - Com shell detetada, o método dela vai **antes** do `hyprlock`:
///   Noctalia v5 → `noctalia msg session lock`; Ryoku → `ryoku-shell lock`;
///   Caelestia → `loginctl lock-session` (o que o README dela indica).
///   Noctalia v4: o comando **não está confirmado** → sem método próprio.
/// - Depois a cadeia genérica: `noctalia` (se existir e a shell não for
///   outra conhecida), `hyprlock`, `loginctl lock-session`.
///
/// Fontes: `docs/COMPATIBILIDADE.md` §1.
pub fn lock_candidates(
    shell: Shell,
    have: &dyn Fn(&str) -> bool,
    forced: Option<&[String]>,
) -> Vec<Vec<String>> {
    if let Some(f) = forced {
        return vec![f.to_vec()];
    }
    let mut c = Vec::new();
    match shell {
        Shell::NoctaliaV5 if have("noctalia") => {
            push_unique(&mut c, argv(&["noctalia", "msg", "session", "lock"]))
        }
        Shell::Ryoku if have("ryoku-shell") => push_unique(&mut c, argv(&["ryoku-shell", "lock"])),
        Shell::Caelestia if have("loginctl") => {
            push_unique(&mut c, argv(&["loginctl", "lock-session"]))
        }
        _ => {}
    }
    if matches!(shell, Shell::Other | Shell::None) && have("noctalia") {
        push_unique(&mut c, argv(&["noctalia", "msg", "session", "lock"]));
    }
    if have("hyprlock") {
        push_unique(&mut c, argv(&["hyprlock"]));
    }
    if have("loginctl") {
        push_unique(&mut c, argv(&["loginctl", "lock-session"]));
    }
    c
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotTool {
    Grimblast,
    Grim,
}

/// A ferramenta de captura: `grimblast` antes de `grim`; a opção força uma
/// (e, se faltar, não há captura em vez de usar outra).
pub fn screenshot_tool(choice: ScreenshotChoice, have: &dyn Fn(&str) -> bool) -> Option<ShotTool> {
    match choice {
        ScreenshotChoice::Auto => {
            if have("grimblast") {
                Some(ShotTool::Grimblast)
            } else if have("grim") {
                Some(ShotTool::Grim)
            } else {
                None
            }
        }
        ScreenshotChoice::Grimblast => have("grimblast").then_some(ShotTool::Grimblast),
        ScreenshotChoice::Grim => have("grim").then_some(ShotTool::Grim),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioBackend {
    Wpctl,
    Pactl,
}

/// O programa do volume: `wpctl` (PipeWire) antes de `pactl`; a opção força
/// um (se faltar, não há volume).
pub fn audio_backend(choice: AudioChoice, have: &dyn Fn(&str) -> bool) -> Option<AudioBackend> {
    match choice {
        AudioChoice::Auto => {
            if have("wpctl") {
                Some(AudioBackend::Wpctl)
            } else if have("pactl") {
                Some(AudioBackend::Pactl)
            } else {
                None
            }
        }
        AudioChoice::Wpctl => have("wpctl").then_some(AudioBackend::Wpctl),
        AudioChoice::Pactl => have("pactl").then_some(AudioBackend::Pactl),
    }
}

/// Comando de volume (`up`/`down`/`mute`) do backend.
pub fn volume_argv(b: AudioBackend, what: &str) -> Option<Vec<String>> {
    Some(match (b, what) {
        (AudioBackend::Wpctl, "up") => argv(&[
            "wpctl",
            "set-volume",
            "-l",
            "1",
            "@DEFAULT_AUDIO_SINK@",
            "5%+",
        ]),
        (AudioBackend::Wpctl, "down") => {
            argv(&["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"])
        }
        (AudioBackend::Wpctl, "mute") => {
            argv(&["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
        }
        (AudioBackend::Pactl, "up") => argv(&["pactl", "set-sink-volume", "@DEFAULT_SINK@", "+5%"]),
        (AudioBackend::Pactl, "down") => {
            argv(&["pactl", "set-sink-volume", "@DEFAULT_SINK@", "-5%"])
        }
        (AudioBackend::Pactl, "mute") => {
            argv(&["pactl", "set-sink-mute", "@DEFAULT_SINK@", "toggle"])
        }
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioServer {
    PipeWire,
    PulseAudio,
    Other,
    None,
}

impl AudioServer {
    pub fn label(self) -> &'static str {
        match self {
            AudioServer::PipeWire => "PipeWire",
            AudioServer::PulseAudio => "PulseAudio",
            AudioServer::Other => "outro (há pactl, sem servidor reconhecido)",
            AudioServer::None => "nenhum detetado",
        }
    }
}

/// O servidor de áudio, pelos processos a correr: `pipewire` (com ou sem
/// `pipewire-pulse`) → PipeWire; `pulseaudio` → PulseAudio.
pub fn audio_server(procs: &[crate::shell::Proc], have: &dyn Fn(&str) -> bool) -> AudioServer {
    let running = |n: &str| procs.iter().any(|p| p.comm == n);
    if running("pipewire") {
        AudioServer::PipeWire
    } else if running("pulseaudio") {
        AudioServer::PulseAudio
    } else if have("pactl") || have("wpctl") {
        AudioServer::Other
    } else {
        AudioServer::None
    }
}

/// O tap/microfone usam `pipewiresrc` (GStreamer) e `pw-*`: só com PipeWire.
pub fn pipewire_features(server: AudioServer) -> bool {
    server == AudioServer::PipeWire
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(set: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |t| set.contains(&t)
    }

    fn first(c: &[Vec<String>]) -> String {
        c.first().map(|a| a.join(" ")).unwrap_or_default()
    }

    #[test]
    fn bloqueio_pela_shell_antes_do_hyprlock() {
        let t = has(&["noctalia", "hyprlock", "loginctl", "ryoku-shell"]);
        assert_eq!(
            first(&lock_candidates(Shell::NoctaliaV5, &t, None)),
            "noctalia msg session lock"
        );
        assert_eq!(
            first(&lock_candidates(Shell::Ryoku, &t, None)),
            "ryoku-shell lock"
        );
        assert_eq!(
            first(&lock_candidates(Shell::Caelestia, &t, None)),
            "loginctl lock-session"
        );
        // Noctalia v4: sem método próprio confirmado → cadeia genérica.
        assert_eq!(
            first(&lock_candidates(Shell::NoctaliaV4, &t, None)),
            "hyprlock"
        );
        // Sem shell: cadeia atual (noctalia, hyprlock, loginctl).
        let c = lock_candidates(Shell::None, &t, None);
        assert_eq!(c.len(), 3);
        assert_eq!(first(&c), "noctalia msg session lock");
        assert_eq!(c.last().unwrap().join(" "), "loginctl lock-session");
    }

    #[test]
    fn bloqueio_sem_duplicados_e_sem_ferramentas() {
        let t = has(&["loginctl"]);
        assert_eq!(lock_candidates(Shell::Caelestia, &t, None).len(), 1);
        assert!(lock_candidates(Shell::None, &has(&[]), None).is_empty());
    }

    #[test]
    fn bloqueio_forcado() {
        let f = vec!["meu-lock".to_string(), "--x".to_string()];
        let c = lock_candidates(Shell::Ryoku, &has(&["ryoku-shell"]), Some(&f));
        assert_eq!(c, vec![f]);
    }

    #[test]
    fn captura_auto_e_forcada() {
        let t = has(&["grim", "grimblast"]);
        assert_eq!(
            screenshot_tool(ScreenshotChoice::Auto, &t),
            Some(ShotTool::Grimblast)
        );
        assert_eq!(
            screenshot_tool(ScreenshotChoice::Grim, &t),
            Some(ShotTool::Grim)
        );
        assert_eq!(
            screenshot_tool(ScreenshotChoice::Auto, &has(&["grim"])),
            Some(ShotTool::Grim)
        );
        assert_eq!(
            screenshot_tool(ScreenshotChoice::Grimblast, &has(&["grim"])),
            None
        );
        assert_eq!(screenshot_tool(ScreenshotChoice::Auto, &has(&[])), None);
    }

    #[test]
    fn volume_wpctl_ou_pactl() {
        assert_eq!(
            audio_backend(AudioChoice::Auto, &has(&["wpctl", "pactl"])),
            Some(AudioBackend::Wpctl)
        );
        assert_eq!(
            audio_backend(AudioChoice::Auto, &has(&["pactl"])),
            Some(AudioBackend::Pactl)
        );
        assert_eq!(audio_backend(AudioChoice::Pactl, &has(&["wpctl"])), None);
        let up = volume_argv(AudioBackend::Pactl, "up").unwrap();
        assert_eq!(up[0], "pactl");
        assert_eq!(
            volume_argv(AudioBackend::Wpctl, "mute").unwrap()[1],
            "set-mute"
        );
        assert_eq!(volume_argv(AudioBackend::Wpctl, "x"), None);
    }

    #[test]
    fn servidor_de_audio() {
        use crate::shell::Proc;
        let p = |c: &str| Proc {
            comm: c.into(),
            args: vec![],
        };
        assert_eq!(
            audio_server(&[p("pipewire"), p("wireplumber")], &has(&[])),
            AudioServer::PipeWire
        );
        assert_eq!(
            audio_server(&[p("pulseaudio")], &has(&[])),
            AudioServer::PulseAudio
        );
        assert_eq!(audio_server(&[], &has(&["pactl"])), AudioServer::Other);
        assert_eq!(audio_server(&[], &has(&[])), AudioServer::None);
        assert!(pipewire_features(AudioServer::PipeWire));
        assert!(!pipewire_features(AudioServer::PulseAudio));
    }
}
