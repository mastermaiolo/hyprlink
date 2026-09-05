# Prompt para o AI Studio — fix mic + tap de áudio

Aplicar 6 mudanças cirúrgicas, uma de cada vez, confirmando build limpo entre
cada uma. Não mexer em mais nada além do listado.

## 1. WebcamStreamer.kt — mic ligar mesmo com câmera desligada

Função `startMic`, linha ~300. Trocar:

```kotlin
val conn = activeConn ?: return
```

por:

```kotlin
val conn = activeConn ?: ConnectionRepository.activeConnection ?: return
```

## 2. WebcamStreamer.kt — tirar o DSP de chamada telefónica do microfone

Mesma função `startMic`, no construtor do `AudioRecord` (linha ~320). Trocar:

```kotlin
recorder = AudioRecord(
    MediaRecorder.AudioSource.MIC,
    48000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
    maxOf(minBuf, 9600)
)
```

por:

```kotlin
recorder = AudioRecord(
    MediaRecorder.AudioSource.CAMCORDER,
    48000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
    maxOf(minBuf, 9600)
)
```

`CAMCORDER` não aplica o filtro passa-faixa/NS/AGC de chamada de voz que
causava o efeito "aquário" — é o motivo do som abafado, não o ganho (o
ganho já foi corrigido no daemon Rust, volume 5.0→1.0).

## 3. ConnectionRepository.kt — responder ao pedido de mic vindo do PC

No bloco `"webcam" -> { ... }` do router de pacotes (perto de
`"webcam.start"`/`"webcam.stop"`), ANTES do `else if (packet.type == "webcam.stop")`,
adicionar dois novos ramos. O bloco inteiro deve ficar assim (contexto
completo pra localizar, só os dois `else if` novos são adição):

```kotlin
"webcam" -> {
    if (packet.type == "webcam.start") {
        val width = (packet.body?.get("width") as? Number)?.toInt() ?: 1280
        val height = (packet.body?.get("height") as? Number)?.toInt() ?: 720
        val fps = (packet.body?.get("fps") as? Number)?.toInt() ?: 24
        val codec = (packet.body?.get("codec") as? String) ?: "h264"
        val conn = activeConnection
        if (conn != null) {
            WebcamStreamer.start(context, conn, packet.id, width, height, fps, codec)
            val intent = Intent(context, MainActivity::class.java).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            }
            context.startActivity(intent)
        }
    } else if (packet.type == "webcam.stop") {
        WebcamStreamer.stop()
    } else if (packet.type == "webcam.mic_start_request") {
        WebcamStreamer.startMic(context)
    } else if (packet.type == "webcam.mic_stop_request") {
        WebcamStreamer.stopMic()
    }
}
```

Sem isso o botão de microfone da GUI do Linux não faz nada — o PC manda
`webcam.mic_start_request`/`webcam.mic_stop_request` mas o Android nunca
tratava esses dois tipos.

## 4. MainActivity.kt — parar de tentar mudar pro sink fantasma `hyprlink-speaker`

Por volta da linha 5440-5446. O sink `"hyprlink-speaker"` nunca foi criado
no daemon (é só uma constante reservada em `audio.rs`) — tentar mudar pra
ele suspende os players de áudio no PC depois de alguns segundos, o que
mata o audio tap. Localizar:

```kotlin
val phoneSink = audioState?.sinks?.firstOrNull { it.is_phone }
val targetSinkName = phoneSink?.name ?: "hyprlink-speaker"
val successId = ConnectionRepository.startAudioTap()
if (successId != null) {
    ConnectionRepository.setAudioDefaultSink(targetSinkName)
}
```

e remover só a chamada de `setAudioDefaultSink`, ficando:

```kotlin
val successId = ConnectionRepository.startAudioTap()
```

Apagar também a variável `phoneSink`/`targetSinkName` se não forem usadas
em mais nenhum lugar desse mesmo bloco (confirmar antes de apagar).

## 5. HyprLinkConnectionService.kt — parar de matar a conexão durante áudio/webcam

No loop de keepalive (por volta da linha 280-300), o ping de aplicação a
cada 10s é redundante com o keepalive nativo QUIC do daemon (5s) e mata a
conexão por falso positivo quando o link está saturado de mídia contínua
(áudio tap = ~192KB/s). Localizar:

```kotlin
if (pingTimer >= 10_000) {
    pingTimer = 0
    if (ConnectionRepository.activeConnection != null) {
        val pingSuccess = ConnectionRepository.sendPing()
```

e trocar por:

```kotlin
if (pingTimer >= 10_000) {
    pingTimer = 0
    if (AudioStreamPlayer.isPlaying.value || WebcamStreamer.isStreaming.value) {
        // ping de app é redundante e destrutivo durante streaming contínuo —
        // o keep_alive_interval nativo do QUIC (5s, lado do daemon) já cobre isso.
        consecutiveFailures = 0
    } else if (ConnectionRepository.activeConnection != null) {
        val pingSuccess = ConnectionRepository.sendPing()
```

(o resto do bloco original continua igual, só ganhou esse `else if` na
frente).

## 6. AudioStreamPlayer.kt — esperar o coroutine antigo morrer antes de soltar o AudioTrack

Em `stop()` (por volta da linha 166), `playJob?.cancel()` só sinaliza
cancelamento e retorna na hora — se `playStream()` for chamado de novo logo
em seguida, o `AudioTrack.Builder().build()` novo pode competir com o
antigo ainda preso em `track.write()` bloqueante, esgotando as tracks do
AudioFlinger (sintoma: "só volta a funcionar reinstalando o app"). Trocar:

```kotlin
playJob?.cancel()
playJob = null
```

por:

```kotlin
runBlocking { playJob?.cancelAndJoin() }
playJob = null
```

(`stop()` é `@Synchronized`, não suspend — `runBlocking` aqui é seguro
porque a função já não é chamada de dentro de uma coroutine que precise
não-bloquear; confirmar que `kotlinx.coroutines.runBlocking` está
importado, já tem `import kotlinx.coroutines.*` no topo do arquivo).

---

Depois de aplicar as 6, subir uma versão nova (`versionName`
`v1.0.xxx-alpha`, incrementar `versionCode`) e testar no telemóvel real:
mic isolado sem câmera aberta, botão de mic pelo PC, e o tap tocando por
mais de 30s seguidos sem cair.
