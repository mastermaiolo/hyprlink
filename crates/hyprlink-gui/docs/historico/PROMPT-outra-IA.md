# Prompt de passagem — GUI do HyprLink (iced) + plugin do Noctalia

> Cola isto inteiro no início da conversa com a outra IA, junto com os ficheiros.
> Responde sempre em **português europeu (pt-PT)**.

---

## 1. Quem és e o que recebes

Vais trabalhar na **nova GUI do HyprLink**. O HyprLink é um daemon (`hyprlinkd`, Rust) do Maggio que liga um
telemóvel **Android** a um PC **Arch Linux + Hyprland** — uma alternativa ao KDE Connect: QUIC + mTLS na porta
`:7443`, mensagens em CBOR, extensão do protocolo KDE Connect com pacotes `hyprland.*`, áudio/vídeo por
PipeWire/GStreamer (módulos `mic.rs`, `tap.rs`, `webcam.rs`…).

Recebes **dois projetos**:

| Projeto | Caminho na máquina do Maggio | O que é |
|---|---|---|
| `hyprlink-gui` | `~/Projectos/iced/hyprlink-gui` | App de desktop em **Rust + iced 0.14**, mais a CLI `hyprlinkctl` e um ícone de tray |
| `hyprlink` (plugin) | `~/Projectos/noctalia-plugins/hyprlink` | Plugin em **Luau** para a barra **Noctalia v5** (widget + painel) |

**Estado atual:** a GUI está completa e funcional mas fala com um **daemon simulado** (`src/link/mock.rs`).
O daemon real existe noutro repositório; a adoção desta GUI por ele está planeada em fases
(`docs/PROMPT-adocao-daemon.md`, `docs/RESPOSTA-fase0.md`, `docs/PAGINAS-novas.md`). Não tens o código do daemon —
o teu lado da fronteira é o **contrato** em `src/link/mod.rs`.

---

## 2. A estética (não negociável)

Cyberpunk contido + revista impressa. Preto, tipografia forte e simples, acabamento "AAA".

- **Cor** (`src/theme.rs`): fundo `VOID #000`; superfícies `INK_0 #080808 / INK_1 #0E0E0E / INK_2 #161616`;
  filetes `LINE #1C1C1C`; texto `PAPER #EDEBE4` (**nunca `#FFF`**). **Um** acento: ácido `ACID #D4FF3A`.
  Alarme: `HOT #FF3355`. Ciano `COLD #5FD4E8` só para tráfego RX.
  Estado da ligação: `LINK_UP #2BE07A` (verde), `LINK_WAIT #F4B731` (âmbar dourado), `LINK_DOWN = HOT`.
- **Tipo** (embutidas, OFL, em `assets/fonts/`): **Anton** — títulos e números, sempre em CAIXA ALTA;
  **Instrument Serif itálico** — os "decks", a voz de revista; **Inter** — interface; **IBM Plex Mono** — etiquetas,
  dados, protocolo.
- **Grelha**: múltiplos de 4 px, cantos a **0**, filetes de 1 px, goteira de 40 px.
- **Ritmo**: cada página abre igual — kicker `§NN`, filete duplo, título Anton, deck itálico, conteúdo
  (`ui::opener`).
- Nada de emojis, sombras suaves, gradientes ou cantos redondos.

As capturas de referência estão em `docs/screenshots/` (`indice.png` mostra todas as páginas;
`link-estados.png` as três cores do LINK). **Usa-as como verdade visual.**

---

## 3. Regras de arquitetura

1. **Telemóvel primeiro, PC depois.** Em todas as páginas, no tray, no JSON e no plugin, a informação do
   dispositivo ligado vem antes da do próprio PC ("Este PC" fica no fim). Na app Android é o inverso.
2. **O daemon só envia dados** ("regra 2"). Nenhum texto para humanos atravessa o fio. `None`/`null` para
   desconhecido — nunca um 0 falso. Tamanhos em bytes, tempos em segundos Unix ou ms.
3. **Todo o texto visível está em `src/fmt.rs`** (pt-PT). Se precisares de uma palavra nova, acrescenta uma
   função lá; não escrevas strings de estado nas views.
4. **Nomes de pacotes** (`módulo.ação`) vivem em `src/link/packets.rs`, em três grupos: *reais*, *existem no
   daemon mas nome por confirmar*, *propostos*. Não inventes nomes fora desse ficheiro.
5. A GUI só conhece o trait `Transport` (`send(Command)` / `poll(dt) -> Vec<Event>`). Trocar o simulador pelo
   daemon real é **uma linha** em `App::boot`.
6. Funcionalidade que o daemon ainda não tem → `pages::proposed_banner(...)` e/ou
   `Notice::Failed { op, error: NotImplemented }`. Não escondas, marca.
7. O JSON do `hyprlinkctl` (`src/snapshot.rs`, `v = 1`) é um contrato público para plugins de shell:
   mudanças aditivas mantêm `v = 1`; qualquer quebra sobe `v`. Há um teste dourado (`v1_shape`).

---

## 4. Árvore — `hyprlink-gui`

```
hyprlink-gui/
├── Cargo.toml            iced 0.14 (canvas, tokio, qr_code), ksni 0.3.6, serde, serde_json, chrono
│                         dois binários: hyprlink-gui (src/main.rs) e hyprlinkctl (src/bin/)
├── Cargo.lock
├── README.md             visão geral, secções, tray, LINK, hyprlinkctl, plugin, variáveis, estrutura
├── assets/fonts/         Anton, InstrumentSerif (Regular/Italic), Inter (4 pesos .otf),
│                         IBMPlexMono (3 pesos) + licenças OFL
├── docs/
│   ├── PROMPT-adocao-daemon.md   prompt dado ao Claude Code para adotar a GUI no daemon (Fases 0–4)
│   ├── RESPOSTA-fase0.md         decisões tomadas sobre o relatório da Fase 0 do daemon
│   ├── PAGINAS-novas.md          tabela página → Event2/Command2 → módulo do daemon
│   ├── PROMPT-outra-IA.md        este ficheiro
│   └── screenshots/              00-definicoes … 10-diario, 02b-emparelhar, 04a-camara, 04b-ecra,
│                                 indice.png, link-estados.png
└── src/
    ├── main.rs           arranque: iced::daemon(App::boot, App::update, App::view), regista fontes
    ├── lib.rs            biblioteca partilhada com o hyprlinkctl: pub mod fmt, host, link, snapshot
    │
    ├── app.rs            ★ O CENTRO. Estado (App), enum Message, update, subscriptions, janela,
    │                     moldura: rail lateral (logótipo HYPR+LINK, índice, Definições no rodapé),
    │                     masthead, ticker, toasts. Section enum + teclas 1–9/0. Integra tray.
    │                     Variáveis HYPRLINK_SECTION / HYPRLINK_DEMO / HYPRLINK_WINDOW_H.
    │                     `phase: LinkPhase` + `link_color()` (verde/dourado a respirar/vermelho).
    ├── theme.rs          design system: tokens de cor, fontes, funções de estilo
    │                     (nav, primary, danger, ghost, bare, chip_style, tile, card, switch_style,
    │                     fader, input, scroll, frame, frame_active, outline, fill, toast, scrim…)
    ├── ui.rs             blocos editoriais reutilizáveis: t, kicker, kicker_c, headline, deck, deck_s,
    │                     mono, rule, rule_c, vrule, gap, hgap, fill_x, square, tag, tag_outline,
    │                     opener, subhead, stat, kv, kv_text, btn, chip, switch, setting, panel
    ├── graphics.rs       desenho em canvas: Spark (sparkline), Meter (medidor LED), LinkDiagram
    │                     (telemóvel à esquerda, TX→), Phone (moldura do espelho), RssiScale,
    │                     Ticker (clip por glifo), CameraFrame (visor 16:9)
    ├── views.rs          páginas originais: Capa, Dispositivos (lista + ficha A/B/C/D),
    │                     modal de emparelhamento (QR), Secretária, espelho, Áudio, Sensores &
    │                     Presença, Diário. Helpers públicos: bar, spark, number_word, small_stat,
    │                     signal_bars…
    ├── pages.rs          páginas novas: Câmara & Ecrã, Notificações, Partilha, Multimédia,
    │                     Definições + blocos (proposed_banner, battery_block, shortcuts, trackpad,
    │                     phone_audio, mixer)
    ├── fmt.rs            ★ todas as palavras (pt-PT): kind, state, phase, cap, codec, name, model,
    │                     os, battery, latency, fingerprint, date, network, notice, stream, ringer,
    │                     size ("1,2 MB"), time, ago ("há 3 min"), clock…
    ├── host.rs           "Este PC": CPU, memória, bateria do portátil, uptime — lido de /proc e /sys
    ├── tray.rs           StatusNotifierItem (ksni): ícone desenhado em código (núcleo com a cor
    │                     do LINK), tooltip, menu Abrir/Ping/Clipboard/Microfone✓/Espelho✓/Sair,
    │                     status Active/Passive/NeedsAttention
    ├── snapshot.rs       JSON v1 para plugins (campo `link`, `device`, `phone`, …, `host` no fim)
    │                     + teste dourado
    ├── bin/hyprlinkctl.rs  CLI: watch [--json] [--interval MS], status, ping, clipboard, pair,
    │                     mic|tap|speaker on|off|toggle, mirror start|stop|toggle, ws N, open.
    │                     Toggles simulados em $XDG_RUNTIME_DIR/hyprlink-mock.json
    └── link/
        ├── mod.rs        ★ O CONTRATO (futuro crate `hyprlink-proto`): Device, PhoneStatus,
        │                 LinkState, LinkPhase, Cap, Codec, Notice, Op, ErrorKind, PairingTicket,
        │                 Levels, Command / Event, Command2 / Event2 (páginas novas, via
        │                 Command::More / Event::More), trait Transport. Só dados, serde.
        ├── packets.rs    nomes de pacotes: reais / a confirmar / propostos
        ├── mock.rs       daemon simulado: valores que derivam, picos, respostas a comandos,
        │                 máquina de fases do LINK (HYPRLINK_LINK=offline|connecting|cycle)
        └── mock_more.rs  dados simulados das páginas novas
```

★ = lê primeiro.

### Secções (enum `Section` em `app.rs`)

| Tecla | § | Secção | Ficheiro |
|---|---|---|---|
| 1 | 01 | Capa | views.rs |
| 2 | 02 | Dispositivos (+ emparelhamento) | views.rs, pages.rs (bateria) |
| 3 | 03 | Secretária (workspaces, janela ativa, atalhos, trackpad) | views.rs, pages.rs |
| 4 | 04 | Câmara & Ecrã | pages.rs (Ecrã reutiliza views::mirror_body) |
| 5 | 05 | Áudio (telemóvel → mic/retorno/coluna → misturador do PC) | views.rs, pages.rs |
| 6 | 06 | Notificações | pages.rs |
| 7 | 07 | Partilha (clipboard + ficheiros, largar na janela) | pages.rs |
| 8 | 08 | Multimédia (telemóvel + MPRIS) | pages.rs |
| 9 | 09 | Sensores & Presença (proposto) | views.rs |
| 0 | 10 | Diário (pacotes TX/RX) | views.rs |
| — | 00 | Definições (rodapé do rail) | pages.rs |

### Fluxo de dados

```
Transport (mock.rs | futuro socket do daemon)
   │ poll() → Vec<Event>            ▲ send(Command)
   ▼                                │
App::update(Message::Tick) ── aplica Event → estado ── App::view → views.rs / pages.rs
   │                                                         │ (palavras via fmt.rs,
   ├─► tray.rs (Snapshot)                                     │  estilos via theme.rs/ui.rs)
   └─► snapshot.rs (JSON v1) ◄── hyprlinkctl watch --json ◄── plugin Noctalia
```

---

## 5. Árvore — plugin `hyprlink` (Noctalia v5)

```
hyprlink/
├── plugin.toml           manifesto (id maggio/hyprlink, entradas service/widget/panel)
├── common.luau           utilitários partilhados: formatação (C.upper com acentos — string.upper
│                         parte UTF-8 em Luau), cores, leitura do snapshot
├── service.luau          runStream("hyprlinkctl watch --json") → noctalia.state; runAsync para
│                         comandos (mic, ping, …)
├── widget.luau           widget da barra (papéis nativos da paleta do Noctalia)
├── panel.luau            painel "edição de bolso": estilo HYPRLINK (Anton + ácido) ou Shell
├── translations/         en.json, pt.json, pt-BR.json
├── fonts/                Anton-Regular.ttf + OFL
└── tests/                run.sh (31 verificações), bundle.py, *_test.luau, fixtures/*.json(l)
```

API Noctalia v5: Luau, `plugin.toml`, `runStream`/`runAsync`, `noctalia.state`, `ui.*` declarativo.
**Pendente:** o plugin ainda não usa o campo `link` do JSON (devia pintar o ponto/etiqueta do cabeçalho do
painel e o widget com verde/dourado/vermelho, como a GUI).

---

## 6. Correr e testar

```sh
cargo run --release                       # GUI (fecha para o tray; --hidden arranca escondida)
cargo test                                # inclui o teste dourado do JSON v1
cargo run --bin hyprlinkctl -- status --json

HYPRLINK_SECTION=4  HYPRLINK_DEMO=webcam   cargo run
HYPRLINK_SECTION=4  HYPRLINK_DEMO=screen   cargo run
HYPRLINK_SECTION=7  HYPRLINK_DEMO=transfer cargo run
HYPRLINK_SECTION=2  HYPRLINK_DEMO=pair     cargo run
HYPRLINK_SECTION=11 cargo run             # Definições
HYPRLINK_LINK=offline|connecting|cycle cargo run
HYPRLINK_WINDOW_H=1800 cargo run          # janela alta para capturas

cd ~/Projectos/noctalia-plugins/hyprlink && tests/run.sh   # precisa do CLI luau
```

Hyprland: `app_id = dev.hyprlink.gui`.

O build `release` compila **sem avisos** — mantém assim.

---

## 7. Armadilhas conhecidas

- iced 0.14: nomes de estilo colidem com widgets (por isso `chip_style`, `switch_style`); não há `push_maybe`;
  closures em `view` com lifetimes difíceis → usa funções aninhadas.
- Renderer `tiny-skia` (capturas sem GPU): uma vez um texto não foi redesenhado após mudança de altura no
  primeiro frame — há um "warm-up" de 10 frames depois das demos. Confirma no `wgpu`.
- Luau: `string.upper` estraga acentos → `C.upper`.
- No repositório `noctalia-plugins` do Maggio, evita `git status` sem `GIT_OPTIONAL_LOCKS=0` (deixou um
  `index.lock` uma vez).

---

## 8. Como trabalhar comigo

- Antes de mudar algo visual, olha para a captura da página em `docs/screenshots/`.
- Mantém as regras da secção 3. Se uma tarefa obrigar a quebrar uma, diz qual e porquê antes de o fazer.
- Mudanças no contrato (`link/mod.rs`, `snapshot.rs`) → atualiza `docs/PAGINAS-novas.md`, o README e, se
  tocar no JSON, o plugin e os seus testes.
- Respostas curtas, em pt-PT; código e identificadores em inglês, como já está.
