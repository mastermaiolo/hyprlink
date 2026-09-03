# HyprLink

Integração de ecossistema entre Android e Linux/Hyprland — no espírito do
Continuity/Handoff da Apple, mas para quem usa Hyprland. Clipboard,
notificações, mídia, bateria, touchpad/teclado remoto e controlo do Hyprland
sincronizados entre o telemóvel e o desktop, em tempo real, sem depender de
nenhum serviço na nuvem.

A ligação é direta na rede local via **QUIC + mTLS** (autenticação mútua por
certificados autoassinados, fingerprint pinning), com o protocolo de controlo
serializado em **CBOR**. Sem servidor intermediário: o telemóvel fala
diretamente com o PC.

## Estrutura do projeto

```
├── app android hyprlink/     App Android (Kotlin/Compose), feito no Google AI Studio
├── hyprlink-daemon/          Daemon desktop em Rust (Linux/Hyprland)
└── PROTOCOL.md               Especificação completa do protocolo (fonte de verdade)
```

## Estado atual

| Módulo | Estado |
|---|---|
| Conectividade (QUIC/mTLS, pareamento por QR) | ✅ |
| GUI do daemon (painel flutuante, vidro fosco) | ✅ |
| Clipboard bidirecional | ✅ |
| Hyprland (workspaces, janelas, dispatch, eventos ao vivo) | ✅ |
| Bateria (PC ↔ telemóvel) | ✅ |
| Mídia (MPRIS — controlo e now-playing) | ✅ |
| Touchpad / teclado remoto | ✅ |
| Notificações (espelhar, ações, dispensar) | ✅ |
| Transferência de ficheiros | 🚧 |
| Áudio (mixer, ouvir o PC no telemóvel, microfone virtual) | 🚧 |
| Webcam (telemóvel como câmara/microfone do PC) | 🚧 |

Detalhes de cada fase, decisões de arquitetura e o roteiro completo estão no
histórico de commits e em `PROTOCOL.md`.

## Daemon desktop (Rust)

Requisitos: Linux com Hyprland (Wayland), Rust estável, `wl-clipboard`
instalado, kernel com o módulo `uinput` carregado.

```bash
cd hyprlink-daemon
cargo run
```

Na primeira execução gera um certificado próprio e mostra um QR Code no
terminal — aponte a câmara do app pra parear. A GUI (janela flutuante,
transparente) abre junto; pra ficar bonita de verdade com o vidro fosco, o
teu Hyprland precisa ter blur ligado (`decoration { blur { enabled = true } }`)
— qualquer janela transparente já é borrada pelo compositor, não precisa de
regra nenhuma específica pro HyprLink.

Stack: [`quinn`](https://github.com/quinn-rs/quinn) (QUIC), `rustls` (TLS),
`ciborium` (CBOR), [`iced`](https://github.com/iced-rs/iced) (GUI), `zbus`
(D-Bus — MPRIS e notificações), `uinput` (touchpad/teclado remoto).

## App Android

Em `app android hyprlink/` — projeto Kotlin/Jetpack Compose gerado e mantido
no [Google AI Studio](https://ai.studio). Abra a pasta no Android Studio para
compilar, ou continue editando no AI Studio.

## Protocolo

Toda a especificação de wire — framing, formato CBOR, catálogo completo de
pacotes por módulo — está em [`PROTOCOL.md`](PROTOCOL.md), extraída
diretamente do código-fonte do app (não é documentação escrita à parte, é a
fonte de verdade real).

## Licença

MIT — veja [`LICENSE`](LICENSE).
