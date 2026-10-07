//! `hyprlinkctl` — the scriptable face of HYPRLINK, for shell plugins and keybinds.
//!
//!   hyprlinkctl watch --json [--interval MS]   one JSON snapshot per line
//!   hyprlinkctl status [--json]
//!   hyprlinkctl ping | clipboard | pair
//!   hyprlinkctl mic on|off|toggle
//!   hyprlinkctl tap on|off|toggle
//!   hyprlinkctl speaker on|off|toggle          retorno a partir do sink hyprlink-speaker
//!   hyprlinkctl mirror start|stop|toggle
//!   hyprlinkctl ws <N>
//!   hyprlinkctl open                           open (or focus) the GUI
//!
//! Two halves, one binary:
//! - **text** (no `--json`): the original `ok …` / `erro: …` protocol over
//!   `$XDG_RUNTIME_DIR/hyprlink/cmd.sock` (`legacy.rs`), for `contrib/` and old
//!   scripts — `status ping mic tap speaker` plus `send dispatch lock notif url
//!   phone-url phone-app`.
//! - **JSON v1** (`--json`, and the commands only this half has).
//!
//! The JSON half still talks to the *mock* daemon: commands persist toggles in
//! `$XDG_RUNTIME_DIR/hyprlink-mock.json`, and `watch` folds them into the
//! simulation, so a Noctalia panel behaves exactly as it will against the
//! real `hyprlinkd`. Swapping in the real socket changes only `Backend`.

mod legacy;

use hyprlink_proto::fmt;
use hyprlink_proto::host::Probe;
use hyprlink_proto::i18n::{self, Lang};
#[cfg(feature = "mock")]
use hyprlink_proto::link::mock::Simulator;
use hyprlink_proto::link::{Codec, Command, MirrorConfig, Transport};
use hyprlink_proto::snapshot::Snapshot;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
struct Toggles {
    mic: bool,
    tap: bool,
    mirror: bool,
    speaker: bool,
    ws: u8,
    pair_until: u64,
}

impl Default for Toggles {
    fn default() -> Self {
        Self {
            mic: true,
            tap: false,
            mirror: false,
            speaker: true,
            ws: 3,
            pair_until: 0,
        }
    }
}

fn state_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("hyprlink-mock.json")
}

fn load() -> Toggles {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(t: &Toggles) {
    let _ = std::fs::write(state_path(), serde_json::to_string(t).unwrap_or_default());
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

const MIRROR: MirrorConfig = MirrorConfig {
    codec: Codec::Hevc,
    bitrate_mbps: 12.0,
    max_fps: 60,
    scale: 0.75,
};

/// Without the `mock` feature there is no JSON backend yet (the real socket
/// lands in Fase 3): every snapshot says `"daemon": "down"`.
#[cfg(not(feature = "mock"))]
struct Simulator;

#[cfg(not(feature = "mock"))]
impl Simulator {
    fn new() -> Self {
        Simulator
    }
}

#[cfg(not(feature = "mock"))]
impl Transport for Simulator {
    fn send(&mut self, _command: Command) {}
    fn poll(&mut self, _dt: Duration) -> Vec<hyprlink_proto::link::Event> {
        Vec::new()
    }
}

const DAEMON: &str = if cfg!(feature = "mock") {
    "mock"
} else {
    "down"
};

/// Mock backend: the simulator plus the persisted toggles.
struct Backend {
    sim: Simulator,
    applied: Toggles,
    snap: Snapshot,
    probe: Probe,
}

impl Backend {
    fn new() -> Self {
        let mut b = Self {
            sim: Simulator::new(),
            applied: Toggles::default(),
            snap: Snapshot::new(DAEMON),
            probe: Probe::default(),
        };
        b.sync(&load());
        // Warm up so the first line already carries history.
        for _ in 0..60 {
            b.step(Duration::from_millis(50));
        }
        b.snap.host = Some(b.probe.sample());
        b
    }

    fn sync(&mut self, want: &Toggles) {
        let have = self.applied.clone();
        if want.mic != have.mic {
            self.sim.send(Command::SetMic(want.mic));
        }
        if want.tap != have.tap {
            self.sim.send(Command::SetTap(want.tap));
        }
        if want.mirror != have.mirror {
            self.sim.send(if want.mirror {
                Command::StartMirror(MIRROR)
            } else {
                Command::StopMirror
            });
        }
        if want.speaker != have.speaker {
            self.sim.send(Command::SetSpeakerMode(want.speaker));
        }
        if want.ws != have.ws {
            self.sim.send(Command::SwitchWorkspace(want.ws));
        }
        let pairing = want.pair_until > now_s();
        if pairing && self.snap.pairing.is_none() {
            self.sim.send(Command::BeginPairing);
        }
        self.applied = want.clone();
        self.fold_toggles();
    }

    /// The persisted toggles are the mock's state; with no daemon behind the
    /// JSON half they mean nothing and the snapshot stays at its defaults.
    fn fold_toggles(&mut self) {
        if cfg!(feature = "mock") {
            self.snap.mic.on = self.applied.mic;
            self.snap.tap.on = self.applied.tap;
        }
    }

    fn step(&mut self, dt: Duration) {
        for e in self.sim.poll(dt) {
            self.snap.apply(&e);
        }
        self.fold_toggles();
    }
}

fn print_json(v: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string(v).unwrap_or_else(|_| "{}".into())
    );
}

fn human(s: &Snapshot) -> String {
    let phone = match &s.device {
        Some(d) => format!(
            "{} · {} · {}%{} · {} · {} ms",
            d.name,
            fmt::state(d.state).to_lowercase(),
            fmt::opt(d.battery),
            if d.charging { "+" } else { "" },
            s.phone
                .as_ref()
                .map(fmt::network_line)
                .unwrap_or_else(|| fmt::DASH.into()),
            fmt::latency(s.latency_ms)
        ),
        None => i18n::t("sem telemóvel").into(),
    };
    let pc = s
        .host
        .as_ref()
        .map(|h| {
            format!(
                "{} · cpu {:.0}% · ram {:.1}/{:.1} GB",
                h.hostname, h.cpu_pct, h.ram_used_gb, h.ram_total_gb
            )
        })
        .unwrap_or_default();
    format!(
        "{phone}\n  mic {} · tap {}{} · {} {} · {} {}\n{pc}",
        if s.mic.on { "on" } else { "off" },
        if s.tap.on { "on" } else { "off" },
        if s.tap.speaker {
            format!(" ({})", i18n::t("coluna"))
        } else {
            String::new()
        },
        i18n::t("espelho"),
        if s.mirror.on { "on" } else { "off" },
        i18n::t("workspace"),
        fmt::opt(s.workspace)
    )
}

fn toggle(arg: Option<&str>, current: bool) -> Option<bool> {
    match arg {
        Some("on") | Some("start") => Some(true),
        Some("off") | Some("stop") => Some(false),
        Some("toggle") | None => Some(!current),
        _ => None,
    }
}

fn ok(json: bool, msg: &str, extra: serde_json::Value) -> ExitCode {
    if json {
        let mut v = serde_json::json!({ "ok": true, "message": msg });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        print_json(&v);
    } else {
        println!("{msg}");
    }
    ExitCode::SUCCESS
}

fn usage() -> ExitCode {
    eprintln!(
        "uso: hyprlinkctl <watch|status|ping|clipboard|pair|mic|tap|speaker|mirror|ws|open> [args] [--json] [--lang L]\n\
         \n  sem --json, status|ping|mic|tap|speaker usam o protocolo de texto do daemon (ok …/erro: …)\
         \n  só texto: send <ficheiro> · dispatch <cmd> · lock · notif <título> [corpo] · url · phone-url · phone-app\n\
         \n  watch --json [--interval MS]   uma linha JSON por instantâneo (por omissão 500 ms)\
         \n  mic|tap|speaker on|off|toggle\n  mirror start|stop|toggle\n  ws <1-10>\
         \n  --lang pt-PT|pt-BR|en|es|zh   idioma dos textos (por omissão, o do ambiente)"
    );
    ExitCode::from(2)
}

/// Tira `--lang X` / `--lang=X` dos argumentos; devolve o idioma pedido
/// (`Err` com o valor se não for um dos cinco).
fn take_lang(args: Vec<String>) -> (Vec<String>, Result<Option<Lang>, String>) {
    let mut out = Vec::with_capacity(args.len());
    let mut lang = Ok(None);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let value = if a == "--lang" {
            Some(it.next().unwrap_or_default())
        } else {
            a.strip_prefix("--lang=").map(str::to_owned)
        };
        match value {
            Some(v) => lang = Lang::parse(&v).map(Some).ok_or(v),
            None => out.push(a),
        }
    }
    (out, lang)
}

fn main() -> ExitCode {
    let (args, lang) = take_lang(std::env::args().skip(1).collect());
    match lang {
        // Sem flag, o idioma vem do ambiente (LC_ALL / LC_MESSAGES / LANG).
        Ok(l) => i18n::set(l.unwrap_or_else(i18n::detect)),
        Err(v) => {
            eprintln!("erro: --lang {v:?} desconhecido (pt-PT|pt-BR|en|es|zh)");
            return ExitCode::from(2);
        }
    }
    let json = args.iter().any(|a| a == "--json");
    let first = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(String::as_str);
    if let Some(cmd) = first
        && (legacy::ONLY_HERE.contains(&cmd) || (!json && legacy::SHARED.contains(&cmd)))
    {
        legacy::run(args);
    }
    let pos: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(String::as_str)
        .collect();
    let flag = |name: &str| -> Option<u64> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
    };

    match pos.first().copied() {
        Some("watch") => {
            let interval =
                Duration::from_millis(flag("--interval").unwrap_or(500).clamp(100, 10_000));
            let mut b = Backend::new();
            let mut last_emit = Instant::now() - interval;
            let mut last_sync = Instant::now();
            let mut last_host = Instant::now();
            let stdout = std::io::stdout();
            loop {
                b.step(Duration::from_millis(50));
                if last_sync.elapsed() > Duration::from_millis(250) {
                    last_sync = Instant::now();
                    b.sync(&load());
                }
                if last_host.elapsed() > Duration::from_secs(2) {
                    last_host = Instant::now();
                    b.snap.host = Some(b.probe.sample());
                }
                if last_emit.elapsed() >= interval {
                    last_emit = Instant::now();
                    let line = if json {
                        serde_json::to_string(&b.snap).unwrap_or_default()
                    } else {
                        human(&b.snap).replace('\n', "  ")
                    };
                    let mut out = stdout.lock();
                    // A closed pipe (plugin reloaded) ends the stream cleanly.
                    if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
                        return ExitCode::SUCCESS;
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Some("status") => {
            let b = Backend::new();
            if json {
                print_json(&b.snap);
            } else {
                println!("{}", human(&b.snap));
            }
            ExitCode::SUCCESS
        }
        Some("ping") => {
            let b = Backend::new();
            let Some(lat) = b.snap.latency_ms else {
                eprintln!("sem telemóvel ligado");
                return ExitCode::from(1);
            };
            let rtt = (lat as f64 * 20.0).round() / 10.0;
            let name = b
                .snap
                .device
                .as_ref()
                .map(|d| d.name.clone())
                .unwrap_or_default();
            ok(
                json,
                &format!("{name} respondeu em {rtt} ms"),
                serde_json::json!({ "device": name, "rtt_ms": rtt }),
            )
        }
        Some("clipboard") => ok(json, "área de transferência enviada", serde_json::json!({})),
        Some("pair") => {
            let mut t = load();
            t.pair_until = now_s() + 120;
            save(&t);
            ok(
                json,
                "emparelhamento aberto durante 120 s",
                serde_json::json!({ "expires_in": 120 }),
            )
        }
        Some(which @ ("mic" | "tap" | "mirror" | "speaker")) => {
            let mut t = load();
            let cur = match which {
                "mic" => t.mic,
                "tap" => t.tap,
                "speaker" => t.speaker,
                _ => t.mirror,
            };
            let Some(v) = toggle(pos.get(1).copied(), cur) else {
                return usage();
            };
            match which {
                "mic" => t.mic = v,
                "tap" => t.tap = v,
                "speaker" => t.speaker = v,
                _ => t.mirror = v,
            }
            save(&t);
            ok(
                json,
                &format!("{which} {}", if v { "on" } else { "off" }),
                serde_json::json!({ which: v }),
            )
        }
        Some("ws") => {
            let Some(n) = pos
                .get(1)
                .and_then(|v| v.parse::<u8>().ok())
                .filter(|n| (1..=10).contains(n))
            else {
                return usage();
            };
            let mut t = load();
            t.ws = n;
            save(&t);
            ok(
                json,
                &format!("workspace {n}"),
                serde_json::json!({ "workspace": n }),
            )
        }
        Some("open") => {
            // The GUI is a single tray-resident process; launching it again is
            // the simplest "open" until single-instance activation lands.
            match std::process::Command::new("hyprlink-gui").spawn() {
                Ok(_) => ok(json, "a abrir HYPRLINK", serde_json::json!({})),
                Err(e) => {
                    eprintln!("não foi possível abrir hyprlink-gui: {e}");
                    ExitCode::from(1)
                }
            }
        }
        _ => usage(),
    }
}

#[cfg(test)]
mod lang_tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn take_lang_strips_the_flag_in_both_spellings() {
        let (rest, l) = take_lang(v(&["--lang", "es", "status", "--json"]));
        assert_eq!(rest, v(&["status", "--json"]));
        assert_eq!(l, Ok(Some(Lang::EsEs)));
        let (rest, l) = take_lang(v(&["status", "--lang=zh"]));
        assert_eq!(rest, v(&["status"]));
        assert_eq!(l, Ok(Some(Lang::Zh)));
        let (rest, l) = take_lang(v(&["status"]));
        assert_eq!(rest, v(&["status"]));
        assert_eq!(l, Ok(None));
        assert_eq!(
            take_lang(v(&["--lang", "klingon"])).1,
            Err("klingon".into())
        );
    }
}
