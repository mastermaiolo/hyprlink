# Prompt para o AI Studio — aumentar buffer de recepção QUIC (causa raiz real do "244KB sempre")

Causa raiz definitiva encontrada lendo o código-fonte do Kwik (biblioteca QUIC
que o app usa, `github.com/ptrd/kwik`), não é mais suposição:

Em `QuicClientConnectionImpl.java` do Kwik, o valor padrão de flow control
por stream é `DEFAULT_MAX_STREAM_DATA = 250_000` bytes = **exatamente 244KB**
— o número que sempre aparecia travando o audio tap/mic ("sending stopped by
peer: error 0" sempre perto de 244KB no total). O app nunca configura esse
valor, então fica no padrão. Numa rede local perfeita isso nem seria
perceptível (a renovação da janela de flow control é rápida o bastante), mas
com qualquer jitter de rede (WiFi entrando em economia de energia, por
exemplo) o ciclo de renovação da janela demora demais e o stream trava
esperando um `MAX_STREAM_DATA` que não chega a tempo.

**Bug extra descoberto na própria lib**: o único método exposto no
`QuicClientConnection.Builder` pra isso (`defaultStreamReceiveBufferSize`)
só configura streams **bidirecionais** — não afeta streams
**unidirecionais**, que são exatamente o tipo usado pelo áudio tap e pelo
microfone (`ConnectionRepository.kt`/`WebcamStreamer.kt` abrem uni-streams
pra isso, ver `PROTOCOL.md`). Ou seja, mesmo se o app já tivesse chamado
esse método do builder, não teria resolvido nada.

## Fix — `ConnectionUtils.kt::connectPinned`

A API correta e não-deprecada fica na própria conexão (`QuicConnection`),
não no builder: `setDefaultUnidirectionalStreamReceiveBufferSize(long)` e
`setDefaultBidirectionalStreamReceiveBufferSize(long)`. Chamar os dois logo
depois de `.build()`, antes de `.connect()`, pra garantir que já valham
pra qualquer stream que o PC abrir assim que a conexão subir.

Localizar em `ConnectionUtils.kt`:

```kotlin
        val quicConn = QuicClientConnection.newBuilder()
            .host(host)
            .port(port)
            .applicationProtocol(applicationProtocol)
            .noServerCertificateCheck()
            .maxOpenPeerInitiatedBidirectionalStreams(100)
            .maxOpenPeerInitiatedUnidirectionalStreams(100)
            .clientCertificate(identity.certificate)
            .clientCertificateKey(identity.privateKey)
            .build()

        quicConn.connect()
```

Trocar por:

```kotlin
        val quicConn = QuicClientConnection.newBuilder()
            .host(host)
            .port(port)
            .applicationProtocol(applicationProtocol)
            .noServerCertificateCheck()
            .maxOpenPeerInitiatedBidirectionalStreams(100)
            .maxOpenPeerInitiatedUnidirectionalStreams(100)
            .clientCertificate(identity.certificate)
            .clientCertificateKey(identity.privateKey)
            .build()

        // Padrão do Kwik é 250_000 bytes (~244KB) por stream — baixo demais
        // pro audio tap/mic (streams unidirecionais contínuos): qualquer
        // soneca do rádio WiFi (jitter de RTT) atrasa a renovação da janela
        // de flow control o bastante pra travar o stream inteiro. 10MB dá
        // margem confortável mesmo com vários segundos de rede degradada.
        // NOTA: defaultStreamReceiveBufferSize() do builder NÃO cobre uni-streams
        // (bug/limitação do Kwik) — precisa das duas chamadas abaixo, na conexão.
        quicConn.setDefaultUnidirectionalStreamReceiveBufferSize(10_000_000L)
        quicConn.setDefaultBidirectionalStreamReceiveBufferSize(10_000_000L)

        quicConn.connect()
```

Não mexer em mais nada. Buildar, instalar, testar: ligar o audio tap e
deixar tocando por pelo menos 2 minutos seguidos, apagando a tela do
telemóvel no meio — não deve mais travar nem parar de tocar.
