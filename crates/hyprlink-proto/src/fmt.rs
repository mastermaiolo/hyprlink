//! Everything a human reads about the link is written here — never on the
//! wire. Source text is pt-PT; [`crate::i18n`] turns it into the current
//! language. The daemon sends data; this module turns it into words.

use crate::i18n::{self, t};
use crate::link::{
    CamCodec, Cap, Codec, Device, ErrorKind, Kind, LinkState, Network, Notice, Op, Origin,
    PhoneStatus, PhoneStream, Ringer, TransferState,
};

pub const DASH: &str = "—";

pub fn kind(k: Kind) -> &'static str {
    match k {
        Kind::Phone => t("TELEMÓVEL"),
        Kind::Tablet => t("TABLET"),
        Kind::Wearable => t("WEARABLE"),
    }
}

pub fn state(s: LinkState) -> &'static str {
    match s {
        LinkState::Linked => t("LIGADO"),
        LinkState::Idle => t("EM ESPERA"),
        LinkState::Offline => t("FORA DE ALCANCE"),
    }
}

pub fn phase(p: crate::link::LinkPhase) -> &'static str {
    use crate::link::LinkPhase::*;
    match p {
        Connected => t("LIGADO"),
        Connecting => t("A LIGAR…"),
        Disconnected => t("DESLIGADO"),
    }
}

pub fn cap(c: Cap) -> &'static str {
    match c {
        Cap::Workspaces => t("workspaces"),
        Cap::Mirror => t("espelho"),
        Cap::Webcam => t("câmara"),
        Cap::Mic => t("mic"),
        Cap::Tap => t("retorno"),
        Cap::Speaker => t("coluna"),
        Cap::Sensors => t("sensores"),
        Cap::Presence => t("presença"),
        Cap::Clipboard => t("clipboard"),
        Cap::Files => t("ficheiros"),
        Cap::Notifications => t("notificações"),
        Cap::Input => t("trackpad"),
        Cap::PhoneStatus => t("estado do telemóvel"),
        Cap::MediaSession => t("multimédia do telemóvel"),
    }
}

pub fn codec(c: Codec) -> &'static str {
    match c {
        Codec::H264 => "H.264",
        Codec::Hevc => "HEVC",
        Codec::Av1 => "AV1",
    }
}

/// Display name: uppercase for Anton.
pub fn name(d: &Device) -> String {
    d.name.to_uppercase()
}

/// "Xiaomi 22021211RG" / "22021211RG" / "—".
pub fn model(d: &Device) -> String {
    match (&d.manufacturer, &d.model) {
        (Some(m), Some(x)) => format!("{m} {x}"),
        (None, Some(x)) => x.clone(),
        (Some(m), None) => m.clone(),
        _ => DASH.into(),
    }
}

/// "Android 15" / "—".
pub fn os(d: &Device) -> String {
    d.android
        .as_ref()
        .map(|v| format!("Android {v}"))
        .unwrap_or_else(|| DASH.into())
}

pub fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| DASH.into())
}

pub fn battery(d: &Device) -> String {
    d.battery
        .map(|b| b.to_string())
        .unwrap_or_else(|| DASH.into())
}

pub fn latency(v: Option<f32>) -> String {
    v.map(|v| decimal(v as f64, 1))
        .unwrap_or_else(|| DASH.into())
}

/// Lowercase hex → two lines of four groups, uppercase, `·`-free.
pub fn fingerprint(hex: &str) -> (String, String) {
    let up: Vec<String> = hex
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_uppercase())
        .collect();
    let half = up.len().min(8) / 2;
    let a = up.iter().take(half).cloned().collect::<Vec<_>>().join(" ");
    let b = up
        .iter()
        .skip(half)
        .take(half)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    (a, b)
}

/// Unix seconds → "2026-08-14".
pub fn date(unix: Option<u64>) -> String {
    let Some(s) = unix else { return DASH.into() };
    chrono::DateTime::from_timestamp(s as i64, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| DASH.into())
}

pub fn gb(bytes: Option<u64>) -> Option<f32> {
    bytes.map(|b| b as f32 / 1_000_000_000.0)
}

/// Big word + small rest, e.g. ("5G", "Woo") or ("Wi-Fi", "Casa-5G").
pub fn network(p: &PhoneStatus) -> (String, String) {
    match p.network {
        Some(Network::Cellular) => (
            p.cell_gen.clone().unwrap_or_else(|| t("REDE").into()),
            p.carrier.clone().unwrap_or_default(),
        ),
        Some(Network::Wifi) => (
            "Wi-Fi".into(),
            p.wifi
                .as_ref()
                .and_then(|w| w.ssid.clone())
                .unwrap_or_default(),
        ),
        Some(Network::Ethernet) => ("LAN".into(), String::new()),
        Some(Network::None) => (t("SEM REDE").into(), String::new()),
        None => (DASH.into(), String::new()),
    }
}

/// One line: "5G · Woo" / "Wi-Fi · Casa-5G".
pub fn network_line(p: &PhoneStatus) -> String {
    let (a, b) = network(p);
    if b.is_empty() {
        a
    } else {
        format!("{a} · {b}")
    }
}

fn op(o: Op) -> &'static str {
    match o {
        Op::Ping => t("Ping"),
        Op::Clipboard => t("Área de transferência"),
        Op::Mic => t("Microfone"),
        Op::Tap => t("Retorno"),
        Op::Speaker => t("Modo coluna"),
        Op::Mirror => t("Espelho"),
        Op::Sensors => t("Sensores"),
        Op::Presence => t("Presença"),
        Op::Pairing => t("Emparelhamento"),
        Op::Workspace => t("Workspace"),
        Op::Webcam => t("Câmara"),
        Op::Files => t("Ficheiros"),
        Op::Media => t("Multimédia"),
        Op::Dispatch => t("Dispatch"),
        Op::PhoneAudio => t("Volume do telemóvel"),
    }
}

/// A toast line. `lookup` resolves device ids to display names.
pub fn notice(n: &Notice, lookup: impl Fn(u32) -> Option<String>) -> String {
    let who = |id: u32| lookup(id).unwrap_or_else(|| format!("#{id}"));
    match n {
        Notice::PingReply { device, rtt_ms } => {
            let rtt = decimal(*rtt_ms as f64, 1);
            crate::tr!("PING · {} respondeu em {} ms", who(*device), rtt)
        }
        Notice::ClipboardSent { device } => crate::tr!("ÁREA DE TRANSFERÊNCIA → {}", who(*device)),
        Notice::Paired { name, .. } => crate::tr!("EMPARELHADO · {}", name.to_uppercase()),
        Notice::Revoked { name, .. } => {
            crate::tr!("{} · certificado revogado", name.to_uppercase())
        }
        Notice::MirrorStarted => t("ESPELHO · transmissão iniciada").into(),
        Notice::Failed { op: o, error } => crate::tr!(
            "{} · {}",
            op(*o).to_uppercase(),
            match error {
                ErrorKind::NotImplemented => t("ainda por implementar"),
                ErrorKind::Offline => t("sem telemóvel ligado"),
                ErrorKind::Refused => t("recusado pelo telemóvel"),
                ErrorKind::Timeout => t("sem resposta"),
            }
        ),
    }
}

pub fn stream(s: PhoneStream) -> &'static str {
    match s {
        PhoneStream::Media => t("Multimédia"),
        PhoneStream::Ring => t("Toque"),
        PhoneStream::Notification => t("Notificações"),
        PhoneStream::Alarm => t("Alarme"),
    }
}

pub fn ringer(r: Ringer) -> &'static str {
    match r {
        Ringer::Normal => t("SOM"),
        Ringer::Vibrate => t("VIBRAR"),
        Ringer::Silent => t("SILÊNCIO"),
    }
}

pub fn cam_codec(c: CamCodec) -> &'static str {
    match c {
        CamCodec::Mjpeg => "MJPEG",
        CamCodec::H264 => "H.264",
    }
}

pub fn origin(o: Origin) -> &'static str {
    match o {
        Origin::Phone => t("TELEMÓVEL"),
        Origin::Pc => t("PC"),
    }
}

pub fn transfer_state(s: TransferState) -> &'static str {
    match s {
        TransferState::Active => t("A TRANSFERIR"),
        TransferState::Done => t("CONCLUÍDO"),
        TransferState::Failed => t("FALHOU"),
        TransferState::Cancelled => t("CANCELADO"),
    }
}

/// `v` with `places` decimals; comma in pt-PT / pt-BR / es, point in en / zh.
pub fn decimal(v: f64, places: usize) -> String {
    let out = format!("{v:.places$}");
    if i18n::get().decimal_comma() {
        out.replace('.', ",")
    } else {
        out
    }
}

/// 1 234 567 bytes → "1,2 MB" (decimal comma in pt/es, point in en/zh).
pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    let (v, u) = if b >= 1e9 {
        (b / 1e9, "GB")
    } else if b >= 1e6 {
        (b / 1e6, "MB")
    } else if b >= 1e3 {
        (b / 1e3, "KB")
    } else {
        return format!("{bytes} B");
    };
    format!("{} {u}", decimal(v, 1))
}

/// Short month name in the current language (`month0` is 0–11).
pub fn month(month0: usize) -> &'static str {
    const M: [&str; 12] = [
        "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
    ];
    t(M[month0.min(11)])
}

/// Unix seconds → "14:05" (today) or "3 out 14:05".
pub fn time(unix: u64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(t) = Local.timestamp_opt(unix as i64, 0).single() else {
        return DASH.into();
    };
    let now = Local::now();
    if t.date_naive() == now.date_naive() {
        t.format("%H:%M").to_string()
    } else {
        crate::tr!(
            "{0} {1} {2}",
            t.day(),
            month(t.month0() as usize),
            t.format("%H:%M")
        )
    }
}

/// "há 3 min" style, for lists.
pub fn ago(unix: u64) -> String {
    let now = chrono::Local::now().timestamp().max(0) as u64;
    let d = now.saturating_sub(unix);
    match d {
        0..=59 => t("agora").into(),
        60..=3599 => crate::tr!("há {} min", d / 60),
        3600..=86_399 => crate::tr!("há {} h", d / 3600),
        _ => time(unix),
    }
}

/// ms → "3:07".
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}
