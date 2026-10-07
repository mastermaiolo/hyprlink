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

## Onde está o quê

```
├── crates/
│   ├── hyprlinkd/            Daemon (Rust, binário hyprlink-daemon) — sem janela
│   ├── hyprlink-gui/         GUI do PC (iced, com tray próprio)
│   ├── hyprlink-proto/       Contrato entre daemon, GUI e hyprlinkctl (+ textos em fmt.rs)
│   └── hyprlinkctl/          Linha de comandos, para scripts e atalhos
├── app android hyprlink/     App Android (Kotlin/Compose), feita no Google AI Studio
├── android-design-kit/       Kit de design da app: tokens, mockups e prompts
│   └── prompts/              Prompts por aplicar (ordem em ORDEM.md); os feitos em prompts/Done/
├── docs/
│   ├── prompts-ai-studio/    Prompts soltos para o AI Studio (aplicados em aplicados/)
│   ├── testes/               Relatórios do teste real e o prompt do Claude Code
│   └── historico/            Planos e materiais antigos, só para consulta
├── contrib/                  Waybar, .desktop, script de instalação
├── scripts/                  gui-capture.sh (capturas da GUI para comparar)
├── PLANO_COMPLETAR.md        Estado e plano (a secção do topo é a mais recente)
└── PROTOCOL.md               Especificação do protocolo (fonte de verdade)
```

## Estado atual

| Módulo | Estado |
|---|---|
| Conectividade (QUIC/mTLS, pareamento por QR) | ✅ |
| GUI do PC (iced, separada do daemon) | ✅ |
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

## Correr no PC

Requisitos: Linux com Hyprland (Wayland), Rust estável, `wl-clipboard`,
GStreamer com `gst-plugin-pipewire`, e o módulo `uinput` carregado.

```bash
# terminal 1 — o daemon (mostra o QR de emparelhamento no terminal)
cargo run --release -p hyprlinkd
# terminal 2 — a GUI
cargo run --release -p hyprlink-gui
```

Na primeira execução o daemon gera um certificado próprio. A GUI fala com ele
pelo socket `$XDG_RUNTIME_DIR/hyprlink.sock`; `HYPRLINK_MOCK=1` abre-a com um
daemon simulado, sem telemóvel.

Stack: [`quinn`](https://github.com/quinn-rs/quinn) (QUIC), `rustls` (TLS),
`ciborium` (CBOR), [`iced`](https://github.com/iced-rs/iced) (GUI), `ksni`
(tray), `zbus` (D-Bus — MPRIS e notificações), `uinput` (touchpad/teclado),
GStreamer/PipeWire (áudio e câmara).

## Compilar

O repositório traz `.cargo/config.toml` com **`clang` + `lld`** como ligador
(`pacman -S clang lld`): o *link* de um build debug do daemon passa de ~5 s
para ~2 s. No release o ganho é nulo (o tempo vai para o LTO e o codegen).

```bash
cargo build --profile fast -p hyprlinkd   # para iterar: sem LTO, 16 unidades de codegen
cargo build --release -p hyprlinkd        # para distribuir (LTO, overflow-checks = true)
```

O perfil `fast` herda do `release` com `lto = false`, `codegen-units = 16` e
`overflow-checks = false`; **não é para distribuir**. A primeira compilação
depois de mudar o ligador recompila tudo (as `rustflags` mudam).

Opcional, nada disto está ligado por omissão: `mold` (`paru -S mold`, depois
`-C link-arg=-fuse-ld=mold` no lugar de `lld`) liga ainda mais depressa; e
`sccache` (`paru -S sccache` + `RUSTC_WRAPPER=sccache`) só compensa com vários
`target` ou depois de `cargo clean`, porque o `cargo` já guarda o que não mudou.

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
