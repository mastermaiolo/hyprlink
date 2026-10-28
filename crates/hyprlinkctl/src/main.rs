//! `hyprlinkctl` — the scriptable face of HYPRLINK, for shell plugins and keybinds.
//!
//!   hyprlinkctl watch --json [--interval MS]   one JSON snapshot per line
//!   hyprlinkctl status [--json | --waybar]
//!   hyprlinkctl ping | clipboard | pair
//!   hyprlinkctl mic on|off|toggle
//!   hyprlinkctl tap on|off|toggle
//!   hyprlinkctl speaker on|off|toggle          retorno a partir do sink hyprlink-speaker
//!   hyprlinkctl mirror start|stop|toggle
//!   hyprlinkctl ws <N>
//!   hyprlinkctl open                           open (or focus) the GUI
//!   hyprlinkctl doctor [--json] [--report [f]] [--no-dispatch]  relatório de compatibilidade (só lê)
//!
//! Two halves, one binary:
//! - **text** (no `--json`): the original `ok …` / `erro: …` protocol over
//!   `$XDG_RUNTIME_DIR/hyprlink/cmd.sock` (`legacy.rs`), for `contrib/` and old
//!   scripts — `status ping mic tap speaker` plus `send dispatch lock notif url
//!   phone-url phone-app`.
//! - **JSON v1** (`--json`, and the commands only this half has).
//!
//! The JSON half talks to the real `hyprlinkd` over
//! `$XDG_RUNTIME_DIR/hyprlink.sock` (`real.rs`, `hyprlink_proto::client::Session`).
//! With the daemon stopped, `watch` keeps emitting `"daemon":"down"` lines and
//! retries; one-shot commands answer `{"ok":false,"error":"daemon_down",…}`
//! with a non-zero exit.
//!
//! `HYPRLINK_MOCK=1` (or `--mock`) switches the JSON half to the simulator
//! (`Backend` below, built with the `mock` feature): screenshots, fixtures
//! and tests. Its commands persist toggles in
//! `$XDG_RUNTIME_DIR/hyprlink-mock.json` and `watch` folds them into the
//! simulation; every snapshot says `"daemon":"mock"`.

mod doctor;
mod legacy;
mod real;
mod usb;
mod waybar;

use hyprlink_proto::fmt;
use hyprlink_proto::host::Probe;
use hyprlink_proto::i18n::{self, Lang};
#[cfg(feature = "mock")]
use hyprlink_proto::link::mock::Simulator;
use hyprlink_proto::link::{Codec, Command, FindPhoneRequest, MirrorConfig, Transport};
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
    // Por onde vai a ligação (USB / Wi-Fi…), só com telemóvel ligado.
    let phone = match s.link_path.filter(|_| s.device.is_some()) {
        Some(p) => format!("{phone} · {}", fmt::link_path(p)),
        None => phone,
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
        "uso: hyprlinkctl <watch|status|ping|clipboard|pair|mic|tap|speaker|mirror|ws|open|doctor|usb|shortcuts|find> [args] [--json] [--lang L]\n\
         \n  sem --json, status|ping|mic|tap|speaker usam o protocolo de texto do daemon (ok …/erro: …)\
         \n  só texto: send <ficheiro> · dispatch <cmd> · lock · notif <título> [corpo] · url · phone-url · phone-app\n\
         \n  watch --json [--interval MS]   uma linha JSON por instantâneo (por omissão 500 ms)\
         \n  mic|tap|speaker on|off|toggle\n  mirror start|stop|toggle\n  ws <1-10>\
         \n  status --waybar                 uma linha {{text,tooltip,class}} para a Waybar (lê o status.json)\
         \n  doctor [--json] [--report [f]] [--no-dispatch]  relatório de compatibilidade (só lê;\n                                 --no-dispatch não faz hyprctl dispatch)\
         \n  usb setup                       tira a rota por omissão à ancoragem USB (nmcli, como\n                                 utilizador, com confirmação; o doctor só sugere)\
         \n  shortcuts [--json]              lista os atalhos configurados\
         \n  find [sound|vibrate|torch|all] [--secs N] [--json]   faz tocar o telemóvel (por omissão\n                                 all, 30 s; N entre 5 e 60)\
         \n  find stop [--json]              para o toque\
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

/// O simulador foi pedido? `--mock` nos argumentos ou `HYPRLINK_MOCK` com
/// um valor que não seja vazio nem `0`.
fn mock_requested(args: &[String], env: Option<&str>) -> bool {
    args.iter().any(|a| a == "--mock") || env.is_some_and(|v| !v.is_empty() && v != "0")
}

/// A metade JSON contra o daemon real (`real.rs`).
fn run_real(pos: &[&str], json: bool, interval: Duration, secs: u32) -> ExitCode {
    match pos.first().copied() {
        Some("watch") => real::watch(json, interval, human),
        Some("status") => real::status(json, human),
        Some("ping") => real::ping(json),
        Some("clipboard") => real::clipboard(json),
        Some("pair") => real::pair(json),
        Some(which @ ("mic" | "tap" | "mirror" | "speaker")) => {
            real::channel(json, which, pos.get(1).copied()).unwrap_or_else(usage)
        }
        Some("ws") => match pos
            .get(1)
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|n| (1..=10).contains(n))
        {
            Some(n) => real::workspace(json, n),
            None => usage(),
        },
        Some("open") => real::open_gui(json),
        Some("find") => match find_arg(pos.get(1).copied(), secs) {
            Some(req) => real::find(json, req),
            None => usage(),
        },
        _ => usage(),
    }
}

/// Flags que levam um valor a seguir (`--secs 15`): o valor não é posicional.
const VALUE_FLAGS: &[&str] = &["--interval", "--secs", "--lang"];

/// Os argumentos que não são flags nem valores de flags.
fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if VALUE_FLAGS.contains(&a.as_str()) {
            skip = true;
        } else if !a.starts_with("--") {
            out.push(a.as_str());
        }
    }
    out
}

/// `find [sound|vibrate|torch|all|stop]`: `Some(None)` = parar; `None` = modo
/// desconhecido. A duração fica cortada a 5–60 s.
fn find_arg(mode: Option<&str>, secs: u32) -> Option<Option<FindPhoneRequest>> {
    let off = FindPhoneRequest {
        sound: false,
        vibrate: false,
        torch: false,
        duration_s: secs,
    };
    let req = match mode.unwrap_or("all") {
        "stop" => return Some(None),
        "all" => FindPhoneRequest::all(secs),
        "sound" => FindPhoneRequest { sound: true, ..off },
        "vibrate" => FindPhoneRequest {
            vibrate: true,
            ..off
        },
        "torch" => FindPhoneRequest { torch: true, ..off },
        _ => return None,
    };
    req.normalized().map(Some)
}

/// `shortcuts`: lê o `config.json` local (igual com e sem daemon).
fn shortcuts(json: bool) -> ExitCode {
    let env = hyprlink_env::Env::real();
    let config_path = env.hyprlink_config_path();
    #[derive(serde::Deserialize)]
    struct Cfg {
        #[serde(default)]
        shortcuts: Vec<ShortcutItem>,
    }
    #[derive(serde::Deserialize, serde::Serialize)]
    struct ShortcutItem {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        command: String,
    }
    let list: Vec<ShortcutItem> = config_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Cfg>(&s).ok())
        .map(|c| c.shortcuts)
        .unwrap_or_default();
    if json {
        print_json(&serde_json::json!({ "shortcuts": list }));
    } else if list.is_empty() {
        println!("sem atalhos configurados");
    } else {
        for s in list {
            println!("{}: {} -> {}", s.id, s.name, s.command);
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let (args, lang) = take_lang(std::env::args().skip(1).collect());
    let mock = mock_requested(&args, std::env::var("HYPRLINK_MOCK").ok().as_deref());
    let args: Vec<String> = args.into_iter().filter(|a| a != "--mock").collect();
    match lang {
        // Sem flag, o idioma vem do ambiente (LC_ALL / LC_MESSAGES / LANG).
        Ok(l) => i18n::set(l.unwrap_or_else(i18n::detect)),
        Err(v) => {
            eprintln!("erro: --lang {v:?} desconhecido (pt-PT|pt-BR|en|es|zh)");
            return ExitCode::from(2);
        }
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        let _ = usage();
        return ExitCode::SUCCESS;
    }
    if args
        .iter()
        .any(|a| a == "--version" || a == "-V" || a == "-v")
    {
        println!("hyprlinkctl {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let json = args.iter().any(|a| a == "--json");
    if args.first().map(String::as_str) == Some("usb") {
        return usb::run(&args);
    }
    if args.first().map(String::as_str) == Some("doctor") {
        return doctor::run(&args);
    }
    // Antes do protocolo de texto: o `status` sem `--json` iria para o cmd.sock.
    if args.first().map(String::as_str) == Some("status") && args.iter().any(|a| a == "--waybar") {
        return waybar::run();
    }
    let first = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(String::as_str);
    // Com o simulador, os comandos partilhados ficam no simulador mesmo sem
    // `--json` (o protocolo de texto é sempre o daemon real).
    if let Some(cmd) = first
        && (legacy::ONLY_HERE.contains(&cmd) || (!json && !mock && legacy::SHARED.contains(&cmd)))
    {
        legacy::run(args);
    }
    let pos = positional(&args);
    let flag = |name: &str| -> Option<u64> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
    };

    let interval = Duration::from_millis(flag("--interval").unwrap_or(500).clamp(100, 10_000));
    let secs = flag("--secs")
        .map(|s| s.min(u64::from(u32::MAX)) as u32)
        .unwrap_or(FindPhoneRequest::DEFAULT_S);
    if pos.first().copied() == Some("shortcuts") {
        return shortcuts(json);
    }
    if !mock {
        return run_real(&pos, json, interval, secs);
    }

    // Simulador (`HYPRLINK_MOCK=1` / `--mock`).
    match pos.first().copied() {
        Some("watch") => {
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
        Some("find") => match find_arg(pos.get(1).copied(), secs) {
            Some(Some(req)) => ok(
                json,
                &format!(
                    "a tocar no telemóvel ({}) · {} s",
                    real::find_modes(&req),
                    req.duration_s
                ),
                serde_json::json!({ "find_phone": real::find_state_json(true) }),
            ),
            Some(None) => ok(
                json,
                "toque parado",
                serde_json::json!({ "find_phone": real::find_state_json(false) }),
            ),
            None => usage(),
        },
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
    fn valores_de_flags_nao_sao_posicionais() {
        let a = v(&["find", "--secs", "15", "torch", "--json"]);
        assert_eq!(positional(&a), vec!["find", "torch"]);
        let a = v(&["watch", "--interval", "250", "--json"]);
        assert_eq!(positional(&a), vec!["watch"]);
    }

    #[test]
    fn find_modos_e_duracao() {
        let r = find_arg(None, 30).unwrap().unwrap();
        assert_eq!(r, FindPhoneRequest::all(30));
        let r = find_arg(Some("vibrate"), 2).unwrap().unwrap();
        assert!(r.vibrate && !r.sound && !r.torch);
        assert_eq!(r.duration_s, 5, "cortado ao mínimo");
        assert_eq!(
            find_arg(Some("torch"), 600).unwrap().unwrap().duration_s,
            60
        );
        assert_eq!(find_arg(Some("stop"), 30), Some(None));
        assert_eq!(find_arg(Some("xpto"), 30), None);
    }

    #[test]
    fn status_mostra_por_onde_vai_a_ligacao() {
        use hyprlink_proto::link::{Event, Event2, Kind, LinkPath, LinkState};
        use hyprlink_proto::snapshot::DeviceBrief;
        let mut s = Snapshot::new("up");
        s.apply(&Event::More(Event2::LinkPath(Some(LinkPath::Usb))));
        assert!(
            !human(&s).contains("USB"),
            "sem telemóvel não mostra o caminho"
        );
        s.device = Some(DeviceBrief {
            id: 1,
            name: "Poco".into(),
            manufacturer: None,
            model: None,
            android: None,
            kind: Kind::Phone,
            state: LinkState::Linked,
            battery: Some(80),
            charging: false,
        });
        assert!(
            human(&s).lines().next().unwrap().ends_with("· USB"),
            "{}",
            human(&s)
        );
        s.apply(&Event::More(Event2::LinkPath(None)));
        assert!(!human(&s).contains("USB"));
        let j = serde_json::to_value(&s).unwrap();
        assert!(j["link_path"].is_null());
        s.apply(&Event::More(Event2::LinkPath(Some(LinkPath::Wifi))));
        assert_eq!(serde_json::to_value(&s).unwrap()["link_path"], "wifi");
    }

    #[test]
    fn simulador_so_quando_pedido() {
        assert!(!mock_requested(&v(&["watch", "--json"]), None));
        assert!(mock_requested(&v(&["watch", "--mock"]), None));
        assert!(mock_requested(&v(&["watch"]), Some("1")));
        assert!(!mock_requested(&v(&["watch"]), Some("0")));
        assert!(!mock_requested(&v(&["watch"]), Some("")));
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
