# Prompt para o AI Studio — WifiLock durante streaming de áudio

Causa raiz encontrada por diagnóstico direto (não é hipótese): o app nunca
adquire um `WifiLock`. Sem isso, o rádio WiFi do Android entra em modo de
economia de energia (dorme entre beacons) mesmo com o app em foreground
service — isso faz o RTT real entre telemóvel e PC variar entre 6ms e 124ms
(medido com `ping`, 0% de perda, mas jitter enorme) numa rede local que
suporta 780+ Mbit/s. Isso é lento o bastante pra travar o fluxo de controle
de fluxo do QUIC e o `AudioTrack`, explicando por que o audio tap
("ouvir no telemóvel") funciona de forma inconsistente e trava depois de
alguns segundos sem erro nenhum, sempre que o telemóvel entra em
economia de energia de WiFi.

**Diagnóstico completo, sem isso não seria necessário**: GStreamer, PipeWire,
o flow control do Kwik (biblioteca QUIC do Android) e a lógica de envio em
Rust já foram testados isoladamente e confirmados corretos. O daemon
capturava e tentava enviar dados continuamente, mas cada `write_all` no
QUIC ficava preso por dezenas de segundos esperando o telemóvel confirmar
recebimento — típico de rádio WiFi dormindo entre pacotes.

## Mudança 1 — `AndroidManifest.xml`

Adicionar a permissão (se ainda não existir):

```xml
<uses-permission android:name="android.permission.WAKE_LOCK" />
```

## Mudança 2 — Adquirir/liberar `WifiLock` junto com o streaming de áudio

Em `HyprLinkConnectionService.kt` (o foreground service que já monitora
`AudioStreamPlayer.isPlaying`/`WebcamStreamer.isStreaming` no loop de
keepalive), adicionar um `WifiManager.WifiLock` em modo
`WIFI_MODE_FULL_HIGH_PERF`, adquirido quando qualquer um dos dois fica
`true` pela primeira vez, e liberado quando os dois voltam a `false`.

Exemplo de estrutura (adaptar ao ponto certo do serviço, perto de onde o
foreground service já é criado/gerido):

```kotlin
private var wifiLock: WifiManager.WifiLock? = null

private fun acquireWifiLockIfNeeded() {
    if (wifiLock?.isHeld == true) return
    val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
    wifiLock = wifiManager.createWifiLock(
        WifiManager.WIFI_MODE_FULL_HIGH_PERF,
        "HyprLink:AudioStreamingLock"
    ).apply {
        setReferenceCounted(false)
        acquire()
    }
}

private fun releaseWifiLockIfHeld() {
    wifiLock?.let { if (it.isHeld) it.release() }
    wifiLock = null
}
```

E no loop de monitorização existente (onde já checa
`AudioStreamPlayer.isPlaying.value || WebcamStreamer.isStreaming.value`),
chamar `acquireWifiLockIfNeeded()` quando esse valor virar `true` e
`releaseWifiLockIfHeld()` quando virar `false` (comparar com o estado
anterior pra não adquirir/liberar toda hora — usar um `var wasStreaming = false`
fora do loop, ou reaproveitar variável existente se já houver algo assim).

Não mexer em mais nada. Buildar, instalar, e testar: ligar o audio tap e
deixar tocando por pelo menos 1-2 minutos seguidos, ligando/desligando a
tela do telemóvel no meio do teste — deve continuar tocando sem travar.
