//! Everything a human reads about the link is written here — never on the
//! wire. pt-PT. The daemon sends data; this module turns it into words.

use crate::link::{
    CamCodec, Cap, Codec, Device, ErrorKind, Kind, LinkState, Network, Notice, Op, Origin,
    PhoneStatus, PhoneStream, Ringer, TransferState,
};

pub const DASH: &str = "—";

pub fn kind(k: Kind) -> &'static str {
    match k {
        Kind::Phone => "TELEMÓVEL",
        Kind::Tablet => "TABLET",
        Kind::Wearable => "WEARABLE",
    }
}

pub fn state(s: LinkState) -> &'static str {
    match s {
        LinkState::Linked => "LIGADO",
        LinkState::Idle => "EM ESPERA",
        LinkState::Offline => "FORA DE ALCANCE",
    }
}

pub fn phase(p: crate::link::LinkPhase) -> &'static str {
    use crate::link::LinkPhase::*;
    match p {
        Connected => "LIGADO",
        Connecting => "A LIGAR…",
        Disconnected => "DESLIGADO",
    }
}

pub fn cap(c: Cap) -> &'static str {
    match c {
        Cap::Workspaces => "workspaces",
        Cap::Mirror => "espelho",
        Cap::Webcam => "câmara",
        Cap::Mic => "mic",
        Cap::Tap => "retorno",
        Cap::Speaker => "coluna",
        Cap::Sensors => "sensores",
        Cap::Presence => "presença",
        Cap::Clipboard => "clipboard",
        Cap::Files => "ficheiros",
        Cap::Notifications => "notificações",
        Cap::Input => "trackpad",
        Cap::PhoneStatus => "estado do telemóvel",
        Cap::MediaSession => "mídia do telemóvel",
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
    v.map(|v| format!("{v:.1}")).unwrap_or_else(|| DASH.into())
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
            p.cell_gen.clone().unwrap_or_else(|| "REDE".into()),
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
        Some(Network::None) => ("SEM REDE".into(), String::new()),
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
        Op::Ping => "Ping",
        Op::Clipboard => "Área de transferência",
        Op::Mic => "Microfone",
        Op::Tap => "Retorno",
        Op::Speaker => "Modo coluna",
        Op::Mirror => "Espelho",
        Op::Sensors => "Sensores",
        Op::Presence => "Presença",
        Op::Pairing => "Emparelhamento",
        Op::Workspace => "Workspace",
        Op::Webcam => "Câmara",
        Op::Files => "Ficheiros",
        Op::Media => "Multimédia",
        Op::Dispatch => "Dispatch",
        Op::PhoneAudio => "Volume do telemóvel",
    }
}

/// A toast line. `lookup` resolves device ids to display names.
pub fn notice(n: &Notice, lookup: impl Fn(u32) -> Option<String>) -> String {
    let who = |id: u32| lookup(id).unwrap_or_else(|| format!("#{id}"));
    match n {
        Notice::PingReply { device, rtt_ms } => {
            format!("PING · {} respondeu em {rtt_ms:.1} ms", who(*device))
        }
        Notice::ClipboardSent { device } => format!("ÁREA DE TRANSFERÊNCIA → {}", who(*device)),
        Notice::Paired { name, .. } => format!("EMPARELHADO · {}", name.to_uppercase()),
        Notice::Revoked { name, .. } => format!("{} · certificado revogado", name.to_uppercase()),
        Notice::MirrorStarted => "ESPELHO · transmissão iniciada".into(),
        Notice::Failed { op: o, error } => format!(
            "{} · {}",
            op(*o).to_uppercase(),
            match error {
                ErrorKind::NotImplemented => "ainda por implementar",
                ErrorKind::Offline => "sem telemóvel ligado",
                ErrorKind::Refused => "recusado pelo telemóvel",
                ErrorKind::Timeout => "sem resposta",
            }
        ),
    }
}

pub fn stream(s: PhoneStream) -> &'static str {
    match s {
        PhoneStream::Media => "Multimédia",
        PhoneStream::Ring => "Toque",
        PhoneStream::Notification => "Notificações",
        PhoneStream::Alarm => "Alarme",
    }
}

pub fn ringer(r: Ringer) -> &'static str {
    match r {
        Ringer::Normal => "SOM",
        Ringer::Vibrate => "VIBRAR",
        Ringer::Silent => "SILÊNCIO",
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
        Origin::Phone => "TELEMÓVEL",
        Origin::Pc => "PC",
    }
}

pub fn transfer_state(s: TransferState) -> &'static str {
    match s {
        TransferState::Active => "A TRANSFERIR",
        TransferState::Done => "CONCLUÍDO",
        TransferState::Failed => "FALHOU",
        TransferState::Cancelled => "CANCELADO",
    }
}

/// 1 234 567 bytes → "1,2 MB" (pt-PT decimal comma).
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
    format!("{v:.1} {u}").replace('.', ",")
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
        const M: [&str; 12] = [
            "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
        ];
        format!(
            "{} {} {}",
            t.day(),
            M[t.month0() as usize],
            t.format("%H:%M")
        )
    }
}

/// "há 3 min" style, for lists.
pub fn ago(unix: u64) -> String {
    let now = chrono::Local::now().timestamp().max(0) as u64;
    let d = now.saturating_sub(unix);
    match d {
        0..=59 => "agora".into(),
        60..=3599 => format!("há {} min", d / 60),
        3600..=86_399 => format!("há {} h", d / 3600),
        _ => time(unix),
    }
}

/// ms → "3:07".
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}
