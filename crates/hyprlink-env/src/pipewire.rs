//! O grafo do PipeWire (`pw-dump`) e o estado do EasyEffects, lidos do grafo
//! real e não da configuração.
//!
//! Factos (código do EasyEffects em `TRAY/easyeffects/src`, lido):
//! - cria os nós virtuais `easyeffects_sink` e `easyeffects_source`
//!   (`priority.session=0`, `node.group=ee_sink_group`) e filtros `ee_*`
//!   (`ee_soe_*` no caminho de saída: `easyeffects_sink → ee_soe_spectrum →
//!   ee_soe_output_level → sink real`; `pw_node_manager.cpp`);
//! - move cada stream de saída para o `easyeffects_sink` por metadados
//!   (`target.object`, `pw_manager.cpp`) e toca no dispositivo de saída: o
//!   predefinido (`useDefaultOutputDevice`, por omissão ligado) ou o escolhido
//!   (`outputDevice`), no grupo `StreamOutputs` de
//!   `~/.config/easyeffects/db/easyeffectsrc`;
//! - ignora a mudança do `default.audio.sink` para o `easyeffects_sink`
//!   (`pw_metadata_manager.cpp`), ignora streams com `target.object` explícito
//!   para outro dispositivo e, por omissão (`excludeMonitorStreams`), streams
//!   com `stream.capture.sink=true` — por isso o tap mantém as duas coisas;
//! - sem actividade, desliga os filtros (`Inactivity Timeout`,
//!   `docs/user_interface/general.md`): ocioso, o grafo não tem ligações dos
//!   `ee_*` para o sink real;
//! - socket `$XDG_RUNTIME_DIR/EasyEffectsServer`: `get_global_bypass` devolve
//!   `1` (ligado) ou `2` (desligado) (`docs/user_interface/local_server.md`).

use std::collections::VecDeque;

use serde::Serialize;
use serde_json::Value;

use crate::Env;
use crate::overrides::TapChoice;

pub const EE_SINK: &str = "easyeffects_sink";
pub const SPEAKER_SINK: &str = "hyprlink-speaker";

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: u64,
    pub name: String,
    pub class: Option<String>,
    pub group: Option<String>,
    pub priority: Option<i64>,
    /// `running`, `idle`, `suspended`…
    pub state: Option<String>,
}

impl Node {
    fn is_sink(&self) -> bool {
        self.class.as_deref() == Some("Audio/Sink")
    }

    fn is_output_stream(&self) -> bool {
        self.class.as_deref() == Some("Stream/Output/Audio")
    }

    /// Nó interno do EasyEffects (o sink virtual, a fonte virtual e os `ee_*`).
    fn is_ee(&self) -> bool {
        self.name.starts_with("ee_")
            || self.name == EE_SINK
            || self.name == "easyeffects_source"
            || self.group.as_deref().is_some_and(|g| g.starts_with("ee_"))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    /// (nó de saída, nó de entrada) das ligações.
    pub links: Vec<(u64, u64)>,
    /// `default.audio.sink`.
    pub default_sink: Option<String>,
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

impl Graph {
    /// Lê a saída de `pw-dump`. Tolerante: o que não se reconhece salta-se.
    pub fn parse(json: &str) -> Option<Graph> {
        let all: Value = serde_json::from_str(json).ok()?;
        let mut g = Graph::default();
        for o in all.as_array()? {
            let ty = str_of(o, "type").unwrap_or("");
            if ty.ends_with("Node") {
                let info = o.get("info")?;
                let props = info.get("props")?;
                let Some(name) = str_of(props, "node.name") else {
                    continue;
                };
                g.nodes.push(Node {
                    id: o.get("id")?.as_u64()?,
                    name: name.to_string(),
                    class: str_of(props, "media.class").map(str::to_string),
                    group: str_of(props, "node.group").map(str::to_string),
                    priority: props.get("priority.session").and_then(Value::as_i64),
                    state: str_of(info, "state").map(str::to_string),
                });
            } else if ty.ends_with("Link") {
                let info = o.get("info")?;
                // Só ligações em uso (`active` ou a negociar), não `error`.
                if str_of(info, "state") == Some("error") {
                    continue;
                }
                if let (Some(a), Some(b)) = (
                    info.get("output-node-id").and_then(Value::as_u64),
                    info.get("input-node-id").and_then(Value::as_u64),
                ) {
                    g.links.push((a, b));
                }
            } else if ty.ends_with("Metadata")
                && o.get("props").and_then(|p| str_of(p, "metadata.name")) == Some("default")
            {
                for m in o
                    .get("metadata")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if str_of(m, "key") == Some("default.audio.sink") {
                        g.default_sink = m
                            .get("value")
                            .and_then(|v| str_of(v, "name"))
                            .map(str::to_string);
                    }
                }
            }
        }
        Some(g)
    }

    pub fn by_name(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.name == name)
    }

    fn by_id(&self, id: u64) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Há um nó com este nome (sink existente)?
    pub fn has_sink(&self, name: &str) -> bool {
        self.by_name(name).is_some_and(Node::is_sink)
    }

    /// O sink real que tem a maior `priority.session` (não é do EasyEffects
    /// nem o da coluna): para onde o EasyEffects toca quando segue o
    /// predefinido e não há ligações a mostrá-lo.
    fn best_real_sink(&self) -> Option<&Node> {
        self.nodes
            .iter()
            .filter(|n| n.is_sink() && !n.is_ee() && n.name != SPEAKER_SINK)
            .max_by_key(|n| n.priority.unwrap_or(0))
    }

    /// Segue as ligações a partir de `from` (por nós internos do EasyEffects)
    /// até aos sinks reais: o destino do som.
    fn real_sinks_after(&self, from: u64) -> Vec<&Node> {
        let mut seen = vec![from];
        let mut queue = VecDeque::from([from]);
        let mut out = Vec::new();
        while let Some(cur) = queue.pop_front() {
            for (a, b) in &self.links {
                if *a != cur || seen.contains(b) {
                    continue;
                }
                seen.push(*b);
                let Some(n) = self.by_id(*b) else { continue };
                if n.is_ee() {
                    queue.push_back(*b);
                } else if n.is_sink() {
                    out.push(n);
                }
            }
        }
        out
    }

    /// O som que entra em `from` chega a `to` (por qualquer caminho)?
    pub fn reaches(&self, from: &str, to: &str) -> bool {
        let (Some(a), Some(b)) = (self.by_name(from), self.by_name(to)) else {
            return false;
        };
        if a.id == b.id {
            return true;
        }
        let mut seen = vec![a.id];
        let mut queue = VecDeque::from([a.id]);
        while let Some(cur) = queue.pop_front() {
            for (x, y) in &self.links {
                if *x == cur && !seen.contains(y) {
                    if *y == b.id {
                        return true;
                    }
                    seen.push(*y);
                    queue.push_back(*y);
                }
            }
        }
        false
    }

    /// Sinks onde há streams de saída **a tocar** (estado `running`): o
    /// destino direto de cada uma, sem repetidos.
    pub fn playing_sinks(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (a, b) in &self.links {
            let (Some(s), Some(k)) = (self.by_id(*a), self.by_id(*b)) else {
                continue;
            };
            if s.is_output_stream()
                && s.state.as_deref() == Some("running")
                && k.is_sink()
                && !out.contains(&k.name)
            {
                out.push(k.name.clone());
            }
        }
        out
    }
}

/// Como se chegou ao destino do EasyEffects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DestHow {
    /// Pelas ligações do grafo (o EasyEffects está a processar agora).
    Graph,
    /// Pela opção `outputDevice` do `easyeffectsrc` (com «Usar predefinido» desligado).
    Config,
    /// Sem ligações: o sink real predefinido do sistema.
    Default,
    /// Sem ligações nem predefinido real: o sink real de maior prioridade.
    Priority,
}

impl DestHow {
    pub fn label(self) -> &'static str {
        match self {
            DestHow::Graph => "pelas ligações do grafo",
            DestHow::Config => "pela configuração (outputDevice)",
            DestHow::Default => "sem ligações: o sink predefinido",
            DestHow::Priority => "sem ligações: o sink real de maior prioridade",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dest {
    pub sink: String,
    pub how: DestHow,
}

/// O que interessa do `easyeffectsrc` (só se o ficheiro existir).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EeConfig {
    pub use_default_output: Option<bool>,
    pub output_device: Option<String>,
}

/// Lê `[StreamOutputs]` do `easyeffectsrc` (KConfig: `chave=valor`).
pub fn parse_ee_config(text: &str) -> EeConfig {
    let mut cfg = EeConfig::default();
    let mut group = "";
    for line in text.lines().map(str::trim) {
        if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = g;
        } else if group == "StreamOutputs"
            && let Some((k, v)) = line.split_once('=')
        {
            match k.trim() {
                "useDefaultOutputDevice" => cfg.use_default_output = Some(v.trim() == "true"),
                "outputDevice" if !v.trim().is_empty() => {
                    cfg.output_device = Some(v.trim().to_string())
                }
                _ => {}
            }
        }
    }
    cfg
}

/// Resposta de `get_global_bypass`: `1` = ligado, `2` = desligado.
pub fn parse_bypass(reply: &str) -> Option<bool> {
    match reply.trim() {
        "1" => Some(true),
        "2" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EeState {
    /// O nó `easyeffects_sink` existe.
    pub running: bool,
    /// Para onde o EasyEffects está a tocar (sink real).
    pub destination: Option<Dest>,
    /// O sink predefinido do sistema (`default.audio.sink`).
    pub system_default: Option<String>,
    pub default_is_ee_sink: bool,
    /// `get_global_bypass` (só leitura). `None` = sem resposta.
    pub bypass: Option<bool>,
    /// Configuração, se o ficheiro existir.
    pub config: Option<EeConfig>,
}

/// O estado do EasyEffects a partir do grafo (e do que se leu do socket e da
/// configuração).
pub fn ee_state(g: &Graph, bypass: Option<bool>, config: Option<EeConfig>) -> EeState {
    let sink = g.by_name(EE_SINK);
    let running = sink.is_some();
    let system_default = g.default_sink.clone();
    let default_is_ee_sink = system_default.as_deref() == Some(EE_SINK);
    let destination = sink.and_then(|s| {
        if let Some(d) = g.real_sinks_after(s.id).first() {
            return Some(Dest {
                sink: d.name.clone(),
                how: DestHow::Graph,
            });
        }
        if let Some(c) = &config
            && c.use_default_output == Some(false)
            && let Some(dev) = c.output_device.as_deref()
            && g.has_sink(dev)
        {
            return Some(Dest {
                sink: dev.to_string(),
                how: DestHow::Config,
            });
        }
        if let Some(def) = system_default.as_deref()
            && def != EE_SINK
            && g.has_sink(def)
        {
            return Some(Dest {
                sink: def.to_string(),
                how: DestHow::Default,
            });
        }
        g.best_real_sink().map(|n| Dest {
            sink: n.name.clone(),
            how: DestHow::Priority,
        })
    });
    EeState {
        running,
        destination,
        system_default,
        default_is_ee_sink,
        bypass,
        config,
    }
}

/// O grafo atual (`pw-dump`) e o estado do EasyEffects, só por leitura:
/// o socket do EasyEffects só recebe `get_global_bypass`, e o
/// `easyeffectsrc` só se lê se existir. `None` = sem `pw-dump` ou sem
/// resposta (PulseAudio puro, PipeWire parado).
pub fn snapshot(env: &Env) -> Option<(Graph, EeState)> {
    let out = env.run("pw-dump", &[]).filter(|o| o.ok)?;
    let g = Graph::parse(&out.stdout)?;
    let bypass = g
        .by_name(EE_SINK)
        .and_then(|_| (env.sock_query)("EasyEffectsServer", "get_global_bypass"))
        .and_then(|r| parse_bypass(&r));
    let config = env
        .config_dir()
        .map(|d| d.join("easyeffects/db/easyeffectsrc"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_ee_config(&t));
    let ee = ee_state(&g, bypass, config);
    Some((g, ee))
}

/// O alvo do tap: o sink cujo **monitor** se captura, e porquê.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TapTarget {
    pub sink: String,
    pub why: String,
}

/// Resolve a fonte do tap (`tap_source`). `Err` = frase para o Diário (o tap
/// recusa-se a arrancar em vez de ligar ao microfone ou ficar mudo).
///
/// - `default`: o sink predefinido do sistema (o comportamento antigo).
/// - `easyeffects_pre`: o monitor do `easyeffects_sink` (som **sem** efeitos).
/// - `easyeffects_post`: o monitor do sink real para onde o EasyEffects toca
///   (som **com** efeitos).
/// - `auto`: sem EasyEffects ou em bypass → o predefinido; com EasyEffects e o
///   predefinido é o `easyeffects_sink` → o monitor dele (sem efeitos); com o
///   EasyEffects a tocar noutro sink que não o predefinido, o `default` ficaria
///   mudo → segue o sink real de destino.
pub fn resolve_tap(choice: TapChoice, ee: &EeState) -> Result<TapTarget, String> {
    let default = ee.system_default.clone();
    let need_default = || {
        default
            .clone()
            .ok_or_else(|| "não consegui resolver a saída padrão — recuso-me a capturar sem alvo, que ligaria ao microfone".to_string())
    };
    let t = |sink: String, why: &str| {
        Ok(TapTarget {
            sink,
            why: why.to_string(),
        })
    };
    match choice {
        TapChoice::Default => t(
            need_default()?,
            "tap_source=default: a saída predefinida do sistema",
        ),
        TapChoice::EasyeffectsPre => {
            if !ee.running {
                return Err("tap_source=easyeffects_pre, mas o EasyEffects não está a correr (não há easyeffects_sink)".into());
            }
            t(
                EE_SINK.into(),
                "tap_source=easyeffects_pre: monitor do easyeffects_sink, som sem efeitos",
            )
        }
        TapChoice::EasyeffectsPost => {
            if !ee.running {
                return Err("tap_source=easyeffects_post, mas o EasyEffects não está a correr (não há easyeffects_sink)".into());
            }
            match &ee.destination {
                Some(d) => t(
                    d.sink.clone(),
                    "tap_source=easyeffects_post: monitor do sink onde o EasyEffects toca, som com efeitos",
                ),
                None => Err(
                    "tap_source=easyeffects_post, mas não sei onde o EasyEffects está a tocar"
                        .into(),
                ),
            }
        }
        TapChoice::Auto => {
            if !ee.running {
                return t(need_default()?, "sem EasyEffects: a saída predefinida");
            }
            if ee.bypass == Some(true) {
                return t(
                    need_default()?,
                    "EasyEffects em bypass: a saída predefinida",
                );
            }
            if ee.default_is_ee_sink {
                return t(
                    EE_SINK.into(),
                    "o EasyEffects é a saída predefinida: monitor do easyeffects_sink (som sem efeitos)",
                );
            }
            let def = need_default()?;
            match &ee.destination {
                Some(d) if d.sink != def => t(
                    d.sink.clone(),
                    &format!(
                        "o EasyEffects toca em {}, não na predefinida ({def}): segue o sink real de destino",
                        d.sink
                    ),
                ),
                _ => t(def, "EasyEffects a tocar na saída predefinida"),
            }
        }
    }
}

/// Aviso do modo coluna (`hyprlink-speaker` existe): o som não está a ir para
/// a coluna. `None` = tudo bem (ou o modo coluna não está ativo).
pub fn speaker_routing_warning(g: &Graph, ee: &EeState) -> Option<String> {
    g.by_name(SPEAKER_SINK)?;
    if ee.running {
        let d = ee.destination.as_ref()?;
        if d.sink != SPEAKER_SINK && d.how != DestHow::Priority {
            return Some(format!(
                "o EasyEffects está a enviar o som para {}, não para a coluna ({SPEAKER_SINK}): põe «Usar predefinido» nas saídas do EasyEffects (o HyprLink não altera a configuração dele)",
                d.sink
            ));
        }
        return None;
    }
    let elsewhere: Vec<String> = g
        .playing_sinks()
        .into_iter()
        .filter(|s| s != SPEAKER_SINK)
        .collect();
    if elsewhere.is_empty() {
        None
    } else {
        Some(format!(
            "o som está a tocar em {}, não na coluna ({SPEAKER_SINK})",
            elsewhere.join(", ")
        ))
    }
}

/// Streams a tocar num sink que **não** chega ao `target` (nem por dentro do
/// EasyEffects): é o som que o monitor escolhido não apanha.
pub fn playing_elsewhere(g: &Graph, target: &str) -> Vec<String> {
    g.playing_sinks()
        .into_iter()
        .filter(|s| !g.reaches(s, target))
        .collect()
}

/// Aviso do tap em silêncio: o monitor de `target` não dá som há muito, mas há
/// streams a tocar num sink que não chega a ele. `None` = não há som
/// «noutro sítio» (silêncio verdadeiro).
pub fn silence_warning(g: &Graph, target: &str, secs: u64) -> Option<String> {
    let elsewhere = playing_elsewhere(g, target);
    if elsewhere.is_empty() {
        return None;
    }
    Some(format!(
        "o monitor de {target} está em silêncio há {secs} s, mas há streams a tocar em {}: escolhe outra fonte em tap_source (config.json)",
        elsewhere.join(", ")
    ))
}

/// Ordem de restauro do sink predefinido ao desligar o modo coluna. Se antes
/// o predefinido era o `easyeffects_sink` e o EasyEffects tocava num sink real
/// `ee_dest`, devolve-se primeiro o predefinido a `ee_dest` (o EasyEffects
/// segue o predefinido) e só depois ao `easyeffects_sink` (que ele ignora);
/// senão ficaria a apontar para o sink da coluna, já descarregado.
pub fn restore_default_order(prev: &str, ee_dest: Option<&str>) -> Vec<String> {
    match ee_dest {
        Some(d) if prev == EE_SINK && d != EE_SINK => vec![d.to_string(), prev.to_string()],
        _ => vec![prev.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(profile: &str) -> (Graph, Option<bool>, Option<EeConfig>) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hyprlinkd/tests/fixtures")
            .join(profile);
        let g = Graph::parse(&std::fs::read_to_string(dir.join("pw-dump.json")).unwrap()).unwrap();
        let bypass = std::fs::read_to_string(dir.join("bypass.txt"))
            .ok()
            .and_then(|s| parse_bypass(&s));
        let cfg = std::fs::read_to_string(dir.join("easyeffectsrc"))
            .ok()
            .map(|s| parse_ee_config(&s));
        (g, bypass, cfg)
    }

    fn state(profile: &str) -> (Graph, EeState) {
        let (g, b, c) = load(profile);
        let s = ee_state(&g, b, c);
        (g, s)
    }

    const ALSA: &str = "alsa_output.pci-0000_03_00.6.analog-stereo";
    const BT: &str = "bluez_output.AA_BB_CC_01_02_03.1";

    #[test]
    fn predefinido_o_caso_do_maggio() {
        let (g, s) = state("ee-predefinido");
        assert!(s.running && s.default_is_ee_sink);
        let d = s.destination.as_ref().unwrap();
        assert_eq!((d.sink.as_str(), d.how), (ALSA, DestHow::Graph));
        assert_eq!(s.bypass, Some(false));
        assert!(s.config.is_none(), "o easyeffectsrc pode não existir");
        // auto: o monitor do easyeffects_sink (som sem efeitos).
        let t = resolve_tap(TapChoice::Auto, &s).unwrap();
        assert_eq!(t.sink, EE_SINK);
        assert!(t.why.contains("sem efeitos"));
        assert_eq!(
            resolve_tap(TapChoice::EasyeffectsPre, &s).unwrap().sink,
            EE_SINK
        );
        assert_eq!(
            resolve_tap(TapChoice::EasyeffectsPost, &s).unwrap().sink,
            ALSA
        );
        assert_eq!(resolve_tap(TapChoice::Default, &s).unwrap().sink, EE_SINK);
    }

    #[test]
    fn saida_diferente_segue_o_destino_real() {
        let (g, s) = state("ee-saida-diferente");
        assert!(!s.default_is_ee_sink);
        assert_eq!(s.system_default.as_deref(), Some(ALSA));
        assert_eq!(s.destination.as_ref().unwrap().sink, BT);
        // `default` ficaria mudo (a predefinida não toca nada); `auto` segue o BT.
        let t = resolve_tap(TapChoice::Auto, &s).unwrap();
        assert_eq!(t.sink, BT);
        assert!(t.why.contains(BT) && t.why.contains(ALSA), "{}", t.why);
        assert_eq!(resolve_tap(TapChoice::Default, &s).unwrap().sink, ALSA);
        assert_eq!(
            resolve_tap(TapChoice::EasyeffectsPost, &s).unwrap().sink,
            BT
        );
        assert_eq!(
            resolve_tap(TapChoice::EasyeffectsPre, &s).unwrap().sink,
            EE_SINK
        );
    }

    #[test]
    fn bypass_usa_a_saida_predefinida() {
        let (g, s) = state("ee-bypass");
        assert_eq!(s.bypass, Some(true));
        let t = resolve_tap(TapChoice::Auto, &s).unwrap();
        assert_eq!(t.sink, EE_SINK, "a predefinida é o easyeffects_sink");
        assert!(t.why.contains("bypass"));
    }

    #[test]
    fn sem_easyeffects() {
        let (g, s) = state("sem-ee");
        assert!(!s.running && s.destination.is_none());
        assert_eq!(resolve_tap(TapChoice::Auto, &s).unwrap().sink, ALSA);
        assert!(
            resolve_tap(TapChoice::EasyeffectsPre, &s)
                .unwrap_err()
                .contains("não está a correr")
        );
        assert!(resolve_tap(TapChoice::EasyeffectsPost, &s).is_err());
    }

    #[test]
    fn ocioso_sem_ligacoes_cai_no_sink_real_de_maior_prioridade() {
        let (g, s) = state("ee-ocioso");
        let d = s.destination.as_ref().unwrap();
        assert_eq!((d.sink.as_str(), d.how), (ALSA, DestHow::Priority));
        assert_eq!(
            resolve_tap(TapChoice::EasyeffectsPost, &s).unwrap().sink,
            ALSA
        );
    }

    #[test]
    fn config_com_usar_predefinido_desligado_vale_sem_ligacoes() {
        let (g, _, _) = load("ee-ocioso");
        let cfg = parse_ee_config(&format!(
            "[Other]\noutputDevice=x\n[StreamOutputs]\nuseDefaultOutputDevice=false\noutputDevice={ALSA}\n"
        ));
        assert_eq!(cfg.use_default_output, Some(false));
        let s = ee_state(&g, None, Some(cfg));
        assert_eq!(s.destination.unwrap().how, DestHow::Config);
        // Um dispositivo que já não existe não vale.
        let cfg = parse_ee_config(
            "[StreamOutputs]\nuseDefaultOutputDevice=false\noutputDevice=desapareceu\n",
        );
        assert_ne!(
            ee_state(&g, None, Some(cfg)).destination.unwrap().how,
            DestHow::Config
        );
    }

    #[test]
    fn sem_predefinido_nem_ee_recusa() {
        let g = Graph::default();
        let s = ee_state(&g, None, None);
        assert!(
            resolve_tap(TapChoice::Auto, &s)
                .unwrap_err()
                .contains("microfone")
        );
    }

    #[test]
    fn bypass_e_config_parsers() {
        assert_eq!(parse_bypass("1\n"), Some(true));
        assert_eq!(parse_bypass("2"), Some(false));
        assert_eq!(parse_bypass(""), None);
        assert_eq!(parse_bypass("erro"), None);
    }

    #[test]
    fn coluna_avisos() {
        // EE a seguir o predefinido (a coluna): sem aviso.
        let (g, s) = state("ee-coluna-ok");
        assert_eq!(speaker_routing_warning(&g, &s), None);
        // EE a tocar no alsa: aviso a dizer onde.
        let (g, s) = state("ee-coluna-desviada");
        let w = speaker_routing_warning(&g, &s).unwrap();
        assert!(
            w.contains(ALSA) && w.contains("EasyEffects") && w.contains("não altera"),
            "{w}"
        );
        // Sem EE, som noutro sink.
        let (g, s) = state("coluna-desviada-sem-ee");
        let w = speaker_routing_warning(&g, &s).unwrap();
        assert!(w.contains(ALSA), "{w}");
        // Sem coluna ativa, nunca avisa.
        let (g, s) = state("ee-predefinido");
        assert_eq!(speaker_routing_warning(&g, &s), None);
    }

    #[test]
    fn som_noutro_sitio() {
        let (g, _) = state("ee-predefinido");
        // O monitor do alsa apanha o que passa pelo EasyEffects: nada «noutro sítio».
        assert!(playing_elsewhere(&g, ALSA).is_empty());
        assert!(playing_elsewhere(&g, EE_SINK).is_empty());
        // O monitor de um sink que o som não toca: há.
        let (g, _) = state("ee-saida-diferente");
        assert_eq!(playing_elsewhere(&g, ALSA), vec![EE_SINK.to_string()]);
        assert!(playing_elsewhere(&g, BT).is_empty());
    }

    #[test]
    fn aviso_de_silencio_so_com_som_noutro_sitio() {
        let (g, _) = state("ee-saida-diferente");
        let w = silence_warning(&g, ALSA, 10).unwrap();
        assert!(
            w.contains(ALSA) && w.contains("10 s") && w.contains("tap_source"),
            "{w}"
        );
        assert_eq!(
            silence_warning(&g, BT, 10),
            None,
            "o BT recebe o som: sem aviso"
        );
        let (g, _) = state("ee-ocioso");
        assert_eq!(
            silence_warning(&g, ALSA, 10),
            None,
            "nada a tocar: silêncio verdadeiro"
        );
    }

    #[test]
    fn restaurar_o_predefinido_com_easyeffects() {
        assert_eq!(
            restore_default_order(EE_SINK, Some(ALSA)),
            vec![ALSA, EE_SINK]
        );
        assert_eq!(restore_default_order(ALSA, Some(ALSA)), vec![ALSA]);
        assert_eq!(restore_default_order(EE_SINK, None), vec![EE_SINK]);
    }
}
