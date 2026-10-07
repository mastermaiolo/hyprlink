//! Perfis de hardware e de ambiente (`fixtures/<perfil>/`): uma árvore
//! `/sys`, `/proc`, `/etc` e `/dev` falsa, as ferramentas presentes e o modo
//! do `hyprctl` de cada máquina. Testa a deteção que o daemon e o
//! `hyprlinkctl doctor` partilham (`hyprlink-env`), e a redação de dados
//! pessoais do relatório.

use std::path::{Path, PathBuf};

use hyprlink_env::battery;
use hyprlink_env::camera::DeviceState;
use hyprlink_env::gpu::{self, Vendor};
use hyprlink_env::hyprland::DispatchMode;
use hyprlink_env::overrides::{Overrides, Resolved};
use hyprlink_env::plan::{self, AudioBackend, AudioServer, ShotTool};
use hyprlink_env::report::{self, Report};
use hyprlink_env::shell::Shell;
use hyprlink_env::{Env, RunOut, Runner};

fn dir(profile: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(profile)
}

/// `hyprctl` falso: `hypr.txt` diz `lua`, `classic` ou `absent`; `extra` junta
/// saídas para comandos de teste (versões com dados pessoais).
struct FakeRunner {
    hypr: String,
    version: String,
    /// Saída do `pw-dump` (perfis `ee-*`); `None` = sem `pw-dump`.
    pw: Option<String>,
}

impl Runner for FakeRunner {
    fn run(&self, prog: &str, args: &[&str]) -> Option<RunOut> {
        match (prog, args) {
            ("hyprctl", ["dispatch", expr]) => {
                if self.hypr == "absent" {
                    return None;
                }
                Some(RunOut {
                    ok: expr.starts_with("hl.") == (self.hypr == "lua"),
                    stdout: String::new(),
                })
            }
            ("hyprctl", ["version"]) if self.hypr != "absent" => Some(RunOut {
                ok: true,
                stdout: self.version.clone(),
            }),
            ("pw-dump", []) => self.pw.clone().map(|stdout| RunOut { ok: true, stdout }),
            _ => None,
        }
    }
}

fn env_with_version(profile: &str, version: &str) -> Env {
    let d = dir(profile);
    let read = |f: &str| std::fs::read_to_string(d.join(f)).unwrap_or_default();
    let tools: Vec<String> = read("tools.txt").lines().map(str::to_string).collect();
    let vars = read("vars.txt")
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Env {
        sys: d.join("sys"),
        proc_dir: d.join("proc"),
        etc: d.join("etc"),
        dev: d.join("dev"),
        vars,
        have: Box::new(move |t| tools.iter().any(|x| x == t)),
        runner: Box::new(FakeRunner {
            hypr: read("hypr.txt").trim().to_string(),
            version: version.to_string(),
            pw: None,
        }),
        sock_query: Box::new(|_, _| None),
    }
}

fn env(profile: &str) -> Env {
    env_with_version(
        profile,
        "Hyprland 0.56.2 built from branch v0.56.2 at commit abc clean\n",
    )
}

fn rep(profile: &str) -> Report {
    report::collect(&env(profile), &Overrides::default())
}

fn temp_name(r: &Report) -> String {
    r.temp_sensor
        .as_ref()
        .map(|t| t.why.clone())
        .unwrap_or_default()
}

// ── AMD, Noctalia v5, Hyprland Lua ─────────────────────────────────────────

#[test]
fn amd_so() {
    let r = rep("amd-so");
    assert_eq!(r.cpu_vendor.as_deref(), Some("AuthenticAMD"));
    assert_eq!(r.distro.as_deref(), Some("Arch Linux"));
    assert_eq!(r.dispatch_mode, DispatchMode::Lua);
    assert_eq!(r.shell.shell, Shell::NoctaliaV5);
    assert!(
        temp_name(&r).contains("Tdie"),
        "Tdie antes de Tctl: {}",
        temp_name(&r)
    );
    assert_eq!(r.gpus.len(), 1);
    assert_eq!(
        (r.gpus[0].vendor, r.gpus[0].driver.as_deref()),
        (Vendor::Amd, Some("amdgpu"))
    );
    assert!(!r.hybrid);
    assert!(r.gpu_source.amd && !r.gpu_source.nvidia && !r.gpu_source.intel);
    assert!(r.battery_present);
    assert_eq!(r.audio_server, AudioServer::PipeWire);
    assert_eq!(r.audio_backend, Some(AudioBackend::Wpctl));
    assert_eq!(
        r.lock_plan[0], "noctalia msg session lock",
        "{:?}",
        r.lock_plan
    );
    assert_eq!(r.screenshot_plan, Some(ShotTool::Grim));
    assert_eq!(r.camera_device_state, DeviceState::Free);
}

// ── Intel (coretemp, i915), Hyprland clássico, sem shell ───────────────────

#[test]
fn intel_so() {
    let r = rep("intel-so");
    assert_eq!(r.cpu_vendor.as_deref(), Some("GenuineIntel"));
    assert_eq!(r.kernel.as_deref(), Some("6.6.50-1-lts"));
    assert_eq!(r.dispatch_mode, DispatchMode::Classic);
    assert!(r.classic_dispatch_accepted.contains("esperado"));
    assert_eq!(r.shell.shell, Shell::None);
    assert!(
        temp_name(&r).contains("coretemp"),
        "coretemp vence acpitz: {}",
        temp_name(&r)
    );
    assert_eq!(
        (r.gpus[0].vendor, r.gpus[0].driver.as_deref()),
        (Vendor::Intel, Some("i915"))
    );
    // Intel: sem valor por omissão, mesmo com intel_gpu_top instalado.
    assert!(
        !r.gpu_source.intel && !r.gpu_source.amd,
        "{:?}",
        r.gpu_source
    );
    assert!(r.gpu_source.why.contains("Intel"));
    assert_eq!(r.lock_plan[0], "hyprlock");
    // Com gpu_source = "intel" passa a usar o intel_gpu_top (driver i915).
    let ov: Overrides = serde_json::from_str(r#"{"gpu_source":"intel"}"#).unwrap();
    let forced = report::collect(&env("intel-so"), &ov);
    assert!(forced.gpu_source.intel, "{:?}", forced.gpu_source);
    let b = battery::read_at(&env("intel-so").sys).unwrap();
    assert_eq!(
        (b.level, b.charging, b.plugged),
        (100, false, true),
        "cheia na corrente"
    );
}

// ── AMD + NVIDIA híbrido, Ryoku ────────────────────────────────────────────

#[test]
fn amd_nvidia_hibrido() {
    let r = rep("amd-nvidia-hibrido");
    assert_eq!(r.gpus.len(), 2);
    assert!(r.hybrid);
    let v: Vec<Vendor> = r.gpus.iter().map(|g| g.vendor).collect();
    assert_eq!(v, vec![Vendor::Nvidia, Vendor::Amd]);
    assert!(r.gpu_source.amd && r.gpu_source.nvidia);
    assert_eq!(r.shell.shell, Shell::Ryoku);
    assert_eq!(r.lock_plan[0], "ryoku-shell lock", "{:?}", r.lock_plan);
    // Bateria no limite de carga: ligada à corrente, sem carregar.
    let b = battery::read_at(&env("amd-nvidia-hibrido").sys).unwrap();
    assert_eq!((b.level, b.charging, b.plugged), (80, false, true));
    assert!(r.supplies.iter().any(|s| s.name == "BAT1" && s.used));
}

// ── NVIDIA desktop, PulseAudio, sem bateria, só acpitz ─────────────────────

#[test]
fn nvidia_desktop() {
    let r = rep("nvidia-desktop");
    assert_eq!(r.gpus[0].vendor, Vendor::Nvidia);
    assert!(!r.hybrid);
    assert!(r.gpu_source.nvidia && !r.gpu_source.amd);
    assert!(!r.battery_present, "só há a fonte AC");
    assert!(temp_name(&r).contains("acpitz"), "{}", temp_name(&r));
    assert_eq!(r.audio_server, AudioServer::PulseAudio);
    assert_eq!(r.audio_backend, Some(AudioBackend::Pactl), "sem wpctl");
    assert!(
        !r.tap_and_mic_available,
        "retorno e microfone precisam de PipeWire"
    );
    assert_eq!(r.screenshot_plan, Some(ShotTool::Grimblast));
    assert_eq!(r.shell.shell, Shell::None);
}

// ── Desktop sem nenhuma fonte de energia, Caelestia ────────────────────────

#[test]
fn desktop_sem_bateria() {
    let r = rep("desktop-sem-bateria");
    assert!(!r.battery_present);
    assert!(r.supplies.is_empty());
    assert!(battery::read_at(&env("desktop-sem-bateria").sys).is_none());
    assert_eq!(r.shell.shell, Shell::Caelestia);
    assert_eq!(r.lock_plan[0], "loginctl lock-session", "{:?}", r.lock_plan);
    assert!(temp_name(&r).contains("Tdie"));
}

// ── Duas baterias do sistema + a de um rato ────────────────────────────────

#[test]
fn duas_baterias() {
    let r = rep("duas-baterias");
    let used: Vec<&str> = r
        .supplies
        .iter()
        .filter(|s| s.used)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        used,
        vec!["BAT0"],
        "a primeira do sistema; a do rato (scope=Device) fica de fora"
    );
    let b = battery::read_at(&env("duas-baterias").sys).unwrap();
    assert_eq!((b.level, b.charging, b.plugged), (40, true, true));
    assert_eq!(r.shell.shell, Shell::NoctaliaV4);
    // Noctalia v4: sem método de bloqueio confirmado → cadeia genérica.
    assert_eq!(r.lock_plan[0], "hyprlock");
}

// ── O que o daemon faz segundo as opções ───────────────────────────────────

#[test]
fn opcoes_forcam_o_ambiente() {
    let ov: Overrides = serde_json::from_str(
        r#"{"lock_command":["meu-lock"],"screenshot_tool":"grim","audio_backend":"pactl","v4l2_device_nr":10,"hypr_dispatch_mode":"classic"}"#,
    )
    .unwrap();
    let r = report::collect(&env("amd-so"), &ov);
    assert_eq!(r.lock_plan, vec!["meu-lock"]);
    assert_eq!(r.screenshot_plan, Some(ShotTool::Grim));
    assert_eq!(r.audio_backend, Some(AudioBackend::Pactl));
    assert_eq!(r.camera_device, "/dev/video10");
    assert_eq!(
        r.dispatch_mode,
        DispatchMode::Classic,
        "forçado, apesar do hyprctl Lua"
    );
    assert!(r.dispatch_mode_source.contains("forçado"));
    assert!(r.option_problems.is_empty());
}

#[test]
fn opcoes_invalidas_dao_aviso_e_voltam_ao_automatico() {
    let ov: Overrides =
        serde_json::from_str(r#"{"gpu_source":"matrox","temp_sensor":"/nao/existe"}"#).unwrap();
    let r = report::collect(&env("amd-so"), &ov);
    assert_eq!(r.option_problems.len(), 2);
    assert_eq!(r.options, Resolved::default());
    assert!(r.gpu_source.amd);
}

#[test]
fn temp_sensor_forcado_vale() {
    let e = env("amd-so");
    let path = e.sys.join("class/hwmon/hwmon4/temp1_input");
    let ov: Overrides = serde_json::from_value(serde_json::json!({ "temp_sensor": path })).unwrap();
    let r = report::collect(&e, &ov);
    assert!(r.temp_sensor.unwrap().forced);
}

#[test]
fn sem_hyprctl_assume_classico_e_diz_porque() {
    let mut e = env("amd-so");
    e.runner = Box::new(FakeRunner {
        hypr: "absent".into(),
        version: String::new(),
        pw: None,
    });
    let r = report::collect(&e, &Overrides::default());
    assert_eq!(r.dispatch_mode, DispatchMode::Classic);
    assert!(r.dispatch_mode_source.contains("assumido"));
    assert_eq!(r.classic_dispatch_accepted, "desconhecido");
}

#[test]
fn lock_e_captura_pelo_plano_partilhado() {
    // O mesmo `plan` que o daemon usa em `action.rs`.
    let e = env("amd-nvidia-hibrido");
    let have = |t: &str| e.have(t);
    let c = plan::lock_candidates(Shell::Ryoku, &have, None);
    assert_eq!(c[0][0], "ryoku-shell");
    assert_eq!(gpu::enumerate(&e.sys).len(), 2);
}

// ── Redação de dados pessoais ──────────────────────────────────────────────

const PESSOAL: &[&str] = &[
    "zeca",
    "pc-do-zeca",
    "/home/zeca",
    "3c:22:fb:01:02:ab",
    "3C:22:FB:01:02:AB",
    "192.168.1.77",
    "MinhaCasaWiFi",
];

fn assert_limpo(text: &str) {
    for p in PESSOAL {
        assert!(!text.contains(p), "o relatório ainda tem {p:?}:\n{text}");
    }
}

#[test]
fn relatorio_nao_leva_dados_pessoais() {
    // Uma «versão» com tudo o que não pode sair: caminho da casa, utilizador,
    // máquina, MAC, IP e SSID.
    let sujo = "Hyprland 0.56.2 em /home/zeca/.config (pc-do-zeca) zeca 3c:22:fb:01:02:ab 192.168.1.77 MinhaCasaWiFi built from branch x\n";
    let e = env_with_version("amd-so", sujo);
    // E um lock_command com o caminho da casa.
    let ov: Overrides = serde_json::from_str(
        r#"{"lock_command":["/home/zeca/bin/bloquear","--rede","MinhaCasaWiFi","3C:22:FB:01:02:AB"]}"#,
    )
    .unwrap();
    let r = report::collect(&e, &ov);
    // O SSID só entra porque o pusemos à mão; a redação conhece utilizador,
    // máquina, casa, MAC e IP (o SSID nunca é recolhido, por isso não se limpa).
    let text = report::text_redacted(&e, &r).replace("MinhaCasaWiFi", "");
    let json = report::json_redacted(&e, &r).replace("MinhaCasaWiFi", "");
    assert_limpo(&text);
    assert_limpo(&json);
    assert!(text.contains("~/bin/bloquear"), "a casa vira ~:\n{text}");
}

#[test]
fn relatorio_normal_nao_tem_nada_pessoal_nem_o_ssid() {
    // Nada do que o `doctor` recolhe de propósito inclui isto.
    for p in [
        "amd-so",
        "intel-so",
        "amd-nvidia-hibrido",
        "nvidia-desktop",
        "desktop-sem-bateria",
        "duas-baterias",
    ] {
        let e = env(p);
        let r = report::collect(&e, &Overrides::default());
        assert_limpo(&report::text_redacted(&e, &r));
        assert_limpo(&report::json_redacted(&e, &r));
    }
}

// ── EasyEffects e o retorno de áudio (perfis `ee-*`, grafos `pw-dump`) ──────

use hyprlink_env::overrides::TapChoice;
use hyprlink_env::pipewire;

const ALSA: &str = "alsa_output.pci-0000_03_00.6.analog-stereo";
const BT: &str = "bluez_output.AA_BB_CC_01_02_03.1";

/// O ambiente `amd-so` com o `pw-dump`, o socket (`bypass.txt`) e o
/// `easyeffectsrc` (se houver) do perfil `ee`.
fn env_ee(ee: &str) -> Env {
    let mut e = env("amd-so");
    let d = dir(ee);
    let pw = std::fs::read_to_string(d.join("pw-dump.json")).unwrap();
    e.runner = Box::new(FakeRunner {
        hypr: "lua".into(),
        version: "Hyprland 0.56.2 built from branch x\n".into(),
        pw: Some(pw),
    });
    let bypass = std::fs::read_to_string(d.join("bypass.txt")).ok();
    e.sock_query = Box::new(move |name, cmd| {
        assert_eq!(
            (name, cmd),
            ("EasyEffectsServer", "get_global_bypass"),
            "só leitura"
        );
        bypass.clone().map(|b| b.trim().to_string())
    });
    // XDG_CONFIG_HOME temporário com o easyeffectsrc do perfil (ou vazio).
    let cfg = std::env::temp_dir().join(format!("hyprlink-ee-{ee}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cfg);
    if let Ok(rc) = std::fs::read_to_string(d.join("easyeffectsrc")) {
        std::fs::create_dir_all(cfg.join("easyeffects/db")).unwrap();
        std::fs::write(cfg.join("easyeffects/db/easyeffectsrc"), rc).unwrap();
    } else {
        std::fs::create_dir_all(&cfg).unwrap();
    }
    e.vars
        .insert("XDG_CONFIG_HOME".into(), cfg.to_string_lossy().into_owned());
    e
}

fn tap(ee: &str, choice: &str) -> Report {
    let ov: Overrides =
        serde_json::from_value(serde_json::json!({ "tap_source": choice })).unwrap();
    report::collect(&env_ee(ee), &ov)
}

fn tap_sink(r: &Report) -> String {
    r.tap_target
        .as_ref()
        .map(|t| t.sink.clone())
        .unwrap_or_default()
}

#[test]
fn ee_predefinido_o_caso_do_maggio() {
    let r = tap("ee-predefinido", "auto");
    assert!(r.pipewire_graph);
    let ee = r.easyeffects.as_ref().unwrap();
    assert!(ee.running && ee.default_is_ee_sink);
    assert_eq!(ee.destination.as_ref().unwrap().sink, ALSA);
    assert_eq!(ee.bypass, Some(false));
    assert!(ee.config.is_none());
    assert_eq!(tap_sink(&r), pipewire::EE_SINK, "som sem efeitos");
    assert_eq!(tap_sink(&tap("ee-predefinido", "easyeffects_post")), ALSA);
    assert_eq!(
        tap_sink(&tap("ee-predefinido", "easyeffects_pre")),
        pipewire::EE_SINK
    );
    assert_eq!(
        tap_sink(&tap("ee-predefinido", "default")),
        pipewire::EE_SINK
    );
    assert!(r.speaker_warning.is_none());
}

#[test]
fn ee_saida_diferente_cada_valor_de_tap_source() {
    let r = tap("ee-saida-diferente", "auto");
    let ee = r.easyeffects.as_ref().unwrap();
    assert!(!ee.default_is_ee_sink);
    assert_eq!(ee.config.as_ref().unwrap().use_default_output, Some(false));
    assert_eq!(tap_sink(&r), BT, "auto segue o sink real de destino");
    assert_eq!(
        tap_sink(&tap("ee-saida-diferente", "default")),
        ALSA,
        "default ficaria mudo"
    );
    assert_eq!(tap_sink(&tap("ee-saida-diferente", "easyeffects_post")), BT);
    assert_eq!(
        tap_sink(&tap("ee-saida-diferente", "easyeffects_pre")),
        pipewire::EE_SINK
    );
}

#[test]
fn ee_bypass_usa_a_saida_predefinida() {
    let r = tap("ee-bypass", "auto");
    assert_eq!(r.easyeffects.as_ref().unwrap().bypass, Some(true));
    assert!(r.tap_target.as_ref().unwrap().why.contains("bypass"));
}

#[test]
fn ee_ausente_e_ocioso() {
    let r = tap("sem-ee", "auto");
    assert!(!r.easyeffects.as_ref().unwrap().running);
    assert_eq!(tap_sink(&r), ALSA);
    let r = tap("sem-ee", "easyeffects_pre");
    assert!(r.tap_target.is_none() && r.tap_error.as_ref().unwrap().contains("EasyEffects"));
    let r = tap("ee-ocioso", "easyeffects_post");
    assert_eq!(
        tap_sink(&r),
        ALSA,
        "sem ligações: o sink real de maior prioridade"
    );
}

#[test]
fn modo_coluna_avisa_quando_o_som_nao_vai_para_a_coluna() {
    assert!(tap("ee-coluna-ok", "auto").speaker_warning.is_none());
    let w = tap("ee-coluna-desviada", "auto").speaker_warning.unwrap();
    assert!(w.contains(ALSA) && w.contains("EasyEffects"), "{w}");
    let w = tap("coluna-desviada-sem-ee", "auto")
        .speaker_warning
        .unwrap();
    assert!(w.contains(ALSA), "{w}");
}

#[test]
fn vigia_deteta_a_mudanca_de_destino() {
    // A predefinida não muda (alsa) e o som muda de sítio: o alvo `auto` muda.
    let antes = tap("ee-predefinido", "easyeffects_post");
    let depois = tap("ee-saida-diferente", "easyeffects_post");
    assert_ne!(tap_sink(&antes), tap_sink(&depois));
    // O EasyEffects fecha: o easyeffects_sink desaparece e `auto` volta à predefinida.
    let fechou = tap("sem-ee", "auto");
    assert_ne!(tap_sink(&tap("ee-predefinido", "auto")), tap_sink(&fechou));
    // Entra em bypass com a mesma predefinida: o motivo muda, o vigia compara o sink.
    assert_eq!(
        tap_sink(&tap("ee-bypass", "auto")),
        tap_sink(&tap("ee-predefinido", "auto"))
    );
}

#[test]
fn doctor_com_easyeffects_nao_leva_o_mac_do_bluetooth() {
    let e = env_ee("ee-saida-diferente");
    let r = report::collect(&e, &Overrides::default());
    let text = report::text_redacted(&e, &r);
    let json = report::json_redacted(&e, &r);
    assert!(text.contains("EasyEffects"), "{text}");
    assert!(
        text.contains("bluez_output.<mac>.1"),
        "o MAC do sink Bluetooth vira <mac>:\n{text}"
    );
    for out in [&text, &json] {
        assert!(
            !out.contains("AA_BB_CC_01_02_03"),
            "MAC no relatório:\n{out}"
        );
        assert_limpo(out);
    }
}

#[test]
fn tap_source_invalido_da_aviso() {
    let ov: Overrides = serde_json::from_str(r#"{"tap_source":"gravar"}"#).unwrap();
    let r = report::collect(&env_ee("ee-predefinido"), &ov);
    assert_eq!(r.options.tap_source, TapChoice::Auto);
    assert!(r.option_problems.iter().any(|p| p.contains("tap_source")));
}
