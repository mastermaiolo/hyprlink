//! Ícone na bandeja do sistema (StatusNotifierItem via `ksni`/D-Bus) — clicar
//! nele ou no menu "Mostrar" sinaliza a GUI (que faz polling no `Tick`, já
//! que ksni roda numa thread/executor próprio, separado da thread da GUI).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ksni::menu::StandardItem;
use ksni::TrayMethods;

pub type ShowRequested = Arc<AtomicBool>;

pub fn new_show_flag() -> ShowRequested {
    Arc::new(AtomicBool::new(false))
}

/// Chamado pela GUI a cada `Tick` — devolve `true` uma vez só por clique
/// (consome o pedido).
pub fn take_show_requested(flag: &ShowRequested) -> bool {
    flag.swap(false, Ordering::Relaxed)
}

struct HyprlinkTray {
    show_requested: ShowRequested,
}

impl ksni::Tray for HyprlinkTray {
    fn id(&self) -> String {
        "hyprlink".into()
    }

    fn title(&self) -> String {
        "HyprLink".into()
    }

    fn icon_name(&self) -> String {
        "phone".into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.show_requested.store(true, Ordering::Relaxed);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        // ponytail: só "mostrar" — sair fica só no botão × da GUI (com
        // fechamento educado da conexão QUIC), não é pra ter um jeito de
        // matar o processo inteiro sem confirmação num clique de menu.
        vec![StandardItem {
            label: "Mostrar HyprLink".into(),
            activate: Box::new(|this: &mut Self| this.show_requested.store(true, Ordering::Relaxed)),
            ..Default::default()
        }
        .into()]
    }
}

/// Roda em background (chamar dentro do runtime tokio do daemon) — se o
/// sistema não tiver um `StatusNotifierWatcher` (raro, mas alguns setups não
/// têm), só loga e segue sem ícone, não é fatal.
pub async fn spawn(show_requested: ShowRequested) {
    let tray = HyprlinkTray { show_requested };
    if let Err(e) = tray.spawn().await {
        eprintln!("[!] tray: não foi possível criar o ícone da bandeja: {e}");
    }
}
