//! Opções de correção do `config.json`: deixam forçar o ambiente sem esperar
//! por uma versão nova. São lidas **sem rigidez** (um valor mal escrito não
//! pode deitar fora a configuração toda) e validadas à parte: o que for
//! inválido volta ao automático e fica registado como problema.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Os campos tal como estão no `config.json` (todos opcionais).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Overrides {
    /// Lista de argumentos que bloqueia a sessão.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_command: Option<Value>,
    /// `auto|grim|grimblast`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_tool: Option<Value>,
    /// Caminho de um `…_input` (ou de uma zona térmica `temp`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_sensor: Option<Value>,
    /// `auto|amd|nvidia|intel|none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_source: Option<Value>,
    /// Número do `/dev/videoN` do v4l2loopback (por omissão 42).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v4l2_device_nr: Option<Value>,
    /// `auto|wpctl|pactl`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_backend: Option<Value>,
    /// `auto|classic|lua`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypr_dispatch_mode: Option<Value>,
    /// `auto|default|easyeffects_pre|easyeffects_post`: o que o retorno de
    /// áudio (tap) captura.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap_source: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotChoice {
    Auto,
    Grim,
    Grimblast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuChoice {
    Auto,
    Amd,
    Nvidia,
    Intel,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioChoice {
    Auto,
    Wpctl,
    Pactl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchChoice {
    Auto,
    Classic,
    Lua,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TapChoice {
    Auto,
    Default,
    EasyeffectsPre,
    EasyeffectsPost,
}

pub const V4L2_DEFAULT_NR: u32 = 42;

/// Valores validados, prontos a usar.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    pub lock_command: Option<Vec<String>>,
    pub screenshot_tool: ScreenshotChoice,
    pub temp_sensor: Option<PathBuf>,
    pub gpu_source: GpuChoice,
    pub v4l2_device_nr: u32,
    pub audio_backend: AudioChoice,
    pub hypr_dispatch_mode: DispatchChoice,
    pub tap_source: TapChoice,
}

impl Default for Resolved {
    fn default() -> Self {
        Self {
            lock_command: None,
            screenshot_tool: ScreenshotChoice::Auto,
            temp_sensor: None,
            gpu_source: GpuChoice::Auto,
            v4l2_device_nr: V4L2_DEFAULT_NR,
            audio_backend: AudioChoice::Auto,
            hypr_dispatch_mode: DispatchChoice::Auto,
            tap_source: TapChoice::Auto,
        }
    }
}

/// Lê o `config.json` (só estes campos) de `path`; ficheiro ausente ou
/// ilegível → tudo por omissão.
pub fn load(path: &Path) -> Overrides {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.trim().to_ascii_lowercase())
}

impl Overrides {
    /// Valida. `sys_ok` diz se um caminho de sensor existe e lê bem. Devolve os
    /// valores e a lista de problemas (frases para o Diário/`doctor`); um
    /// valor inválido fica no automático.
    pub fn resolve(&self, sensor_ok: &dyn Fn(&Path) -> bool) -> (Resolved, Vec<String>) {
        let mut r = Resolved::default();
        let mut bad = Vec::new();

        if let Some(v) = &self.lock_command {
            match v.as_array().map(|a| {
                a.iter()
                    .map(|x| x.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
            }) {
                Some(Some(argv)) if !argv.is_empty() && !argv[0].trim().is_empty() => {
                    r.lock_command = Some(argv)
                }
                _ => bad.push(
                    "lock_command: tem de ser uma lista de textos não vazia (ex.: [\"loginctl\", \"lock-session\"])"
                        .into(),
                ),
            }
        }
        if let Some(v) = &self.screenshot_tool {
            match text(v).as_deref() {
                Some("auto") => {}
                Some("grim") => r.screenshot_tool = ScreenshotChoice::Grim,
                Some("grimblast") => r.screenshot_tool = ScreenshotChoice::Grimblast,
                _ => bad.push(
                    "screenshot_tool: valor desconhecido (suportados: auto, grim, grimblast)"
                        .into(),
                ),
            }
        }
        if let Some(v) = &self.temp_sensor {
            match v.as_str().map(PathBuf::from) {
                Some(p) if p.is_absolute() && sensor_ok(&p) => r.temp_sensor = Some(p),
                _ => bad.push(
                    "temp_sensor: o caminho não existe ou não dá uma temperatura plausível".into(),
                ),
            }
        }
        if let Some(v) = &self.gpu_source {
            match text(v).as_deref() {
                Some("auto") => {}
                Some("amd") => r.gpu_source = GpuChoice::Amd,
                Some("nvidia") => r.gpu_source = GpuChoice::Nvidia,
                Some("intel") => r.gpu_source = GpuChoice::Intel,
                Some("none") => r.gpu_source = GpuChoice::None,
                _ => bad.push(
                    "gpu_source: valor desconhecido (suportados: auto, amd, nvidia, intel, none)"
                        .into(),
                ),
            }
        }
        if let Some(v) = &self.v4l2_device_nr {
            match v.as_u64().filter(|n| (1..=255).contains(n)) {
                Some(n) => r.v4l2_device_nr = n as u32,
                None => bad.push("v4l2_device_nr: tem de ser um inteiro entre 1 e 255".into()),
            }
        }
        if let Some(v) = &self.audio_backend {
            match text(v).as_deref() {
                Some("auto") => {}
                Some("wpctl") => r.audio_backend = AudioChoice::Wpctl,
                Some("pactl") => r.audio_backend = AudioChoice::Pactl,
                _ => bad.push(
                    "audio_backend: valor desconhecido (suportados: auto, wpctl, pactl)".into(),
                ),
            }
        }
        if let Some(v) = &self.hypr_dispatch_mode {
            match text(v).as_deref() {
                Some("auto") => {}
                Some("classic") => r.hypr_dispatch_mode = DispatchChoice::Classic,
                Some("lua") => r.hypr_dispatch_mode = DispatchChoice::Lua,
                _ => bad.push(
                    "hypr_dispatch_mode: valor desconhecido (suportados: auto, classic, lua)"
                        .into(),
                ),
            }
        }
        if let Some(v) = &self.tap_source {
            match text(v).as_deref() {
                Some("auto") => {}
                Some("default") => r.tap_source = TapChoice::Default,
                Some("easyeffects_pre") => r.tap_source = TapChoice::EasyeffectsPre,
                Some("easyeffects_post") => r.tap_source = TapChoice::EasyeffectsPost,
                _ => bad.push(
                    "tap_source: valor desconhecido (suportados: auto, default, easyeffects_pre, easyeffects_post)"
                        .into(),
                ),
            }
        }
        (r, bad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ov(v: Value) -> Overrides {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn vazio_e_tudo_automatico() {
        let (r, bad) = Overrides::default().resolve(&|_| false);
        assert_eq!(r, Resolved::default());
        assert!(bad.is_empty());
        assert_eq!(r.v4l2_device_nr, 42);
    }

    #[test]
    fn valores_validos() {
        let o = ov(json!({
            "lock_command": ["loginctl", "lock-session"],
            "screenshot_tool": "GRIM",
            "temp_sensor": "/sys/class/hwmon/hwmon3/temp1_input",
            "gpu_source": "intel",
            "v4l2_device_nr": 10,
            "audio_backend": "pactl",
            "hypr_dispatch_mode": "lua",
            "tap_source": "easyeffects_post",
        }));
        let (r, bad) = o.resolve(&|_| true);
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(r.lock_command.as_deref().unwrap()[0], "loginctl");
        assert_eq!(r.screenshot_tool, ScreenshotChoice::Grim);
        assert_eq!(r.gpu_source, GpuChoice::Intel);
        assert_eq!(r.v4l2_device_nr, 10);
        assert_eq!(r.audio_backend, AudioChoice::Pactl);
        assert_eq!(r.hypr_dispatch_mode, DispatchChoice::Lua);
        assert_eq!(r.tap_source, TapChoice::EasyeffectsPost);
    }

    #[test]
    fn invalidos_voltam_ao_automatico_e_dao_problemas() {
        let o = ov(json!({
            "lock_command": "loginctl lock-session",
            "screenshot_tool": "hyprshot",
            "temp_sensor": "/nao/existe",
            "gpu_source": "matrox",
            "v4l2_device_nr": 0,
            "audio_backend": 3,
            "hypr_dispatch_mode": "x",
            "tap_source": "pre",
        }));
        let (r, bad) = o.resolve(&|_| false);
        assert_eq!(r, Resolved::default());
        assert_eq!(bad.len(), 8, "{bad:?}");
    }

    #[test]
    fn temp_sensor_relativo_e_recusado() {
        let o = ov(json!({"temp_sensor": "hwmon0/temp1_input"}));
        assert_eq!(o.resolve(&|_| true).1.len(), 1);
    }

    #[test]
    fn lock_command_com_elemento_nao_texto_e_recusado() {
        let o = ov(json!({"lock_command": ["a", 1]}));
        assert_eq!(o.resolve(&|_| true).1.len(), 1);
        let o = ov(json!({"lock_command": []}));
        assert_eq!(o.resolve(&|_| true).1.len(), 1);
    }

    #[test]
    fn config_com_outros_campos_nao_estraga_a_leitura() {
        let o: Overrides =
            serde_json::from_str(r#"{"download_dir":"/x","shortcuts":[],"gpu_source":"amd"}"#)
                .unwrap();
        assert_eq!(o.gpu_source, Some(json!("amd")));
    }
}
