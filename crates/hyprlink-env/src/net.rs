//! Rede do PC: por que interface o telemóvel pode chegar ao daemon (Wi-Fi,
//! cabo, **ancoragem USB**) e o que o `doctor` verifica quando há uma
//! interface USB.
//!
//! Só lê (`/sys/class/net`, `ip`, `nmcli`, `firewall-cmd`, `ufw`); nunca
//! altera nada. A configuração (`never-default`, firewall) só se sugere.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Env, read_trim};

/// Porta UDP (QUIC) em que o daemon escuta.
pub const DAEMON_PORT: u16 = 7443;

/// Por onde chega a ligação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Usb,
    Eth,
    Wifi,
    Other,
}

impl LinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::Usb => "usb",
            LinkKind::Eth => "eth",
            LinkKind::Wifi => "wifi",
            LinkKind::Other => "other",
        }
    }
}

/// Um endereço do PC onde o telemóvel pode ligar (`pc_addrs` do `core.hello`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PcAddr {
    pub ip: String,
    pub kind: LinkKind,
    pub iface: String,
}

/// Controladores do kernel de uma interface de ancoragem USB.
const USB_DRIVERS: &[&str] = &["rndis_host", "cdc_ncm", "cdc_ether"];

/// Interfaces que nunca se anunciam: bridges e pares de containers/VMs.
pub fn is_excluded_iface(name: &str) -> bool {
    name == "lo"
        || ["docker", "virbr", "veth", "br-"]
            .iter()
            .any(|p| name.starts_with(p))
}

/// O controlador de `/sys/class/net/<if>/device/driver` (é um symlink para
/// `…/drivers/<nome>`; no `uevent` vem como `DRIVER=<nome>`).
fn driver_of(sys: &Path, iface: &str) -> Option<String> {
    let dev = sys.join("class/net").join(iface).join("device");
    if let Ok(target) = std::fs::read_link(dev.join("driver"))
        && let Some(n) = target.file_name()
    {
        return Some(n.to_string_lossy().into_owned());
    }
    read_trim(&dev.join("uevent"))?
        .lines()
        .find_map(|l| l.strip_prefix("DRIVER="))
        .map(str::to_string)
}

/// Que tipo de ligação é esta interface? Função pura sobre o `/sys`.
pub fn classify(sys: &Path, iface: &str) -> LinkKind {
    let base = sys.join("class/net").join(iface);
    if base.join("wireless").exists() || base.join("phy80211").exists() {
        return LinkKind::Wifi;
    }
    if driver_of(sys, iface).is_some_and(|d| USB_DRIVERS.contains(&d.as_str())) {
        return LinkKind::Usb;
    }
    if iface.starts_with("en") || iface.starts_with("eth") {
        LinkKind::Eth
    } else {
        LinkKind::Other
    }
}

/// Privado (RFC 1918) ou link-local, nunca loopback: o que um telemóvel
/// ligado diretamente ou na mesma LAN consegue alcançar.
pub fn is_candidate_ip(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && (ip.is_private() || ip.is_link_local())
}

/// `(interface, IPv4)` de `ip -o -4 addr show up`:
/// `2: wlan0    inet 192.168.1.5/24 brd … scope global …`.
pub fn parse_ip_addr(text: &str) -> Vec<(String, Ipv4Addr)> {
    text.lines()
        .filter_map(|l| {
            let mut t = l.split_whitespace();
            t.next()?; // «2:»
            let iface = t.next()?.split('@').next()?.trim_end_matches(':');
            (t.next()? == "inet").then_some(())?;
            let ip = t.next()?.split('/').next()?.parse().ok()?;
            Some((iface.to_string(), ip))
        })
        .collect()
}

/// Filtra e classifica: só interfaces ativas, endereços candidatos, sem
/// bridges de containers. O daemon faz `bind` a `0.0.0.0`, por isso escuta em
/// todos os IPv4 destas interfaces (IPv6 não: o socket é só IPv4).
pub fn filter_addrs(sys: &Path, raw: &[(String, Ipv4Addr)]) -> Vec<PcAddr> {
    let set: BTreeSet<PcAddr> = raw
        .iter()
        .filter(|(i, ip)| !is_excluded_iface(i) && is_candidate_ip(*ip))
        .map(|(i, ip)| PcAddr {
            ip: ip.to_string(),
            kind: classify(sys, i),
            iface: i.clone(),
        })
        .collect();
    set.into_iter().collect()
}

/// Os endereços do PC agora (vazio se o `ip` não responde).
pub fn pc_addrs(env: &Env) -> Vec<PcAddr> {
    env.run("ip", &["-o", "-4", "addr", "show", "up"])
        .filter(|o| o.ok)
        .map(|o| filter_addrs(&env.sys, &parse_ip_addr(&o.stdout)))
        .unwrap_or_default()
}

/// Tipo da interface a que pertence o IP local de uma ligação; um IP que não
/// é de nenhuma interface anunciada (ex. público) conta como `other`.
pub fn kind_for_ip(addrs: &[PcAddr], local_ip: &str) -> LinkKind {
    addrs
        .iter()
        .find(|a| a.ip == local_ip)
        .map_or(LinkKind::Other, |a| a.kind)
}

/// Guarda a última lista enviada e diz quando há que a enviar de novo.
#[derive(Debug, Default)]
pub struct AddrTracker {
    last: Option<Vec<PcAddr>>,
}

impl AddrTracker {
    /// `true` se `now` difere da última lista (ou se ainda não houve nenhuma).
    pub fn changed(&mut self, now: &[PcAddr]) -> bool {
        if self.last.as_deref() == Some(now) {
            return false;
        }
        self.last = Some(now.to_vec());
        true
    }

    /// Sem ligação: a próxima lista conta como nova.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

// ── `doctor`: ancoragem USB ────────────────────────────────────────────────

/// O que a firewall diz da porta do daemon na zona/interface USB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirewallNote {
    /// `firewalld` ou `ufw`.
    pub tool: &'static str,
    /// `Some(false)` = fechada; `None` = não deu para ler (ufw sem root).
    pub open: Option<bool>,
    /// O comando que a abre só nessa interface/zona (a correr pelo utilizador).
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsbReport {
    /// Interfaces de ancoragem USB ativas com IPv4.
    pub interfaces: Vec<String>,
    /// A rota por omissão passa por uma delas (a internet do PC vai pelo
    /// telemóvel).
    pub default_route_via_usb: bool,
    /// A ligação do NetworkManager dessa interface.
    pub connection: Option<String>,
    /// `nmcli connection modify … never-default` (só se sugere).
    pub never_default_cmd: Option<String>,
    pub firewall: Option<FirewallNote>,
}

/// O comando que tira a rota por omissão a uma ligação do NetworkManager.
pub fn never_default_args(connection: &str) -> Vec<String> {
    ["connection", "modify", connection]
        .into_iter()
        .chain(["ipv4.never-default", "yes", "ipv6.never-default", "yes"])
        .map(str::to_string)
        .collect()
}

/// O mesmo, como o utilizador o escreve (nome entre aspas).
pub fn never_default_cmd(connection: &str) -> String {
    let q = connection.replace('\\', "\\\\").replace('"', "\\\"");
    format!("nmcli connection modify \"{q}\" ipv4.never-default yes ipv6.never-default yes")
}

/// `nmcli -t -f NAME,DEVICE connection show --active` → `(nome, dispositivo)`.
/// O modo `-t` escapa `:` e `\` com `\`.
pub fn parse_nmcli_active(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut fields = vec![String::new()];
            let mut chars = l.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => fields.last_mut()?.push(chars.next()?),
                    ':' => fields.push(String::new()),
                    c => fields.last_mut()?.push(c),
                }
            }
            (fields.len() == 2 && !fields[1].is_empty())
                .then(|| (fields.remove(0), fields.remove(0)))
        })
        .collect()
}

/// Interfaces com rota por omissão (`ip route show default`, `ip -6 …`).
fn default_route_ifaces(env: &Env) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for args in [
        &["route", "show", "default"][..],
        &["-6", "route", "show", "default"],
    ] {
        let Some(o) = env.run("ip", args).filter(|o| o.ok) else {
            continue;
        };
        for l in o.stdout.lines().filter(|l| l.starts_with("default")) {
            let mut t = l.split_whitespace();
            if let Some(dev) = t.position(|x| x == "dev").and_then(|_| t.next()) {
                out.insert(dev.to_string());
            }
        }
    }
    out
}

fn firewall_note(env: &Env, iface: &str, port: u16) -> Option<FirewallNote> {
    let rule = format!("{port}/udp");
    if env.have("firewall-cmd") {
        let running = env
            .run("firewall-cmd", &["--state"])
            .is_some_and(|o| o.ok && o.stdout.trim() == "running");
        if running {
            let zone = env
                .run("firewall-cmd", &["--get-zone-of-interface", iface])
                .filter(|o| o.ok)
                .map(|o| o.stdout.trim().to_string())
                .filter(|z| !z.is_empty());
            let zone_arg = zone.as_ref().map(|z| format!("--zone={z}"));
            let query = zone_arg
                .as_deref()
                .map_or(vec!["--query-port", rule.as_str()], |z| {
                    vec![z, "--query-port", rule.as_str()]
                });
            let open = env
                .run("firewall-cmd", &query)
                .map(|o| o.stdout.trim() == "yes");
            let command = (open != Some(true)).then(|| match &zone_arg {
                Some(z) => format!("pkexec firewall-cmd {z} --add-port={rule}"),
                None => format!("pkexec firewall-cmd --add-port={rule}"),
            });
            return Some(FirewallNote {
                tool: "firewalld",
                open,
                command,
            });
        }
    }
    if env.have("ufw") {
        // O `ufw status` precisa de root: sem ele não se vê nada.
        let st = env.run("ufw", &["status"]).filter(|o| o.ok);
        let (open, active) = match &st {
            Some(o) => (
                Some(o.stdout.lines().any(|l| {
                    l.contains(&port.to_string()) && l.to_ascii_uppercase().contains("ALLOW")
                })),
                o.stdout.contains("Status: active"),
            ),
            None => (None, true),
        };
        if active {
            let command = (open != Some(true))
                .then(|| format!("pkexec ufw allow in on {iface} to any port {port} proto udp"));
            return Some(FirewallNote {
                tool: "ufw",
                open,
                command,
            });
        }
    }
    None
}

/// O relatório da ancoragem USB; `None` sem interface USB ativa.
pub fn usb_report(env: &Env, port: u16) -> Option<UsbReport> {
    let usb: Vec<PcAddr> = pc_addrs(env)
        .into_iter()
        .filter(|a| a.kind == LinkKind::Usb)
        .collect();
    let first = usb.first()?;
    let interfaces: Vec<String> = usb
        .iter()
        .map(|a| a.iface.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let routes = default_route_ifaces(env);
    let default_route_via_usb = interfaces.iter().any(|i| routes.contains(i));
    let connection = env
        .run(
            "nmcli",
            &["-t", "-f", "NAME,DEVICE", "connection", "show", "--active"],
        )
        .filter(|o| o.ok)
        .and_then(|o| {
            parse_nmcli_active(&o.stdout)
                .into_iter()
                .find(|(_, dev)| interfaces.contains(dev))
                .map(|(name, _)| name)
        });
    let never_default_cmd = (default_route_via_usb)
        .then(|| connection.as_deref().map(never_default_cmd))
        .flatten();
    Some(UsbReport {
        firewall: firewall_note(env, &first.iface, port),
        interfaces,
        default_route_via_usb,
        connection,
        never_default_cmd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{RunOut, Runner};
    use std::collections::BTreeMap;
    use std::os::unix::fs::symlink;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hyprlink-net-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("class/net")).unwrap();
        d
    }

    fn iface(sys: &Path, name: &str, driver: Option<&str>, wireless: bool) {
        let base = sys.join("class/net").join(name);
        std::fs::create_dir_all(base.join("device")).unwrap();
        if wireless {
            std::fs::create_dir_all(base.join("wireless")).unwrap();
        }
        if let Some(d) = driver {
            symlink(
                format!("../../../../bus/usb/drivers/{d}"),
                base.join("device/driver"),
            )
            .unwrap();
        }
    }

    #[test]
    fn classificacao_com_sys_falso() {
        let sys = tmp("classe");
        iface(&sys, "usb0", Some("rndis_host"), false);
        iface(&sys, "enp0s20f0u3", Some("cdc_ncm"), false);
        iface(&sys, "enx0011", Some("cdc_ether"), false);
        iface(&sys, "wlan0", Some("iwlwifi"), true);
        iface(&sys, "enp3s0", Some("r8169"), false);
        iface(&sys, "tailscale0", None, false);
        assert_eq!(classify(&sys, "usb0"), LinkKind::Usb);
        assert_eq!(classify(&sys, "enp0s20f0u3"), LinkKind::Usb);
        assert_eq!(classify(&sys, "enx0011"), LinkKind::Usb);
        assert_eq!(classify(&sys, "wlan0"), LinkKind::Wifi);
        assert_eq!(classify(&sys, "enp3s0"), LinkKind::Eth);
        assert_eq!(classify(&sys, "tailscale0"), LinkKind::Other);
        // Sem nada no /sys: pelo nome.
        assert_eq!(classify(&sys, "eth9"), LinkKind::Eth);
        assert_eq!(classify(&sys, "ppp0"), LinkKind::Other);
    }

    #[test]
    fn classificacao_pelo_uevent_quando_nao_ha_symlink() {
        let sys = tmp("uevent");
        iface(&sys, "enx1", None, false);
        std::fs::write(
            sys.join("class/net/enx1/device/uevent"),
            "DRIVER=rndis_host\nINTERFACE=x\n",
        )
        .unwrap();
        assert_eq!(classify(&sys, "enx1"), LinkKind::Usb);
    }

    #[test]
    fn filtro_de_enderecos() {
        let sys = tmp("filtro");
        iface(&sys, "usb0", Some("rndis_host"), false);
        iface(&sys, "wlan0", None, true);
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        let raw = vec![
            ("lo".to_string(), ip("127.0.0.1")),
            ("wlan0".into(), ip("192.168.1.5")),
            ("usb0".into(), ip("192.168.42.129")),
            ("docker0".into(), ip("172.17.0.1")),
            ("virbr0".into(), ip("192.168.122.1")),
            ("veth12ab".into(), ip("10.1.1.1")),
            ("br-1a2b".into(), ip("172.18.0.1")),
            ("enp3s0".into(), ip("203.0.113.9")),  // público
            ("enp4s0".into(), ip("169.254.10.2")), // link-local
            ("tun0".into(), ip("10.8.0.2")),
        ];
        let got = filter_addrs(&sys, &raw);
        let ips: Vec<_> = got
            .iter()
            .map(|a| (a.iface.as_str(), a.ip.as_str(), a.kind))
            .collect();
        // Ordenado por IP (texto): determinístico para comparar listas.
        assert_eq!(
            ips,
            vec![
                ("tun0", "10.8.0.2", LinkKind::Other),
                ("enp4s0", "169.254.10.2", LinkKind::Eth),
                ("wlan0", "192.168.1.5", LinkKind::Wifi),
                ("usb0", "192.168.42.129", LinkKind::Usb),
            ]
        );
    }

    #[test]
    fn ip_addr_parse() {
        let t = "1: lo    inet 127.0.0.1/8 scope host lo\\       valid_lft forever\n\
                 3: wlan0    inet 192.168.1.5/24 brd 192.168.1.255 scope global dynamic wlan0\\ x\n\
                 7: usb0@if3    inet 192.168.42.129/24 brd 192.168.42.255 scope global usb0\\ x\n\
                 lixo\n";
        let v = parse_ip_addr(t);
        assert_eq!(v.len(), 3);
        assert_eq!(
            v[2],
            ("usb0".to_string(), "192.168.42.129".parse().unwrap())
        );
    }

    #[test]
    fn tipo_pelo_ip_local_e_rastreador() {
        let a = |ip: &str, kind| PcAddr {
            ip: ip.into(),
            kind,
            iface: "x".into(),
        };
        let l = vec![
            a("192.168.1.5", LinkKind::Wifi),
            a("192.168.42.129", LinkKind::Usb),
        ];
        assert_eq!(kind_for_ip(&l, "192.168.42.129"), LinkKind::Usb);
        assert_eq!(kind_for_ip(&l, "192.168.1.5"), LinkKind::Wifi);
        assert_eq!(kind_for_ip(&l, "8.8.8.8"), LinkKind::Other);

        let mut t = AddrTracker::default();
        assert!(t.changed(&l), "a primeira lista conta");
        assert!(!t.changed(&l));
        assert!(t.changed(&l[..1]), "a interface USB desceu");
        assert!(!t.changed(&l[..1]));
        t.reset();
        assert!(t.changed(&l[..1]));
    }

    #[test]
    fn nmcli_com_escapes() {
        let t = "Wired connection 1:usb0\nCasa\\: 2.4:wlan0\nsem-dispositivo:\nlo:lo\n";
        let v = parse_nmcli_active(t);
        assert_eq!(
            v,
            vec![
                ("Wired connection 1".to_string(), "usb0".to_string()),
                ("Casa: 2.4".to_string(), "wlan0".to_string()),
                ("lo".to_string(), "lo".to_string()),
            ]
        );
        assert_eq!(
            never_default_cmd("Wired \"x\" 1"),
            "nmcli connection modify \"Wired \\\"x\\\" 1\" ipv4.never-default yes ipv6.never-default yes"
        );
        assert_eq!(never_default_args("A")[2], "A");
    }

    /// Executor falso: `(programa args…)` → saída.
    struct Fake(BTreeMap<String, (bool, String)>);
    impl Runner for Fake {
        fn run(&self, prog: &str, args: &[&str]) -> Option<RunOut> {
            let key = format!("{prog} {}", args.join(" "));
            self.0.get(&key).map(|(ok, s)| RunOut {
                ok: *ok,
                stdout: s.clone(),
            })
        }
    }

    fn env_fake(sys: &Path, cmds: &[(&str, bool, &str)], tools: &'static [&'static str]) -> Env {
        let mut e = Env::real();
        e.sys = sys.to_path_buf();
        e.have = Box::new(move |t| tools.contains(&t));
        e.runner = Box::new(Fake(
            cmds.iter()
                .map(|(k, ok, s)| (k.to_string(), (*ok, s.to_string())))
                .collect(),
        ));
        e
    }

    const ADDR: &str = "wlan0 x";
    const IP_ADDR_CMD: &str = "ip -o -4 addr show up";
    const IP_OUT: &str = "3: wlan0    inet 192.168.1.5/24 scope global wlan0\n\
                          7: usb0    inet 192.168.42.129/24 scope global usb0\n";

    fn sys_usb(tag: &str) -> std::path::PathBuf {
        let sys = tmp(tag);
        iface(&sys, "usb0", Some("rndis_host"), false);
        iface(&sys, "wlan0", None, true);
        sys
    }

    #[test]
    fn doctor_sem_interface_usb_nao_diz_nada() {
        let _ = ADDR;
        let sys = tmp("sem-usb");
        iface(&sys, "wlan0", None, true);
        let e = env_fake(
            &sys,
            &[(
                IP_ADDR_CMD,
                true,
                "3: wlan0    inet 192.168.1.5/24 scope global wlan0\n",
            )],
            &[],
        );
        assert_eq!(usb_report(&e, 7443), None);
    }

    #[test]
    fn doctor_rota_por_omissao_pelo_usb_e_firewalld_fechada() {
        let sys = sys_usb("rota");
        let e = env_fake(
            &sys,
            &[
                (IP_ADDR_CMD, true, IP_OUT),
                (
                    "ip route show default",
                    true,
                    "default via 192.168.42.129 dev usb0 proto dhcp metric 100\ndefault via 192.168.1.1 dev wlan0 metric 600\n",
                ),
                ("ip -6 route show default", true, ""),
                (
                    "nmcli -t -f NAME,DEVICE connection show --active",
                    true,
                    "Casa:wlan0\nWired connection 1:usb0\n",
                ),
                ("firewall-cmd --state", true, "running\n"),
                (
                    "firewall-cmd --get-zone-of-interface usb0",
                    true,
                    "public\n",
                ),
                (
                    "firewall-cmd --zone=public --query-port 7443/udp",
                    false,
                    "no\n",
                ),
            ],
            &["firewall-cmd", "nmcli"],
        );
        let r = usb_report(&e, 7443).unwrap();
        assert_eq!(r.interfaces, vec!["usb0"]);
        assert!(r.default_route_via_usb);
        assert_eq!(r.connection.as_deref(), Some("Wired connection 1"));
        assert_eq!(
            r.never_default_cmd.as_deref(),
            Some(
                "nmcli connection modify \"Wired connection 1\" ipv4.never-default yes ipv6.never-default yes"
            )
        );
        let fw = r.firewall.unwrap();
        assert_eq!((fw.tool, fw.open), ("firewalld", Some(false)));
        assert_eq!(
            fw.command.as_deref(),
            Some("pkexec firewall-cmd --zone=public --add-port=7443/udp")
        );
    }

    #[test]
    fn doctor_rota_certa_e_firewall_aberta_nao_avisa() {
        let sys = sys_usb("certo");
        let e = env_fake(
            &sys,
            &[
                (IP_ADDR_CMD, true, IP_OUT),
                (
                    "ip route show default",
                    true,
                    "default via 192.168.1.1 dev wlan0 metric 600\n",
                ),
                ("ip -6 route show default", true, ""),
                (
                    "nmcli -t -f NAME,DEVICE connection show --active",
                    true,
                    "Wired connection 1:usb0\n",
                ),
                ("firewall-cmd --state", true, "running\n"),
                (
                    "firewall-cmd --get-zone-of-interface usb0",
                    true,
                    "trusted\n",
                ),
                (
                    "firewall-cmd --zone=trusted --query-port 7443/udp",
                    true,
                    "yes\n",
                ),
            ],
            &["firewall-cmd", "nmcli"],
        );
        let r = usb_report(&e, 7443).unwrap();
        assert!(!r.default_route_via_usb);
        assert_eq!(r.never_default_cmd, None);
        let fw = r.firewall.unwrap();
        assert_eq!((fw.open, fw.command), (Some(true), None));
    }

    #[test]
    fn doctor_ufw_sem_root_sugere_so_nessa_interface() {
        let sys = sys_usb("ufw");
        let e = env_fake(
            &sys,
            &[
                (IP_ADDR_CMD, true, IP_OUT),
                ("ip route show default", true, ""),
                ("ip -6 route show default", true, ""),
                ("ufw status", false, ""),
            ],
            &["ufw"],
        );
        let r = usb_report(&e, 7443).unwrap();
        let fw = r.firewall.unwrap();
        assert_eq!((fw.tool, fw.open), ("ufw", None));
        assert_eq!(
            fw.command.as_deref(),
            Some("pkexec ufw allow in on usb0 to any port 7443 proto udp")
        );
    }

    #[test]
    fn doctor_ufw_ativa_com_regra_nao_avisa() {
        let sys = sys_usb("ufw2");
        let e = env_fake(
            &sys,
            &[
                (IP_ADDR_CMD, true, IP_OUT),
                ("ip route show default", true, ""),
                ("ip -6 route show default", true, ""),
                (
                    "ufw status",
                    true,
                    "Status: active\n\n7443/udp ALLOW Anywhere\n",
                ),
            ],
            &["ufw"],
        );
        let fw = usb_report(&e, 7443).unwrap().firewall.unwrap();
        assert_eq!((fw.open, fw.command), (Some(true), None));
    }
}
