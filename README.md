<!-- MAIOLO / SYSTEMS LAB — HYPRLINK -->

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="HyprLink — Android ⇄ Hyprland Ecosystem Integration">
</p>

<p align="center"><sub><strong>ANDROID ⇄ LINUX/HYPRLAND · QUIC + mTLS · CBOR · CONTINUITY · UINPUT</strong></sub></p>

<p align="center">
  🇬🇧 <strong>English (UK)</strong>
  · <a href="README.pt-br.md">🇧🇷 Português (BR)</a>
  · <a href="README.pt-pt.md">🇵🇹 Português (PT)</a>
  · <a href="README.es-es.md">🇪🇸 Español (ES)</a>
  · <a href="README.zh-cn.md">🇨🇳 简体中文</a>
  · <a href="CHANGELOG.md">CHANGELOG</a>
  · <a href="PROTOCOL.md">PROTOCOL</a>
</p>

> **Android ⇄ Linux/Hyprland ecosystem integration** — in the spirit of Apple Continuity/Handoff, engineered specifically for Hyprland. Clipboard, notifications, media, battery, remote touchpad, keyboard, file transfer, webcam, audio and compositor control synced in real time over local QUIC + mTLS with zero cloud relay.

> [!NOTE]
> **Alpha (0.1.0).** Stable for everyday development use, but the wire protocol may evolve between releases and select modules remain in testing (see [03 / Subsystem Matrix](#03--subsystem-matrix)). Bug reports and contributions are welcome.

<p align="center">
  <img src="assets/hyprlink_tour.gif" width="720" alt="Tour of the HyprLink desktop application: cover, devices, desk, camera, audio, notifications, sharing, media, sensors and diary">
</p>

---

## 01 / AT A GLANCE

<p align="center">
  <img src="assets/readme/at-a-glance.svg" width="100%" alt="HyprLink system profile at a glance">
</p>

The link is established directly on your local network over **QUIC + mTLS** (mutual authentication using self-signed certificates and fingerprint pinning); the control protocol is serialised in **CBOR**. There is no relay server: your phone communicates straight to your PC on **7443/UDP**.

---

## 02 / CAPABILITIES

<p align="center">
  <img src="assets/readme/capabilities.svg" width="100%" alt="HyprLink capabilities matrix">
</p>

### What is actually happening?

- **Notifications**: phone alerts mirrored to desktop with action triggers, remote dismissal, and active list restored upon reconnection.
- **Two-way clipboard**: seamless clipboard sync, including raw UTF-8 text and lossless PNG image buffers.
- **File transfer**: bidirectional transfer queue, real-time progress tracking, transfer history, and configurable download destination.
- **Media**: control desktop MPRIS players from your phone with playback progress and album artwork; desktop playback is mirrored to phone notifications.
- **Phone as webcam**: stream phone camera as a virtual video device (`/dev/video42` via `v4l2loopback`) using hardware H.264 or H.265.
- **Audio bridge**: phone microphone as PC input, phone as desktop speaker, and PC audio streamed to phone.
- **Remote touchpad & keyboard**: `/dev/uinput` integration providing cursor movement, click/drag, live typing, Enter, and hotkeys with fail-safe key release on disconnect.
- **Quick actions**: lock screen, suspend, full/area screenshot (copied to clipboard and stored), volume adjustment, media keys, and arbitrary `hyprctl dispatch` triggers.
- **Hyprland IPC**: workspace switching, active window lists, and live event monitoring; phone gestures mapped to dispatches (supports both classic and `hyprland.lua` configurations).
- **Telemetry & Wake-on-LAN**: bidirectional battery indicators; PC CPU, RAM, hwmon temperatures, free disk space and GPU metrics displayed on mobile; PC MAC address broadcast for remote wake.
- **Desktop GUI**: standalone reactive interface (`iced`) with system tray integration and multilingual localisation (English UK, PT-BR, PT-PT, ES-ES, ZH-CN).

---

## 03 / SUBSYSTEM MATRIX

<p align="center">
  <img src="assets/readme/showcase.svg" width="100%" alt="HyprLink subsystem integration matrix">
</p>

| Subsystem | State | Notes |
|---|---|---|
| **Connectivity** (QUIC/mTLS, QR pairing) | ✅ Ready | Mutual certificate pinning on 7443/UDP |
| **Desktop GUI** (iced, separate daemon, tray) | ✅ Ready | Dedicated process, system tray via ksni |
| **Two-way clipboard** | ✅ Ready | UTF-8 text and PNG images |
| **Hyprland IPC** (workspaces, windows, dispatch) | ✅ Ready | Socket2 event integration |
| **Battery sync** (PC ⇄ Phone) | ✅ Ready | Bidirectional telemetry and alerts |
| **Media** (MPRIS control, art, notifications) | ✅ Ready | D-Bus integration via zbus |
| **Remote touchpad & keyboard** | ✅ Ready | Direct /dev/uinput driver emulation |
| **Quick actions** (lock, suspend, screenshot) | ✅ Ready | Integration with hyprlock, noctalia, grim |
| **Notifications** (mirror, actions, dismiss) | ✅ Ready | Restores active notifications on reconnect |
| **File transfer** (queue, history, custom folder) | ✅ Ready | QUIC unidirectional binary stream |
| **Telemetry on phone** (CPU, RAM, temp, GPU, disk) | 🚧 Testing | Hwmon/NVIDIA GPU sleep states under validation |
| **Mouse buttons & drag** (`input.button`) | 🚧 Testing | Daemon and Android ready; pending field verification |
| **Phone gestures** (`gesture`) | 🚧 Testing | Daemon, GUI and Android ready; pending verification |
| **Audio bridge** (mic, speaker, tap stream) | 🚧 Testing | PipeWire and GStreamer pipeline verification |
| **Virtual webcam** (H.264 / H.265 on /dev/video42) | 🚧 Testing | v4l2loopback module loading via pkexec |

Full protocol details are documented in [`PROTOCOL.md`](PROTOCOL.md) and version history in [`CHANGELOG.md`](CHANGELOG.md).

---

## 04 / INSTALL

### Requirements

Tested on **Arch Linux / CachyOS** with **Hyprland** (Wayland). The package identifiers below reflect Arch Linux:

| Domain | Required Packages (Arch Linux) |
|---|---|
| **Base / Core** | `hyprland`, `pipewire`, `wireplumber`, `libpulse` (`pactl`), `wl-clipboard` |
| **Audio & Camera (GStreamer)** | `gst-plugins-base-libs`, `gst-plugins-good`, `gst-plugins-bad-libs` (H.265), `gst-libav` (H.264/H.265), `gst-plugin-pipewire` |
| **Virtual Webcam** | `v4l2loopback-dkms` (loaded onto `/dev/video42` via `pkexec`) |
| **GUI File Picker** | `xdg-desktop-portal` + `xdg-desktop-portal-gtk` |
| **Quick Actions (Optional)** | Lock: `noctalia`, `hyprlock`, or `loginctl`; Screenshots: `grimblast` or `grim` + `slurp` |
| **Build Toolchain** | `rust`, `clang`, `lld`, `pkgconf` |

> [!IMPORTANT]
> The remote touchpad and keyboard write directly to `/dev/uinput`. Ensure your user has write access (verify with `getfacl /dev/uinput`).

### Fast path: Pre-built binary tarball (x86_64)

Download `hyprlink-0.1.0-linux-x86_64.tar.gz` from [GitHub Releases](https://github.com/mastermaiolo/hyprlink/releases):

```bash
tar -xzf hyprlink-0.1.0-linux-x86_64.tar.gz
cd hyprlink-0.1.0-linux-x86_64
./install.sh                                     # Installs to ~/.local (no root required)
systemctl --user enable --now hyprlink-bridge   # Start daemon user unit
hyprlink-gui                                     # Launch desktop GUI
```

`./install.sh` supports `--prefix DIR` (defaults to `~/.local`), `--dry-run`, and validates dependencies. Run `./uninstall.sh` to remove installed files.

### Arch Linux (PKGBUILD)

A dedicated local PKGBUILD is supplied in `packaging/arch/` under the name `hyprlink-bridge`:

```bash
git clone https://github.com/mastermaiolo/hyprlink.git
cd hyprlink/packaging/arch
makepkg -si
systemctl --user enable --now hyprlink-bridge
hyprlink-gui
```

This installs `hyprlink-daemon`, `hyprlink-gui`, and `hyprlinkctl` into `/usr/bin`, registers the `hyprlink-bridge.service` unit, and adds desktop launchers. To build from current working tree before tagging: `HYPRLINK_LOCAL=1 makepkg -si`.

<details>
<summary><strong>Session autostart and uwsm integration</strong></summary>

The background service targets `graphical-session.target`.
- **With `uwsm`**: activation is automatic.
- **Without `uwsm`**: ensure session environment variables are imported by placing in `hyprland.conf`:
  ```ini
  exec-once = dbus-update-activation-environment --systemd --all
  exec-once = systemctl --user start hyprlink-bridge
  ```
- **GUI tray autostart at login**:
  ```bash
  cp /usr/share/hyprlink-bridge/hyprlink-bridge-autostart.desktop ~/.config/autostart/
  ```
  Or add `exec-once = hyprlink-gui` to `hyprland.conf`.

</details>

---

## 05 / CONTROL SURFACE

<p align="center">
  <img src="assets/readme/control-surface.svg" width="100%" alt="HyprLink control surface: GUI and CLI">
</p>

### Pairing your phone

1. Install the companion Android app APK from [Releases](https://github.com/mastermaiolo/hyprlink/releases) (enable "Install unknown apps" on Android).
2. With the daemon active, open the GUI to **Devices → Pair new** (or inspect terminal output if running daemon interactively).
3. Scan the generated QR code in the app. The payload includes the SHA-256 certificate fingerprint, `<PC-IP>:7443` endpoint, and an ephemeral pairing token.
4. The link automatically reconnects whenever both devices reside on the same local subnet.

> [!NOTE]
> Ensure port **7443/UDP** is accessible through your firewall. Local PC identity credentials reside in `~/.config/hyprlink/`; deleting this directory resets all pairings.

### Desktop GUI navigation

The desktop app prioritises the phone view over local desktop metrics:
- Keys `1`–`9`: Direct page jump (Cover, Devices, Desk, Camera & Screen, Audio, Notifications, Sharing, Multimedia, Sensors & Presence, Diary).
- Key `0`: Open Settings overlay.

### Command-line interface (`hyprlinkctl`)

```bash
hyprlinkctl status             # Connection state and device details
hyprlinkctl ping               # Round-trip latency check to phone
hyprlinkctl watch --json       # Continuous JSON telemetry stream
hyprlinkctl clipboard          # Push desktop clipboard buffer to phone
hyprlinkctl pair               # Open 120-second discovery pairing window
hyprlinkctl mirror toggle      # Toggle phone screen mirror stream
hyprlinkctl mic toggle         # Toggle phone microphone input (on | off | toggle)
hyprlinkctl tap toggle         # Toggle PC sound stream to phone
hyprlinkctl speaker toggle     # Toggle phone as desktop output sink
hyprlinkctl ws 3               # Switch Hyprland active workspace to 3
hyprlinkctl open               # Open or focus the desktop GUI window
hyprlinkctl doctor             # Diagnostic environment and compatibility audit
```

Explore additional options with `hyprlinkctl --help`. A Waybar integration widget and a context-menu desktop action are supplied in `contrib/` (install via `contrib/install.sh`).

---

## 06 / CONFIGURATION

Settings reside in `~/.config/hyprlink/config.json` and are manageable directly from the GUI:

```json
{
  "download_dir": "~/Downloads",
  "track": {
    "sensitivity": 1.0,
    "scroll_speed": 1.0,
    "natural_scroll": true
  },
  "shortcuts": [],
  "gestures": [],
  "battery_alerts": {
    "low": 20,
    "full": 90
  },
  "disk_path": "/home/user",
  "gpu_nvidia_wake": false,
  "lang": "en-GB"
}
```

* `download_dir`: Destination directory for files sent from the phone.
* `track`: Sensitivity, natural scrolling, acceleration, and virtual keyboard toggles.
* `shortcuts` & `gestures`: Mappings between phone gesture events and `hyprctl dispatch` routines.
* `battery_alerts`: Low and high threshold percentage boundaries.
* `disk_path`: Mount point inspected for storage telemetry (defaults to `$HOME`).
* `gpu_nvidia_wake`: When `false` (default), prevents telemetry scans from waking sleeping discrete NVIDIA GPUs.
* `lang`: Interface language identifier (`en-GB`, `pt-BR`, `pt-PT`, `es-ES`, `zh-CN`).

To test the GUI without an active daemon or phone, launch mock mode:
```bash
HYPRLINK_MOCK=1 hyprlink-gui
```

---

## 07 / ARCHITECTURE

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="HyprLink system architecture and module tree">
</p>

```text
├── crates/
│   ├── hyprlinkd/            Headless daemon (binary: hyprlink-daemon, no window)
│   ├── hyprlink-gui/         Desktop GUI (iced, independent process with tray icon)
│   ├── hyprlink-proto/       Shared contract, CBOR framing, wire types, fmt.rs
│   └── hyprlinkctl/          Command-line tool for keybindings, Waybar, shell scripts
├── contrib/                  Waybar integration module, context menu .desktop entries
├── packaging/arch/           Local PKGBUILD definition (package hyprlink-bridge)
├── scripts/                  hyprlink-start.sh / stop, gui-capture.sh, release.sh
├── CHANGELOG.md              Version history and release notes
└── PROTOCOL.md               Formal network protocol specification
```

The daemon runs headlessly; the GUI communicates with it via a local UNIX domain socket at `$XDG_RUNTIME_DIR/hyprlink.sock`. 

Over the local network, a single QUIC connection on **7443/UDP** multiplexes:
- **Control messages**: Bidirectional CBOR stream.
- **Bulk data**: Unidirectional streams for file payloads and clipboard images.
- **Audio stream**: Low-latency loss-tolerant QUIC DATAGRAM frames.

**Core stack**: [`quinn`](https://github.com/quinn-rs/quinn) (QUIC), `rustls` (TLS), `ciborium` (CBOR), [`iced`](https://github.com/iced-rs/iced) (GUI runtime), `ksni` (StatusNotifierItem tray), `zbus` (D-Bus MPRIS & notifications), `uinput` (kernel input emulation), GStreamer / PipeWire (multimedia pipelines).

---

## 08 / FAILURE MODES

<p align="center">
  <img src="assets/readme/failure-modes.svg" width="100%" alt="HyprLink failure modes and recovery matrix">
</p>

<details>
<summary><strong>Detailed troubleshooting procedures</strong></summary>

1. **Phone fails to connect**:
   - Verify both devices share the exact same local network subnet.
   - Confirm firewall allows incoming traffic on port **7443/UDP**.
   - Check daemon service logs: `systemctl --user status hyprlink-bridge`.
   - Remove the paired device in the desktop GUI and perform a fresh QR scan.
2. **Touchpad or keyboard inputs produce no effect**:
   - Verify write permission to the virtual input character device: `getfacl /dev/uinput`.
   - Grant user access via udev rule if missing.
3. **Virtual webcam device absent**:
   - Ensure `v4l2loopback-dkms` is installed.
   - The daemon invokes `pkexec` to initialise `/dev/video42`.
4. **Service does not start upon graphical login**:
   - If running without `uwsm`, verify `graphical-session.target` activation.
   - Insert `exec-once = dbus-update-activation-environment --systemd --all` in `hyprland.conf`.
5. **Lock screen or screenshot commands fail**:
   - Ensure an active screen lock utility (`noctalia`, `hyprlock`, `loginctl`) is installed.
   - Ensure screenshot utilities (`grimblast` or `grim` + `slurp`) are present on `$PATH`.
6. **Full configuration reset**:
   - Stop daemon and purge configuration state: `rm -rf ~/.config/hyprlink/`.
   - A clean cryptographic identity and pairing database will be generated on next launch.

</details>

---

<details>
<summary><strong>Supplementary reference & building from source</strong></summary>

### Building from source

```bash
# Automated launch (builds and runs daemon + GUI using target/release binaries)
scripts/hyprlink-start.sh            # Use --restart to bounce; hyprlink-stop.sh to terminate

# Manual launch in dedicated terminals
cargo run --release -p hyprlinkd     # Daemon (outputs pairing QR in terminal)
cargo run --release -p hyprlink-gui  # Desktop GUI
```

The repository includes `.cargo/config.toml` configured to leverage **`clang` + `lld`** linker acceleration (`pacman -S clang lld`), cutting debug daemon link times from ~5s down to ~2s.

Compilation profiles:
```bash
cargo build --profile fast -p hyprlinkd   # Development iteration: no LTO, 16 codegen units
cargo build --release -p hyprlinkd        # Release distribution: LTO enabled, overflow checks on
```

The `fast` profile inherits from `release` with `lto = false`, `codegen-units = 16`, and `overflow-checks = false`; it is not intended for release distribution.

### Companion Android client

The companion mobile application (Kotlin / Jetpack Compose) is versioned in a dedicated workspace. Pre-compiled signed APKs are released on the [GitHub Releases](https://github.com/mastermaiolo/hyprlink/releases) page. Developers seeking to implement alternative mobile or desktop clients should refer to [`PROTOCOL.md`](PROTOCOL.md).

</details>

---

## 09 / PROVENANCE

<p align="center">
  <img src="assets/readme/provenance.svg" width="100%" alt="HyprLink provenance and attribution">
</p>

Core engineering builds upon [`quinn`](https://github.com/quinn-rs/quinn), [`rustls`](https://github.com/rustls/rustls), [`iced`](https://github.com/iced-rs/iced), `ciborium`, `ksni`, and `zbus`.

Typefaces bundled with the desktop and mobile clients — Anton, Instrument Serif, Inter, IBM Plex Mono, and Noto Sans SC — are distributed under the SIL Open Font License 1.1; licensing statements reside in `crates/hyprlink-gui/assets/fonts/OFL-*.txt`.

---

## 10 / LICENCE

**GPL-3.0-or-later** — see [`LICENSE`](LICENSE).

**Relicensing notice**: Up to version 0.1.0, earlier code snapshots were distributed under the MIT licence. With single-author provenance established, releases starting from 0.1.0 are published under GNU GPL v3.0 or later. Recipients of earlier MIT-tagged builds retain their rights under MIT terms.

---

<p align="center"><sub>MAIOLO / SYSTEMS LAB · HYPRLINK · HL / 01 · 食</sub></p>
