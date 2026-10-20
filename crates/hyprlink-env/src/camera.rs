//! Câmara virtual (v4l2loopback): módulo, dkms, e o estado do `/dev/videoN`.

use std::path::Path;

use serde::Serialize;

use crate::read_trim;

/// Nome com que o daemon cria o dispositivo (`card_label`).
pub const CARD_LABEL: &str = "HyprLink Webcam";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    /// Não existe `/dev/videoN`: o número está livre.
    Free,
    /// Existe e é o nosso (`card_label` HyprLink Webcam).
    Ours,
    /// Existe e é outro dispositivo: o número está ocupado.
    Busy,
}

/// Estado do `/dev/video{nr}`. Existe e é o nosso (`card_label`) → `Ours`; existe
/// e o v4l2loopback está carregado → `Ours` também (assume-se que é um
/// dispositivo do próprio v4l2loopback, p. ex. criado por `modprobe.d`; **a
/// confirmar** em máquinas com câmaras reais em `/dev/video42`); existe sem o
/// módulo carregado → `Busy` (é outro dispositivo: usa `v4l2_device_nr`).
pub fn device_state(sys: &Path, dev: &Path, nr: u32) -> DeviceState {
    let name = read_trim(&sys.join(format!("class/video4linux/video{nr}/name")));
    let exists = name.is_some() || dev.join(format!("video{nr}")).exists();
    match name {
        Some(n) if n == CARD_LABEL => DeviceState::Ours,
        _ if !exists => DeviceState::Free,
        _ if module_loaded(sys) => DeviceState::Ours,
        _ => DeviceState::Busy,
    }
}

/// O módulo está carregado? (`sys/module/v4l2loopback`)
pub fn module_loaded(sys: &Path) -> bool {
    sys.join("module/v4l2loopback").exists()
}

/// Texto do `modinfo` indica que o módulo existe para o kernel em uso?
/// `modinfo -n v4l2loopback` imprime o caminho do `.ko`; falha se não existir.
pub fn module_available(env: &crate::Env) -> Option<bool> {
    env.run("modinfo", &["-n", "v4l2loopback"])
        .map(|o| o.ok && !o.stdout.trim().is_empty())
}

/// `dkms status` menciona o v4l2loopback?
pub fn dkms_has_v4l2loopback(env: &crate::Env) -> Option<bool> {
    let o = env.run("dkms", &["status"])?;
    Some(o.stdout.lines().any(|l| l.contains("v4l2loopback")))
}

/// Descodificação de vídeo por hardware (plugin `va` do GStreamer, VA-API).
/// Só conta o que o plugin regista: sem driver (`libva-mesa-driver`,
/// `intel-media-driver`) os elementos `va*dec` não aparecem.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct VaSupport {
    /// O plugin `va` existe (no Arch: `gst-plugin-va`). Sem ele não há nada a
    /// fazer; com ele mas sem elementos, falta o driver VA-API.
    pub plugin: bool,
    pub h264: bool,
    pub h265: bool,
}

/// Pergunta ao `gst-inspect-1.0` (só lê o registo; não abre a GPU).
pub fn va_support(env: &crate::Env) -> VaSupport {
    let has = |el: &str| env.run("gst-inspect-1.0", &[el]).is_some_and(|o| o.ok);
    VaSupport {
        plugin: env.run("gst-inspect-1.0", &["va"]).is_some_and(|o| o.ok),
        h264: has("vah264dec"),
        h265: has("vah265dec"),
    }
}

/// Existe o elemento `pipewiresink` (plugin `gst-plugin-pipewire`)?
pub fn pipewiresink_present(env: &crate::Env) -> bool {
    env.run("gst-inspect-1.0", &["pipewiresink"])
        .is_some_and(|o| o.ok)
}

/// A saída efetiva da câmara virtual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    V4l2loopback,
    Pipewire,
}

/// `auto` = v4l2loopback se o módulo está carregado ou carregável (o kernel
/// tem-no; `None` = não se sabe, conta como carregável), senão PipeWire.
pub fn resolve_output(
    choice: crate::overrides::OutputChoice,
    module_loaded: bool,
    module_available: Option<bool>,
) -> Output {
    use crate::overrides::OutputChoice as C;
    match choice {
        C::V4l2loopback => Output::V4l2loopback,
        C::Pipewire => Output::Pipewire,
        C::Auto if module_loaded || module_available != Some(false) => Output::V4l2loopback,
        C::Auto => Output::Pipewire,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hyprlink-env-cam-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("sys/class/video4linux")).unwrap();
        fs::create_dir_all(d.join("dev")).unwrap();
        d
    }

    #[test]
    fn livre_nosso_e_ocupado() {
        let r = root("estados");
        let (sys, dev) = (r.join("sys"), r.join("dev"));
        assert_eq!(device_state(&sys, &dev, 42), DeviceState::Free);
        fs::create_dir_all(sys.join("class/video4linux/video42")).unwrap();
        fs::write(
            sys.join("class/video4linux/video42/name"),
            format!("{CARD_LABEL}\n"),
        )
        .unwrap();
        assert_eq!(device_state(&sys, &dev, 42), DeviceState::Ours);
        fs::write(
            sys.join("class/video4linux/video42/name"),
            "Integrated Camera\n",
        )
        .unwrap();
        assert_eq!(device_state(&sys, &dev, 42), DeviceState::Busy);
        assert_eq!(device_state(&sys, &dev, 10), DeviceState::Free);
    }

    #[test]
    fn saida_auto_segue_o_modulo() {
        use crate::overrides::OutputChoice::*;
        assert_eq!(
            resolve_output(V4l2loopback, false, Some(false)),
            Output::V4l2loopback
        );
        assert_eq!(resolve_output(Pipewire, true, Some(true)), Output::Pipewire);
        assert_eq!(resolve_output(Auto, true, None), Output::V4l2loopback);
        assert_eq!(
            resolve_output(Auto, false, Some(true)),
            Output::V4l2loopback
        );
        assert_eq!(resolve_output(Auto, false, None), Output::V4l2loopback);
        assert_eq!(resolve_output(Auto, false, Some(false)), Output::Pipewire);
    }

    #[test]
    fn modulo_carregado() {
        let r = root("mod");
        let sys = r.join("sys");
        assert!(!module_loaded(&sys));
        fs::create_dir_all(sys.join("module/v4l2loopback")).unwrap();
        assert!(module_loaded(&sys));
    }
}
