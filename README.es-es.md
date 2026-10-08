<!-- MAIOLO / SYSTEMS LAB — HYPRLINK -->

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="HyprLink — Integración de Ecosistema Android ⇄ Hyprland">
</p>

<p align="center"><sub><strong>ANDROID ⇄ LINUX/HYPRLAND · QUIC + mTLS · CBOR · CONTINUIDAD · UINPUT</strong></sub></p>

<p align="center">
  <a href="README.md">🇬🇧 English (UK)</a>
  · <a href="README.pt-br.md">🇧🇷 Português (BR)</a>
  · <a href="README.pt-pt.md">🇵🇹 Português (PT)</a>
  · 🇪🇸 <strong>Español (ES)</strong>
  · <a href="README.zh-cn.md">🇨🇳 简体中文</a>
  · <a href="CHANGELOG.md">CHANGELOG</a>
  · <a href="PROTOCOL.md">PROTOCOL</a>
</p>

> **Integración de ecosistema entre Android y Linux/Hyprland** — en el espíritu de Continuity/Handoff de Apple, pero diseñado específicamente para entornos Hyprland. Portapapeles, notificaciones, reproducción multimedia, batería, panel táctil y teclado remotos, transferencia de archivos, cámara web, audio y control del compositor sincronizados en tiempo real mediante conexión local QUIC + mTLS, sin servidores en la nube intermediarios.

> [!NOTE]
> **Versión alfa (0.1.0).** Funciona en el uso diario para quien desarrolla el proyecto, pero el protocolo puede variar entre versiones y varios módulos continúan en fase de validación (consúltese [03 / Matriz de Subsistemas](#03--matriz-de-subsistemas)). Los informes de fallos y las contribuciones son bienvenidos.

<p align="center">
  <img src="assets/hyprlink_tour.gif" width="720" alt="Recorrido por la aplicación de escritorio de HyprLink: portada, dispositivos, escritorio, cámara, audio, notificaciones, compartir, multimedia, sensores y diario">
</p>

---

## 01 / DE UN VISTAZO

<p align="center">
  <img src="assets/readme/at-a-glance.svg" width="100%" alt="Perfil del sistema HyprLink de un vistazo">
</p>

El enlace se establece de forma directa en su red local mediante **QUIC + mTLS** (autenticación mutua por certificados autofirmados y fijación de huellas digitales o *fingerprint pinning*); el protocolo de control se serializa en **CBOR**. Sin servidores intermedios: el móvil se comunica directamente con el ordenador en el puerto **7443/UDP**.

---

## 02 / CAPACIDADES

<p align="center">
  <img src="assets/readme/capabilities.svg" width="100%" alt="Matriz de capacidades de HyprLink">
</p>

### ¿Qué ocurre en la práctica?

- **Notificaciones**: alertas del móvil replicadas en el escritorio con acciones remotas, descarte y recuperación de la lista de avisos activos al reconectar.
- **Portapapeles bidireccional**: sincronización continua de texto plano UTF-8 e imágenes PNG sin pérdida.
- **Transferencia de archivos**: cola de envíos en ambos sentidos, supervisión del progreso en tiempo real, historial y carpeta de descargas configurable.
- **Control multimedia**: control de los reproductores MPRIS del ordenador desde el móvil (con posición de reproducción y carátula del álbum); el reproductor del PC también se muestra como notificación en el móvil.
- **Móvil como cámara web**: transmisión de la cámara del móvil como dispositivo de vídeo virtual (`/dev/video42` mediante `v4l2loopback`) en H.264 o H.265 acelerado por hardware.
- **Puente de audio**: micrófono del móvil como entrada de sonido del ordenador, móvil como altavoz del PC y transmisión del audio del PC hacia el móvil.
- **Panel táctil y teclado remotos**: integración con `/dev/uinput` que proporciona movimiento de cursor, clics, gestos, escritura en tiempo real, tecla Enter y combinaciones de teclas con liberación de seguridad en caso de desconexión.
- **Acciones rápidas**: bloqueo de pantalla, suspensión, captura de pantalla (área o completa, copiada al portapapeles y guardada), control de volumen y llamadas arbitrarias mediante `hyprctl dispatch`.
- **Hyprland IPC**: cambio de espacios de trabajo (*workspaces*), listado de ventanas activas y escucha de eventos en directo; gestos del móvil asignados a órdenes del compositor (compatible con la configuración clásica y con `hyprland.lua`).
- **Telemetría y Wake-on-LAN**: monitorización bidireccional de batería; carga de CPU, uso de RAM, temperaturas (hwmon), espacio en disco y métricas de GPU del PC visibles en el móvil; difusión de dirección MAC para encendido remoto.
- **Interfaz de escritorio**: aplicación reactiva independiente en `iced`, con icono en la bandeja del sistema y soporte para 5 idiomas (EN-GB, PT-BR, PT-PT, ES-ES, ZH-CN).

---

## 03 / MATRIZ DE SUBSISTEMAS

<p align="center">
  <img src="assets/readme/showcase.svg" width="100%" alt="Matriz de subsistemas integrados de HyprLink">
</p>

| Subsistema | Estado | Observaciones |
|---|---|---|
| **Conectividad** (QUIC/mTLS, emparejamiento QR) | ✅ Operativo | Fijación mutua de certificados en el puerto 7443/UDP |
| **GUI de escritorio** (iced, proceso separado, bandeja) | ✅ Operativo | Proceso dedicado, icono en bandeja mediante ksni |
| **Portapapeles bidireccional** | ✅ Operativo | Texto UTF-8 e imágenes PNG |
| **Hyprland IPC** (workspaces, ventanas, dispatch) | ✅ Operativo | Integración con socket2 del compositor |
| **Batería** (PC ⇄ Móvil) | ✅ Operativo | Telemetria bidireccional y alertas |
| **Multimedia** (MPRIS: control, en reproducción, carátula) | ✅ Operativo | Integración D-Bus mediante zbus |
| **Panel táctil y teclado remotos** | ✅ Operativo | Emulación directa de dispositivo mediante /dev/uinput |
| **Acciones rápidas** (bloqueo, suspensión, captura) | ✅ Operativo | Integración con hyprlock, noctalia, grim |
| **Notificaciones** (reflejo, acciones, descarte) | ✅ Operativo | Restaura avisos activos al reconectar |
| **Transferencia de archivos** (cola, historial, carpeta) | ✅ Operativo | Flujo binario unidireccional QUIC |
| **Telemetría en el móvil** (CPU, RAM, temp, GPU, disco) | 🚧 En pruebas | Estados de reposo GPU NVIDIA/hwmon en validación |
| **Botones del ratón y arrastre** (`input.button`) | 🚧 En pruebas | Daemon y app Android listos; pruebas en móvil pendientes |
| **Gestos en el móvil** (`gesture`) | 🚧 En pruebas | Daemon, GUI y app listos; pruebas en móvil pendientes |
| **Puente de audio** (micrófono, altavoz, transmisión) | 🚧 En pruebas | Validación de canalizaciones PipeWire y GStreamer |
| **Cámara web virtual** (H.264 / H.265 en /dev/video42) | 🚧 En pruebas | Carga del módulo v4l2loopback mediante pkexec |

Las especificaciones detalladas del protocolo se encuentran en [`PROTOCOL.md`](PROTOCOL.md) y el registro de cambios en [`CHANGELOG.md`](CHANGELOG.md).

---

## 04 / INSTALACIÓN

### Requisitos del sistema

Probado en **Arch Linux / CachyOS** con **Hyprland** (Wayland). Los nombres de paquetes indicados corresponden a Arch Linux:

| Área | Paquetes requeridos (Arch Linux) |
|---|---|
| **Base / Núcleo** | `hyprland`, `pipewire`, `wireplumber`, `libpulse` (`pactl`), `wl-clipboard` |
| **Audio y Cámara (GStreamer)** | `gst-plugins-base-libs`, `gst-plugins-good`, `gst-plugins-bad-libs` (H.265), `gst-libav` (H.264/H.265), `gst-plugin-pipewire` |
| **Cámara Web Virtual** | `v4l2loopback-dkms` (cargado en `/dev/video42` mediante `pkexec`) |
| **Selector de archivos de la GUI** | `xdg-desktop-portal` + `xdg-desktop-portal-gtk` |
| **Acciones rápidas (Opcional)** | Bloqueo: `noctalia`, `hyprlock`, o `loginctl`; Capturas: `grimblast` o `grim` + `slurp` |
| **Herramientas de compilación** | `rust`, `clang`, `lld`, `pkgconf` |

> [!IMPORTANT]
> El panel táctil y teclado remotos escriben directamente en `/dev/uinput`. Compruebe que su usuario dispone de permisos de escritura (verifique mediante `getfacl /dev/uinput`).

### Método rápido: Paquete binario precompilado (x86_64)

Descargue `hyprlink-0.1.0-linux-x86_64.tar.gz` desde los [Lanzamientos de GitHub](https://github.com/mastermaiolo/hyprlink/releases):

```bash
tar -xzf hyprlink-0.1.0-linux-x86_64.tar.gz
cd hyprlink-0.1.0-linux-x86_64
./install.sh                                     # Instala en ~/.local (sin privilegios root)
systemctl --user enable --now hyprlink-bridge   # Activa la unidad de servicio de usuario
hyprlink-gui                                     # Inicia la interfaz gráfica
```

El instalador `./install.sh` admite `--prefix DIR` (por defecto: `~/.local`), `--dry-run` y valida dependencias de ejecución. Ejecute `./uninstall.sh` para desinstalar los archivos.

### Arch Linux (PKGBUILD)

Se proporciona un archivo PKGBUILD local en `packaging/arch/` con el nombre `hyprlink-bridge`:

```bash
git clone https://github.com/mastermaiolo/hyprlink.git
cd hyprlink/packaging/arch
makepkg -si
systemctl --user enable --now hyprlink-bridge
hyprlink-gui
```

Esto instala `hyprlink-daemon`, `hyprlink-gui` y `hyprlinkctl` en `/usr/bin`, configura el servicio `hyprlink-bridge.service` y añade los accesos directos `.desktop`. Para compilar el árbol de trabajo actual antes de generar una etiqueta de versión: `HYPRLINK_LOCAL=1 makepkg -si`.

<details>
<summary><strong>Inicio de sesión e integración con uwsm</strong></summary>

El servicio en segundo plano se vincula a `graphical-session.target`.
- **Con `uwsm`**: la activación es completamente automática.
- **Sin `uwsm`**: asegúrese de exportar las variables de entorno de la sesión añadiendo en su `hyprland.conf`:
  ```ini
  exec-once = dbus-update-activation-environment --systemd --all
  exec-once = systemctl --user start hyprlink-bridge
  ```
- **Inicio automático de la GUI en la bandeja del sistema**:
  ```bash
  cp /usr/share/hyprlink-bridge/hyprlink-bridge-autostart.desktop ~/.config/autostart/
  ```
  O añada `exec-once = hyprlink-gui` en su `hyprland.conf`.

</details>

---

## 05 / SUPERFICIE DE CONTROL

<p align="center">
  <img src="assets/readme/control-surface.svg" width="100%" alt="Superficie de control de HyprLink: GUI y CLI">
</p>

### Emparejamiento con el móvil

1. Instale la aplicación móvil para Android disponible en la sección de [Lanzamientos](https://github.com/mastermaiolo/hyprlink/releases) (habilite la opción de instalar aplicaciones de fuentes desconocidas).
2. Con el servicio en ejecución, abra la interfaz gráfica en **Dispositivos → Emparejar nuevo** (o visualice el código QR en el terminal si ejecutó el daemon manualmente).
3. Escanee el código QR desde la aplicación. Contiene la huella digital SHA-256 del certificado, la dirección `<IP-DEL-PC>:7443` y un token temporal.
4. A partir de ese momento, el enlace se reanudará automáticamente siempre que ambos dispositivos compartan la misma red local.

> [!NOTE]
> Compruebe que el puerto **7443/UDP** esté abierto en el cortafuegos para su red local. Las credenciales criptográficas del ordenador se almacenan en `~/.config/hyprlink/`; borrar este directorio restablece todos los emparejamientos.

### Navegación en la interfaz de escritorio

La aplicación de escritorio prioriza la vista del móvil:
- Teclas `1` a `9`: Salto directo de pestaña (Portada, Dispositivos, Escritorio, Cámara y Pantalla, Audio, Notificaciones, Compartir, Multimedia, Sensores y Presencia, Diario).
- Tecla `0`: Abre los Ajustes (*Settings*).

### Línea de órdenes (`hyprlinkctl`)

```bash
hyprlinkctl status             # Estado de conexión y detalles del dispositivo
hyprlinkctl ping               # Comprobación de latencia de ida y vuelta al móvil
hyprlinkctl watch --json       # Flujo continuo de telemetría en líneas JSON
hyprlinkctl clipboard          # Envía el portapapeles del PC al móvil
hyprlinkctl pair               # Abre una ventana de emparejamiento de 120 segundos
hyprlinkctl mirror toggle      # Alterna la duplicación de pantalla del móvil
hyprlinkctl mic toggle         # Alterna el micrófono del móvil (on | off | toggle)
hyprlinkctl tap toggle         # Alterna el envío de sonido del PC al móvil
hyprlinkctl speaker toggle     # Alterna el uso del móvil como altavoz del PC
hyprlinkctl ws 3               # Cambia al espacio de trabajo 3 de Hyprland
hyprlinkctl open               # Abre o enfoca la ventana de la interfaz gráfica
hyprlinkctl doctor             # Diagnóstico del entorno e informe de compatibilidad
```

Consulte el resto de opciones mediante `hyprlinkctl --help`. El directorio `contrib/` incluye un módulo para Waybar y una acción para el menú contextual del explorador de archivos (instálelos con `contrib/install.sh`).

---

## 06 / CONFIGURACIÓN

Los parámetros se guardan en `~/.config/hyprlink/config.json` y pueden modificarse directamente desde la interfaz gráfica:

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
  "lang": "es-ES"
}
```

* `download_dir`: Directorio donde se guardan los archivos recibidos desde el móvil.
* `track`: Sensibilidad del panel táctil, desplazamiento natural, aceleración y conmutador del teclado.
* `shortcuts` y `gestures`: Asignaciones entre gestos en el móvil y órdenes `hyprctl dispatch`.
* `battery_alerts`: Umbrales porcentuales para alertas de batería baja y carga completa.
* `disk_path`: Punto de montaje supervisado para la telemetría de almacenamiento (por defecto: `$HOME`).
* `gpu_nvidia_wake`: Si es `false` (por defecto), evita que las comprobaciones de telemetría activen tarjetas NVIDIA en suspensión.
* `lang`: Idioma de la interfaz (`en-GB`, `pt-BR`, `pt-PT`, `es-ES`, `zh-CN`).

Para evaluar la interfaz gráfica sin daemon ni teléfono móvil conectado:
```bash
HYPRLINK_MOCK=1 hyprlink-gui
```

---

## 07 / ARQUITECTURA

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="Arquitectura del sistema HyprLink y estructura de módulos">
</p>

```text
├── crates/
│   ├── hyprlinkd/            Servicio en segundo plano (binario hyprlink-daemon, sin ventana)
│   ├── hyprlink-gui/         Interfaz de escritorio (iced, proceso independiente con icono en bandeja)
│   ├── hyprlink-proto/       Contrato compartido, tramas CBOR, tipos de red, fmt.rs
│   └── hyprlinkctl/          Herramienta de línea de órdenes para atajos, Waybar y scripts
├── contrib/                  Módulo de integración para Waybar, accesos .desktop
├── packaging/arch/           Definición de paquete PKGBUILD para Arch Linux (hyprlink-bridge)
├── scripts/                  hyprlink-start.sh / stop, gui-capture.sh, release.sh
├── CHANGELOG.md              Historial de versiones y notas de publicación
└── PROTOCOL.md               Especificación formal del protocolo de red
```

El daemon funciona en segundo plano sin ventana; la interfaz gráfica se comunica con él mediante un socket UNIX local en `$XDG_RUNTIME_DIR/hyprlink.sock`.

A través de la red local, una única conexión QUIC en el puerto **7443/UDP** multiplexa:
- **Mensajes de control**: Flujo bidireccional serializado en CBOR.
- **Datos masivos**: Flujos unidireccionales para transferencias de archivos e imágenes del portapapeles.
- **Flujo de audio**: Tramas QUIC DATAGRAM de baja latencia con tolerancia a pérdidas.

**Pila tecnológica**: [`quinn`](https://github.com/quinn-rs/quinn) (QUIC), `rustls` (TLS), `ciborium` (CBOR), [`iced`](https://github.com/iced-rs/iced) (interfaz gráfica), `ksni` (bandeja del sistema StatusNotifierItem), `zbus` (D-Bus para MPRIS y notificaciones), `uinput` (emulación de dispositivos en el kernel), GStreamer / PipeWire (canales multimedia).

---

## 08 / MODOS DE FALLO

<p align="center">
  <img src="assets/readme/failure-modes.svg" width="100%" alt="Modos de fallo y matriz de recuperación de HyprLink">
</p>

<details>
<summary><strong>Procedimientos detallados de resolución de problemas</strong></summary>

1. **El móvil no se conecta**:
   - Compruebe que ambos dispositivos se encuentran en la misma subred de red local.
   - Verifique que el cortafuegos permite el tráfico en el puerto **7443/UDP**.
   - Revise el estado del servicio: `systemctl --user status hyprlink-bridge`.
   - Elimine el dispositivo en la interfaz gráfica y vuelva a emparejar mediante código QR.
2. **El panel táctil o el teclado no responden**:
   - Asegúrese de que su usuario dispone de permisos de escritura en `/dev/uinput`: `getfacl /dev/uinput`.
   - Configure reglas udev adecuadas si no dispone de acceso.
3. **La cámara web virtual no aparece**:
   - Compruebe que el paquete `v4l2loopback-dkms` está instalado.
   - El daemon requiere permisos elevados (`pkexec`) para crear el nodo `/dev/video42`.
4. **El servicio no se inicia al abrir sesión**:
   - Si no utiliza `uwsm`, compruebe la activación de `graphical-session.target`.
   - Añada `exec-once = dbus-update-activation-environment --systemd --all` en su `hyprland.conf`.
5. **Las órdenes de bloqueo o captura de pantalla fallan**:
   - Asegúrese de tener instalada una utilidad de bloqueo (`noctalia`, `hyprlock`, `loginctl`).
   - Compruebe que las herramientas de captura (`grimblast` o `grim` + `slurp`) están en su `$PATH`.
6. **Restablecimiento completo de la configuración**:
   - Detenga el servicio y elimine la carpeta de configuración: `rm -rf ~/.config/hyprlink/`.
   - Se generará un nuevo par de claves criptográficas e historial limpio en el siguiente inicio.

</details>

---

<details>
<summary><strong>Documentación complementaria y compilación desde código fuente</strong></summary>

### Compilación desde el código fuente

```bash
# Inicio automatizado (compila y ejecuta daemon + GUI usando los binarios de target/release)
scripts/hyprlink-start.sh            # Use --restart para reiniciar; hyprlink-stop.sh para detener

# Inicio manual en terminales independientes
cargo run --release -p hyprlinkd     # Daemon (muestra el código QR de emparejamiento)
cargo run --release -p hyprlink-gui  # Interfaz gráfica
```

El repositorio incluye un archivo `.cargo/config.toml` optimizado para utilizar el enlazador rápido **`clang` + `lld`** (`pacman -S clang lld`), reduciendo los tiempos de enlace de ~5s a ~2s.

Perfiles de compilación:
```bash
cargo build --profile fast -p hyprlinkd   # Para desarrollo: sin LTO, 16 unidades de codegen
cargo build --release -p hyprlinkd        # Para distribución: LTO activo, comprobaciones de desbordamiento activadas
```

El perfil `fast` hereda de `release` con `lto = false`, `codegen-units = 16` y `overflow-checks = false`; **no está concebido para distribución**.

### Aplicación móvil para Android

La aplicación móvil para Android (desarrollada en Kotlin con Jetpack Compose) se mantiene en un espacio de trabajo independiente. Los paquetes APK firmados se publican en la página de [Lanzamientos](https://github.com/mastermaiolo/hyprlink/releases). Quienes deseen desarrollar clientes alternativos pueden consultar la especificación completa en [`PROTOCOL.md`](PROTOCOL.md).

</details>

---

## 09 / PROCEDENCIA

<p align="center">
  <img src="assets/readme/provenance.svg" width="100%" alt="Procedencia y reconocimientos de HyprLink">
</p>

La base técnica se apoya en [`quinn`](https://github.com/quinn-rs/quinn), [`rustls`](https://github.com/rustls/rustls), [`iced`](https://github.com/iced-rs/iced), `ciborium`, `ksni` y `zbus`.

Las fuentes tipográficas distribuidas con la interfaz de escritorio y móvil — Anton, Instrument Serif, Inter, IBM Plex Mono y Noto Sans SC — se licencian bajo SIL Open Font License 1.1; sus textos están en `crates/hyprlink-gui/assets/fonts/OFL-*.txt`.

---

## 10 / LICENCIA

**GPL-3.0-or-later** — véase [`LICENSE`](LICENSE).

**Nota de relicenciamiento**: Hasta la versión 0.1.0, el código se distribuyó bajo la licencia MIT. Al contar el proyecto con un único autor, a partir de la versión 0.1.0 se publica bajo la licencia GNU GPL v3.0 o posterior. Quienes hayan recibido versiones anteriores conservan sus derechos bajo los términos de la licencia MIT.

---

<p align="center"><sub>MAIOLO / SYSTEMS LAB · HYPRLINK · HL / 01 · 食</sub></p>
