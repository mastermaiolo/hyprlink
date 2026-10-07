//! O relatório do `hyprlinkctl doctor`: só lê, nunca altera nada.
//!
//! **Privacidade:** o relatório não leva nome de utilizador, nome da máquina,
//! endereços MAC/IP, SSID nem o caminho da pasta pessoal (usa `~`): nada disso
//! é recolhido de propósito, e `Redactor` limpa o que escape (versões de
//! ferramentas, valores das opções) antes de sair.

use std::path::Path;

use serde::Serialize;

use crate::battery::{self, Supply};
use crate::camera::{self, DeviceState};
use crate::gpu::{self, Gpu, GpuPlan};
use crate::hyprland::{self, DispatchMode};
use crate::overrides::{self, DispatchChoice, Resolved};
use crate::pipewire::{self, EeState, TapTarget};
use crate::plan::{self, AudioBackend, AudioServer, ShotTool};
use crate::sensors::{self, Sensor};
use crate::shell::{self, Detection};
use crate::{Env, read_trim};

#[derive(Debug, Serialize)]
pub struct ToolInfo {
    pub name: &'static str,
    pub present: bool,
    pub version: Option<String>,
    /// O que o daemon faz com ela (ou «informativo»).
    pub used_for: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub distro: Option<String>,
    pub kernel: Option<String>,
    pub session_type: Option<String>,
    pub desktop: Option<String>,
    pub hyprland_version: Option<String>,
    /// `hyprland.lua` / `hyprland.conf` encontrado em `~/.config/hypr`.
    pub hyprland_config_file: Option<String>,
    pub dispatch_mode: DispatchMode,
    /// Como se chegou ao modo: «teste», «forçado» ou «assumido».
    pub dispatch_mode_source: &'static str,
    pub classic_dispatch_accepted: &'static str,
    pub shell: Detection,
    pub cpu_vendor: Option<String>,
    pub cpu_model: Option<String>,
    pub gpus: Vec<Gpu>,
    pub hybrid: bool,
    pub temp_sensor: Option<Sensor>,
    pub gpu_source: GpuPlan,
    pub ram_total_b: Option<u64>,
    pub disk_free_b: Option<u64>,
    pub disk_total_b: Option<u64>,
    pub supplies: Vec<Supply>,
    pub battery_present: bool,
    pub audio_server: AudioServer,
    pub audio_backend: Option<AudioBackend>,
    /// tap/microfone (pipewiresrc, pw-*) só funcionam com PipeWire.
    pub tap_and_mic_available: bool,
    pub tools: Vec<ToolInfo>,
    pub v4l2loopback_module: bool,
    pub v4l2loopback_available_for_kernel: Option<bool>,
    pub v4l2loopback_dkms: Option<bool>,
    pub camera_device: String,
    pub camera_device_state: DeviceState,
    pub notifications_owner: Option<String>,
    pub mpris_players: Vec<String>,
    /// `pw-dump` respondeu (o grafo do PipeWire foi lido).
    pub pipewire_graph: bool,
    pub easyeffects: Option<EeState>,
    /// O que o retorno de áudio vai capturar (`tap_source`).
    pub tap_target: Option<TapTarget>,
    pub tap_error: Option<String>,
    /// Modo coluna ativo mas o som não vai para a coluna.
    pub speaker_warning: Option<String>,
    pub lock_plan: Vec<String>,
    pub screenshot_plan: Option<ShotTool>,
    pub options: Resolved,
    pub option_problems: Vec<String>,
}

const TOOLS: &[(&str, &str)] = &[
    ("hyprctl", "consultas e dispatch do Hyprland"),
    ("grim", "captura de ecrã e de área"),
    ("grimblast", "captura (preferida a grim)"),
    ("slurp", "escolher a área da captura (com grim)"),
    ("hyprlock", "bloqueio (cadeia genérica)"),
    ("wl-copy", "copiar capturas e clipboard"),
    ("wl-paste", "clipboard"),
    ("loginctl", "bloqueio e suspensão"),
    ("upower", "informativo (o daemon lê a bateria do sysfs)"),
    ("playerctl", "informativo (o daemon fala MPRIS pelo D-Bus)"),
    ("wpctl", "volume e saída predefinida (PipeWire)"),
    ("pactl", "mixer, modo coluna e volume alternativo"),
    ("nvidia-smi", "carga da GPU NVIDIA"),
    (
        "intel_gpu_top",
        "carga da GPU Intel (só com gpu_source=intel e i915)",
    ),
    ("gst-launch-1.0", "webcam e áudio (GStreamer)"),
    ("modinfo", "ver se o v4l2loopback existe para o kernel"),
    ("dkms", "informativo (v4l2loopback via dkms)"),
    (
        "busctl",
        "ver quem serve as notificações e os leitores MPRIS",
    ),
];

fn first_line(s: &str) -> Option<String> {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(100).collect())
}

/// `Hyprland 0.56.2 built from branch …` → `Hyprland 0.56.2`.
fn short_version(line: &str) -> String {
    line.split(" built ")
        .next()
        .unwrap_or(line)
        .trim()
        .to_string()
}

fn mode_label(m: DispatchMode) -> &'static str {
    match m {
        DispatchMode::Classic => "clássico",
        DispatchMode::Lua => "Lua",
    }
}

fn shot_label(t: Option<ShotTool>) -> &'static str {
    match t {
        Some(ShotTool::Grimblast) => "grimblast",
        Some(ShotTool::Grim) => "grim (+ slurp para áreas, wl-copy para copiar)",
        None => "nada (sem grim nem grimblast)",
    }
}

fn backend_label(b: Option<AudioBackend>) -> &'static str {
    match b {
        Some(AudioBackend::Wpctl) => "wpctl",
        Some(AudioBackend::Pactl) => "pactl",
        None => "nada (sem wpctl nem pactl)",
    }
}

fn device_label(d: DeviceState) -> &'static str {
    match d {
        DeviceState::Free => "livre",
        DeviceState::Ours => "já é o do HyprLink",
        DeviceState::Busy => "OCUPADO por outro dispositivo (usa v4l2_device_nr)",
    }
}

fn cpuinfo(proc_dir: &Path) -> (Option<String>, Option<String>) {
    let text = std::fs::read_to_string(proc_dir.join("cpuinfo")).unwrap_or_default();
    let get = |key: &str| {
        text.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    (get("vendor_id"), get("model name"))
}

fn meminfo_total(proc_dir: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(proc_dir.join("meminfo")).ok()?;
    let kb: u64 = text
        .lines()
        .find(|l| l.starts_with("MemTotal:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

fn os_pretty(etc: &Path) -> Option<String> {
    let text = std::fs::read_to_string(etc.join("os-release")).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim().trim_matches('"').to_string())
}

fn disk(home: &Path) -> (Option<u64>, Option<u64>) {
    match rustix::fs::statvfs(home) {
        Ok(st) if st.f_blocks > 0 => (
            Some(st.f_bavail.saturating_mul(st.f_frsize)),
            Some(st.f_blocks.saturating_mul(st.f_frsize)),
        ),
        _ => (None, None),
    }
}

/// Valida o caminho de `temp_sensor`: existe e dá uma temperatura plausível.
fn sensor_ok(p: &Path) -> bool {
    sensors::read_temp(p).is_some()
}

/// Recolhe tudo (o que o `doctor` mostra). `ov` vem do `config.json`.
pub fn collect(env: &Env, ov: &overrides::Overrides) -> Report {
    collect_with(env, ov, true)
}

/// `deep = false` salta o que é lento e só serve para mostrar (versões das
/// ferramentas, D-Bus, `modinfo`, `dkms`): é o que o daemon usa ao arrancar.
pub fn collect_with(env: &Env, ov: &overrides::Overrides, deep: bool) -> Report {
    let (options, option_problems) = ov.resolve(&sensor_ok);
    let have = |t: &str| env.have(t);
    let procs = shell::list_procs(&env.proc_dir);
    let shell = shell::detect(&procs, &have);

    // Hyprland
    let hyprland_version = env
        .run("hyprctl", &["version"])
        .filter(|_| deep)
        .filter(|o| o.ok)
        .and_then(|o| first_line(&o.stdout))
        .map(|l| short_version(&l));
    let cfg_dir = env.config_dir().map(|d| d.join("hypr"));
    let hyprland_config_file = cfg_dir.as_ref().and_then(|d| {
        ["hyprland.lua", "hyprland.conf"]
            .into_iter()
            .find(|f| d.join(f).exists())
            .map(str::to_string)
    });
    let probed = hyprland::probe(&*env.runner);
    let (dispatch_mode, source) = match options.hypr_dispatch_mode {
        DispatchChoice::Classic => (DispatchMode::Classic, "forçado (hypr_dispatch_mode)"),
        DispatchChoice::Lua => (DispatchMode::Lua, "forçado (hypr_dispatch_mode)"),
        DispatchChoice::Auto => match probed {
            Some(m) => (m, "teste com hl.dsp.no_op()"),
            None => (DispatchMode::Classic, "assumido (sem resposta do hyprctl)"),
        },
    };
    let classic_dispatch_accepted = match (dispatch_mode, probed) {
        (DispatchMode::Lua, Some(DispatchMode::Lua)) => "não (o Hyprland está em modo Lua)",
        (DispatchMode::Classic, Some(DispatchMode::Classic)) => {
            "esperado que sim (não testado: um teste clássico alteraria o estado)"
        }
        _ => "desconhecido",
    };

    // Hardware
    let (cpu_vendor, cpu_model) = cpuinfo(&env.proc_dir);
    let gpus = gpu::enumerate(&env.sys);
    let hybrid = gpu::is_hybrid(&gpus);
    let temp_sensor = sensors::pick(&env.sys, options.temp_sensor.as_deref());
    let gpu_source = gpu::plan(&env.sys, options.gpu_source, &have);
    let (disk_free_b, disk_total_b) = env.home().map_or((None, None), |h| disk(&h));
    let supplies = battery::list(&env.sys);
    let battery_present = battery::read_at(&env.sys).is_some();

    // Áudio
    let audio_server = plan::audio_server(&procs, &have);
    let audio_backend = plan::audio_backend(options.audio_backend, &have);
    let pipewiresrc = env
        .run("gst-inspect-1.0", &["pipewiresrc"])
        .is_some_and(|o| o.ok);
    let tap_and_mic_available = plan::pipewire_features(audio_server) && pipewiresrc;

    // Ferramentas
    let tools = TOOLS
        .iter()
        .map(|(name, used_for)| {
            let present = have(name);
            let version = (present && deep)
                .then(|| {
                    if *name == "hyprctl" {
                        return hyprland_version.clone();
                    }
                    env.run(name, &["--version"])
                        .filter(|o| o.ok)
                        .and_then(|o| first_line(&o.stdout))
                })
                .flatten();
            ToolInfo {
                name,
                present,
                version,
                used_for,
            }
        })
        .chain(std::iter::once(ToolInfo {
            name: "pipewiresrc (GStreamer)",
            present: pipewiresrc,
            version: None,
            used_for: "retorno de áudio e microfone",
        }))
        .collect();

    // Câmara
    let nr = options.v4l2_device_nr;
    let camera_device_state = camera::device_state(&env.sys, &env.dev, nr);

    // D-Bus
    let notifications_owner = env
        .run(
            "busctl",
            &["--user", "status", "org.freedesktop.Notifications"],
        )
        .filter(|o| o.ok)
        .and_then(|o| {
            o.stdout
                .lines()
                .find_map(|l| l.strip_prefix("Comm="))
                .map(|c| c.trim().to_string())
        });
    let mut mpris_players: Vec<String> = env
        .run("busctl", &["--user", "list", "--no-legend", "--no-pager"])
        .filter(|_| deep)
        .filter(|o| o.ok)
        .map(|o| {
            o.stdout
                .lines()
                .filter_map(|l| l.split_whitespace().next())
                .filter_map(|n| n.strip_prefix("org.mpris.MediaPlayer2."))
                .map(|n| n.split(".instance").next().unwrap_or(n).to_string())
                .collect()
        })
        .unwrap_or_default();
    mpris_players.sort();
    mpris_players.dedup();

    // PipeWire / EasyEffects (do grafo real, não da configuração).
    let snap = pipewire::snapshot(env);
    let (tap_target, tap_error, speaker_warning) = match &snap {
        Some((g, ee)) => {
            let (t, e) = match pipewire::resolve_tap(options.tap_source, ee) {
                Ok(t) => (Some(t), None),
                Err(e) => (None, Some(e)),
            };
            (t, e, pipewire::speaker_routing_warning(g, ee))
        }
        None => (None, None, None),
    };

    let lock_plan = plan::lock_candidates(shell.shell, &have, options.lock_command.as_deref())
        .into_iter()
        .map(|a| a.join(" "))
        .collect();
    let screenshot_plan = plan::screenshot_tool(options.screenshot_tool, &have);

    Report {
        distro: os_pretty(&env.etc),
        kernel: read_trim(&env.proc_dir.join("sys/kernel/osrelease")),
        session_type: env.var("XDG_SESSION_TYPE").map(str::to_string),
        desktop: env.var("XDG_CURRENT_DESKTOP").map(str::to_string),
        hyprland_version,
        hyprland_config_file,
        dispatch_mode,
        dispatch_mode_source: source,
        classic_dispatch_accepted,
        shell,
        cpu_vendor,
        cpu_model,
        gpus,
        hybrid,
        temp_sensor,
        gpu_source,
        ram_total_b: meminfo_total(&env.proc_dir),
        disk_free_b,
        disk_total_b,
        supplies,
        battery_present,
        audio_server,
        audio_backend,
        tap_and_mic_available,
        tools,
        v4l2loopback_module: camera::module_loaded(&env.sys),
        v4l2loopback_available_for_kernel: deep.then(|| camera::module_available(env)).flatten(),
        v4l2loopback_dkms: deep.then(|| camera::dkms_has_v4l2loopback(env)).flatten(),
        camera_device: format!("/dev/video{nr}"),
        camera_device_state,
        notifications_owner,
        mpris_players,
        pipewire_graph: snap.is_some(),
        easyeffects: snap.map(|(_, ee)| ee),
        tap_target,
        tap_error,
        speaker_warning,
        lock_plan,
        screenshot_plan,
        options,
        option_problems,
    }
}

fn gib(b: u64) -> String {
    format!("{:.1} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0))
}

fn yn(b: bool) -> &'static str {
    if b { "sim" } else { "não" }
}

fn opt<T: std::fmt::Display>(v: &Option<T>) -> String {
    v.as_ref().map_or("desconhecido".into(), |x| x.to_string())
}

/// O relatório legível (pt-PT).
pub fn render_text(r: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let w = &mut s;
    let _ = writeln!(w, "HyprLink doctor — relatório de compatibilidade");
    let _ = writeln!(
        w,
        "(só leitura; sem nome de utilizador, máquina, MAC, IP, SSID nem pasta pessoal)\n"
    );

    let _ = writeln!(w, "[Sistema]");
    let _ = writeln!(w, "  distribuição : {}", opt(&r.distro));
    let _ = writeln!(w, "  kernel       : {}", opt(&r.kernel));
    let _ = writeln!(
        w,
        "  sessão       : {} · {}",
        opt(&r.session_type),
        opt(&r.desktop)
    );
    let _ = writeln!(
        w,
        "  CPU          : {} · {}",
        opt(&r.cpu_vendor),
        opt(&r.cpu_model)
    );
    let _ = writeln!(
        w,
        "  RAM          : {}",
        r.ram_total_b.map_or("desconhecida".into(), gib)
    );
    let _ = writeln!(
        w,
        "  disco (casa) : {} livres de {}",
        r.disk_free_b.map_or("?".into(), gib),
        r.disk_total_b.map_or("?".into(), gib)
    );

    let _ = writeln!(w, "\n[Hyprland]");
    let _ = writeln!(w, "  versão       : {}", opt(&r.hyprland_version));
    let _ = writeln!(w, "  configuração : {}", opt(&r.hyprland_config_file));
    let _ = writeln!(
        w,
        "  dispatch     : {} — {}",
        mode_label(r.dispatch_mode),
        r.dispatch_mode_source
    );
    let _ = writeln!(w, "  clássico aceite: {}", r.classic_dispatch_accepted);
    let _ = writeln!(
        w,
        "  shell        : {} ({})",
        r.shell.shell.label(),
        r.shell.evidence
    );
    let _ = writeln!(
        w,
        "  notificações : servidas por {}",
        opt(&r.notifications_owner)
    );
    let _ = writeln!(
        w,
        "  leitores MPRIS: {}",
        if r.mpris_players.is_empty() {
            "nenhum".into()
        } else {
            r.mpris_players.join(", ")
        }
    );

    let _ = writeln!(w, "\n[GPU e temperatura]");
    for g in &r.gpus {
        let _ = writeln!(
            w,
            "  {} : {} · driver {}",
            g.card,
            g.vendor.name(),
            g.driver.as_deref().unwrap_or("?")
        );
    }
    if r.gpus.is_empty() {
        let _ = writeln!(w, "  nenhuma GPU em /sys/class/drm");
    }
    let _ = writeln!(w, "  híbrido      : {}", yn(r.hybrid));
    let _ = writeln!(w, "  carga da GPU : {}", r.gpu_source.why);
    match &r.temp_sensor {
        Some(t) => {
            let name = t
                .path
                .components()
                .rev()
                .take(2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let _ = writeln!(w, "  temperatura  : …/{name} — {}", t.why);
        }
        None => {
            let _ = writeln!(
                w,
                "  temperatura  : sem sensor plausível (o campo sai omitido)"
            );
        }
    }

    let _ = writeln!(w, "\n[Baterias]");
    for b in &r.supplies {
        let _ = writeln!(
            w,
            "  {:<8} tipo {:<8} escopo {:<8} {}",
            b.name,
            b.kind,
            b.scope.as_deref().unwrap_or("-"),
            if b.used { "← usada pelo daemon" } else { "" }
        );
    }
    let _ = writeln!(
        w,
        "  bateria do sistema: {}",
        if r.battery_present {
            "presente"
        } else {
            "ausente (battery.state sai com present:false)"
        }
    );

    let _ = writeln!(w, "\n[Áudio]");
    let _ = writeln!(w, "  servidor     : {}", r.audio_server.label());
    let _ = writeln!(w, "  volume       : {}", backend_label(r.audio_backend));
    let _ = writeln!(
        w,
        "  retorno/microfone (pipewiresrc): {}",
        if r.tap_and_mic_available {
            "disponíveis"
        } else {
            "DESATIVADOS (precisam de PipeWire e do plugin pipewiresrc)"
        }
    );

    let _ = writeln!(w, "\n[EasyEffects e retorno de áudio]");
    match &r.easyeffects {
        None => {
            let _ = writeln!(
                w,
                "  grafo do PipeWire : não lido (sem pw-dump ou sem PipeWire)"
            );
        }
        Some(ee) => {
            let _ = writeln!(
                w,
                "  EasyEffects       : {}",
                if ee.running {
                    "a correr (easyeffects_sink no grafo)"
                } else {
                    "não está a correr"
                }
            );
            if ee.running {
                let _ = writeln!(
                    w,
                    "  toca em           : {}",
                    ee.destination
                        .as_ref()
                        .map_or("desconhecido".into(), |d| format!(
                            "{} ({})",
                            d.sink,
                            d.how.label()
                        ))
                );
                let _ = writeln!(
                    w,
                    "  bypass global     : {}",
                    ee.bypass.map_or("sem resposta do socket", |b| if b {
                        "ligado"
                    } else {
                        "desligado"
                    })
                );
                let _ = writeln!(
                    w,
                    "  configuração      : {}",
                    match &ee.config {
                        None => "easyeffectsrc não existe".to_string(),
                        Some(c) => format!(
                            "usar predefinido = {}; saída escolhida = {}",
                            c.use_default_output
                                .map_or("?", |b| if b { "sim" } else { "não" }),
                            c.output_device.as_deref().unwrap_or("-")
                        ),
                    }
                );
            }
            let _ = writeln!(w, "  saída predefinida : {}", opt(&ee.system_default));
        }
    }
    match (&r.tap_target, &r.tap_error) {
        (Some(t), _) => {
            let _ = writeln!(
                w,
                "  o retorno captura : o monitor de {} — {}",
                t.sink, t.why
            );
        }
        (_, Some(e)) => {
            let _ = writeln!(w, "  o retorno captura : NADA — {e}");
        }
        _ => {}
    }
    if let Some(w2) = &r.speaker_warning {
        let _ = writeln!(w, "  ! modo coluna     : {w2}");
    }

    let _ = writeln!(w, "\n[Câmara]");
    let _ = writeln!(
        w,
        "  módulo v4l2loopback carregado : {}",
        yn(r.v4l2loopback_module)
    );
    let _ = writeln!(
        w,
        "  existe para este kernel       : {}",
        r.v4l2loopback_available_for_kernel
            .map_or("desconhecido", yn)
    );
    let _ = writeln!(
        w,
        "  dkms                          : {}",
        r.v4l2loopback_dkms.map_or("desconhecido", yn)
    );
    let _ = writeln!(
        w,
        "  {} : {}",
        r.camera_device,
        device_label(r.camera_device_state)
    );

    let _ = writeln!(w, "\n[O que o daemon vai fazer]");
    let _ = writeln!(
        w,
        "  bloquear     : {}",
        if r.lock_plan.is_empty() {
            "nada (sem ferramenta de bloqueio)".into()
        } else {
            r.lock_plan.join("  →  ")
        }
    );
    let _ = writeln!(w, "  captura      : {}", shot_label(r.screenshot_plan));
    let _ = writeln!(w, "  volume       : {}", backend_label(r.audio_backend));

    let _ = writeln!(w, "\n[Ferramentas]");
    for t in &r.tools {
        let _ = writeln!(
            w,
            "  {:<24} {:<8} {:<30} {}",
            t.name,
            if t.present { "presente" } else { "falta" },
            t.version.as_deref().unwrap_or(""),
            t.used_for
        );
    }

    let _ = writeln!(w, "\n[Opções em uso (config.json)]");
    let o = &r.options;
    let _ = writeln!(
        w,
        "  lock_command       : {}",
        o.lock_command
            .as_ref()
            .map_or("auto".into(), |a| a.join(" "))
    );
    let _ = writeln!(w, "  screenshot_tool    : {:?}", o.screenshot_tool);
    let _ = writeln!(
        w,
        "  temp_sensor        : {}",
        o.temp_sensor
            .as_ref()
            .map_or("auto".into(), |p| p.display().to_string())
    );
    let _ = writeln!(w, "  gpu_source         : {:?}", o.gpu_source);
    let _ = writeln!(w, "  v4l2_device_nr     : {}", o.v4l2_device_nr);
    let _ = writeln!(w, "  audio_backend      : {:?}", o.audio_backend);
    let _ = writeln!(w, "  hypr_dispatch_mode : {:?}", o.hypr_dispatch_mode);
    let _ = writeln!(w, "  tap_source         : {:?}", o.tap_source);
    for p in &r.option_problems {
        let _ = writeln!(w, "  ! {p}");
    }
    s
}

/// Tira o que identifica a pessoa ou a máquina.
pub struct Redactor {
    home: Option<String>,
    user: Option<String>,
    host: Option<String>,
}

impl Redactor {
    pub fn new(env: &Env) -> Self {
        let host = read_trim(&env.etc.join("hostname"))
            .or_else(|| read_trim(&env.proc_dir.join("sys/kernel/hostname")));
        Self {
            home: env
                .home()
                .map(|h| h.to_string_lossy().into_owned())
                .filter(|h| h.len() > 1),
            user: env.var("USER").map(str::to_string).filter(|u| u.len() >= 3),
            host: host.filter(|h| h.len() >= 3),
        }
    }

    pub fn apply(&self, text: &str) -> String {
        let mut t = text.to_string();
        if let Some(h) = &self.home {
            t = t.replace(h.as_str(), "~");
        }
        if let Some(h) = &self.host {
            t = t.replace(h.as_str(), "<máquina>");
        }
        if let Some(u) = &self.user {
            t = t.replace(u.as_str(), "<utilizador>");
        }
        scrub_addresses(&t)
    }
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// Troca endereços MAC (`aa:bb:cc:dd:ee:ff` ou com `-`) e IPv4 por marcas.
fn scrub_addresses(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        // MAC: 6 pares hex separados por ':' ou '-' (17 bytes).
        if i + 17 <= b.len()
            && (i == 0 || !is_hex(b[i - 1]))
            && (0..6).all(|k| is_hex(b[i + 3 * k]) && is_hex(b[i + 3 * k + 1]))
            && (0..5).all(|k| matches!(b[i + 3 * k + 2], b':' | b'-' | b'_'))
            && (i + 17 == b.len() || !is_hex(b[i + 17]))
        {
            out.push_str("<mac>");
            i += 17;
            continue;
        }
        // IPv4: quatro números 0–255 separados por pontos.
        if b[i].is_ascii_digit()
            && (i == 0 || !(b[i - 1].is_ascii_digit() || b[i - 1] == b'.'))
            && let Some(len) = ipv4_len(&b[i..])
        {
            out.push_str("<ip>");
            i += len;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn ipv4_len(b: &[u8]) -> Option<usize> {
    let mut pos = 0;
    for octet in 0..4 {
        let start = pos;
        while pos < b.len() && b[pos].is_ascii_digit() && pos - start < 3 {
            pos += 1;
        }
        let n: u16 = std::str::from_utf8(&b[start..pos]).ok()?.parse().ok()?;
        if pos == start || n > 255 {
            return None;
        }
        if octet < 3 {
            if pos >= b.len() || b[pos] != b'.' {
                return None;
            }
            pos += 1;
        }
    }
    if pos < b.len()
        && (b[pos].is_ascii_digit()
            || b[pos] == b'.' && pos + 1 < b.len() && b[pos + 1].is_ascii_digit())
    {
        return None;
    }
    Some(pos)
}

/// O relatório em texto, já limpo.
pub fn text_redacted(env: &Env, r: &Report) -> String {
    Redactor::new(env).apply(&render_text(r))
}

/// O relatório em JSON, já limpo.
pub fn json_redacted(env: &Env, r: &Report) -> String {
    let json = serde_json::to_string_pretty(r).unwrap_or_else(|_| "{}".into());
    Redactor::new(env).apply(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_e_ip_saem() {
        let s = scrub_addresses(
            "a 3C:22:FB:01:02:ab b 192.168.1.20 c 10.0.0.1/24 d v1.2.3 e 6.12.4.1-x",
        );
        assert!(!s.contains("3C:22"), "{s}");
        assert!(!s.contains("192.168"), "{s}");
        assert!(s.contains("<mac>") && s.contains("<ip>"));
        assert!(s.contains("v1.2.3"), "versões de 3 números ficam: {s}");
        assert!(!s.contains("10.0.0.1"), "{s}");
    }

    #[test]
    fn octetos_invalidos_nao_sao_ips() {
        assert_eq!(scrub_addresses("1.2.3.999"), "1.2.3.999");
    }
}
