# Páginas novas — prontas em mock, para ligar ao daemon

Complemento a `RESPOSTA-fase0.md`. O índice aprovado está desenhado e a correr com o simulador.
**Não alteres a composição destas páginas**; liga-as. As capturas de referência estão em
`docs/screenshots/` (vê `indice.png`).

## Índice final

`01 Capa · 02 Dispositivos · 03 Secretária · 04 Câmara & Ecrã · 05 Áudio · 06 Notificações ·
07 Partilha · 08 Multimédia · 09 Sensores & Presença · 10 Diário` + `00 Definições` no rodapé do rail.
Teclas 1–9 e 0. Largar um ficheiro na janela → `SendFile`.

## Contrato acrescentado (`src/link/mod.rs`)

Para não inchar os enums originais, o que é novo vive em `Command::More(Command2)` e
`Event::More(Event2)`. No proto, podes achatar para os enums principais se preferires — os nomes dos
variantes mantêm-se.

| Página / bloco | Eventos (`Event2`) | Comandos (`Command2`) | Módulo do daemon |
|---|---|---|---|
| 02 Bateria 12 h + alertas | `BatteryHistory(Vec<BatteryPoint>)`, `BatteryAlerts` | `SetBatteryAlerts` | `battery.rs` (guardar 12 h no daemon) |
| 03 Janela ativa | `ActiveWindow(Option<…>)` | — | `hypr.rs` (socket2 `activewindow>>`) |
| 03 Atalhos | `Shortcuts(Vec<Shortcut>)` | `RunDispatch(String)`, `SetShortcuts` | `hypr.rs` |
| 03 Trackpad | `Trackpad(TrackpadConfig)` | `SetTrackpad` | `input.rs` |
| 04 Câmara | `Webcam(Option<WebcamStats>)`, `NetTest` | `StartWebcam(WebcamConfig)`, `StopWebcam`, `TestNetwork` | `webcam.rs` (`fps: None` até haver medição) |
| 04 Ecrã | `Mirror` (já existia) | `StartMirror/StopMirror` | **proposto** → `Notice::Failed{op: Mirror, NotImplemented}` |
| 05 Telemóvel | `PhoneAudio{streams, ringer, dnd}` | `SetPhoneVolume`, `SetRinger`, `SetDnd` | `phone_audio.rs` |
| 05 Misturador | `Mixer{sinks, apps}` | `SetSinkVolume/Mute`, `SetDefaultSink`, `SetAppVolume/Mute` | `audio.rs` (0–150 %) |
| 06 Notificações | `Notifications(Vec<PhoneNotification>)` | `DismissNotification(key)`, `DismissAllNotifications` | `notif.rs` |
| 07 Clipboard | `Clipboard(Vec<ClipEntry>)` | `CopyClip`, `SendClipToPhone`, `PinClip`, `DeleteClip` | `clip.rs` (histórico no daemon) |
| 07 Ficheiros | `Transfers(Vec<Transfer>)` | `SendFile(path)`, `CancelTransfer`, `OpenDownloads` | `share.rs` (+ handler de `share.progress/done`, bug da Fase 0) |
| 08 Multimédia PC | `Players(Vec<Player>)` | `Media{player, action}` | `media.rs` (MPRIS) |
| 08 Multimédia telemóvel | `PhoneStatus.now_playing` | `PhoneMedia(action)` | **proposto** (media-session no Android) |
| 09 Sensores & Presença | `Sensors`, `Rssi` | `SetSensorBridge`, `SetRule` | **proposto** |
| 00 Definições | `Settings{downloads_dir, daemon_version, socket}` | `SetDownloadsDir`, `RestartDaemon` | config do daemon |

Regras de sempre: só dados, `None` para desconhecido, tamanhos em bytes, tempos em segundos Unix ou ms.
`PhoneNotification.app`, `Sink.description` e afins são dados vindos do sistema — podem ir em bruto.

## Nomes de pacotes

`src/link/packets.rs` tem agora três grupos: **reais**, **existem no daemon mas o nome está por
confirmar** (`webcam.stop`, `webcam.nettest`, `phone.volume_set`, `phone.ringer_set`, `phone.dnd_set`,
`input.config`, `share.offer`, `notif.dismiss`) e **propostos**. Acerta o segundo grupo com o
PROTOCOL.md — é um ficheiro.

## Estados que tens de cobrir ao ligar

- Sem telemóvel: as páginas de telemóvel usam os blocos vazios que já existem («Sem volumes do telemóvel.»,
  «SILÊNCIO.», «Nada a tocar no telemóvel.»).
- Funcionalidade proposta: `pages::proposed_banner(...)` (já usado em §04 Ecrã e §09).
- Erros: `Notice::Failed{op, error}`; os `Op` novos são `Webcam`, `Files`, `Media`, `Dispatch`, `PhoneAudio`.

## Nota de render

Com o renderer `tiny-skia` (o que uso nas capturas, sem GPU) houve um caso em que um texto não foi
redesenhado depois de a lista mudar de altura no primeiro frame. Não é lógica da página; confirma no
renderer `wgpu` da máquina real e, se aparecer, abre issue no iced.
