//! Presence Lock (Pista C2): bloquear o PC quando te afastas com o
//! telemóvel — e saber que voltaste. A app Android anuncia BLE
//! continuamente (ver prompt `prompt_ai_studio_2026-10-05_ble_presence.md`);
//! o daemon regista um `AdvertisementMonitor` no BlueZ com **filtro RSSI**:
//! é o próprio BlueZ que faz a histerese e chama `DeviceFound`/`DeviceLost`
//! no nosso objeto quando os limiares/tempos são cumpridos — nada de
//! polling nosso, e em hardware que suporta, o filtro desce pro
//! controlador (offload).
//!
//! Decisões de segurança (importante):
//! - **Só bloqueia** (`exec hyprlock`) — não existe "unlock remoto":
//!   hyprlock abre por autenticação, e é assim que um locker deve ser.
//!   Voltar com o telemóvel só atualiza o estado (`presença: perto`).
//! - O padrão anunciado não é segredo (é presença, não autenticação) — o
//!   pior caso de alguém o imitar é bloquear o PC (aborrecimento, não
//!   brecha). Se um dia o desbloqueio automático existir, aí o anúncio
//!   terá de criptograficamente identificar o telemóvel pareado.
//! - A funcionalidade vem **desligada** por omissão (`config.json`,
//!   `presence_enabled`), e liga-se por `hyprlinkctl presence on`.
//!
//! Lifecycle: a tarefa `supervise` polia a config a cada 2s — liga ⇒ regista
//! o monitor no BlueZ, desliga ⇒ ReleaseMonitor. Se o daemon morrer, o
//! monitor morre com a conexão D-Bus (o BlueZ limpa sozinho) — zero órfãos.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zbus::interface;
use zbus::proxy;

use crate::config::SharedConfig;
use crate::state::{self, push_log, HudState};

/// O padrão que a app anuncia e o monitor procura: Service Data com UUID
/// 16-bit 0xF00D (o AI Studio usa o ParcelUuid "0000F00D-0000-1000-8000-
/// 00805F9B34FB", que o Android comprime a 16-bit no anúncio) + payload
/// "HYPR". No Service Data os bytes do UUID vão em LITTLE-ENDIAN.
const SERVICE_UUID_LE: [u8; 2] = [0x0D, 0xF0];
const PAYLOAD: &[u8] = b"HYPR";

/// Histerese: DeviceLost quando o RSSI fica abaixo de -85 dBm por 5 s
/// (bloqueia); DeviceFound quando volta acima de -75 dBm por 2 s (presença
/// registada, sem desbloquear). Valores de partida — afinar em uso real.
const RSSI_LOW: i16 = -85;
const RSSI_LOW_TIMEOUT_S: u16 = 5;
const RSSI_HIGH: i16 = -75;
const RSSI_HIGH_TIMEOUT_S: u16 = 2;

/// Caminho D-Bus do nosso monitor (sob um ObjectManager, como o BlueZ
/// exige para descobrir as propriedades do objeto).
const MONITOR_PATH: &str = "/org/hyprlink/monitor0";
const MANAGER_ROOT: &str = "/org/hyprlink";

/// A interface `org.bluez.AdvertisementMonitor1` — o BlueZ lê as
/// PROPRIEDADES no CreateMonitor (via ObjectManager) e chama os MÉTODOS
/// quando o filtro RSSI dispara.
struct Monitor {
    hud: Arc<Mutex<HudState>>,
}

#[interface(name = "org.bluez.AdvertisementMonitor1")]
impl Monitor {
    /// Chamado quando o monitor é removido (ex.: BlueZ a reiniciar).
    fn release(&self) {
        // Nada a limpar — o supervisor volta a registar no próximo ciclo.
    }

    /// O telemóvel voltou ao alcance (RSSI ≥ alto pelo tempo alto).
    fn device_found(&self) {
        state::set_presence(&self.hud, Some(true));
        push_log(&self.hud, "[+] presença: telemóvel ao alcance".to_string());
    }

    /// O telemóvel saiu do alcance (RSSI < baixo pelo tempo baixo) —
    /// bloquear. `std::process::Command` do `hypr::dispatch` é rápido
    /// (um `hyprctl` fire-and-forget); em contexto async do zbus, isolar
    /// mesmo assim pra não travar o executor da conexão.
    fn device_lost(&self) {
        state::set_presence(&self.hud, Some(false));
        push_log(&self.hud, "[i] presença: telemóvel fora de alcance — a bloquear o PC".to_string());
        let hud = self.hud.clone();
        tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(|| crate::hypr::dispatch("exec hyprlock")).await;
            let _ = hud;
        });
    }

    #[zbus(property)]
    fn type_(&self) -> String {
        "or_patterns".to_string()
    }

    #[zbus(property)]
    fn rssi_low_threshold(&self) -> i16 {
        RSSI_LOW
    }

    #[zbus(property)]
    fn rssi_high_threshold(&self) -> i16 {
        RSSI_HIGH
    }

    #[zbus(property)]
    fn rssi_low_timeout(&self) -> u16 {
        RSSI_LOW_TIMEOUT_S
    }

    #[zbus(property)]
    fn rssi_high_timeout(&self) -> u16 {
        RSSI_HIGH_TIMEOUT_S
    }

    /// Padrões `or`: Service Data - 16 bit UUID (ad_type 1), do início do
    /// campo, UUID LE + "HYPR".
    #[zbus(property)]
    fn patterns(&self) -> Vec<(u8, u8, Vec<u8>)> {
        let mut data = Vec::with_capacity(2 + PAYLOAD.len());
        data.extend_from_slice(&SERVICE_UUID_LE);
        data.extend_from_slice(PAYLOAD);
        vec![(0, 1, data)]
    }
}

#[proxy(
    interface = "org.bluez.AdvertisementMonitorManager1",
    default_service = "org.bluez"
)]
trait AdvertisementMonitorManager {
    fn create_monitor(&self, monitor: &str) -> zbus::Result<()>;
    fn release_monitor(&self, monitor: &str) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.DBus.ObjectManager",
    default_service = "org.bluez",
    default_path = "/"
)]
trait BluezObjectManager {
    fn get_managed_objects(
        &self,
    ) -> zbus::Result<std::collections::HashMap<
        zbus::zvariant::OwnedObjectPath,
        std::collections::HashMap<String, std::collections::HashMap<String, zbus::zvariant::OwnedValue>>,
    >>;
}

/// O caminho do primeiro adaptador BLE (ex.: `/org/bluez/hci0`).
async fn adapter_path(dbus: &zbus::Connection) -> Option<String> {
    let om = BluezObjectManagerProxy::new(dbus).await.ok()?;
    let objects = om.get_managed_objects().await.ok()?;
    for (path, ifaces) in objects.iter() {
        if ifaces.contains_key("org.bluez.Adapter1") {
            return Some(path.to_string());
        }
    }
    None
}

/// Tarefa de fundo: acompanha `presence_enabled` na config e regista/liberta
/// o monitor conforme. Erros não são fatais — tenta de novo no próximo
/// ciclo (Bluetooth desligado, BlueZ a arrancar, sem adaptador…).
pub async fn supervise(
    dbus: Option<zbus::Connection>,
    config: SharedConfig,
    hud: Arc<Mutex<HudState>>,
) {
    let Some(dbus) = dbus else {
        push_log(&hud, "[!] presença: sem sessão D-Bus, monitor de BLE desativado".to_string());
        return;
    };
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut registado = false;
    let mut last_error_log = std::time::Instant::now() - Duration::from_secs(3600);
    loop {
        interval.tick().await;
        let want = crate::config::presence_enabled(&config);
        if want && !registado {
            match register(&dbus, &hud).await {
                Ok(()) => {
                    registado = true;
                    push_log(&hud, "[+] presença: monitor BLE registado no BlueZ (bloqueia fora de alcance)".to_string());
                }
                Err(e) => {
                    // Log com ritmo próprio — o ciclo é de 2s, não é spam
                    // que interessa repetir.
                    if last_error_log.elapsed() > Duration::from_secs(30) {
                        last_error_log = std::time::Instant::now();
                        push_log(&hud, format!("[!] presença: monitor não registado ({e}) — a tentar de novo".to_string()));
                    }
                }
            }
        } else if !want && registado {
            let adapter = adapter_path(&dbus).await.unwrap_or_else(|| "/org/bluez/hci0".to_string());
            let released = AdvertisementMonitorManagerProxy::builder(&dbus)
                .path(adapter)
                .ok()
                .and_then(|b| b.build().ok());
            if let Some(mgr) = released {
                let _ = mgr.release_monitor(MONITOR_PATH).await;
            }
            registado = false;
            state::set_presence(&hud, None);
            push_log(&hud, "[i] presença: monitor BLE removido".to_string());
        }
    }
}

async fn register(dbus: &zbus::Connection, hud: &Arc<Mutex<HudState>>) -> zbus::Result<()> {
    let adapter = adapter_path(dbus)
        .await
        .ok_or_else(|| zbus::Error::Handshake("nenhum adaptador Bluetooth (Adapter1) visível".into()))?;

    // O BlueZ exige o objeto acessível por um ObjectManager: registar o
    // gestor na raiz ANTES das interfaces por baixo dela.
    dbus.object_server()
        .at(MANAGER_ROOT, zbus::fdo::ObjectManager)
        .await?;
    dbus.object_server()
        .at(MONITOR_PATH, Monitor { hud: hud.clone() })
        .await?;

    let mgr = AdvertisementMonitorManagerProxy::builder(dbus)
        .path(adapter)?
        .build()
        .await?;
    if let Err(e) = mgr.create_monitor(MONITOR_PATH).await {
        // Limpar o objeto registado pra um registo futuro começar limpo.
        let _ = dbus.object_server().remove::<Monitor, _>(MONITOR_PATH).await;
        return Err(e);
    }
    Ok(())
}
