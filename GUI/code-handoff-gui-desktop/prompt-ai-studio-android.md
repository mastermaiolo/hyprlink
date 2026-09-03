# Prompt — HyprLink UI (ronda 4)

Copiar tudo abaixo desta linha.

---

Estou a desenvolver o **HyprLink**, uma app Android (Kotlin + Jetpack Compose, Material 3) que integra um telemóvel Android com um desktop Linux a correr Hyprland — equivalente ao ecossistema da Apple, mas para Linux. O daemon do PC é em Rust e fala um protocolo próprio (KDEC/1.6) sobre TCP com CBOR e TLS (ed25519), com descoberta por mDNS (`_hyprlink._tcp`, porta 7331).

Preciso que implementes cinco ecrãs em Compose seguindo **exatamente** a especificação visual e estrutural abaixo. Não inventes ecrãs, secções nem funcionalidades que não estejam descritas.

## Sistema de design

**Fundo:** preto puro `#000000`. Superfícies de cartão `#0B0B0B`, cartões elevados `#0D0D0D`, terminal `#070707`. Bordas `#1C1C1C` (normal) e `#1E1E1E` (elevado); borda de destaque neutro `#262626`.

**Cores de estado — regra rígida:**
- Verde néon `#3DFF9E` — ativo, ligado, ok. É a cor da marca.
- Âmbar `#FFB020` — pendente, a sincronizar, a negociar, aviso, zona de volume acima de 100%.
- Vermelho `#FF4757` — **apenas erro real ou ausência de ligação**. Nunca para "desligado por opção".
- Cinza `#5E5E5E` / `#6E6E6E` — inativo, desligado por opção, sem dados.

Esta distinção é importante: "a webcam não está a ser usada" é cinza; "as notificações não têm permissão" é vermelho.

**Texto:** `#FFFFFF` (títulos), `#E4E4E4` (corpo), `#B4B4B4` (secundário), `#6E6E6E` (legendas), `#4E4E4E` (log).

**Tipografia:** JetBrains Mono em todo o corpo, dados, logs e etiquetas. Archivo (peso 700) nos títulos de ecrã e nomes de cartão. Etiquetas de secção em maiúsculas, 9sp, letter-spacing 0.16em, cor `#6E6E6E`.

**Formas:** raio 9–12dp em botões e cartões pequenos, 14–16dp em cartões grandes, 18dp na superfície do touchpad, `CircleShape` em pílulas e botões de play.

**Movimento:** um pulso lento e respirante nos indicadores de estado — escala 1.0→1.55 e opacidade 0.45→1.0, `infiniteRepeatable` com easing suave. Períodos diferentes por elemento (2.4s a 3.2s) para não sincronizarem. Quando o estado é vermelho, o pulso acelera (1.1–1.5s). Nada de gradientes agressivos nem animações rápidas.

**Barra de navegação inferior:** seis destinos fixos — INÍCIO, MÉDIA, RATO, AÇÕES, FICH., CONFIG. Ícone e etiqueta a 8sp; ativo em `#3DFF9E`, inativo em `#6E6E6E`. Alvos de toque nunca abaixo de 48dp.

---

## Ecrã 1 — Dashboard (INÍCIO)

Ordem vertical, tudo numa coluna com scroll:

**1. Cabeçalho colapsado (uma linha).** À esquerda o wordmark "HYPR" em branco + "LINK" em `#3DFF9E` com sombra de brilho, 19sp Archivo. À direita um ponto pulsante e o texto do estado ("LINK ATIVO", 10sp, letter-spacing 0.12em). O wordmark é grande (40sp) apenas no primeiro arranque; depois fica sempre nesta forma compacta.

**2. Barra da estação.** Cartão discreto — sem cor de fundo, apenas borda `#1E1E1E`. Ponto pulsante à esquerda, nome do PC (`maggio-ryoku`, 14sp) com chevron de dropdown, e sob ele a linha técnica "KDEC/1.6 · latência 4 ms · TLS emparelhado" a 9sp. À direita um botão de estado com borda e texto da cor do estado ("ON"). Tocar no botão abre o diálogo de ligação.

Quando não há ligação, este cartão muda inteiro: título "CONECTAR AO LINUX", subtítulo "última: maggio-ryoku · há 12 min", botão "LIGAR" em vermelho. O wordmark, o ponto e o botão mudam de cor em conjunto — é a peça de identidade da app.

**3. Player de média.** Cartão elevado. Capa quadrada 50dp com raio 12dp (usar padrão diagonal listado como placeholder quando não há arte). Título da faixa a 14sp branco com ellipsis, sob ele "artista · aplicação · MPRIS" a 9sp. À direita, em linha: anterior (◀◀), play/pause num círculo cheio de 38dp em `#3DFF9E` com o glifo em preto, e seguinte (▶▶). Abaixo, barra de progresso de 3dp com preenchimento verde e os tempos decorrido/total nos extremos.

**4. Secção "AQUI MESMO"** — ações que se resolvem sem sair do ecrã.

*Cartão de clipboard:* etiqueta "CLIPBOARD" à esquerda, estado à direita (ponto + "a sincronizar" em âmbar). Abaixo, o conteúdo real da área de transferência num bloco `#060606` com borda, 11sp, uma linha com ellipsis. Depois dois botões: "ENVIAR AO PC ↑" a ocupar a largura, em verde cheio com texto preto, e um botão quadrado de 44dp ao lado com "↓" (receber).

*Par de botões compactos:* dois cartões iguais lado a lado, cada um com um quadrado de ícone 30dp com borda e duas linhas de texto. Esquerda: "Enviar" / "3 hoje". Direita: "Webcam" / "inativa".

**5. Secção "SERVIÇOS"** — apenas estado, não navega (a navegação já vive na barra inferior). Grelha de 2 colunas com quatro linhas compactas: ponto de estado, nome (11sp), e a palavra do estado à direita em 9sp. Exemplos: CONTROL/ativo (verde), WEBCAM/inativo (cinza), MIC BRIDGE/ativo (verde), NOTIFS/sem permissão (vermelho, borda do cartão avermelhada).

**6. Secção "TELEMETRIA DO PC".** Cinco caixas numa linha. As quatro primeiras iguais: etiqueta 8sp, valor 14sp, barra de progresso de 2dp por baixo — CPU 34%, RAM 11G, TEMP 52° (âmbar), BAT 87%. A quinta é mais estreita (fator 0.72) e é um botão: "＋" e a palavra "DETALHE" a 7sp, só borda. Quando não há ligação, os valores mostram "--" e as barras ficam vazias.

**7. Terminal.** Cartão `#070707` que fecha o ecrã e **nunca é comprimido** — altura intrínseca, não encolhe. Cabeçalho: "TERMINAL · SESSÃO 001" a 9sp com letter-spacing, e à direita "copiar ⧉" e um menu "☰". Abaixo, uma fila de filtros em pílulas pequenas: TUDO (ativa, verde cheia com texto preto), TX/RX, AVISOS, ERROS (só borda). Depois as linhas de log, 9sp, formato `HH:mm:ss` em `#4E4E4E` + etiqueta colorida por nível + mensagem:

```
03:04:51  mdns   a procurar _hyprlink._tcp
03:04:53  tls    fingerprint ok 4f:a2:9c…
03:04:54  link   aberto · rtt 4 ms
03:05:02  tx     mpris.playpause → opera
03:05:02  rx     ack 214B · 12 ms
```

O terminal registra desde o arranque da app, **antes de haver qualquer ligação** — mDNS, retries com backoff, ligações recusadas. No estado desligado o cabeçalho lê "TERMINAL · À PROCURA" com "scan" a pulsar em âmbar, e o log mostra a sequência de tentativa:

```
03:04:40  mdns   a anunciar _hyprlink._tcp
03:04:42  warn   maggio-ryoku não responde
03:04:44  retry  tentativa 2/5 · backoff 2s
03:04:48  erro   ligação recusada :7331
```

---

## Ecrã 2 — Áudio

Cabeçalho com seta de voltar e o título "ÁUDIO" (19sp Archivo).

**Controlo de média:** cartão com etiqueta "CONTROLO DE MÉDIA" e o nome da app-fonte em verde à direita; capa, título, "artista · 2:14 / 4:03" e o mesmo trio de botões do dashboard.

**Ouvir no telemóvel:** cartão com título 14sp Archivo, subtítulo "Toca o áudio do PC diretamente aqui", e um switch M3 à direita com o track em `#3DFF9E`.

**Secção "SAÍDA DE SOM":** um cartão por sink. O ativo tem fundo `#071510`, borda verde, radio button preenchido, nome em verde e a pílula "ATIVO"; os restantes ficam neutros com radio vazio.

Cada cartão de sink tem um slider com esta construção específica: à esquerda um botão de mudo de 30dp com borda e o glifo "◀))" (não um ícone ambíguo de partilha). O track tem 5dp de altura e vai de 0 a 150%, mas está dividido em dois segmentos — de 0 a 100% ocupa 2/3 da largura, de 100 a 150% o último 1/3 com fundo `rgba(255,176,32,.16)`. Há um **entalhe vertical de 1dp e 18dp de altura em `#4E4E4E` na posição dos 100%**, marcando onde o volume deixa de ser seguro. O thumb é um círculo de 12dp em verde com sombra de brilho. À direita o valor em percentagem, 12sp, largura fixa de 38dp alinhada à direita. Sob o slider do sink ativo, três marcas: "0", "100 · seguro", "150".

**Secção "APLICAÇÕES":** um cartão por app com som. Nome 14sp Archivo, estado à direita ("a reproduzir" em verde), e o mesmo slider. Uma app sem som há muito mostra apenas o subtítulo "sem som há 3 min" e um botão "MUDO" com borda neutra.

---

## Ecrã 3 — Mission Control (AÇÕES)

Cabeçalho: título "MISSION CONTROL" (20sp Archivo) com o subtítulo "Gere os workspaces e janelas ativas", e à direita um botão circular de 36dp com "⟳".

**Secção "WORKSPACES":** fila horizontal. **Só o workspace ativo é preenchido** — fundo `#3DFF9E` cheio, número "WS 1" a 17sp em `#001a0e`, e "2 janelas · ativo" a 9sp em `#04301c`; recebe também mais largura (fator 1.25). Os outros são apenas contorno `#262626` com número em `#B4B4B4` e contagem em `#6E6E6E`. No fim, uma caixa estreita com "›" para os restantes.

**Fila de lançadores:** quatro pílulas de largura igual com nomes de aplicação (firefox, code, ghostty, spotify), só borda.

**Secção "JANELAS ATIVAS · 7":** lista agrupada por workspace, com um cabeçalho de grupo a 9sp ("WORKSPACE 1"). Cada janela é um cartão com o título em 13sp Archivo (ellipsis numa linha), a linha "classe · handle" a 9sp por baixo, e um "×" vermelho à direita para fechar. A janela em foco leva uma borda esquerda verde de 2dp e o subtítulo em verde ("opera · em foco").

---

## Ecrã 4 — Transferências (FICH.)

Cabeçalho com seta, título "TRANSFERÊNCIAS", subtítulo "Ficheiros partilhados na sessão", e "Limpar" à direita.

**"SESSÃO ATUAL"** (etiqueta em verde). Transferência em curso: quadrado de ícone 32dp com fundo `#071510` e borda verde, direção em "↑"/"↓", nome do ficheiro 12sp com ellipsis, "enviado · 231.9 KB" a 9sp, pílula "EM CURSO" com borda verde. Barra de progresso de 3dp e, abaixo, "78%" à esquerda e "181 KB de 231.9 KB" à direita.

Transferência concluída: **o hash SHA-256 não aparece por defeito** — fica atrás de um link "ver hash ▾" a 9sp em `#4E4E4E`, ao lado do tamanho. O que se vê é só a pílula "✓ VERIFICADO".

**"HISTÓRICO PERSISTIDO"** com "limpar histórico" à direita. Linhas mais leves — ícone só com borda, nome em `#D4D4D4`, "tamanho · ontem", o mesmo "ver hash ▾" e um "✓" verde simples.

**Estado de erro** — o único que abre por defeito, porque é o único que exige leitura. Borda do cartão `#2A1418`, ícone com fundo `#150809` e glifo vermelho, pílula "ERRO". Abaixo, um bloco `#120609` com a mensagem técnica na primeira linha (`write failed because stream was aborted`) e a **explicação em português na segunda**, em tom mais escuro: "o PC fechou a ligação a meio · tenta de novo". Depois dois botões: "TENTAR DE NOVO" com borda vermelha a ocupar a largura, e "apagar" estreito com borda neutra.

**FAB:** botão "＋ ENVIAR FICHEIRO" ancorado em baixo à direita, verde cheio com texto `#001a0e`, raio 14dp, padding generoso (14dp vertical, 22dp horizontal).

---

## Ecrã 5 — Rato & Teclado (RATO)

Cabeçalho: "RATO & TECLADO" (19sp Archivo), subtítulo em verde "touchpad sensível · maggio-ryoku", e um botão circular de 36dp com "⌨" à direita.

**Superfície do touchpad:** ocupa todo o espaço vertical disponível (`weight(1f)`) — é o elemento dominante do ecrã. Borda `#1E3E30`, raio 18dp, e um brilho radial muito suave a partir do centro (`rgba(61,255,158,.06)`). Um ponto de cursor de 10dp em verde com sombra de brilho segue o dedo.

As instruções **não** ficam no meio da superfície. São **uma única linha discreta no rodapé da superfície**, 9sp em `#3A3A3A`: "1 dedo move · 2 dedos scroll · toque longo para ajuda". Desvanecem após o primeiro uso bem-sucedido e voltam com um toque longo.

**Fila de modificadores:** pílulas com scroll horizontal — ESC, TAB, SUPER, CTRL, ALT, ↑ — borda `#262626`, texto verde 10sp.

**Botões do rato:** três alvos grandes (padding vertical 15dp) com borda `#1E3E30` — ESQUERDO, MEIO (mais estreito), DIREITO.

**Entrada de texto:** campo com placeholder "escrever no PC…" e um botão quadrado de 52dp em verde cheio com "↵".

---

## Requisitos técnicos

- Kotlin + Compose, Material 3, tema escuro fixo (a app não tem tema claro).
- Um `object HyprColors` com os tokens acima; nada de cores hard-coded espalhadas pelos composables.
- Um `enum class LinkState { DISCONNECTED, NEGOTIATING, CONNECTED }` que conduz a cor do wordmark, do ponto de estado, do cartão da estação e do botão de ligação em conjunto.
- Um `enum class ServiceState { ACTIVE, PENDING, INACTIVE, ERROR }` mapeado para as quatro cores, respeitando a regra de que inativo é cinza e não vermelho.
- Estado com `StateFlow` num ViewModel por ecrã; os composables recebem estado imutável e emitem eventos.
- O pulso respirante num único composable reutilizável (`BreathingDot(color, periodMs)`).
- Navegação com `NavHost` e os seis destinos da barra inferior.
- Dados de exemplo idênticos aos valores citados acima, para eu comparar o resultado com o desenho.
