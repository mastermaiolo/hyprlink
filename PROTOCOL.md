# HyprLink — Protocolo (fonte canônica)

Extraído do código real do app Android (`ConnectionRepository.kt`,
`ConnectionUtils.kt`, `PairingScanner.kt`, `WebcamStreamer.kt`,
`AudioStreamPlayer.kt`, `HyprNotificationListenerService.kt`,
`HyprLinkConnectionService.kt`), não da documentação em prosa (que está
desatualizada/incompleta em vários pontos). Este é o documento a manter
atualizado conforme o daemon Rust avança — os outros `.md` do projeto são
histórico, não fonte de verdade.

## 1. Transporte

- QUIC (RFC 9000) sobre UDP, porta **7443** (`HYPRLINK_SERVICE_PORT`, único
  listener — não existe porta de pareamento separada; o valor do QR é
  reaproveitado igual para reconexões).
- ALPN: `"hyprlink/1"` (string fixa, idêntica no handshake de pareamento e
  nas reconexões).
- mTLS 1.3: ambos os lados geram identidade própria — ECDSA P-256
  (`secp256r1`), certificado X.509 autoassinado, `SHA256withECDSA`, validade
  ~10 anos.
- **Validação do certificado do servidor** (lado Android): `.noServerCertificateCheck()`
  desativa toda validação de CA/hostname do motor TLS; em seguida, pin manual —
  SHA-256 do DER completo do `serverCertificateChain[0]`, comparado em hex
  (case/`:`-insensitive) contra o fingerprint do QR. Se a conexão não chegar a
  `isConnected`, o pin é pulado (edge case a não replicar do lado servidor).
- **Certificado do cliente**: sempre apresentado (mTLS), incondicionalmente,
  pareando pela primeira vez ou reconectando. O servidor (daemon) deve
  espelhar a mesma filosofia: aceitar qualquer cert de cliente na camada TLS
  (sem CA) e validar por fingerprint pós-handshake contra uma lista de
  dispositivos pareados — **antes** de processar qualquer pacote de aplicação.

### QR de pareamento

Formato: `<FINGERPRINT_SHA256_HEX>|<HOST>:<PORTA>|<TOKEN_HEX>`
Exemplo: `8F:4E:92:10:BC:...:5A|192.168.1.100:7443|d8e03f56a14c99e120bc`
(host pode ser IPv4/hostname/IPv6 entre colchetes `[fe80::1]`; parse pelo
último `:` da segunda parte, não pelo primeiro).

## 2. Enquadramento (framing)

Duas implementações independentes no app (pareamento vs pós-pareamento) —
**ambas usam o mesmo formato de bytes**, só os limites de tamanho diferem.

- **Streams bidirecionais** (controlo/RPC/eventos): 4 bytes **big-endian**
  (tamanho) + payload CBOR de um `Packet`. Limite de leitura no app: 5 MiB
  durante o handshake de pareamento, 10 MiB depois. Tamanho `<= 0` é rejeitado
  nos dois casos. O daemon pode adotar um limite único generoso (ex: 16 MiB).
- **Streams unidirecionais** (ficheiros, vídeo webcam, PCM áudio/mic): 8 bytes
  **big-endian** (`u64`, id de correlação com um pacote anunciado antes num
  stream bidi) + bytes crus até EOF (fim = stream fechado pelo remetente), sem
  mais nenhuma framing.

```
Packet (envelope, CBOR map, definite-length):
{
  "id": uint64,          // sequencial, ou id de correlação p/ streams unidirecionais
  "type": text,           // "modulo.acao" — OBRIGATÓRIO, senão o pacote é descartado
  "body": map | null,     // ver §3 sobre chunked/indefinite-length
  "has_payload": bool     // ver §4 — NÃO significa "tem resposta"
}
```

## 3. Codificação CBOR

Lib usada no app: `co.nstant.in:cbor:0.9`.

- O **envelope** (`Packet`) é sempre CBOR **definite-length**.
- O **`body`** e qualquer map/array aninhado dentro dele são codificados pelo
  app como **indefinite-length (chunked)** — major type 4/5 com header
  0x9F/0xBF e byte de break 0xFF, sem contagem no head byte. Isso é assim
  desde que um bug de compatibilidade foi corrigido (ver `BUILD_REPORT.md`
  item "Normalização de Codificação Indefinida"); antes disso o daemon
  antigo rejeitava esses pacotes silenciosamente.
- Isso é uma feature padrão do RFC 8949 — qualquer crate/parser CBOR correto
  decodifica indefinite-length nativamente. **Não é preciso nenhum hack no
  lado Rust**, só garantir que a crate escolhida (`ciborium`) realmente lida
  bem com isso via um teste de round-trip com bytes reais capturados do app
  (não confiar só na spec).
- Ao **responder**, o daemon pode mandar CBOR definite-length normal — o
  decoder do app (`decodeCborValue`, walk recursivo genérico) não exige
  chunked-ness no que recebe.
- Mapeamento de tipos: `String↔UnicodeString`; número `≥0→UnsignedInteger`
  senão `NegativeInteger` (sempre truncado pra `Long` — **sem float/double**
  em nenhuma direção); `Bool↔SimpleValue.TRUE/FALSE`; `ByteArray↔ByteString`;
  `null↔SimpleValue.NULL`. **Chaves de mapa são sempre string** (nunca chave
  inteira CBOR).
- `type` ausente ou não-string ⇒ pacote inteiro descartado. `body` que não é
  um CBOR Map (incluindo ausente) ⇒ vira `null` no Kotlin, sem erro. `has_payload`
  só é `true` se o DataItem cru for exatamente `SimpleValue.TRUE`.
- **Exceção**: `core.hello` é montado inteiramente via `CborBuilder`/`putMap`/`putArray`
  fluente, sem chunked — é definite-length dos dois lados, único pacote assim.

## 4. Semântica de `has_payload` e do fluxo de resposta (ponto crítico histórico)

**`has_payload` não indica "espera resposta"** — é usado de dois jeitos
completamente diferentes, e confundir os dois já causou bugs reais:

- **Fire-and-forget** (`sendOneWayPacket`, `has_payload=false`): o app escreve
  o frame, faz half-close do lado de escrita, e **imediatamente** aborta o
  lado de leitura (`stream.closeInput(0)`, um QUIC `STOP_SENDING`) — nunca lê
  nada de volta, não espera. Usado por: `clipboard.set`, todos `input.*`,
  `media.command`, `share.url`, `battery.state` (telefone→PC), `webcam.error`,
  `webcam.transform`, `webcam.mic_stop`, `notification.post`, `notification.dismissed`.
  O daemon deve tolerar o reset abrupto de leitura sem tratar como erro.
- **Pedido/resposta tipado** (hand-rolled, não passa por `sendOneWayPacket`,
  quase sempre também com `has_payload=false` no pacote de saída!): o app
  escreve, half-closes, e **bloqueia lendo até EOF** uma resposta CBOR
  completa na mesma stream, com timeout de 10s (maioria) ou 5s
  (`audio.set_volume`, `audio.set_mute`, `audio.set_default_sink`,
  `audio.tap_stop`). Usado por: `core.ping`, `hypr.workspaces/clients/dispatch`,
  `battery.request`, `audio.state/set_volume/set_mute/set_default_sink/tap_start/tap_stop`.
  **O daemon precisa terminar de escrever a resposta típica e fechar/half-close
  a stream logo em seguida** — se não fechar, o app trava até o timeout e
  desiste (era o bug do daemon antigo travando 10s no controlo de mídia/áudio).
- **`has_payload=true`** de verdade (`sendAnnouncedPacket` com o parâmetro
  explícito): usado só por `share.file` (as duas direções) e
  `webcam.mic_start` — sinaliza que **um stream unidirecional separado vai
  seguir**, correlacionado pelo `id` do pacote. O app ainda bloqueia até 10s
  lendo (e descarta) qualquer coisa que o daemon escreva nessa stream
  bidi antes de prosseguir — então o daemon **precisa fechar essa stream de
  controlo** para não travar o início da transferência/streaming.

## 5. Catálogo de pacotes por módulo

Direção: **P→D** telemóvel→daemon, **D→P** daemon→telemóvel.

### core
| type | dir | body |
|---|---|---|
| `core.hello` | P→D req / D→P resp | P→D: `{device_name, capabilities:[...], pairing_token?}` (token só no 1º pareamento); D→P: `{device_name}` (único campo lido) |
| `core.ping` | P→D req (10s) | `null` |
| `core.pong` | D→P resp | não parseado |

Capabilities enviadas pelo app (hardcoded, note que não reflete tudo que o
app realmente faz): `["core","clipboard","notification","media","battery","share"]`.

### clipboard
| `clipboard.set` | bidi (mesmo tipo nas duas direções) | `{text: String}` |

### input (P→D, fire-and-forget)
`input.move {dx,dy}` · `input.scroll {dx,dy}` · `input.click {button}` (left/right/middle)
· `input.type {text}` · `input.key {key}` (ex: "Return", "BackSpace", "Escape", "Tab")

### hypr
| type | dir | body |
|---|---|---|
| `hypr.workspaces` → `hypr.workspaces_state` | req/resp | resp: `{ok, data}` (data = JSON cru de `hyprctl workspaces -j`) |
| `hypr.clients` → `hypr.clients_state` | req/resp | resp: `{ok, data}` (JSON cru de `hyprctl clients -j`) |
| `hypr.dispatch` → `hypr.dispatch_result` | req/resp | req: `{cmd}` (ex: "workspace 2", "focuswindow address:0x..", "closewindow ..."); resp: `{ok, data}` |
| `hypr.event` | D→P push | `{event: String}` (ex: "workspace>>2") — lido de `/tmp/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock` |

### battery
`battery.request` (P→D req) · `battery.state` (bidi, mesmo tipo — `{level:Int, charging:Bool}`)

### media
`media.command` (P→D one-way, `{command}`: play_pause/play/pause/next/previous)
`media.state` (D→P push, `{player, status, title?, artist?, album?}`, status: Playing/Paused/Stopped)

### notification
| type | dir | body |
|---|---|---|
| `notification.post` | P→D one-way | `{key, app, title, text (≤2000c), actions:[{idx,label,is_reply}]}` |
| `notification.dismissed` | P→D one-way | `{key}` |
| `notification.send` | D→P push | `{title, body, app_name}` |
| `notification.action` | D→P push | `{key, idx (ou action)}` |
| `notification.reply` | D→P push | `{key, idx, text}` |
| `notification.dismiss` | D→P push | `{key}` |

### share
| type | dir | body |
|---|---|---|
| `share.file` | anúncio bidi (`has_payload=true`), **as duas direções** | `{name, size}` — `id` do pacote correlaciona com o uni-stream de bytes |
| `share.progress` | push **D→P sempre** | `{id, bytes, total}` |
| `share.done` | push **D→P sempre** | `{id, ok, sha256 (hex), bytes, error?}` |
| `share.url` | P→D one-way | `{url}` |

Uni-stream de ficheiro: 8 bytes id + bytes crus, chunks de 32 KiB. SHA-256
sobre os bytes exatos. Nome de ficheiro sanitizado (só último componente do
path, rejeita `..`/`/` embutido). Receptor espera até 15s por um
`share.done` após o EOF do uni-stream antes de marcar como "não
verificado". **`share.progress`/`share.done` são sempre mandados pelo PC**
(não é "quem recebe confirma" — o lado Android sempre só ESPERA por eles,
nas duas direções, nunca os envia; ver `ConnectionRepository.kt::saveFileFromStream`,
que já vinha pronto pra receber ficheiro do PC antes até de existir um jeito
de mandar um). **Telemóvel→PC**: telemóvel anuncia+envia, PC recebe e manda
`share.progress`/`share.done` (`share.rs::receive_uni_stream`, salva em
`config::download_dir`). **PC→telemóvel** (`share.rs::send_file`, botão
"enviar" do módulo FILES): PC anuncia+envia e TAMBÉM manda `share.done` no
final (calculando o próprio SHA-256 dos bytes enviados) — sem isso o
telemóvel espera 15s à toa e marca "não verificado".

### audio
| type | dir | body |
|---|---|---|
| `audio.state` → `audio.state_reply` | req/resp | resp: `{default_sink, sinks:[{id,name,description,volume(0-150),muted,is_default,is_phone}], apps:[{id,name,media?,volume(0-150),muted,sink_id}]}` |
| `audio.set_volume` | req/ack (5s) | `{kind:"sink"\|"app", id, volume(0-150)}` |
| `audio.set_mute` | req/ack (5s) | `{kind, id, muted}` |
| `audio.set_default_sink` | req/ack (5s) | `{name}` (nome técnico do sink, não a descrição) |
| `audio.tap_start` → `audio.tap_ready` | req/resp | resp: `{id, rate:48000, channels:2}` — `id` correlaciona uni-stream PCM |
| `audio.tap_stop` | req/ack (5s) | `null` |
| `audio.ack` | resp genérica | não parseado |

Sink virtual do telemóvel convencionado: nome técnico `"hyprlink-speaker"`,
descrição `"HyprLink-Phone"`, `is_phone:true`. Uni-stream de tap (D→P): 8
bytes id + PCM cru **16-bit LE, 48000Hz, estéreo intercalado**, infinito até
`tap_stop`/desconexão.

### phone_audio

Inverso do `audio.*` acima — aqui é o **daemon quem pergunta/comanda** e o
telemóvel quem responde/executa (único caso do protocolo nessa direção; até
agora todo req/resp era telemóvel→PC). Volumes sempre em percentual (0-100),
não nos steps reais do `AudioManager` — o telemóvel converte.

| type | dir | body |
|---|---|---|
| `phone_audio.state` → `phone_audio.state_reply` | D→P req/resp (5s) | resp: `{ring_percent, media_percent, alarm_percent, ringer_mode:"normal"\|"vibrate"\|"silent", dnd_access:Bool, dnd_enabled:Bool}` |
| `phone_audio.set_volume` | D→P push | `{stream:"ring"\|"media"\|"alarm", percent(0-100)}` — sem resposta |
| `phone_audio.set_ringer_mode` | D→P push | `{mode:"normal"\|"vibrate"\|"silent"}` — sem resposta; sem `dnd_access` (acesso a "Não Perturbe"), vibrar/silencioso não têm efeito |
| `phone_audio.set_dnd` | D→P push | `{enabled:Bool}` — sem resposta; liga/desliga o filtro de interrupção (`NotificationManager.setInterruptionFilter`, `PRIORITY` quando ligado, `ALL` quando desligado) — diferente do `ringer_mode`; também depende de `dnd_access` |

### webcam
| type | dir | body |
|---|---|---|
| `webcam.start` | D→P push | `{width(def 1280), height(def 720), fps(def 24), codec:"h264"\|"h265"(def h264)}` — `id` do pacote correlaciona o uni-stream de vídeo |
| `webcam.stop` | D→P push | body irrelevante |
| `webcam.error` | P→D one-way | `{message}` |
| `webcam.transform` | P→D one-way | `{rotation:0\|90\|180\|270, mirror:Bool}` |
| `webcam.mic_start` | P→D anúncio (`has_payload=true`) | `{rate:48000, channels:1}` — `id` retornado correlaciona uni-stream de mic |
| `webcam.mic_stop` | P→D one-way | `{}` |

Uni-stream de vídeo (P→D): 8 bytes id + **1 byte codec efetivo**
(`0x01`=H.264/AVC, `0x02`=H.265/HEVC — reflete o que foi realmente usado,
não o pedido; monta o pipeline a partir deste byte) + NALUs/Annex-B cru,
contínuo. Uni-stream de mic (P→D): 8 bytes id + PCM cru **16-bit LE,
48000Hz, mono**, sem mais framing.

Codec HEVC só é usado pelo telemóvel se houver encoder de hardware
disponível — pode divergir do que o `webcam.start` pediu; o byte indicador é
sempre a fonte da verdade. Rotação/espelho resetam para 0°/false a cada
início de sessão de streaming.

## 6. Gotchas para o daemon Rust (não documentados em nenhum lugar antes)

- Nenhum destes tinha implementação real no daemon anterior (só protocolo em
  prosa): `audio.*`, `webcam.*`, `notification.*`, `share.file`. Construir do
  zero, com testes reais contra o app.
- Enumeração/controle de PipeWire, criação do sink `hyprlink-speaker` (ainda
  não implementado, ver plano de fases) e do sink nulo `hyprlink-mic`
  (implementado em `mic.rs` — o "microfone" de verdade é o monitor desse
  sink, `pactl load-module module-null-sink`), e o pipeline GStreamer de
  vídeo/áudio não vêm especificados em lugar nenhum do projeto original —
  são decisões de implementação do daemon Rust (ver plano de fases em
  `/home/maggio/.claude/plans/bubbly-frolicking-wilkes.md`).
- **`hyprctl dispatch` nem sempre aceita a sintaxe clássica.** Em forks de
  Hyprland baseados em Lua (confirmado num, apelidado "ryoku" pelo usuário
  de desenvolvimento), `hyprctl dispatch <dispatcher> <args>` foi substituído
  por avaliação de uma expressão Lua (`hl.dispatch(...)`) e o clássico falha
  com exit code != 0 e uma mensagem tipo
  `[string "return hl.dispatch(...)"]: ')' expected`. `hypr::dispatch` em
  `hypr.rs` tenta o clássico primeiro (funciona em qualquer Hyprland padrão)
  e só cai pro fallback Lua se ele falhar — sintaxe confirmada ao vivo:
  - `workspace <n>` → `hl.dsp.focus({workspace = <n>})`
  - `focuswindow address:0x..` → `hl.dsp.focus({window = "address:0x.."})`
  - `closewindow address:0x..` → `hl.dsp.window.close({address = "address:0x.."})`
  Isso cobre só os 3 dispatchers que o Mission Control do app realmente usa;
  outros dispatchers (`exec`, `reload`, etc.) não têm tradução e falham em
  forks assim — não é um problema pra usuários de Hyprland padrão.
