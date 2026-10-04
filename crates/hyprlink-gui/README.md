# HYPRLINK — GUI

Interface em [iced 0.14](https://iced.rs) para o HyprLink: a ponte Android ⇄ Hyprland
(QUIC + mTLS em `:7443`, extensão do protocolo KDE Connect com pacotes `hyprland.*`).

Preto, editorial, cyberpunk contido. Oito secções, como uma revista:

| §  | Secção        | O que faz |
|----|---------------|-----------|
| 01 | Capa          | Dispositivo em destaque, diagrama do link TX/RX, latência, débito, bateria, ações rápidas, "o fio" |
| 02 | Dispositivos  | Lista + ficha (certificado SHA-256, capacidades), ping, clipboard, revogar, emparelhamento com QR |
| 03 | Secretária    | 10 workspaces do Hyprland clicáveis, gestos do telemóvel → `hyprctl dispatch`, estado do IPC |
| 04 | Espelho       | Pré-visualização do ecrã, codec / fps / escala / débito, `windowrulev2` gerado em direto |
| 05 | Áudio         | Microfone (`webcam.mic_*`) e retorno (`audio.tap_*`) com medidores LED em dBFS e ganho |
| 06 | Sensores      | 6 sensores com sparklines e ponte on/off para dispositivos virtuais |
| 07 | Presença      | Escala RSSI com limiares de bloqueio/desbloqueio e regras (hyprlock, playerctl, DND…) |
| 08 | Diário        | Registo de pacotes TX/RX com filtro, pesquisa, pausa |

![Contact sheet](docs/screenshots/contact-sheet.png)

Teclas `1`–`8` navegam entre secções; `Esc` fecha o emparelhamento.

## Princípio: primeiro o outro lado

Na GUI do PC, o **telemóvel vem primeiro** — bateria, rede, armazenamento, o que está a tocar,
notificações — e só depois **Este PC** (CPU, memória, bateria do portátil, tempo ligado, lidos de
`/proc` e `/sys`, sem dependências). Na app Android será o inverso. A mesma regra vale para o tray,
para o `hyprlinkctl` (campos do JSON pela ordem de leitura) e para o plugin do Noctalia.

## Correr

```sh
cargo run --release
```

A app corre como `iced::daemon`: fechar a janela deixa-a no **system tray** (StatusNotifierItem,
via `ksni`). Para arrancar já escondida, no `hyprland.conf`:

```ini
exec-once = hyprlink-gui --hidden
```

O ícone segue o link: `Passive` sem telemóvel (o tray do Noctalia esconde-o por omissão,
`hide_passive`), `Active` com telemóvel ligado, `NeedsAttention` durante o emparelhamento.
Menu: Abrir · Ping · Enviar clipboard · Microfone ✓ · Espelho ✓ · Sair. A tooltip mostra
telemóvel · bateria · rede · latência.

## hyprlinkctl

CLI para scripts, keybinds e plugins de shell:

```sh
cargo install --path .            # instala hyprlink-gui e hyprlinkctl em ~/.cargo/bin
hyprlinkctl status
hyprlinkctl watch --json          # uma linha JSON a cada 500 ms (--interval MS)
hyprlinkctl mic on|off|toggle     # tap …, speaker …, mirror start|stop|toggle, ws N, ping, clipboard, pair, open
```

Nesta versão fala com o daemon **simulado** (os toggles persistem em
`$XDG_RUNTIME_DIR/hyprlink-mock.json`). Contra o `hyprlinkd` real muda só o `Backend`.
O formato do JSON está em `src/snapshot.rs` (`v = 1`): desconhecido é `null`, nunca um 0 falso; a rede do
telemóvel vem estruturada e os tamanhos em bytes — quem mostra é que formata. `cargo test` verifica a forma.

## Contrato com o daemon

`src/link/mod.rs` é o futuro `hyprlink-proto`: só dados (regra 2). Todo o texto para humanos está em
`src/fmt.rs`; os nomes de pacotes (`módulo.ação`) estão em `src/link/packets.rs`, separados em reais e
propostos.

## Plugin do Noctalia v5

Está em `~/Projectos/noctalia-plugins/hyprlink` (a tua fonte local `local`). Ativar:

```sh
noctalia msg plugins enable maggio/hyprlink
```

e acrescentar o widget à barra (Definições → Barra → Adicionar widget → HYPRLINK).
Serviço com `runStream(hyprlinkctl watch --json)`, widget nativo (papéis da paleta) e um painel
"edição de bolso" com estilo HYPRLINK (Anton + ácido) ou Shell.

Variáveis úteis para desenvolvimento/capturas:

```sh
HYPRLINK_SECTION=4 HYPRLINK_DEMO=mirror cargo run   # abre no Espelho, já em direto
HYPRLINK_SECTION=2 HYPRLINK_DEMO=pair   cargo run   # abre o diálogo de emparelhamento
```

No Hyprland, a janela tem `app_id = dev.hyprlink.gui`:

```ini
windowrulev2 = float, class:^(dev.hyprlink.gui)$
windowrulev2 = size 1480 940, class:^(dev.hyprlink.gui)$
```

## Estrutura

```
src/
  main.rs        arranque (iced::daemon), fontes embutidas
  lib.rs         partilhado com o hyprlinkctl: link, host, snapshot
  tray.rs        StatusNotifierItem (ksni): estado, tooltip, menu, ícone desenhado em código
  host.rs        "Este PC": CPU, memória, bateria, uptime a partir de /proc e /sys
  snapshot.rs    o JSON estável para plugins de shell
  bin/hyprlinkctl.rs  a CLI
  theme.rs       design system: cores, tipografia, escala, estilos de widgets
  ui.rs          blocos editoriais (kicker, headline, deck, stat, kv, setting…)
  graphics.rs    canvas: sparkline, medidor LED, diagrama do link, telemóvel, escala RSSI, ticker
  views.rs       as oito páginas + modal de emparelhamento
  app.rs         estado, mensagens, update, subscrições, moldura (rail, masthead, ticker, toasts)
  link/mod.rs    o contrato com o daemon: Command, Event, trait Transport
  link/mock.rs   daemon simulado (valores que derivam, picos, respostas a comandos)
```

### Ligar ao daemon real

A GUI só conhece o trait `Transport`:

```rust
pub trait Transport {
    fn send(&mut self, command: Command);
    fn poll(&mut self, dt: Duration) -> Vec<Event>;
}
```

Para ligar ao `hyprlinkd`, implementa-o sobre o socket de controlo (por exemplo CBOR
sobre `$XDG_RUNTIME_DIR/hyprlink.sock`) e troca uma linha em `App::boot`:

```rust
link: Box::new(link::mock::Simulator::new()),   // → Box::new(link::daemon::Socket::connect()?)
```

Os `Command` mapeiam 1:1 para os pacotes já existentes (`webcam.mic_*`, `audio.tap_*`,
`hyprland.workspace.switch`, `hyprland.mirror.*`, `kdeconnect.pair`…). O sink
`hyprlink-speaker` aparece marcado como **POR IMPLEMENTAR** até existir no daemon.

## Design system

- **Cor** — fundo `#000`, superfícies `#080808 / #0E0E0E / #161616`, filetes `#1C1C1C`.
  Texto em branco-jornal `#EDEBE4` (nunca `#FFF`). Um acento: ácido `#D4FF3A`.
  Um alarme: `#FF3355`. Ciano `#5FD4E8` reservado ao tráfego RX.
- **Tipo** — Anton (títulos, números, sempre em caixa alta) · Instrument Serif itálico
  (os "decks", a voz de revista) · Inter (interface) · IBM Plex Mono (etiquetas, dados, protocolo).
- **Grelha** — múltiplos de 4 px, cantos a 0, filetes de 1 px, goteira de 40 px.
- **Ritmo** — cada página abre igual: kicker `§NN`, filete duplo, título, deck, conteúdo.

## Fontes

Todas SIL Open Font License 1.1, embutidas em `assets/fonts/` com as respetivas licenças:
Anton (Vernon Adams), Instrument Serif (Instrument), Inter (Rasmus Andersson),
IBM Plex Mono (IBM).
