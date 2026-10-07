//! Camada "ecossistema" do daemon: um socket Unix de comandos
//! (`$XDG_RUNTIME_DIR/hyprlink/cmd.sock`) pro binário `hyprlinkctl`,
//! scripts, atalhos do gestor de ficheiros e afins — e um
//! `status.json` publicado no mesmo diretório pro Waybar/painéis lerem
//! o estado ao vivo sem tocar no daemon.
//!
//! É a carta que um ecossistema fechado não pode jogar: a Apple nunca vai
//! dar um CLI pra sua Continuity; nós damos 10 comandos, um socket e um
//! JSON, e qualquer script/wofi/nautilus/Makefile do utilizador vira parte
//! da integração.
//!
//! ponytail: por que socket Unix e não reusar o QUIC? O CLI corre na MESMA
//! máquina que o daemon — mTLS com certificado aqui seria cerimónia sem
//! ganho de segurança (quem consegue falar no runtime dir do utilizador já
//! é o utilizador); e datagrama/stream QUIC exigiria um cliente QUIC
//! inteiro num binário que devia ser de 50 linhas.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::active::ActiveConn;
use crate::config::SharedConfig;
use crate::speaker::SpeakerHandle;
use crate::state::{ConnState, HudState};
use crate::tap::TapHandle;

/// O que o servidor de comandos precisa — montado a partir do `Ctx`.
#[derive(Clone)]
pub struct Ctl {
    pub hud: Arc<Mutex<HudState>>,
    pub config: SharedConfig,
    pub active: ActiveConn,
    pub tap: TapHandle,
    pub speaker: SpeakerHandle,
}

pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("hyprlink").join("cmd.sock"))
}

pub fn status_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| PathBuf::from(d).join("hyprlink").join("status.json"))
}

/// Snapshot do estado do daemon como JSON — a mesma coisa que o comando
/// `hyprlinkctl status` devolve e que o ficheiro `status.json` publica.
pub fn status_json(hud: &Arc<Mutex<HudState>>) -> serde_json::Value {
    let s = hud.lock().unwrap();
    let (connected, connecting, device, fingerprint) = match &s.conn {
        ConnState::Connected {
            device_name,
            fingerprint_hex,
        } => (
            true,
            false,
            Some(device_name.clone()),
            Some(fingerprint_hex.clone()),
        ),
        ConnState::Connecting => (false, true, None, None),
        ConnState::Pairing => (false, false, None, None),
    };
    let m = &s.modules;
    json!({
        "connected": connected,
        "connecting": connecting,
        "device": device,
        "fingerprint": fingerprint,
        "phone_battery_pct": m.phone_battery_pct,
        "pc_battery_pct": m.pc_battery_pct,
        "pc_battery_charging": m.pc_battery_charging,
        "media": m.media,
        "workspace": m.workspace,
        "audio_tap": m.audio_tap_active,
        "mic": m.mic_active,
        "speaker": m.speaker_active,
        "webcam": m.webcam_active,
    })
}

/// Publica `status.json` a cada 2s — escrita atómica (tmp + rename) pro
/// leitor nunca apanhar meio ficheiro. Barato de propósito: é um lock
/// rápido + serialize + write pequenos.
pub async fn publish_status_forever(hud: Arc<Mutex<HudState>>) {
    let Some(path) = status_path() else { return };
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new("/")));
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let body = serde_json::to_string(&status_json(&hud)).unwrap_or_default();
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, body).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Servidor do socket de comandos — uma linha por conexão, uma resposta
/// por linha. `ok …` sai com exit 0 no CLI, `erro: …` com 1.
pub async fn serve(ctl: Ctl) {
    let Some(path) = socket_path() else {
        eprintln!("[!] ctl: sem XDG_RUNTIME_DIR, socket de comandos desativado");
        return;
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new("/")));

    // Resto de um daemon que morreu sem limpar: o bind falharia com o
    // ficheiro lá. Se não há ninguém a escutar, remove e segue.
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(&path).is_err() {
            let _ = std::fs::remove_file(&path);
        } else {
            eprintln!(
                "[!] ctl: já existe um daemon a escutar em {}",
                path.display()
            );
            return;
        }
    }

    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "[!] ctl: não foi possível abrir o socket {}: {e}",
                path.display()
            );
            return;
        }
    };

    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let ctl = ctl.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut rx, mut tx) = stream.into_split();
            let mut buf = vec![0u8; 4096];
            // Uma linha por conexão é o contrato v1 — ler até EOF do cliente
            // (o CLI faz shutdown(Write) logo depois de enviar).
            let mut acc = Vec::new();
            loop {
                match rx.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        acc.extend_from_slice(&buf[..n]);
                        if acc.len() > 16 * 1024 {
                            break;
                        }
                    }
                }
            }
            let line = String::from_utf8_lossy(&acc).trim().to_string();
            let reply = handle(&line, &ctl).await;
            let _ = tx.write_all(reply.as_bytes()).await;
            let _ = tx.shutdown().await;
        });
    }
}

/// `phone.open_url` (PROTOCOL.md §phone): o telemóvel decide se abre direto ou
/// mostra uma notificação. `false` = sem telemóvel ligado. Partilhado pelo
/// `hyprlinkctl phone-url` e pela GUI (`Command2::OpenOnPhone`).
pub async fn phone_open_url(active: &ActiveConn, url: &str) -> bool {
    let body = Some(ciborium::Value::Map(vec![(
        ciborium::Value::Text("url".into()),
        ciborium::Value::Text(url.into()),
    )]));
    crate::active::push(active, "phone.open_url", body)
        .await
        .is_some()
}

/// `phone.run_app`: abre o package no telemóvel. `false` = sem telemóvel.
pub async fn phone_run_app(active: &ActiveConn, package: &str) -> bool {
    let body = Some(ciborium::Value::Map(vec![(
        ciborium::Value::Text("package".into()),
        ciborium::Value::Text(package.into()),
    )]));
    crate::active::push(active, "phone.run_app", body)
        .await
        .is_some()
}

pub use hyprlink_proto::link::{http_url_ok, package_ok};

async fn handle(line: &str, ctl: &Ctl) -> String {
    let mut parts = line.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();

    match cmd {
        "ping" => "ok pong".to_string(),

        "status" => format!("ok {}", status_json(&ctl.hud)),

        // Ficheiro → telemóvel. Progresso/SHA ficam no log da GUI como
        // qualquer envio iniciado por lá (share.rs é o mesmo caminho).
        "send" => match rest.first() {
            Some(path) if rest.len() == 1 => {
                let path = std::path::PathBuf::from(path);
                if !path.is_file() {
                    return format!("erro: não é um ficheiro: {}", path.display());
                }
                crate::share::send_file(&ctl.active, &ctl.hud, &path).await;
                "ok enviado (progresso no painel do daemon)".to_string()
            }
            _ => "erro: uso: send <caminho>".to_string(),
        },

        // hyprctl dispatch de qualquer script/atalho.
        "dispatch" if !rest.is_empty() => {
            let joined = rest.join(" ");
            let out = crate::hypr::dispatch(&joined);
            format!("ok {out}")
        }

        "lock" => {
            let out = crate::hypr::dispatch("exec hyprlock");
            format!("ok {out}")
        }

        // Tap = espelho do sink padrão, igual ao botão do telemóvel; no
        // modo coluna, do sink virtual (mesma regra do server.rs).
        "tap" => match rest.first().copied() {
            Some("on") => {
                if let Some(connection) = ctl.active.lock().unwrap().clone() {
                    tokio::spawn(crate::tap::start(
                        connection,
                        crate::bridge::tap_sink(&ctl.speaker),
                        ctl.tap.clone(),
                        ctl.hud.clone(),
                    ));
                    "ok tap iniciado".to_string()
                } else {
                    "erro: sem telemóvel conectado".to_string()
                }
            }
            Some("off") => {
                crate::tap::stop(&ctl.tap, &ctl.hud);
                "ok tap parado".to_string()
            }
            _ => "erro: uso: tap on|off".to_string(),
        },

        "speaker" => match rest.first().copied() {
            Some("on") => {
                let ok = crate::speaker::enable(
                    &ctl.active,
                    &ctl.tap,
                    &ctl.hud,
                    &ctl.config,
                    &ctl.speaker,
                )
                .await;
                if ok {
                    "ok telemóvel é agora a coluna do PC".to_string()
                } else {
                    "erro: ver painel do daemon".to_string()
                }
            }
            Some("off") => {
                let ok =
                    crate::speaker::disable(&ctl.tap, &ctl.hud, &ctl.config, &ctl.speaker).await;
                if ok {
                    "ok som devolvido às colunas".to_string()
                } else {
                    "erro: modo coluna não estava ativo".to_string()
                }
            }
            _ => "erro: uso: speaker on|off".to_string(),
        },

        // Headset (C1): coluna + mic num comando — composição das duas peças.
        "headset" => match rest.first().copied() {
            Some("on") => {
                let ok = crate::speaker::enable_headset(
                    &ctl.active,
                    &ctl.tap,
                    &ctl.hud,
                    &ctl.config,
                    &ctl.speaker,
                )
                .await;
                if ok {
                    "ok headset ativo: o som do PC toca no telemóvel e o mic dele é a entrada do PC"
                        .to_string()
                } else {
                    "erro: ver painel do daemon".to_string()
                }
            }
            Some("off") => {
                let ok = crate::speaker::disable_headset(
                    &ctl.active,
                    &ctl.tap,
                    &ctl.hud,
                    &ctl.config,
                    &ctl.speaker,
                )
                .await;
                if ok {
                    "ok headset desligado, som de volta às colunas".to_string()
                } else {
                    "erro: headset não estava ativo".to_string()
                }
            }
            _ => "erro: uso: headset on|off".to_string(),
        },

        "mic" => match rest.first().copied() {
            Some("on") => {
                let ok = crate::mic::request_start(&ctl.active).await;
                if ok {
                    "ok pedido enviado (telemóvel decide)".to_string()
                } else {
                    "erro: sem telemóvel conectado".to_string()
                }
            }
            Some("off") => {
                let ok = crate::mic::request_stop(&ctl.active).await;
                if ok {
                    "ok pedido enviado".to_string()
                } else {
                    "erro: sem telemóvel conectado".to_string()
                }
            }
            _ => "erro: uso: mic on|off".to_string(),
        },

        // Notificação no telemóvel — notification.send já existe no
        // protocolo (PROTOCOL.md §notification), era só ninguém mandar
        // de fora da GUI.
        "notif" if !rest.is_empty() => {
            let title = rest[0];
            let body = rest[1..].join(" ");
            let packet_body = Some(ciborium::Value::Map(vec![
                (
                    ciborium::Value::Text("title".into()),
                    ciborium::Value::Text(title.into()),
                ),
                (
                    ciborium::Value::Text("body".into()),
                    ciborium::Value::Text(body),
                ),
                (
                    ciborium::Value::Text("app_name".into()),
                    ciborium::Value::Text("HyprLink".into()),
                ),
            ]));
            match crate::active::push(&ctl.active, "notification.send", packet_body).await {
                Some(_) => "ok notificação enviada".to_string(),
                None => "erro: sem telemóvel conectado".to_string(),
            }
        }

        // URL → abre no PC (xdg-open). "Abrir no telemóvel" é o pacote
        // phone.open_url, que precisa do lado Android (ver Visão §5.B).
        "url" if !rest.is_empty() => {
            let url = rest.join(" ");
            let _ = tokio::process::Command::new("xdg-open").arg(&url).spawn();
            format!("ok aberto no PC: {url}")
        }

        // URL/app no telemóvel — phone.open_url/phone.run_app (a app decide
        // se abre direto ou mostra notificação tappable, ver PROTOCOL.md).
        "phone-url" if !rest.is_empty() => {
            let url = rest.join(" ");
            if phone_open_url(&ctl.active, &url).await {
                format!("ok enviado pro telemóvel: {url}")
            } else {
                "erro: sem telemóvel conectado".to_string()
            }
        }

        "phone-app" if rest.len() == 1 => {
            let package = rest[0];
            if phone_run_app(&ctl.active, package).await {
                format!("ok pedido pra abrir {package} no telemóvel")
            } else {
                "erro: sem telemóvel conectado".to_string()
            }
        }

        // `phone.media` (PROTOCOL.md §phone): prev/pause/next no que toca
        // NO telemóvel — o inverso do `media.command` (MPRIS do PC).
        "phone-media" if rest.len() == 1 => {
            let action = match rest[0] {
                "prev" | "previous" => "previous",
                "pause" | "pp" | "play_pause" | "toggle" => "play_pause",
                "next" => "next",
                other => return format!("erro: ação desconhecida: {other} (prev|pause|next)"),
            };
            let body = Some(ciborium::Value::Map(vec![(
                ciborium::Value::Text("action".into()),
                ciborium::Value::Text(action.into()),
            )]));
            match crate::active::push(&ctl.active, "phone.media", body).await {
                Some(_) => format!("ok {action} no telemóvel"),
                None => "erro: sem telemóvel conectado".to_string(),
            }
        }

        "" => "erro: comando vazio".to_string(),
        _ => format!(
            "erro: comando desconhecido: {cmd} (ping, status, send, dispatch, lock, tap, speaker, headset, mic, notif, url, phone-url, phone-app, phone-media)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_http_e_https() {
        assert!(http_url_ok("https://exemplo.pt/a?b=c#d"));
        assert!(http_url_ok("http://192.168.1.2:8080"));
        for mau in [
            "",
            "exemplo.pt",
            "ftp://exemplo.pt",
            "javascript:alert(1)",
            "intent://x#Intent;end",
            "file:///etc/passwd",
            "https://",
            "https:///x",
            "https://a b.pt",
            "https://a.pt/\n",
            "HTTPS-x://a.pt",
        ] {
            assert!(!http_url_ok(mau), "devia recusar {mau:?}");
        }
        assert!(!http_url_ok(&format!("https://a.pt/{}", "x".repeat(2048))));
    }

    #[test]
    fn nomes_de_package() {
        assert!(package_ok("com.whatsapp"));
        assert!(package_ok("org.thoughtcrime.securesms"));
        assert!(package_ok("com.google.android.apps_x.v2"));
        for mau in [
            "", "whatsapp", "com.", ".com", "com..x", "com.1x", "com.a-b", "com.a b",
        ] {
            assert!(!package_ok(mau), "devia recusar {mau:?}");
        }
    }

    #[tokio::test]
    async fn sem_telemovel_nao_envia() {
        let active = crate::active::new_registry();
        assert!(!phone_open_url(&active, "https://exemplo.pt").await);
        assert!(!phone_run_app(&active, "com.whatsapp").await);
    }
}
