//! Configuração persistente do daemon (por ora só a pasta de destino dos
//! ficheiros recebidos) — `~/.config/hyprlink/config.json`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hyprlink_proto::link::{GestureRule, default_gesture_rules};
use serde::{Deserialize, Serialize};

pub type SharedConfig = Arc<Mutex<AppConfig>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shortcut {
    /// Identificação estável (8 hex), gerada ao criar o atalho: é o que o
    /// telemóvel envia para o executar. Config antiga sem `id`: migra no
    /// arranque (`ensure_shortcut_ids`).
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// Mesmo formato aceito pelo campo livre do CONTROL: "workspace 2",
    /// "focuswindow address:0x..", etc — vai direto pra `hypr::dispatch`.
    pub command: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TrackSettings {
    #[serde(default = "default_sensitivity")]
    pub sensitivity: f32,
    #[serde(default = "default_scroll_speed")]
    pub scroll_speed: f32,
    /// ponytail: multiplicador linear fixo quando ativo, não uma curva de
    /// aceleração de verdade — suficiente pra "sente mais rápido puxando
    /// forte", revisitar se um dia precisar de algo mais fino.
    #[serde(default)]
    pub acceleration: bool,
    #[serde(default)]
    pub invert_scroll: bool,
    #[serde(default = "default_true")]
    pub virtual_keyboard: bool,
}

fn default_sensitivity() -> f32 {
    1.0
}
fn default_scroll_speed() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}

impl Default for TrackSettings {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            scroll_speed: 1.0,
            acceleration: false,
            invert_scroll: false,
            virtual_keyboard: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Lang {
    PtPt,
    PtBr,
    EnGb,
    EsEs,
    Zh,
}

fn default_lang() -> Lang {
    Lang::PtPt
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BatteryAlerts {
    #[serde(default)]
    pub low: bool,
    #[serde(default)]
    pub full: bool,
    /// Limiar do aviso de bateria baixa (%).
    #[serde(default = "default_low_pct")]
    pub low_pct: u8,
}

fn default_low_pct() -> u8 {
    20
}

impl Default for BatteryAlerts {
    fn default() -> Self {
        Self {
            low: false,
            full: false,
            low_pct: default_low_pct(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub download_dir: PathBuf,
    /// Minimizar pra bandeja usa uma workspace especial do Hyprland (ver
    /// `hypr.rs::tray_hide/tray_show`) — não é Wayland genérico, então dá
    /// pra desligar se causar problema noutro compositor/config.
    #[serde(default = "default_tray_special_workspace")]
    pub tray_special_workspace: bool,
    /// Atalhos do módulo CONTROL (nome → comando `hyprctl dispatch`).
    #[serde(default)]
    pub shortcuts: Vec<Shortcut>,
    #[serde(default)]
    pub battery_alerts: BatteryAlerts,
    #[serde(default)]
    pub track: TrackSettings,
    #[serde(default = "default_lang")]
    pub lang: Lang,
    /// Sink padrão de antes de o modo coluna (`speaker.rs`) ser ligado —
    /// persistido ANTES de qualquer mudança pra que um crash do daemon não
    /// deixe o PC sem som: o arranque seguinte (`speaker::cleanup_orphans`)
    /// devolve o default a este dono e limpa o campo. `None` = modo coluna
    /// desligado (ou nunca usado).
    #[serde(default)]
    pub speaker_prev_sink: Option<String>,
    /// Se antes do modo coluna o predefinido era o `easyeffects_sink`: o sink
    /// real onde o EasyEffects tocava. Serve para o devolver ao desligar (o
    /// EasyEffects ignora a mudança do predefinido para o seu próprio sink).
    #[serde(default)]
    pub speaker_prev_ee_dest: Option<String>,
    /// Ponto cujo sistema de ficheiros alimenta `disk_free_b`/`disk_total_b`
    /// do `pc.status`. Ausente = a pasta pessoal.
    #[serde(default)]
    pub disk_path: Option<PathBuf>,
    /// Consultar o `nvidia-smi` (carga da GPU NVIDIA em `pc.status`) mesmo com
    /// a placa em repouso. Por omissão `false`: só se consulta com a placa já
    /// acordada, porque o `nvidia-smi` tira uma GPU dedicada do repouso e
    /// gasta bateria num portátil híbrido. Sem efeito em AMD/Intel.
    #[serde(default)]
    pub gpu_nvidia_wake: bool,
    /// Gestos do telemóvel (`gesture {name}`): ligado/desligado e a ação.
    /// Só o que difere da origem precisa de estar aqui; `gesture_rules`
    /// junta-o às regras de origem.
    #[serde(default)]
    pub gestures: Vec<GestureRule>,
    /// Opções de correção do ambiente (`lock_command`, `screenshot_tool`,
    /// `temp_sensor`, `gpu_source`, `v4l2_device_nr`, `audio_backend`,
    /// `hypr_dispatch_mode`): ver `hyprlink_env::overrides`. Lidas sem rigidez
    /// e validadas à parte.
    #[serde(flatten)]
    pub env: hyprlink_env::Overrides,
}

fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
        .join("HyprLink")
}

fn default_tray_special_workspace() -> bool {
    true
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .expect("sem diretório de config do usuário (XDG_CONFIG_HOME/HOME)")
        .join("hyprlink")
        .join("config.json")
}

impl AppConfig {
    /// Muda `on` de um gesto (materializa as regras de origem). `false` se o
    /// nome não existe. Não grava.
    fn set_gesture(&mut self, name: &str, on: bool) -> bool {
        let mut rules = merge_gestures(&self.gestures);
        let Some(r) = rules.iter_mut().find(|r| r.name == name) else {
            return false;
        };
        r.on = on;
        self.gestures = rules;
        true
    }

    fn defaults() -> Self {
        Self {
            download_dir: default_download_dir(),
            tray_special_workspace: default_tray_special_workspace(),
            shortcuts: Vec::new(),
            battery_alerts: BatteryAlerts::default(),
            track: TrackSettings::default(),
            lang: default_lang(),
            speaker_prev_sink: None,
            speaker_prev_ee_dest: None,
            disk_path: None,
            gpu_nvidia_wake: false,
            gestures: Vec::new(),
            env: hyprlink_env::Overrides::default(),
        }
    }

    fn load_from_disk() -> Self {
        let mut cfg: Self = std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(Self::defaults);
        // Migração: atalhos sem `id` (config anterior) passam a ter um, sem
        // perder nenhum nem mudar os que já têm; grava-se logo.
        if ensure_shortcut_ids(&mut cfg.shortcuts) {
            cfg.save();
        }
        cfg
    }

    fn save(&self) {
        // Os testes nunca escrevem no config.json real do utilizador.
        if cfg!(test) {
            return;
        }
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Config em memória, sem tocar no disco (só testes: nunca chamar `save`).
#[cfg(test)]
pub fn test_config() -> SharedConfig {
    Arc::new(Mutex::new(AppConfig::defaults()))
}

pub fn load() -> SharedConfig {
    Arc::new(Mutex::new(AppConfig::load_from_disk()))
}

pub fn download_dir(config: &SharedConfig) -> PathBuf {
    config.lock().unwrap().download_dir.clone()
}

pub fn set_download_dir(config: &SharedConfig, dir: &Path) {
    let mut c = config.lock().unwrap();
    c.download_dir = dir.to_path_buf();
    c.save();
}

/// As opções de correção do ambiente tal como estão no `config.json`.
pub fn env_overrides(config: &SharedConfig) -> hyprlink_env::Overrides {
    config.lock().unwrap().env.clone()
}

pub fn shortcuts(config: &SharedConfig) -> Vec<Shortcut> {
    config.lock().unwrap().shortcuts.clone()
}

pub fn battery_alerts(config: &SharedConfig) -> BatteryAlerts {
    config.lock().unwrap().battery_alerts
}

pub fn track_settings(config: &SharedConfig) -> TrackSettings {
    config.lock().unwrap().track
}

pub fn gpu_nvidia_wake(config: &SharedConfig) -> bool {
    config.lock().unwrap().gpu_nvidia_wake
}

pub fn disk_path(config: &SharedConfig) -> Option<PathBuf> {
    config.lock().unwrap().disk_path.clone()
}

/// Sink a restaurar quando o modo coluna desligar (ver `speaker.rs`).
/// As regras de origem com o que o utilizador guardou por cima (ligado e
/// ação, por nome). Há sempre as cinco, pela ordem de `GESTURES`; nomes
/// desconhecidos guardados (de uma versão futura) ficam no fim.
pub fn gesture_rules(config: &SharedConfig) -> Vec<GestureRule> {
    merge_gestures(&config.lock().unwrap().gestures)
}

fn merge_gestures(stored: &[GestureRule]) -> Vec<GestureRule> {
    let mut rules = default_gesture_rules();
    for s in stored {
        match rules.iter_mut().find(|r| r.name == s.name) {
            Some(r) => {
                r.on = s.on;
                if !s.action.trim().is_empty() {
                    r.action = s.action.clone();
                }
            }
            None => rules.push(s.clone()),
        }
    }
    rules
}

/// Liga/desliga um gesto e guarda. `false` se o nome não existe.
pub fn set_gesture_on(config: &SharedConfig, name: &str, on: bool) -> bool {
    let mut c = config.lock().unwrap();
    let found = c.set_gesture(name, on);
    if found {
        c.save();
    }
    found
}

pub fn speaker_prev_sink(config: &SharedConfig) -> Option<String> {
    config.lock().unwrap().speaker_prev_sink.clone()
}

/// `None` limpa (modo coluna desligado de forma limpa).
pub fn speaker_prev_ee_dest(config: &SharedConfig) -> Option<String> {
    config.lock().unwrap().speaker_prev_ee_dest.clone()
}

pub fn set_speaker_prev_ee_dest(config: &SharedConfig, sink: Option<&str>) {
    let mut c = config.lock().unwrap();
    c.speaker_prev_ee_dest = sink.map(Into::into);
    c.save();
}

pub fn set_speaker_prev_sink(config: &SharedConfig, sink: Option<&str>) {
    let mut c = config.lock().unwrap();
    c.speaker_prev_sink = sink.map(Into::into);
    c.save();
}

/// Um id novo (8 hex) que não esteja em `taken`.
pub fn new_shortcut_id(taken: &[String]) -> String {
    loop {
        let id = format!("{:08x}", rand::random::<u32>());
        if !taken.contains(&id) {
            return id;
        }
    }
}

/// Dá id aos atalhos que não têm (config antiga, ou atalho novo da GUI) e
/// troca os repetidos. Os ids que já existem e são únicos **não mudam**.
/// `true` se alterou alguma coisa.
pub fn ensure_shortcut_ids(list: &mut [Shortcut]) -> bool {
    let mut changed = false;
    let mut taken: Vec<String> = Vec::new();
    for sc in list.iter_mut() {
        let id = sc.id.trim().to_string();
        let ok = !id.is_empty() && id.len() <= 64 && !taken.contains(&id);
        if ok {
            if id != sc.id {
                sc.id = id.clone();
                changed = true;
            }
            taken.push(id);
        } else {
            let fresh = new_shortcut_id(&taken);
            taken.push(fresh.clone());
            sc.id = fresh;
            changed = true;
        }
    }
    changed
}

/// O atalho com este `id` (só os que existem na config).
pub fn shortcut_by_id(config: &SharedConfig, id: &str) -> Option<Shortcut> {
    config
        .lock()
        .unwrap()
        .shortcuts
        .iter()
        .find(|s| s.id == id)
        .cloned()
}

/// Valida e normaliza (aparar espaços) uma lista de atalhos vinda da GUI.
/// O erro é a frase para o Diário.
pub fn validate_shortcuts(list: Vec<Shortcut>) -> Result<Vec<Shortcut>, &'static str> {
    use hyprlink_proto::link::{SHORTCUT_COMMAND_MAX, SHORTCUT_NAME_MAX, SHORTCUTS_MAX};
    if list.len() > SHORTCUTS_MAX {
        return Err("demasiados atalhos");
    }
    list.into_iter()
        .map(|s| {
            let name = s.name.trim().to_string();
            let command = s.command.trim().to_string();
            if name.is_empty() {
                Err("nome vazio")
            } else if command.is_empty() {
                Err("comando vazio")
            } else if name.chars().count() > SHORTCUT_NAME_MAX {
                Err("nome demasiado comprido")
            } else if command.chars().count() > SHORTCUT_COMMAND_MAX {
                Err("comando demasiado comprido")
            } else {
                Ok(Shortcut {
                    id: s.id,
                    name,
                    command,
                })
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|mut v| {
            ensure_shortcut_ids(&mut v);
            v
        })
}

pub fn set_shortcuts(config: &SharedConfig, shortcuts: Vec<Shortcut>) {
    let mut c = config.lock().unwrap();
    c.shortcuts = shortcuts;
    c.save();
}

pub fn set_battery_alerts(config: &SharedConfig, alerts: BatteryAlerts) {
    let mut c = config.lock().unwrap();
    c.battery_alerts = alerts;
    c.save();
}

pub fn set_track_settings(config: &SharedConfig, track: TrackSettings) {
    let mut c = config.lock().unwrap();
    c.track = track;
    c.save();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc(name: &str, command: &str) -> Shortcut {
        Shortcut {
            id: String::new(),
            name: name.into(),
            command: command.into(),
        }
    }

    #[test]
    fn atalhos_validos_sao_aparados() {
        let ok = validate_shortcuts(vec![sc("  Terminal ", " exec kitty\n")]).unwrap();
        assert_eq!(
            (ok[0].name.as_str(), ok[0].command.as_str()),
            ("Terminal", "exec kitty")
        );
        assert!(
            validate_shortcuts(vec![]).unwrap().is_empty(),
            "esvaziar a lista é válido"
        );
    }

    #[test]
    fn atalhos_invalidos_sao_recusados() {
        assert!(validate_shortcuts(vec![sc("  ", "exec kitty")]).is_err());
        assert!(validate_shortcuts(vec![sc("Terminal", " ")]).is_err());
        assert!(validate_shortcuts(vec![sc(&"n".repeat(41), "x")]).is_err());
        assert!(validate_shortcuts(vec![sc(&"é".repeat(40), "x")]).is_ok());
        assert!(validate_shortcuts(vec![sc("a", &"c".repeat(201))]).is_err());
        let muitos = (0..33).map(|i| sc(&format!("a{i}"), "x")).collect();
        assert!(validate_shortcuts(muitos).is_err());
        // Um inválido recusa a lista toda.
        assert!(validate_shortcuts(vec![sc("ok", "x"), sc("", "x")]).is_err());
    }

    fn cfg(json: &str) -> AppConfig {
        serde_json::from_str(json).expect("config")
    }

    #[test]
    fn nvidia_wake_vem_desligado_e_aceita_ser_ligado() {
        assert!(!cfg(r#"{"download_dir": "/x"}"#).gpu_nvidia_wake);
        assert!(cfg(r#"{"download_dir": "/x", "gpu_nvidia_wake": true}"#).gpu_nvidia_wake);
    }

    #[test]
    fn config_antiga_sem_gestos_da_as_regras_de_origem() {
        let c = cfg(r#"{"download_dir": "/x"}"#);
        assert!(c.gestures.is_empty());
        assert_eq!(merge_gestures(&c.gestures), default_gesture_rules());
    }

    #[test]
    fn ligar_e_desligar_persiste_so_o_que_muda_e_sobrevive_a_recarregar() {
        let mut c = cfg(r#"{"download_dir": "/x"}"#);
        // `rotate_landscape` vem desligado de origem.
        assert!(c.set_gesture("rotate_landscape", true));
        assert!(c.set_gesture("swipe_left_3", false));
        assert!(!c.set_gesture("nao_existe", true));

        // Grava e relê, como o `save`/`load_from_disk`.
        let json = serde_json::to_string_pretty(&c).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        let rules = merge_gestures(&back.gestures);
        let on = |n: &str| rules.iter().find(|r| r.name == n).unwrap().on;
        assert!(on("rotate_landscape"));
        assert!(!on("swipe_left_3"));
        assert!(
            on("swipe_right_3") && on("volume"),
            "o resto fica como estava"
        );
        assert_eq!(rules.len(), 5);
    }

    #[test]
    fn ordem_fixa_acao_editada_e_nomes_desconhecidos() {
        // Guardado fora de ordem, com uma ação editada à mão e um nome futuro.
        let c = cfg(r#"{"download_dir": "/x", "gestures": [
                {"name": "volume", "on": false, "action": "pc:volume_{dir}"},
                {"name": "swipe_left_3", "on": true, "action": "workspace 1"},
                {"name": "futuro", "on": true, "action": "exec true"},
                {"name": "double_tap_back", "on": true, "action": "  "}
            ]}"#);
        let r = merge_gestures(&c.gestures);
        let names: Vec<&str> = r.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "swipe_left_3",
                "swipe_right_3",
                "double_tap_back",
                "rotate_landscape",
                "volume",
                "futuro"
            ]
        );
        assert_eq!(r[0].action, "workspace 1", "ação editada respeitada");
        assert!(!r[4].on);
        // Ação vazia não apaga a de origem.
        assert_eq!(r[2].action, "togglespecialworkspace");
    }
}
