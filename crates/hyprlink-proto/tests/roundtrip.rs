//! Round-trip CBOR (o socket local) e JSON (hyprlinkctl / plugins) de todos os
//! tipos do contrato. Compara pelo valor JSON: encode → decode → encode tem de
//! dar exatamente o mesmo.

use hyprlink_proto::link::mock::Simulator;
use hyprlink_proto::link::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

fn cbor<T: Serialize + DeserializeOwned>(v: &T) -> T {
    let mut buf = Vec::new();
    ciborium::into_writer(v, &mut buf).expect("cbor encode");
    ciborium::from_reader(buf.as_slice()).expect("cbor decode")
}

fn json<T: Serialize + DeserializeOwned>(v: &T) -> T {
    serde_json::from_str(&serde_json::to_string(v).expect("json encode")).expect("json decode")
}

fn same<T: Serialize + DeserializeOwned>(v: &T) {
    let want = serde_json::to_value(v).unwrap();
    assert_eq!(serde_json::to_value(cbor(v)).unwrap(), want, "CBOR: {want}");
    assert_eq!(serde_json::to_value(json(v)).unwrap(), want, "JSON: {want}");
}

fn device(full: bool) -> Device {
    Device {
        id: 7,
        name: "Poco F4 do Maggio".into(),
        manufacturer: full.then(|| "Xiaomi".into()),
        model: full.then(|| "22021211RG".into()),
        android: full.then(|| "15".into()),
        app_version: full.then(|| "1.0.009-alpha".into()),
        kind: Kind::Phone,
        state: if full {
            LinkState::Linked
        } else {
            LinkState::Offline
        },
        fingerprint: "3f8a2817d3259e730a56035443aa0a3e69f974ce62c5cb96eebdc1eaa13db21d".into(),
        addr: full.then(|| "192.168.1.42:51234".into()),
        battery: full.then_some(78),
        charging: full,
        rssi: full.then_some(-53),
        latency_ms: full.then_some(3.3),
        caps: if full {
            vec![
                Cap::Clipboard,
                Cap::Mic,
                Cap::Tap,
                Cap::Speaker,
                Cap::Webcam,
            ]
        } else {
            vec![]
        },
        paired_since: full.then_some(1_791_000_000),
    }
}

fn phone(full: bool) -> PhoneStatus {
    if !full {
        return PhoneStatus::default();
    }
    PhoneStatus {
        network: Some(Network::Cellular),
        cell_gen: Some("5G".into()),
        carrier: Some("Woo".into()),
        signal_bars: Some(3),
        wifi: Some(Wifi {
            ssid: None,
            rssi_dbm: Some(-61),
        }),
        storage_used_b: Some(98_000_000_000),
        storage_total_b: Some(256_000_000_000),
        ram_used_b: Some(5_100_000_000),
        ram_total_b: Some(8_000_000_000),
        battery_temp_c: Some(31.7),
        screen_on: Some(true),
        dnd: Some(false),
        notifications: Some(4),
        now_playing: Some(NowPlaying {
            title: "Dança Rápida".into(),
            artist: Some("—".into()),
            app: Some("com.spotify.music".into()),
            playing: true,
            position_ms: Some(61_000),
            duration_ms: Some(203_000),
        }),
    }
}

fn all_commands() -> Vec<Command> {
    vec![
        Command::Ping(7),
        Command::SendClipboard(7),
        Command::SwitchWorkspace(3),
        Command::SetMic(true),
        Command::SetTap(false),
        Command::SetSpeakerMode(true),
        Command::StartMirror(MirrorConfig {
            codec: Codec::Hevc,
            bitrate_mbps: 12.0,
            max_fps: 60,
            scale: 0.75,
        }),
        Command::StopMirror,
        Command::SetSensorBridge(SensorKind::Light, true),
        Command::SetRule(2, false),
        Command::BeginPairing,
        Command::CancelPairing,
        Command::Unpair(7),
    ]
    .into_iter()
    .chain(all_commands2().into_iter().map(Command::More))
    .collect()
}

fn all_commands2() -> Vec<Command2> {
    use Command2::*;
    vec![
        SetBatteryAlerts(BatteryAlerts {
            low: Some(20),
            full: true,
        }),
        SetBatteryAlerts(BatteryAlerts {
            low: None,
            full: false,
        }),
        RunDispatch("workspace 3".into()),
        SetShortcuts(vec![Shortcut {
            label: "Bloquear".into(),
            dispatch: "exec hyprlock".into(),
        }]),
        SetTrackpad(TrackpadConfig {
            sensitivity: 1.0,
            scroll: 1.5,
            acceleration: true,
            natural_scroll: false,
            keyboard: true,
        }),
        StartWebcam(WebcamConfig {
            width: 1920,
            height: 1080,
            fps: 30,
            codec: CamCodec::H264,
        }),
        StopWebcam,
        TestNetwork,
        SetPhoneVolume(PhoneStream::Media, 9),
        SetRinger(Ringer::Vibrate),
        SetDnd(true),
        SetSinkVolume(42, 150),
        SetSinkMute(42, true),
        SetDefaultSink(42),
        SetAppVolume(77, 80),
        SetAppMute(77, false),
        DismissNotification("0|com.whatsapp|1|null|10123".into()),
        DismissAllNotifications,
        CopyClip(3),
        SendClipToPhone(3),
        PinClip(3, true),
        DeleteClip(3),
        SendFile("~/Transferências/foto 1.jpg".into()),
        CancelTransfer(9),
        OpenDownloads,
        Media {
            player: "spotify".into(),
            action: MediaAction::PlayPause,
        },
        PhoneMedia(MediaAction::Next),
        SetDownloadsDir("~/Transferências/HyprLink".into()),
        RenameDevice(7, "Poco F4".into()),
        RestartDaemon,
        ReplyNotification {
            key: "0|org.thoughtcrime.securesms|3".into(),
            idx: 1,
            text: "Já vou, 5 min".into(),
        },
        SetGesture("swipe_left_3".into(), false),
        ClearFileHistory,
    ]
}

fn all_events() -> Vec<Event> {
    vec![
        Event::Devices(vec![device(true), device(false)]),
        Event::Devices(vec![]),
        Event::Workspaces {
            list: vec![Workspace {
                id: 3,
                monitor: "eDP-1".into(),
                clients: vec!["kitty".into(), "code".into()],
            }],
            active: 3,
        },
        Event::Telemetry {
            latency_ms: 3.3,
            up_kbps: 32.0,
            down_kbps: 14.0,
        },
        Event::Sensors(Sensors {
            accel: [0.1, -0.2, 9.8],
            gyro: [0.0, 0.01, -0.02],
            lux: 466.0,
            proximity_cm: 5.0,
            pressure_hpa: 1012.5,
            battery_temp: 31.7,
        }),
        Event::Levels {
            mic: None,
            tap: Some(0.42),
        },
        Event::SpeakerMode(true),
        Event::Mirror(None),
        Event::Mirror(Some(MirrorStats {
            fps: 58.0,
            decode_ms: 3.3,
            dropped: 4,
            kbps: 11_800.0,
        })),
        Event::Rssi(-53),
        Event::Packet(Packet {
            at: 12.442,
            dir: Dir::Rx,
            kind: packets::BATTERY_STATE.into(),
            bytes: 32,
            note: "78".into(),
        }),
        Event::Pairing(None),
        Event::Pairing(Some(PairingTicket {
            payload: "3F:8A|192.168.1.100:7443|c1ab918e137fca8c80e72666589d70f0".into(),
            code: None,
            expires_in: None,
        })),
        Event::Notice(Notice::PingReply {
            device: 7,
            rtt_ms: 6.6,
        }),
        Event::Notice(Notice::ClipboardSent { device: 7 }),
        Event::Notice(Notice::Paired {
            device: 7,
            name: "Poco F4".into(),
        }),
        Event::Notice(Notice::Revoked {
            device: 7,
            name: "Poco F4".into(),
        }),
        Event::Notice(Notice::MirrorStarted),
        Event::Notice(Notice::Failed {
            op: Op::Mirror,
            error: ErrorKind::NotImplemented,
        }),
        Event::Notice(Notice::Failed {
            op: Op::Ping,
            error: ErrorKind::Offline,
        }),
        Event::Phone(phone(true)),
        Event::Phone(phone(false)),
    ]
}

#[test]
fn commands_round_trip() {
    for c in all_commands() {
        same(&c);
    }
}

#[test]
fn events_round_trip() {
    for e in all_events() {
        same(&e);
    }
}

/// Tudo o que o simulador produz em 10 s também atravessa os dois formatos.
#[test]
fn simulator_events_round_trip() {
    let mut sim = Simulator::new();
    sim.send(Command::BeginPairing);
    let mut n = 0;
    for _ in 0..200 {
        for e in sim.poll(Duration::from_millis(50)) {
            same(&e);
            n += 1;
        }
    }
    assert!(n > 100, "o simulador quase não emitiu eventos ({n})");
}

/// Valor em falta vai como `null`, nunca como 0 ou "".
#[test]
fn missing_is_null() {
    let v = serde_json::to_value(device(false)).unwrap();
    for k in [
        "manufacturer",
        "model",
        "battery",
        "rssi",
        "latency_ms",
        "addr",
    ] {
        assert!(v[k].is_null(), "{k} devia ser null: {v}");
    }
    let p = serde_json::to_value(phone(false)).unwrap();
    assert!(p.as_object().unwrap().values().all(|x| x.is_null()), "{p}");
}

/// O daemon nunca envia texto para humanos: a única string livre num erro é
/// um enum em snake_case.
#[test]
fn notice_has_no_prose() {
    let v = serde_json::to_value(Notice::Failed {
        op: Op::Speaker,
        error: ErrorKind::Refused,
    })
    .unwrap();
    assert_eq!(
        v,
        serde_json::json!({"Failed": {"op": "speaker", "error": "refused"}})
    );
}

/// O seletor da webcam: só o que o telemóvel codifica. MJPEG fica na variante
/// (um pedido antigo ainda descodifica) mas já não se oferece.
#[test]
fn camera_codecs_offered_and_wire_names() {
    assert_eq!(CamCodec::ALL, [CamCodec::H264, CamCodec::H265]);
    assert!(!CamCodec::ALL.contains(&CamCodec::Mjpeg));
    for (c, name) in [
        (CamCodec::H264, "h264"),
        (CamCodec::H265, "h265"),
        (CamCodec::Mjpeg, "mjpeg"),
    ] {
        assert_eq!(serde_json::to_value(c).unwrap(), serde_json::json!(name));
        same(&c);
    }
}

/// `codec` em `WebcamStats` é aditivo: um daemon antigo, sem a chave, ainda
/// descodifica (`None`); e o novo leva o codec efetivo.
#[test]
fn webcam_stats_codec_is_additive() {
    let old: WebcamStats = serde_json::from_str(r#"{"mbps": 6.2, "fps": null}"#).unwrap();
    assert_eq!(old.codec, None);
    let now = WebcamStats {
        mbps: 6.2,
        fps: None,
        codec: Some(CamCodec::H265),
    };
    same(&now);
    assert_eq!(serde_json::to_value(now).unwrap()["codec"], "h265");
}

#[test]
fn gestures_contract() {
    let rules = default_gesture_rules();
    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, GESTURES, "as regras de origem seguem GESTURES");
    same(&Event2::Gestures {
        rules,
        last: Some(GestureLast {
            name: "volume".into(),
            at_unix: 1_791_100_000,
        }),
    });
    same(&Event2::Gestures {
        rules: vec![],
        last: None,
    });
}
