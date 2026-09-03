# Especificação de implementação — GUI desktop HyprLink (iced 0.14)

Alvo: `iced` 0.14, features `image` + `tokio`. Janela já existente: 900×800, não
redimensionável, `position: Centered`, `decorations: false`, `transparent: true`.
O blur é do compositor (`decoration:blur` do Hyprland) — não desenhar nada para o simular.

---

## 1 · Tokens de cor

Os valores atuais no código estão ligeiramente fora dos do app Android. Alinhar:

```rust
// hyprlink-gui/src/theme.rs
use iced::Color;

const fn hex(r: u8, g: u8, b: u8) -> Color { Color::from_rgb8(r, g, b) }

// acento por estado — UM único acento ativo em toda a UI a cada momento
pub const GREEN:  Color = hex(0x3D, 0xFF, 0x9E); // era rgb(0.220,1.0,0.612) → corrigir
pub const AMBER:  Color = hex(0xFF, 0xB0, 0x20); // era rgb(1.0,0.694,0.231)  → corrigir
pub const RED:    Color = hex(0xFF, 0x47, 0x57); // era rgb(1.0,0.231,0.361)  → corrigir
pub const MUTED:  Color = hex(0x5E, 0x5E, 0x5E); // módulo inativo / sem dados

// texto
pub const TEXT:   Color = hex(0xFF, 0xFF, 0xFF); // títulos
pub const TEXT_1: Color = hex(0xE4, 0xE4, 0xE4); // corpo
pub const TEXT_2: Color = hex(0xB4, 0xB4, 0xB4); // corpo secundário
pub const TEXT_3: Color = hex(0x8A, 0x8A, 0x8A); // labels
pub const TEXT_4: Color = hex(0x6E, 0x6E, 0x6E); // subtítulos, log
pub const TEXT_5: Color = hex(0x4E, 0x4E, 0x4E); // timestamps
pub const TEXT_6: Color = hex(0x3A, 0x3A, 0x3A); // régua numerada inativa

// superfícies
pub const WINDOW_BG: Color = Color { r: 0.031, g: 0.035, b: 0.047, a: 0.82 }; // rgba(8,9,12,.82)
pub const TERMINAL:  Color = hex(0x07, 0x07, 0x07);
pub const GLASS:     Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.045 };
pub const GLASS_BRD: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.08  };
pub const WINDOW_BRD:Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.10  };

// pares fundo/borda por estado (cartões)
pub const GREEN_BG:  Color = hex(0x07, 0x15, 0x10);
pub const GREEN_BRD: Color = hex(0x1E, 0x3E, 0x30);
pub const GREEN_SEL: Color = hex(0x08, 0x22, 0x1A); // linha/módulo selecionado
pub const AMBER_BG:  Color = hex(0x15, 0x0F, 0x03);
pub const AMBER_BRD: Color = hex(0x3A, 0x2C, 0x12);
pub const RED_BG:    Color = hex(0x15, 0x08, 0x09);
pub const RED_BRD:   Color = hex(0x2A, 0x14, 0x18);

// bordas neutras
pub const BRD_1: Color = hex(0x1C, 0x1C, 0x1C); // repouso
pub const BRD_2: Color = hex(0x26, 0x26, 0x26); // interativo
pub const DIVIDER: Color = hex(0x14, 0x14, 0x14); // separador dentro de listas
```

**Regra rígida do acento** (herdada do app Android, não inverter):
verde = ligado/ok · âmbar = a sincronizar/aviso/volume >100% · vermelho = **só** erro real
ou ausência de ligação · cinza `MUTED` = desligado por opção ou sem dados. Um módulo que
ainda não está implementado é **cinza, nunca vermelho**.

## 2 · Tipografia

`Font::MONOSPACE` (mono do sistema) para tudo exceto os títulos display.

| Uso | Tamanho | Peso | Notas |
|---|---|---|---|
| Wordmark dashboard | 22 | Bold | display; `HYPR` em branco + `LINK` no acento |
| Wordmark ecrã de módulo | 18 | Bold | display |
| Título de módulo (`CLIP`, `FILES`) | 20 | Bold | display, caixa alta |
| Nome do dispositivo | 14 | Bold | mono |
| Número grande (bateria) | 56 | Bold | display |
| Corpo primário | 12–13 | Normal | mono |
| Corpo secundário / lista | 11 | Normal | mono |
| Labels de secção | 10 | Normal | mono, caixa alta |

Notas de implementação:
- O protótipo usa Archivo (display) + JetBrains Mono. Se não quiser embutir fontes, use
  `Font::MONOSPACE` para tudo e suba os títulos de 20 → 22 para compensar a perda de peso.
  Se quiser fidelidade, embuta JetBrains Mono via `iced::font::load`.
- **Não há `letter-spacing` no iced 0.14.** Os labels de secção do protótipo têm
  `letter-spacing:.14em–.18em`. Duas saídas: aceitar sem espaçamento, ou espaçar
  manualmente na string (`"M Ó D U L O S"`). Escolher uma e ser consistente.
- `line-height` de listas: 1.55 no título, 1.65 no corpo (`text::LineHeight::Relative`).

## 3 · Geometria

```
janela            900 × 800, radius 28, borda 1px WINDOW_BRD, padding 26 (v) / 28 (h)
gap vertical raiz 18
cabeçalho         altura ~30; ponto 8px + wordmark à esquerda; pill + botão × à direita
pill de estado    radius 999, padding 6/13, ponto 7px, texto 11
botão ×           30 × 30, radius 8, borda BRD_2
cartão            radius 14, padding 14–20
cartão "glass"    fundo GLASS, borda GLASS_BRD
sidebar dashboard largura 296, gap 5 entre linhas
linha de módulo   radius 11, padding 10 (v) / 12 (h); nº 16px · glifo 16px · texto · ponto 6px · chevron
régua (módulo)    largura 44, gap 8, botão ativo 34 × 34 radius 9, números inativos texto 10
lista/histórico   radius 14, fundo TERMINAL, separadores 1px DIVIDER, padding de linha 11–13
slider            trilho 4px radius 2, punho 11–13px círculo, marca dos 100% = 1px × 10–12px
barra de progresso 3–4px radius 2
```

## 4 · Ecrã: dashboard (dois estados)

`code-referencia-visual/01-dashboard-conectado.png`, `02-dashboard-pareando.png`

```
Column [padding 26/28, spacing 18]
├─ Row  header            (ponto+wordmark | pill de estado + ×)
├─ Container device_card  (só no estado conectado; GLASS)
│     └─ Row [nome + fp/latência | CPU · RAM · WS]
└─ Row [spacing 18, height Fill]
   ├─ Column sidebar      (width 296, spacing 5)   ← 10 linhas de módulo
   └─ Column right        (width Fill, spacing 14)
      ├─ Container detalhe_do_módulo_selecionado   (acento do módulo)
      ├─ Container transferência_ativa             (âmbar; só se houver transferência)
      └─ Scrollable consola                       (height Fill, borda GREEN_BRD)
```

### Linha de módulo (sidebar)
`button` de largura total, sem fundo próprio; o estado define fundo/borda:

| Estado | Fundo | Borda | Título | Subtítulo | Indicador |
|---|---|---|---|---|---|
| ativo | `GREEN_BG` | `GREEN_BRD` | `TEXT` | `TEXT_4` | ponto 6px `GREEN` |
| ativo + selecionado | `GREEN_SEL` | `GREEN` | `TEXT` | verde suave `#8AC9AB` | ponto 6px `GREEN` |
| interativo (FILES, CONFIG) | `GLASS` | `BRD_2` | `TEXT_1` | `TEXT_4` | valor ou nada |
| inativo | `rgba(255,255,255,.025)` | `BRD_1` | `TEXT_3` | `TEXT_5` | ponto 6px `MUTED` |

Conteúdo da linha: `nº` (10, `TEXT_6`/`MUTED`) · glifo (13) · coluna com título (12) e
subtítulo (10, **dado real do daemon**, não descrição genérica) · indicador · chevron `›`.

Ordem e glifos: `01 CLIP ⧉ · 02 FILES ⇄ · 03 NOTIF ◔ · 04 MEDIA ▷ · 05 BATT ▮ ·
06 CONTROL ⌘ · 07 AUDIO ≋ · 08 WEBCAM ◎ · 09 TRACK ◈ · 10 CONFIG ⚙`

⚠ **Verificar os glifos na mono do sistema.** `≋ ◔ ◈ ⧉` faltam em muitas fontes
monoespaçadas e saem como caixa vazia. Se faltarem: embutir uma fonte de ícones
(Nerd Font / Material Symbols) ou desenhar com `canvas`.

### Consola
`Scrollable` com seleção de texto, mono 11, `line-height 1.85`, fundo `TERMINAL`.
Formato: `HH:MM:SS [tag] mensagem`. **Colorir a tag** como no app Android
(o desktop hoje não colore): `[+]` `GREEN` · `[i]` `TEXT_3` · `[!]` `AMBER` ·
`[x]` `RED`. Texto da mensagem em `TEXT_4`. Cabeçalho: label + `copiar log` + `limpar`.

### Estado a emparelhar
Acento âmbar em todo o ecrã (ponto, wordmark, pill, cartão). Sidebar inteira em cinza
`MUTED` com legenda "MÓDULOS · EM ESPERA". Cartão central `AMBER_BG`/`AMBER_BRD`:
instrução · QR 196×196 (branco, padding 12, radius 10 — imagem da crate `qrcode`) ·
três linhas copiáveis de largura total (`FINGERPRINT` abreviado, `HOST : PORTA`, `TOKEN`)
com label à esquerda, valor + `copiar` à direita · pé com "à escuta em 0.0.0.0:4711 ·
token expira em MM:SS".

## 5 · Ecrãs de módulo (10)

Ao abrir um módulo, **a sidebar de 296px recolhe para a régua de 44px** e o conteúdo do
módulo ocupa a largura restante. Cabeçalho da janela mantém-se; ganha `‹ VOLTAR`.

```
Column [padding 26/28, spacing 18]
├─ Row header (wordmark | ‹ VOLTAR + ×)
└─ Row [spacing 16, height Fill]
   ├─ Column régua (width 44, spacing 8, align center)
   │     botão ativo 34×34 com glifo + acento; restantes só o número em TEXT_6
   └─ Column conteúdo (width Fill, spacing 14–16)
         ├─ Row título   (título display 20 + subtítulo de estado | ação primária)
         │                separador inferior 1px BRD_1, padding_bottom 12
         └─ … específico do módulo
```

### 01 CLIP — `03-clip.png`
1. Cartão `ATUAL` (verde): tipo + tamanho + origem + hora; conteúdo em 12; ações
   `enviar ao telemóvel` (verde) · `copiar` · `fixar`.
2. Cabeçalho de histórico: "HISTÓRICO · N ITENS · 7 DIAS" + `pesquisar…` + `limpar`.
3. `Scrollable` de itens: glifo de tipo (⧉ URL, ≡ texto, ▣ imagem) · preview em uma
   linha com elipse · meta `TIPO · origem · há X` · ação à direita (`copiar`/`abrir`)
   ou etiqueta `fixado` em verde. Item fixado tem fundo `rgba(61,255,158,.03)`.

### 02 FILES — `04-files.png`
1. Cartão da pasta de destino + botão `escolher pasta` (diálogo nativo via `rfd`, já existe).
2. Cartão de transferência ativa (âmbar): nome · direção/velocidade/ETA · barra de
   progresso 4px · `X / Y MB · retomado no bloco N` · `pausar` / `cancelar`.
   **Isto é novo na GUI** — hoje o progresso só aparece no log.
3. Histórico com filtros `tudo` / `recebidos` / `enviados`; cada linha: seta `↓`/`↑`
   colorida pelo resultado (verde ok, `TEXT_3` enviado, `RED` falhou) · nome · meta
   `recebido · 4.2 MB · 12 s · ontem 18:41` · ação (`abrir pasta` / `reenviar` /
   `retomar` em vermelho quando falhou).

### 03 NOTIF — `05-notif.png`
1. Linha de chips por app com contagem; o ativo em verde; último chip `gerir bloqueios`.
2. Cartão `A CHEGAR AGORA` (verde): remetente 13 · texto 11 · `responder` ·
   `dispensar` · `copiar texto`.
3. Histórico — **altura de linha dinâmica**: `Row [align top]` com app (largura 56,
   `MUTED`, 10) · `Column` com título (`TEXT_1`, 11) e corpo (`TEXT_3`, 11,
   `line-height 1.65`, **sem elipse, envolve em quantas linhas precisar**) · hora
   (`TEXT_5`, 10, alinhada ao topo). Padding 13/15. A lista é `Scrollable` com rodapé
   fixo "N entradas mais antigas · ver tudo" — necessário porque uma entrada de 3 linhas
   já empurra a última fora da janela de 800px.

### 04 MEDIA — `06-media.png`
Cartão do que está a tocar (capa 96×96, título 15, artista/álbum 11, barra de posição
3px, tempos) · fila de 5 botões de largura igual (`⏮ ⏸ ⏭ shuffle repetir`; o ativo com
borda `GREEN`) · slider de volume do player · lista `PLAYERS DETETADOS` com o controlado
em verde.

### 05 BATT — `07-batt.png`
Cartão com percentagem display 56 em verde + glow, "cheia em ~18 min · 21 W", e coluna
`TEMPERATURA` / `SAÚDE` / `CICLOS` à direita · gráfico de barras das últimas 12 h
(altura 110; barra colorida pelo regime: `GREEN` a carregar, `AMBER_BRD` em descarga
baixa, `GREEN_BRD` normal) · lista de alertas com `ativo`/`desligado`.

### 06 CONTROL — `08-control.png`
Cartão de workspaces: quadrados 40×40 radius 9; **só o ativo é preenchido** (fundo
`GREEN`, texto `#04150D`); os existentes com borda `GREEN_BRD`; os vazios com borda
`BRD_1` e texto `TEXT_6`. Abaixo, linha de contexto "3 · code — 4 janelas · foco: kitty".
Depois a lista de atalhos expostos (`nome` → `comando`) com `editar`, e um campo
`hyprctl dispatch …` + `executar`.

### 07 AUDIO — `09-audio.png`
Cartão do tap (verde) com VU de 16 barras (altura 56; barras acima do limiar em `GREEN`,
resto em `GREEN_BRD`) e "N KB enviados · duração" · mixer: `MESTRE`, uma linha por
aplicação, e `MICROFONE DO TELEMÓVEL → PC`. Slider acima de 100% fica **âmbar**
(pista, punho e valor). **Marca dos 100%**: risco de 1px × 12px em `TEXT_6` na posição
correspondente a 100% do intervalo do slider (o intervalo vai até 125%, logo a marca
fica a 80% do trilho). Canal silenciado: punho em `MUTED` na posição 0.

### 08 WEBCAM — `10-webcam.png`
Estado inativo, **cinza, não vermelho**. Área de pré-visualização vazia (fundo
`TERMINAL`, glifo ◎ 26 em `#2A2A2A`, "Pré-visualização desligada" + duas linhas de
explicação) · ação primária `iniciar stream` em verde no cabeçalho · três cartões glass
`DISPOSITIVO` / `RESOLUÇÃO` / `CODEC` · linha de pé com último uso + `permissões`.
Quando ativo: substituir a área pelo frame (`iced::widget::image` alimentado pelo
decoder) e o cabeçalho passa a `parar stream`.

### 09 TRACK — `11-track.png`
Área de espelho do cursor: fundo `TERMINAL` com grelha de 44px (linhas `#0E0E0E`),
label do canto "ESPELHO DO CURSOR · 2560×1440", ponto 9px `GREEN` com glow e anel de
34px `GREEN_BRD` na posição do cursor, coordenadas `x · y` no canto inferior direito ·
sliders `SENSIBILIDADE` e `VELOCIDADE DE SCROLL` (cinza, módulo em repouso) · três
toggles (`aceleração`, `inverter scroll`, `teclado virtual`).

### 10 CONFIG — `12-config.png`
Três blocos de lista (`REDE`, `DISPOSITIVOS EMPARELHADOS`, `DAEMON`) no padrão
cabeçalho + linhas `label | valor`; valores booleanos ativos em verde; ações destrutivas
(`revogar`, `apagar`) em `RED`; o dispositivo ligado agora tem `revogar` em vermelho vivo
e os outros em `MUTED`. Ação de cabeçalho: `reiniciar daemon`.

## 6 · Mensagens

```rust
enum Message {
    // navegação
    OpenModule(ModuleId),      // sidebar → ecrã de módulo (recolhe para régua)
    Back,                      // ‹ VOLTAR
    Quit,                      // × — fecha QUIC, mata órfãos, encerra (já implementado)

    // ligação
    PairingTick,               // countdown do token
    CopyPairingField(Field),   // fingerprint / host:porta / token
    ConsoleAppend(LogLine),
    CopyConsole,
    ClearConsole,

    // clip
    ClipSendToPhone, ClipCopy(usize), ClipPin(usize),
    ClipSearch(String), ClipClearHistory,

    // files
    PickDestination,           // rfd, já implementado
    SendFile, PauseTransfer(TransferId), CancelTransfer(TransferId),
    ResumeTransfer(TransferId), RevealInFolder(TransferId),
    FilesFilter(Direction),

    // notif
    NotifFilter(Option<AppId>), NotifReply(NotifId), NotifDismiss(NotifId),
    NotifCopy(NotifId), NotifSearch(String), NotifClearHistory, NotifMute(Duration),

    // media
    MediaPrev, MediaToggle, MediaNext, MediaShuffle, MediaRepeat,
    MediaVolume(f32), MediaSelectPlayer(PlayerId),

    // batt / control / audio / track / webcam / config
    BattAlertToggle(AlertKind),
    ControlWorkspace(u8), ControlRunShortcut(ShortcutId), ControlRunIpc(String),
    AudioToggleTap, AudioVolume(ChannelId, f32), AudioMute(ChannelId),
    TrackSensitivity(f32), TrackScrollSpeed(f32), TrackToggle(TrackFlag),
    WebcamStart, WebcamStop, WebcamFrame(Frame),
    ConfigSet(ConfigKey, ConfigValue), ConfigRevoke(DeviceId), ConfigRestartDaemon,
}
```

Estado da vista: `screen: Screen { Dashboard { selected: Option<ModuleId> }, Module(ModuleId) }`.
No dashboard, `selected` controla qual painel de detalhe aparece na coluna direita —
é o mesmo conteúdo do ecrã de módulo, em versão compacta.

## 7 · Animação

Um só efeito, herdado do Android: o **ponto de estado respira** (opacidade 0.45→1.0,
escala 1.0→1.5). Período: 2.8 s no verde, 1.5 s no âmbar, 1.2 s no vermelho — a
velocidade comunica urgência. Em iced: `window::frames()` como subscription e interpolar
o raio/alpha no `canvas`, ou um `Timer`. Não animar mais nada.
