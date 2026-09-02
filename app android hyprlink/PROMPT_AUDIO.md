# Prompt para AI Studio — ecrã ÁUDIO: media, mixer, saídas e "ouvir no telemóvel"

Cola isto na AI Studio para o projeto HyprLink.

---

Nova funcionalidade: um ecrã **ÁUDIO** completo no app que controla o
som do PC — o que está a tocar, volumes por aplicação, escolha da saída
de som, e a opção de o áudio do PC tocar NO TELEMÓVEL. O daemon do PC
já implementa todo o protocolo abaixo.

## Protocolo (o PC já entende tudo isto)

Pedidos normais no padrão request/reply do projeto (bi stream, meio-
fecho, ler a resposta até EOF) — igual ao `hypr.workspaces`:

1. **`audio.state` {}** → resposta `audio.state_reply`:
```json
{
  "default_sink": "bluez_output.84.1",
  "sinks": [
    {"id": 54, "name": "alsa_output...", "description": "Altifalantes",
     "volume": 110, "muted": false, "is_default": false, "is_phone": false},
    {"id": 9000, "name": "hyprlink-speaker", "description": "HyprLink-Phone",
     "volume": 100, "muted": false, "is_default": false, "is_phone": true}
  ],
  "apps": [
    {"id": 2462, "name": "Spotify", "media": "Spotify", "volume": 69,
     "muted": false, "sink_id": 3136}
  ]
}
```
   `volume` é percentagem (0–150). O sink com `is_phone: true` é o
   telemóvel — mostrar como "📱 Este telemóvel" na lista de saídas.

2. **`audio.set_volume { kind, id, volume }`** — `kind`: `"sink"` (saída/
   master) ou `"app"` (stream por aplicação); `volume`: 0–150 (%).
   Resposta: `audio.ack {ok}`.
3. **`audio.set_mute { kind, id, muted }`** → `audio.ack`.
4. **`audio.set_default_sink { name }`** (o `name` técnico do sink, não a
   description) — muda a saída por omissão E move as apps já a tocar.
   → `audio.ack`.
5. **`audio.tap_start` {}** → resposta **`audio.tap_ready { id, rate,
   channels }`** (rate=48000, channels=2). Depois da resposta, o PC abre
   um **uni stream** cujos primeiros 8 bytes são esse `id` (big-endian),
   seguido de PCM cru infinito: **16-bit little-endian, 48kHz, ESTÉREO
   intercalado**. O app toca isto num `AudioTrack`. Termina com
   **`audio.tap_stop` {}** (ou fechando/perdendo a ligação).

Nota: o app JÁ tem um handler de uni streams vindos do PC (usado na
receção de ficheiros `share.file`) que lê os 8 bytes de id e procura o
anúncio correspondente — registar o `id` do `audio.tap_ready` nesse
mesmo mecanismo antes de o stream chegar.

## UI pedida

### Tile no dashboard
Ativar/reaproveitar o tile de MEDIA (ou criar tile "ÁUDIO") → abre o
ecrã ÁUDIO. Deve funcionar como as outras tabs (botão voltar).

### Ecrã ÁUDIO (novo)

De cima para baixo:

1. **A TOCAR** — cartão com o que está a tocar no PC: usa o `media.state`
   já existente (title/artist/player/status) + botões ⏮ ⏯ ⏭ que enviam
   os comandos `media.*` já existentes no projeto. (Isto já existe no
   diálogo de media atual — mover/reutilizar aqui.)

2. **OUVIR NO TELEMÓVEL** — switch grande:
   - ON: envia `audio.tap_start`, guarda o `id` do `audio.tap_ready`,
     e quando o uni stream chegar, toca os bytes num `AudioTrack`
     (`AudioFormat.ENCODING_PCM_16BIT`, 48000 Hz,
     `CHANNEL_OUT_STEREO`, modo `MODE_STREAM`, buffer ≥ 4×
     `getMinBufferSize`). Escrever num AudioTrack à medida que os bytes
     chegam (thread/corrotina IO dedicada).
   - Ao ligar o switch, também enviar
     `audio.set_default_sink { name: "hyprlink-speaker" }` — assim o som
     do PC passa a sair no telemóvel sem mais passos.
   - OFF: `audio.tap_stop`, parar/libertar o AudioTrack, e devolver a
     saída anterior (`audio.set_default_sink` com o sink que era default
     antes de ligar — guardar esse nome ao ligar).
   - Atenção ao ciclo de vida: parar o AudioTrack em onDestroy/desligar.

3. **SAÍDA DE SOM** — lista de rádio com os `sinks` (usar `description`;
   para `is_phone: true` mostrar "📱 Este telemóvel"). Tocar num →
   `audio.set_default_sink`. O default atual marcado.
   - Por baixo de cada saída, slider de volume master (0–150%) + botão
     mute → `audio.set_volume`/`audio.set_mute` com `kind: "sink"`.

4. **APLICAÇÕES** — uma linha por app em `apps`: nome + media, slider
   0–150% e mute → `kind: "app"`.
   - Lista vazia → "Nada a tocar no PC."

### Comportamento
- `audio.state` ao abrir o ecrã e refresh a cada ~3s enquanto visível
  (LaunchedEffect com loop + delay, como o auto-refresh do Mission
  Control). Sliders em arrasto não devem ser sobrescritos pelo refresh
  (só aplicar o estado remoto a controlos que não estão a ser tocados).
- Sliders: enviar `set_volume` com debounce/throttle (~150ms) para não
  inundar o canal durante o arrasto.
- Desligado da workstation → placeholder "Liga a uma workstation
  primeiro." como nos outros ecrãs.

## Verificação pedida

- Com Spotify/browser a tocar no PC: o cartão A TOCAR mostra a música e
  os botões controlam-na.
- Mexer no slider de uma app muda o volume só dessa app no PC
  (confirmar no pavucontrol).
- Mudar a SAÍDA DE SOM troca o som entre altifalantes/auscultadores do
  PC ao vivo.
- OUVIR NO TELEMÓVEL ON: o som do PC começa a sair no telemóvel em ~1s,
  contínuo, sem estalidos grosseiros; OFF devolve o som à saída
  anterior do PC.
- Atualizar o BUILD_REPORT.md. NÃO apagar `res/font/` nem reverter
  `Type.kt`, e NÃO tocar no WebcamStreamer.kt neste round.
