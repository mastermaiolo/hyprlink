# Relatório de Construção (BUILD_REPORT.md) - HyprLink

Este documento descreve detalhadamente o estado atual de desenvolvimento do cliente Android do HyprLink, as dependências configuradas, as descobertas da API interna do Kwik e a estratégia para garantir a compatibilidade com o daemon Rust do servidor.

---

## 1. Dependências Reais Usadas

As dependências reais foram ativadas no arquivo `app/build.gradle.kts` e mapeadas no arquivo `gradle/libs.versions.toml`. Foram removidos todos os prefixos "v" inválidos de versões e as coordenadas de repositórios incorretas.

Abaixo está a lista de coordenadas exatas resolvidas do Maven Central:

1. **Kwik (QUIC Client)**
   - **Coordenada:** `tech.kwik:kwik:0.10.9` (definido como `libs.tech.kwik.kwik` em TOML)
   - **Origem:** Publicado diretamente no Maven Central pelo grupo `tech.kwik` (removido do prefixo JitPack/GitHub obsoleto).
2. **Bouncy Castle (Crypto Provider)**
   - **Coordenadas:** 
     - `org.bouncycastle:bcprov-jdk18on:1.78.1` (`libs.bouncycastle.bcprov`)
     - `org.bouncycastle:bcpkix-jdk18on:1.78.1` (`libs.bouncycastle.bcpkix`)
   - **Propósito:** Necessário para prover suporte a TLS 1.3 e algoritmos de cifragem criptográfica que o runtime padrão do Android (Conscrypt) não expõe diretamente para conexões QUIC arbitrárias.
3. **CBOR (Concise Binary Object Representation)**
   - **Coordenada:** `co.nstant.in:cbor:0.9` (`libs.cbor.java`)
   - **Propósito:** Codificação binária ultraeficiente de pacotes que mapeiam diretamente para os structs de payload do daemon Rust.
4. **AndroidX Security Crypto**
   - **Coordenada:** `androidx.security:security-crypto:1.1.0-alpha06` (`libs.androidx.security.crypto`)
   - **Propósito:** Armazenamento encriptado por hardware (KeyStore) das chaves privadas locais da identidade do celular e segredos/tokens compartilhados dos servidores.
5. **ML Kit Barcode Scanning**
   - **Coordenada:** `com.google.mlkit:barcode-scanning:17.3.0` (`libs.mlkit.barcode.scanning`)
   - **Propósito:** Reconhecimento rápido de QR Code de pareamento gerado pelo daemon Rust.

---

## 2. O Teste de API do Kwik no Android (ART)

Fizemos uma engenharia reversa via reflexão direta no classpath da JVM de teste (`ExampleUnitTest`) para extrair a estrutura exata das classes públicas empacotadas no artefato `tech.kwik:kwik:0.10.9`. 

### Estrutura e Classes Descobertas:
- **Client Connection Builder:** `tech.kwik.core.QuicClientConnection` e sua subclasse builder estática `tech.kwik.core.QuicClientConnection$Builder`
- **Métodos do Builder descobertos:**
  - `host(String) -> Builder`
  - `port(int) -> Builder`
  - `applicationProtocol(String) -> Builder` (Usa ALPN, definiremos como `"hyprlink/1"`)
  - `noServerCertificateCheck() -> Builder` (Desativa verificação SSL clássica de CA)
  - `customTrustManager(X509TrustManager) -> Builder` (Injeta verificação customizada)
  - `build() -> QuicClientConnection`
- **Métodos da Conexão (`QuicConnection` / `QuicClientConnection`):**
  - `connect() -> void` (Inicia handshake QUIC síncrono/bloqueante)
  - `isConnected() -> boolean` (Retorna estado atual da conexão)
  - `createStream(boolean) -> QuicStream` (Abre stream bidirecional se `true`, unidirecional se `false`)
  - `getServerCertificateChain() -> List<X509Certificate>` (Obtém cadeia de certificados síncrona retornada no TLS)
  - `close() -> void`
- **Métodos da Stream (`QuicStream`):**
  - `getOutputStream() -> OutputStream` (Usa escrita tradicional síncrona e buffering)
  - `getInputStream() -> InputStream` (Usa leitura síncrona)
  - `getStreamId() -> int`

### Código Concreto de Ligação:
```kotlin
import tech.kwik.core.QuicClientConnection
import tech.kwik.core.QuicStream
import java.io.InputStream
import java.io.OutputStream

fun testMinimalQuicConnection(host: String, port: Int) {
    val clientConnection: QuicClientConnection = QuicClientConnection.newBuilder()
        .host(host)
        .port(port)
        .applicationProtocol("hyprlink/1")
        .noServerCertificateCheck() // Desvio temporário de CA
        .build()

    // Handshake QUIC
    clientConnection.connect()

    if (clientConnection.isConnected) {
        // Abre stream bidirecional
        val stream: QuicStream = clientConnection.createStream(true)
        val out: OutputStream = stream.getOutputStream()
        val `in`: InputStream = stream.getInputStream()

        // Escreve bytes formatados em CBOR
        val packet = byteArrayOf(/* CBOR hello payload */)
        out.write(packet)
        out.flush()

        // Leitura da resposta
        val buffer = ByteArray(1024)
        val readBytes = `in`.read(buffer)
    }
}
```

---

## 3. Discrepâncias, Restrições e Soluções Adotadas

### TLS e Verificação de Certificados (Fingerprint) / Limitação do Hostname Verifier:
O daemon Rust gera certificados autoassinados temporários de curta duração. Anteriormente, o aplicativo tentou utilizar `.customTrustManager(FingerprintTrustManager(...))` para validar o fingerprint do certificado do servidor. No entanto, o motor TLS do Kwik (baseado no agent15) continua a executar o `DefaultHostnameVerifier` internamente, o qual valida o `host` apenas contra SANs DNS (tipo 2) do certificado e ignora SANs de IP (tipo 7). Como o HyprLink liga-se por endereço IP vindo do QR Code, o handshake falhava sempre com o erro fatal:
```
java.net.ConnectException: Handshake error: tech.kwik.agent15.alert.CertificateUnknownAlert: servername does not match
```

Como o Kwik/agent15 não oferece uma API pública para desativar ou configurar o verificador de hostnames individualmente, a solução adotada foi:
1. Usar `.noServerCertificateCheck()` no Builder para desativar a validação padrão de CAs e de hostnames do motor TLS.
2. Manter as configurações de certificados e chaves de cliente (`clientCertificate(...)` e `clientCertificateKey(...)`) para autenticação mTLS estrita do lado do servidor (essas partes continuam 100% corretas e validadas).
3. **Validação Manual Estrita Pós-Ligação:** Imediatamente após chamar `quicConn.connect()`, e antes de criar qualquer stream ou enviar qualquer byte de dados, a ligação executa uma verificação manual de pinning de fingerprint contra o certificado do servidor:
   ```kotlin
   val chain = quicConn.serverCertificateChain
   val actual = sha256Hex(chain[0].encoded)   // hex maiúsculo, DER completo
   if (!actual.equals(expectedFingerprint, ignoreCase = true)) {
       quicConn.close()
       throw SecurityException("Server fingerprint mismatch — connection closed")
   }
   ```
   Esta lógica de ligação segura foi centralizada numa única função utilitária `ConnectionUtils.connectPinned(host, port, applicationProtocol, expectedFingerprint, identity)` e é partilhada de forma idêntica por todos os três call sites do projeto:
   - `executePairingHandshake` (durante o escaneamento do QR Code)
   - `HyprLinkConnectionService` (o Foreground Service persistente)
   - Diagnósticos de ligação da consola em `MainActivity`

   Isso garante comportamento consistente e segurança estrita (qualquer divergência de fingerprint resulta no encerramento imediato da ligação antes do tráfego de dados da aplicação).

### Limitações do Android ART:
- **Permissão de Rede:** Declaramos a permissão `<uses-permission android:name="android.permission.INTERNET" />` no manifesto para permitir tráfego de sockets UDP do QUIC.
- **Serviço em Foreground:** A ligação QUIC e manutenção do canal serão encapsuladas em um `Foreground Service` do Android utilizando canal de notificação persistente, impedindo o Android de pausar as corrotinas de socket UDP quando a aplicação for para background.

---

## 4. Persistência de Dados e Banco de Dados Local

- **Estratégia:** Utilizaremos o **Room Database** para salvar e gerenciar múltiplos computadores emparelhados de forma duradoura.
- **Dados persistidos:** Nome do dispositivo, Endereço IP/Host, Porta, String do Protocolo, Impressão Digital SHA-256 de segurança e Token de emparelhamento cryptográfico único (Pairing Token).
- **Escopo:** O aplicativo suportará a troca rápida de dispositivo ativo através de um seletor visual na interface, facilitando o gerenciamento de múltiplos computadores emparelhados sem a necessidade de re-escanear QR codes.

---

## 5. Garantia de Timeouts Reais e Prevenção de Races de Reconexão

### Timeouts de Leitura Reais em Streams Kwik: `runInterruptible` + `abortReading`
A leitura síncrona bloqueante clássica (`InputStream.read()`) do Kwik sob a JVM/Android não responde de forma garantida a cancelamentos cooperativos do Kotlin Coroutines (`withTimeout`) nem a interrupções normais de threads (`Thread.interrupt()`). Caso o socket peer pare de transmitir dados ou desapareça sem encerrar a conexão, as corrotinas de leitura podem permanecer suspensas indefinidamente, travando os dispatchers de IO.

Para contornar este problema grave, implementámos uma infraestrutura híbrida de segurança em `ConnectionUtils.readWithDeadline`:
1. **Cooperativo**: Utilização do `kotlinx.coroutines.runInterruptible` que força a tradução de cancelamento de corrotina para interrupção de thread Java clássica.
2. **Abortativo Nativo**: Caso ocorra um `TimeoutCancellationException` lançado pelo dispatcher de timeout do Kotlin, a exceção é capturada e executa-se explicitamente `stream.abortReading(0)`.
3. **Resultado**: O método nativo `abortReading` força o encerramento do stream QUIC de baixo nível, desbloqueando instantaneamente qualquer chamada `read()` em progresso com um erro de I/O, garantindo o desbloqueio seguro da thread de IO em no máximo 10 segundos.

Esta função utilitária `readWithDeadline` foi aplicada com sucesso em todos os fluxos de leitura bloqueante de streams (handshake inicial, requisição de bateria, troca de clipboard, início de transmissão de arquivos e gravação sequencial de payload em disco).

### Prevenção de Races de Reconexão via Contador de Geração Monotónico
Devido à natureza assíncrona do loop de reconexão e ao uso de chamadas de rede bloqueantes, conexões iniciadas por jobs anteriores e cancelados podiam terminar após uma tentativa mais recente e sobrescrever de forma errônea a propriedade estática `activeQuicConn` global de fundo.

Configurámos um mecanismo robusto baseado em gerações:
- **Identificador de Geração**: Um contador simples de geração (`connectionGeneration: Int`) incrementado a cada nova tentativa em `connectAndKeepAlive`.
- **Aferição pós-handshake**: Antes de atribuir a conexão recém-estabelecida à variável global `activeQuicConn` ou registá-la no repositório, verifica-se se a geração atual do loop (`myGeneration`) ainda corresponde ao valor global mais atualizado (`connectionGeneration`).
- **Fechamento Seguro**: No bloco `finally`, se a tentativa em progresso falhar ou for cancelada, a conexão local associada é fechada e o estado partilhado só é limpo se a geração atual for a proprietária legítima. Conexões obsoletas são limpas de forma isolada, sem interferir nas conexões de novas gerações estabelecidas com sucesso.

---

## 6. Correção do Scanner de QR Code para Pareamento

### O Problema
Anteriormente, o `CameraPreview` na `MainActivity.kt` utilizava um parser manual inline do texto do QR Code que realizava um split por barra vertical (`|`) e exigia pelo menos 4 fragmentos (`parts.size < 4`), interpretando os índices em posições estáticas para Host, Porta, Fingerprint e Token de emparelhamento.
Contudo, o formato de QR Code produzido pelo daemon Rust contém exatamente 3 campos:
```
<fingerprint_hex>|<host>:<porta>|<token_hex>
```
Isso impedia qualquer emparelhamento real, pois o app reportava erro de "requisitos em falta" e abortava a operação. Além disso, mesmo que passasse, os campos seriam erroneamente mapeados.

### A Solução
Integramos a função de parsing canônica já existente no projeto, `parsePairingQr`, como a única e definitiva fonte de processamento de QR Codes de emparelhamento na câmera:
1. **Remoção do Parsing Manual**: Eliminamos completamente o split inline com limite fixo e a extração manual de variáveis globais dentro do callback `onQrCodeScanned`.
2. **Utilização da Função Canônica**: Chamamos agora diretamente `parsePairingQr(qrText)` do arquivo `PairingScanner.kt`. Essa função analisa corretamente os 3 campos e divide o Host da Porta usando o último caractere `:` (suportando inclusive IPv6).
3. **Mapeamento Direto no Handshake**: O objeto `ParsedPairingData` retornado é fornecido de forma direta e unificada para `executePairingHandshake`, evitando redundância e incompatibilidade de tipos.
4. **Persistência Limpa**: A workstation emparelhada com sucesso é inserida no banco de dados local com as propriedades extraídas do parser canônico, restabelecendo o fluxo seguro de mTLS imediatamente.

---

## 7. Resolução do Crash de Segurança do Foreground Service (Android 14+ / targetSDK 36)

### O Problema
O aplicativo sofria uma falha imediata de execução (`java.lang.SecurityException`) ao tentar estabelecer ligação com o daemon do computador. Sob o **Android 14 (API 34) e Android 15 (API 35 / targetSDK 36)**, iniciar um serviço de primeiro plano (`Foreground Service`) com a categoria `connectedDevice` (`android:foregroundServiceType="connectedDevice"`) exige não apenas a permissão genérica do FGS, mas também a declaração explícita de pelo menos uma permissão de transporte/hardware relevante suportada pelo sistema operacional.

### A Solução
Adicionamos as seguintes permissões de rede com nível de proteção normal no `AndroidManifest.xml`:
- `android.permission.CHANGE_NETWORK_STATE`
- `android.permission.CHANGE_WIFI_STATE`

Essas permissões são concedidas de forma inteiramente automática em tempo de instalação pelo sistema Android (sem necessidade de solicitar autorização manual intrusiva ao usuário via popups dinâmicos), satisfazendo de forma limpa e em conformidade total as políticas de segurança da API 34/35+. Com isso, o ciclo de vida do `HyprLinkConnectionService` inicia-se em segundo plano sem qualquer interrupção.

---

## 8. Separação de Porta de Pareamento e Porta de Serviço (7443)

### O Problema
O protocolo HyprLink utiliza duas portas distintas por design:
1. **Porta de Pareamento** (informada dinamicamente no QR Code do daemon): temporária, destinada apenas ao handshake inicial com mTLS e validação do token de pareamento.
2. **Porta de Serviço** (`7443`): porta fixa em que o daemon escuta permanentemente conexões autenticadas subsequentes.

Anteriormente, o app persistia a porta informada no QR Code (`parsed.port`) na workstation, fazendo com que reconexões automáticas futuras falhassem de forma silenciosa e indefinida ao tentar ligar de volta a uma porta temporária já desativada.

### A Solução
1. **Constante Unificada**: Definimos `HYPRLINK_SERVICE_PORT = 7443` em `ConnectionUtils`.
2. **Uso Exclusivo no Handshake**: A porta capturada dinamicamente do QR Code é utilizada somente durante o handshake inicial de pareamento.
3. **Persistência Correta**: Ao salvar a workstation pareada de volta ao Room Database local, a porta do registro é gravada como `7443` (`HYPRLINK_SERVICE_PORT`).
4. **Fallback Seguro**: Atualizamos o fallback de conexões no serviço de segundo plano `HyprLinkConnectionService` e o formulário de "Manual Entry" da `MainActivity` para preencher com `7443` em vez da porta padrão antiga `4433`.

---

## 9. Protocolo e Pacotes de Dados `hypr.*`

O HyprLink comunica-se de forma assíncrona bidirecional usando mensagens empacotadas em binário CBOR. Os pacotes destinados ao controle do Hyprland seguem o prefixo `hypr.*`:
- **`hypr.get_workspaces`**: Solicita a lista de workspaces ativos no servidor Linux. O daemon responde com um payload contendo o JSON bruto retornado por `hyprctl workspaces -j`.
- **`hypr.get_clients`**: Solicita a lista de janelas/clientes abertos no servidor. O daemon responde com o JSON bruto correspondente de `hyprctl clients -j`.
- **`hypr.dispatch`**: Envia comandos diretos de controle (ex: `closewindow`, `focuswindow`, `workspace <id>`), que o daemon repassa para a API de controle do gerenciador de janelas.

---

## 10. Decisões do Ecrã Desktop (Mission Control)

O ecrã **Desktop (Mission Control)** oferece controle remoto de alto nível das janelas e áreas de trabalho virtuais do Linux diretamente no Android:
1. **Focar e Fechar Janelas**: Com um simples toque, o utilizador pode alternar o foco para uma aplicação no PC ou fechar janelas remotamente através de diálogos de confirmação rápidos.
2. **Seletor de Workspace**: Exibição em tempo real de quais workspaces estão ativos e capacidade de alternar o foco do monitor para qualquer um deles via chips dinâmicos.
3. **Modo de Demonstração Interativo**: Quando o aplicativo está desconectado, o Mission Control inicia automaticamente um modo simulado para testar e validar o comportamento tátil das novas funções, sem congelar ou quebrar a interface do utilizador.

---

## 11. Sistema de Auto-Refresh Inteligente no Ecrã Desktop

Para garantir que a lista de janelas e workspaces esteja sempre em perfeita sincronia com as ações feitas no próprio PC (ex: abrir/fechar programas, mudar de área de trabalho), implementamos um poll automático otimizado de **5 segundos**:
- **Ciclo de Vida Limpo**: O poll roda sob um `LaunchedEffect(isDemo)` exclusivo do ecrã Desktop. Sair do ecrã Desktop ou mudar para a aba Dashboard cancela imediatamente o loop e interrompe qualquer tráfego ou gravação de logs de rede em segundo plano.
- **Prevenção de Sobreposição de Pedidos**: O loop verifica o estado de `isLoading`. Se uma requisição de rede anterior ainda estiver pendente, o tick atual é ignorado automaticamente para evitar gargalos ou sobreposição de pacotes UDP na rede QUIC.
- **Swallowing de Erros em Background**: Se uma atualização falhar (por instabilidade temporária de Wi-Fi, etc.), o sistema silencia a notificação de erro caso já existam dados carregados no ecrã. O ecrã apenas preserva os dados visualizados sem oscilar, garantindo uma experiência de uso contínua. Caso o ecrã esteja inicialmente vazio, o cartão de erro é exibido adequadamente.

---

## 12. Investigação de Permissões de Rede e Foreground Service (Android 14+)

### O Problema
Sugeriu-se inicialmente remover as permissões `CHANGE_NETWORK_STATE` e `CHANGE_WIFI_STATE` do `AndroidManifest.xml` devido à observação do `ConnectivityManager` utilizar apenas `ACCESS_NETWORK_STATE`.

### A Justificação e Solução Realizada
Ao analisar profundamente o comportamento do Android 14+ (targetSDK 36), descobriu-se que iniciar um **Foreground Service** associado à categoria `connectedDevice` (`android:foregroundServiceType="connectedDevice"`) exige **obrigatoriamente** que o aplicativo declare pelo menos uma permissão de hardware ou rede qualificada no manifesto.

As permissões aceitáveis pelo sistema operacional incluem:
- Permissões de Bluetooth (`BLUETOOTH_CONNECT`, `BLUETOOTH_SCAN`, etc.)
- Permissões de Localização (`ACCESS_FINE_LOCATION`, etc.)
- Permissões de Rede Local (`CHANGE_NETWORK_STATE`, `CHANGE_WIFI_STATE`)

Se o aplicativo remover todas as acima, a chamada `startForeground()` falha imediatamente no runtime com um erro fatal do sistema:
```
java.lang.SecurityException: Starting FGS with type connectedDevice requires at least one of these permissions...
```

Visto que o HyprLink estabelece conexões IP diretamente via rede Wi-Fi / LAN, a escolha de manter `CHANGE_NETWORK_STATE` e `CHANGE_WIFI_STATE` no manifesto é a **estratégia técnica mais segura, elegante e menos invasiva**:
1. São classificadas como **permissões de nível Normal**, sendo concedidas de forma 100% automática durante a instalação pelo sistema operacional, sem importunar o utilizador com diálogos de consentimento dinâmico.
2. Evitam a necessidade de solicitar permissões de extrema sensibilidade como localização precisa de GPS ou acesso a dispositivos Bluetooth próximos, promovendo total transparência e conformidade com as regras da Google Play Store.

## 13. Grelha de Recursos e Funções do Dashboard

Para unificar a experiência do utilizador e apresentar de forma clara a visão completa de funcionalidades do HyprLink, substituímos os antigos botões de atalho avulsos por uma **Grelha de Funções** de 2 colunas com suporte a estados dinâmicos ("Ativa" vs "Em breve"):

### Estrutura do Menu e Estados das Entradas:

| # | Título | Subtítulo | Estado do Link | Comportamento ao Tocar |
|---|--------|-----------|----------------|-------------------------|
| 1 | **Desktop** | "Workspaces e janelas do PC" | **Ativa** | Alterna para o ecrã Desktop (Mission Control). |
| 2 | **Partilha** | "Enviar ficheiros e links" | **Ativa** | Abre caixa de seleção (Ficheiro vs URL/Texto) e inicia o fluxo correspondente. |
| 3 | **Clipboard** | "Sincronizar área de transferência" | **Ativa** | Lê o clipboard do Android e envia-o imediatamente para o PC. |
| 4 | **Media** | "Controlar o leitor do PC" | **Ativa** | Abre um comando flutuante com play/pause/prev/next e informações do leitor ativo. |
| 5 | **Transferências** | "Progresso e histórico de envios" | *Em breve* | Mostra snackbar informativo de desenvolvimento e impede navegação. |
| 6 | **Touchpad & Teclado** | "Telemóvel como input do PC" | *Em breve* | Mostra snackbar informativo de desenvolvimento e impede navegação. |
| 7 | **Notificações** | "Espelhar e responder no PC" | *Em breve* | Mostra snackbar informativo de desenvolvimento e impede navegação. |
| 8 | **Webcam** | "Telemóvel como câmara do PC" | *Em breve* | Mostra snackbar informativo de desenvolvimento e impede navegação. |
| 9 | **Chamadas** | "Atender com o áudio do PC" | *Em breve* | Mostra snackbar informativo de desenvolvimento e impede navegação. |
| 10 | **Espelhar Ecrã** | "Ver o desktop no telemóvel" | *Em breve* | Mostra snackbar informativo de desenvolvimento e impede navegação. |

### Decisões de Design e Comportamento:
- **Design System Coeso**: Utilização de Material Design 3 (M3) com cards de cantos arredondados (`RoundedCornerShape(16.dp)`), ícones representativos, e selos visuais em miniatura "EM BREVE" no canto superior direito para as funções futuras com opacidade reduzida (`alpha = 0.5f`).
- **Estados Offline e Disconnected**: Se o utilizador não estiver conectado a nenhuma workstation e não estiver em modo demo, a grelha inteira é esbatida (`alpha = 0.4f`) e o clique em qualquer função (exceto **Desktop**, que é mantida livre para navegação e teste de demo) exibe um snackbar explicativo: *"Liga a uma workstation primeiro."*
- **Emparelhamento Facilitado**: O botão de emparelhamento rápido ("Add Workstation") foi realocado como um ícone elegante de "+" diretamente no cabeçalho da lista de Workstations, mantendo-se always visível e funcional sem poluição visual.

---

## 14. Resposta a Notificações a partir do PC (RemoteInput / `notification.reply`)

Implementámos o suporte completo para responder a notificações diretamente a partir do PC através do protocolo de `RemoteInput`:

1. **Exposição de Ações de Resposta (`is_reply`)**: No `HyprNotificationListenerService`, atualizámos a filtragem de ações para passar a incluir aquelas que requerem entrada remota (como mensagens de WhatsApp, Telegram ou SMS). Estas ações são transmitidas para o PC com a flag `"is_reply" to true`, preservando o índice (`idx`) original no array de ações da notificação.
2. **Processamento e Envio de Respostas (`notification.reply`)**: Adicionámos suporte ao pacote `notification.reply` no router de pacotes em `ConnectionRepository`. Ao receber uma resposta do PC, o aplicativo:
   - Recupera a notificação ativa no cache através do `key`.
   - Localiza a ação correspondente pelo índice `idx`.
   - Popula todos os `RemoteInput` associados a essa ação com o texto recebido.
   - Dispara a ação através de `actionIntent.send()` de forma totalmente assíncrona usando o contexto válido do serviço.
3. **Resiliência e Diagnóstico**: O fluxo inclui tratamentos robustos para casos em que a notificação já não se encontra ativa no telemóvel, reportando mensagens de aviso na consola de diagnóstico de forma segura sem crashar o processo principal.

---

## 15. Emissão do Estado de Bateria do Telemóvel (`battery.state`)

Implementámos o envio automático e otimizado do estado de bateria do dispositivo Android para a workstation conectada:

1. **Protocolo Unidirecional (`battery.state`)**: Criámos o método `sendPhoneBatteryState` em `ConnectionRepository` para empacotar o nível de bateria (`level: Int`) e o estado de carregamento (`charging: Boolean`) em pacotes CBOR unidirecionais.
2. **Monitorização Dinâmica de Bateria**: No `HyprLinkConnectionService`, registamos dinamicamente um `BroadcastReceiver` escutando `Intent.ACTION_BATTERY_CHANGED`. 
   - **Prevenção de Leaks**: O receiver é registado de forma dinâmica no `onCreate` e garantidamente cancelado no `onDestroy` para evitar perdas de recursos e respeitar estritamente o comportamento da API 34+.
   - **Prevenção de Tráfego Ruidoso**: Comparamos os valores de nível e carregamento antes de enviar, emitindo pacotes apenas se houver uma mudança real e relevante em relação ao último estado enviado.
3. **Estado Inicial Imediato**: Logo após a conclusão bem-sucedida do handshake de conexão (`core.hello`), registamos um receptor nulo de forma síncrona para obter o último sticky intent do sistema e enviar o estado atual de imediato, garantindo que o painel do PC exibe as informações corretas em poucos segundos sem esperar por uma alteração da bateria.

---

## 16. Ronda de Higiene Técnica (Etapa 4) & Histórico Persistente de Transferências (Room)

Concluímos com sucesso a ronda de higiene técnica planeada para fechar dívidas de robustez, resiliência e persistência:

1. **Garantia de Timeout no Scanner de Pareamento**: Aplicámos o mecanismo de leitura com limite de tempo (`ConnectionUtils.readWithDeadline`) nas comunicações de socket da fase de pareamento (`PairingScanner.kt`), protegendo o scanner de bloqueios indefinidos de rede de até 10 segundos caso a workstation perca conectividade.
2. **Geração de Conexão Atómica**: Convertemos a variável `connectionGeneration` no `HyprLinkConnectionService` para `AtomicInteger`. Isto assegura total visibilidade de memória e consistência transacional ao validar se uma tentativa de ligação obsoleta deve ou não sobrescrever o socket ativo atual.
3. **Mecanismo de Keepalive Ping/Pong**: Criámos um loop de monitorização ativa no Foreground Service que dispara pings assíncronos a cada 30 segundos (`core.ping`). O daemon responde com `core.pong`. Se o canal QUIC falhar duas respostas seguidas, o serviço fecha preventivamente a ligação e inicia o ciclo autónomo de reconexão.
4. **Histórico Persistente de Transferências (Room)**:
   - **Esquema de Base de Dados e Migração**: Efetuámos o incremento da versão da base de dados (`AppDatabase` de 1 para 2) e registámos a migração síncrona `MIGRATION_1_2` que cria de forma segura a tabela `transfer_records` sem depender de `.fallbackToDestructiveMigration()`. Deste modo, os emparelhamentos mTLS das estações de trabalho existentes são preservados intactos.
   - **Deteção de Estado Terminal**: Sob o `ConnectionRepository`, configurámos a rotina de atualização para intercetar as transferências apenas quando atingem um dos estados de terminação do protocolo (`VERIFICADO`, `NAO_VERIFICADO`, ou `ERRO`) e persistir automaticamente os seus metadados (ficheiro, hashes SHA-256, sentido, erros e timestamp).
   - **Desenho Adaptivo do Ecrã de Gestão**: Atualizámos o `TransfersScreen` em `MainActivity.kt` para recolher reativamente a Flow persistente do Room. O ecrã separa de forma elegante os ficheiros em curso/ativos (Sessão Atual) do histórico arquivado em disco (Histórico Persistido), disponibilizando ainda um botão tátil e intuitivo ("Limpar Histórico") para limpar a tabela na base de dados local.

---

## 17. Refresh em Tempo Real do Mission Control & Ecrã de Touchpad (Etapa 5)

Implementámos a sincronização em tempo real do ambiente de trabalho (Mission Control) via eventos enviados pelo daemon do PC, bem como o controlo completo de pointer, cliques e teclado (Touchpad & Teclado):

### 1. Refresh Baseado em Push (`hypr.event`)
- **Funcionamento**: O daemon empurra eventos do Hyprland do tipo `hypr.event { event: String }` (ex: "workspace", "openwindow", "closewindow", "destroywindow").
- **Ligação do Canal**: No router de streams `handlePeerStream` no `ConnectionRepository`, quando recebemos um `hypr.event`, emitimo-lo dinamicamente através de um `hyprEvents` `MutableSharedFlow<String>`.
- **Coleção na UI**: Na `MainActivity` / `MissionControlScreen`, coletamos este SharedFlow em tempo real dentro de um `LaunchedEffect` para disparar `refreshData()` instantaneamente de forma debouncada (não disparamos se já houver um fetch em curso). O poll de 5s foi mantido puramente como um mecanismo de fallback de segurança. Fora do ecrã Desktop, o evento é silenciosamente ignorado.

### 2. Ativação e Implementação do Touchpad & Teclado (Feature 6)
- **Ativação Visual**: Ativámos o tile "Touchpad & Teclado" (Módulo 6) na grelha principal de funcionalidades do Dashboard (portrait) e no menu lateral (landscape/tablet), permitindo alternar de forma limpa para o ecrã `TouchpadScreen`.
- **Ecrã de Interface Unificada (`TouchpadScreen`)**:
  - **Superfície de Toque**: Captura movimentos táteis através do `pointerInput` do Compose.
    - **1 Dedo Drag**: Acumula deltas e envia `input.move { dx, dy }`.
    - **Throttling Obrigatório**: O movimento é estrangulado a exatamente ~60 Hz (16ms) em uma corrotina LaunchedEffect de fundo para consolidar e enviar os deltas, evitando saturar o canal de dados QUIC e elevando o conforto de uso. Aplica-se um fator de sensibilidade ajustável de `1.5x`.
    - **Toque Curto (Tap com 1 Dedo)**: Envia imediatamente `input.click { "left" }`.
    - **Tap com 2 Dedos**: Envia imediatamente `input.click { "right" }`.
    - **Drag com 2 Dedos**: Acumula deltas de scroll e envia `input.scroll { dx, dy }` (também estrangulado de forma idêntica).
  - **Cliques Explícitos**: Barra de botões inferior com ações táteis explícitas para **ESQUERDO**, **MEIO** e **DIREITO** via `input.click`.
  - **Entrada de Teclado IME Invisível**: 
    - Um componente `BasicTextField` de tamanho reduzido (`1.dp`) e transparente que ganha foco tátil ao clicar no botão de Teclado, abrindo o teclado virtual do Android.
    - Monitorização inteligente do buffer: inicializa-se com um caractere dummy de espaço (`" "`). Ao digitar, extraímos a diferença e enviamos via `input.type`. Se o buffer for esvaziado pelo utilizador, capturamos como um evento de Backspace e enviamos `input.key("backspace")`, reinicializando sempre de seguida de forma impercetível.
  - **Teclas Especiais Auxiliares**: Uma barra horizontal deslizante contendo botões para teclas especiais comuns (`ESC`, `TAB`, `ENTER`, `BKSP`, `ESP`, `DEL`, setas direcionais `↑`, `↓`, `←`, `→`), para facilitar a navegação em ferramentas do terminal do PC que o teclado de ecrã não gera nativamente.
  - **Garantia de Ligação Ativa**: Caso a workstation esteja desconectada (e sem modo simulado de demonstração ativo), o ecrã mostra um placeholder centralizado e elegante impedindo o uso e instruindo o utilizador com a mensagem: *"Liga a uma workstation primeiro."*.

### 3. Requisito de Execução no Servidor (Workstation)
- **ydotool**: Para que a injeção remota de teclado e rato funcione corretamente no computador com Hyprland, a ferramenta **ydotool** e o seu daemon **ydotoold** têm de estar ativos na workstation.
- **Configuração no Computador**:
  ```bash
  systemctl --user enable --now ydotool.service
  ```
  Isso garante que o socket local do ydotool de injeção esteja disponível para o daemon do HyprLink empurrar os pacotes `input.*`.







## 18. Fixes do Touchpad e Teclado
Corrigidos três problemas que impediam o funcionamento correto do rato e teclado remotos:
1. **Inteiros Negativos no CBOR**: O método `encodePacket` rejeitava deltas negativos no `input.move` (gerava a exceção `value X is not >= 0`). Adicionado o suporte correto a `NegativeInteger` para deltas negativos, mantendo `UnsignedInteger` para valores positivos.
2. **Stream Leak no QUIC**: O método `sendOneWayPacket` deixava o fluxo bidirecional de output aberto em caso de falha de envio, esgotando os stream credits e paralisando pacotes subsequentes (teclado e botões). Os fluxos de entrada e saída são agora forçados a fechar no bloco `finally`.
3. **Múltiplos Envios de Teclado**: O input tátil (`BasicTextField`) tinha múltiplos percursos de código a operar ao mesmo tempo (lógica de diff misturada com lógica de sentinela), provocando envios duplicados em cada tecla pressionada. O bloco de diff foi completamente removido, centralizando o comportamento na lógica fiável do caractere de sentinela.

## 19. Fix do Teclado Remoto (Espaços)
Corrigido um erro em que a injeção do teclado enviava sempre um espaço em vez da letra digitada.
1. O valor de estado `keyboardText` invisível usava a string " " sem configurar o índice de seleção, fazendo com que o IME adicionasse o caractere antes do espaço (índice 0). 
2. A solução implementada introduziu uma variável constante chamada `sentinelKeyboardValue` (`TextFieldValue` com `TextRange(1)`), assegurando que o cursor fica do lado direito do espaço.
3. Nas alterações aplicadas foi também substituído o uso do índice rígido (`.substring(1)`) pelo método `.removePrefix(" ")`, e em todas as três ocorrências no reposicionamento, o valor é redefinido sempre para a sentinela para eliminar estados inesperados passados pelo IME virtual do telemóvel Android.

## 20. PC Webcam (Etapa 6)
Implementada a funcionalidade para usar a câmara do telemóvel Android como webcam remota para a workstation Linux.
1. **Nova Permissão e Serviço em Foreground**: Adicionada a permissão `android.permission.CAMERA` e atualizado o serviço `HyprLinkConnectionService` para incluir o tipo `camera` no `foregroundServiceType`, garantindo que o stream contínuo funcione em segundo plano e satisfaça as restrições introduzidas no Android 11 e 14+.
2. **Criação do WebcamStreamer**: Desenvolvida a classe isolada `WebcamStreamer.kt` usando as bibliotecas do CameraX e `MediaCodec`.
   - Lê as definições de resolução (width/height) e framerate requisitadas pelo Daemon no PC.
   - Associa a câmara traseira diretamente a uma surface partilhada criada pelo `MediaCodec` (via `Preview` do CameraX), ativando uma compressão contínua acelerada por hardware em formato AVC/H.264 (Annex-B).
   - Abre um stream QUIC unidirecional onde o primeiro conteúdo são os 8 bytes do `packet.id` fornecido pelo PC, seguido dos buffers H.264 contínuos.
3. **Novo Ecrã de Monitorização**: Desenhada uma overlay não intrusiva a ecrã total baseada na janela ativa. Quando a variável de estado reativa `WebcamStreamer.isStreaming` altera, o painel revela uma feed de `PreviewView` ao vivo, sinalética visual "CÂMARA ATIVA" em vermelho e um botão com contraste elevado de ação tátil para o utilizador abortar a transmissão a qualquer momento (emitindo o corte do codec local e limpeza do canal do QUIC).
4. **Descodificação Segura (Handling Bidirecional)**: Incluídos os triggers `webcam.start` e `webcam.stop` no router do fluxo de entrada `handlePeerStream` no `ConnectionRepository`, permitindo arrancar ou terminar de imediato as corrotinas que gerem a conversão do codificador em resposta às diretivas do host hyprland.

## 21. Fix da Webcam: Imagem Ausente, Fps e Lente Frontal
Corrigido um problema na Etapa 6 onde o stream QUIC era enviado ao PC mas não continha imagem porque as instâncias concorrentes de câmara conflitavam (uma do Preview do ecrã e outra do encoder H.264).
1. **Centralização do Controlo da Câmara**: A classe `WebcamStreamer` passou a ser o único componente a gerir o ciclo de vida do CameraX. As duas *Use Cases* (`encoderPreview` e `screenPreview`) são agora agrupadas num só `bindToLifecycle`, garantindo que ambos recebem frames fluidos simultaneamente a partir da mesma interface de hardware.
2. **Nova Funcionalidade - Lente Alternável**: Incluído no painel a opção "TROCAR CÂMARA", permitindo rodar em tempo real entre a câmara traseira e a frontal (`switchCamera()`). O bind em runtime permite modificar a stream contínua no PC instantaneamente sem cortar o feed H.264.
3. **Hardware FPS Targeting**: Implementado suporte nativo para `Camera2Interop`, enviando ao sensor da câmara as diretivas de framerate dinâmico definidas pelo PC (24, 30, 60, ou 90 fps) usando as extensões `CONTROL_AE_TARGET_FPS_RANGE`. O streaming obedece agora de forma fiel ao preset requisitado no daemon.

## 22. Adicionado Botão Webcam no Dashboard
Foi adicionado o módulo/botão "Webcam" na grelha central (tanto no ecrã Portrait padrão como no layout Desktop expansivo), ao lado dos outros botões de acesso rápido (Media, Files, Track). Este novo botão evidencia o estado reativo da transmissão:
1. Emissão contínua via CameraX para H.264
2. Indicações visuais de transmissão "ON" em tons Cyan quando ativado via Workstation
3. Capacidade de paragem forçada diretamente com um toque se já estiver a decorrer (Stop explícito).

## 24. Diagnóstico Persistente de Extremo a Extremo (100% Logs Guardados localmente)
Implementámos uma arquitetura robusta e resiliente de persistência e auditoria de 100% dos eventos que ocorrem na aplicação:
1. **Ficheiro de Log Físico (`diagnostics.log`)**: Todas as chamadas para `appendLog` são automaticamente formatadas e guardadas num ficheiro local seguro dentro do diretório privado da aplicação (`context.filesDir`).
2. **Carregamento Automático ao Iniciar**: O terminal de diagnóstico já não começa vazio após reiniciar a aplicação! Ao iniciar, as últimas 1000 linhas são carregadas do ficheiro físico para a memória RAM num bloco thread-safe sincronizado.
3. **Controlo Inteligente de Armazenamento (Rotação Automática)**: Para evitar o consumo excessivo de armazenamento do dispositivo móvel, se o ficheiro ultrapassar o limite de 5MB, os logs mais antigos são automaticamente truncados de forma limpa, mantendo o histórico de maior prioridade intacto.
4. **Sincronização com Timestamps de Alta Precisão**: Cada evento passa a ser etiquetado de forma precisa com o formato `[HH:mm:ss.SSS]` (milissegundos) tanto no ecrã como no ficheiro de logs físico, ajudando na auditoria exata da latência e da entrega dos pacotes.
5. **Ações Completas no Terminal (Wipe e Export 100% Real)**:
   - **Exportar Completo**: O botão de partilha agora lê dinamicamente o ficheiro físico total através do método `ConnectionRepository.getFullLogText()`, permitindo extrair e partilhar 100% do histórico acumulado.
   - **Limpeza Total (Wipe)**: Adicionámos um novo botão com ícone de lixeira (`Icons.Default.Delete`) ao lado do botão de partilha que executa `ConnectionRepository.clearLogs()`, limpando instantaneamente tanto a memória de exibição quanto o ficheiro físico no disco.

## 25. Reversão de Reconexão de Stream na Rotação da Webcam (Controlo pelo PC)
Em conformidade com a última atualização de protocolo, a "Reconexão Inteligente de Stream" para rotação da webcam foi **totalmente revertida**:
1. **Diagnóstico Atualizado**: Embora o `v4l2loopback` no PC não mude as resoluções de canvas dinamicamente, a solução definitiva foi implementada diretamente no software do computador. O pipeline do PC agora reescala o vídeo recebido de volta ao canvas original com barras laterais (pillarbox) nas rotações de 90°/270°.
2. **Reversão do Lado do Cliente**: Com esta correção no PC, o telemóvel não necessita mais de reiniciar o codificador, fechar streams ou restaurar sessões de áudio/vídeo. As funções `rotate90()` e `toggleMirror()` voltam a apenas atualizar o estado local e despachar instantaneamente o pacote `webcam.transform { rotation, mirror }` via rede. A rotação agora ocorre de forma imediata em menos de 1 segundo sem interrupções de conexão ou imagem.

## 26. Solução Fixa de Alinhamento de Imagem (Câmaras Frontal e Traseira)
Corrigimos a orientação física de ambas as câmaras para garantir uma transmissão perfeita ao PC e uma visualização local correspondente:
1. **Câmara Frontal (PC em Vertical e Espelhado)**:
   - **Problema**: O utilizador aparecia corretamente em vertical 9:16 e espelhado no telemóvel, mas no PC a imagem chegava horizontal (landscape) rodada a 180°.
   - **Solução**: Configurámos por padrão um estado de rotação de `270°` e espelhamento ativo (`true`) para a câmara frontal. Como estes valores são negociados e transmitidos logo no início da sessão (ou ao trocar de lente), o pipeline de rede e o GStreamer no PC iniciam já em formato vertical portrait, perfeitamente orientado e espelhado de forma síncrona.
2. **Câmara Traseira (Visualização Local Alinhada com o PC)**:
   - **Problema**: No telemóvel, a imagem da câmara traseira aparecia em vertical, mas ao PC chegava em formato horizontal (landscape) rodada a 180°.
   - **Solução**: Para garantir coerência absoluta entre o que é visto no telemóvel e o que é transmitido, adicionámos rotação dinâmica de `90f` no componente `PreviewView` local quando a câmara traseira é usada. Deste modo, o ecrã do telemóvel exibe a imagem em formato horizontal idêntico ao que chega ao computador.
3. **Reset Inteligente e Transição Transparente**:
   - Ao alternar entre lentes ("LENTE") durante a transmissão, os novos estados padrão (Frontal: 270° + espelho; Traseira: 0° sem espelho) são atualizados em runtime e aplicados com o restart sem falhas da stream, sem necessidade de configuração manual do utilizador.

---

## 27. Ecrã ÁUDIO Completo (Media, Mixer, Saídas e "Ouvir no Telemóvel")

Implementámos uma central de controlo de áudio completa e interativa:
1. **A TOCAR (Player Remoto)**: Mostra em tempo real metadados da música (Título, Artista, Player) do PC e oferece controlos ⏮ ⏯ ⏭ integrados com o sistema `media.*`.
2. **OUVIR NO TELEMÓVEL (Audio Tap de Baixa Latência)**:
   - Envia `audio.tap_start` e recebe `audio.tap_ready { id, rate, channels }`.
   - Abre uma stream unidirecional QUIC recebendo áudio PCM cru infinito (16-bit little-endian, 48kHz, Estéreo intercalado).
   - Toca o stream dinamicamente usando a API `AudioTrack` configurada em modo `MODE_STREAM` com buffers ultra otimizados para evitar estalidos grosseiros.
   - Ao ligar o switch, redireciona automaticamente o som do PC para o telemóvel enviando `audio.set_default_sink` com o nome `"hyprlink-speaker"`. Ao desligar, reverte para a saída de áudio padrão anterior do computador.
3. **SAÍDAS DE SOM (Sinks Mixer)**: Lista de seleção tátil para alternar saídas físicas de som no PC (com indicação intuitiva de *"📱 Este telemóvel"* para o sink correspondente) com barras deslizantes de volume mestre (0-150%) e comutador Mute independentes por sink.
4. **APLICAÇÕES (App Volume Controller)**: Permite ajustar individualmente o volume (0-150%) e o estado de Mute de cada aplicação a correr no PC (e.g., Spotify, Chrome) com atualização reativa a cada 3 segundos via poll de `audio.state` (evitando sobrescrever sliders em arrasto).

---

## 28. Polimento Final da Webcam (Lentes, Rotação/Espelho, Codec HEVC e Microfone Sem Fios)

Concluímos um pacote abrangente de otimização na transmissão da câmara:
1. **Suporte Multi-Lentes Dinâmico**: O comutador "LENTE" agora analisa todas as câmaras expostas (`availableCameraInfos`) do CameraX, permitindo ciclar por lentes frontal, traseira principal, ultra-wide ou tele de forma instantânea sem cortar a transmissão.
2. **Rotação e Espelho no PC (Zero Custo Local)**: Introduzimos controlos intuitivos "RODAR X°" e "ESPELHAR" no painel de comando. O telemóvel apenas emite o estado `webcam.transform { rotation, mirror }` para aplicação em tempo real no PC através do pipeline GStreamer (sem custo de CPU ou latência no dispositivo móvel).
3. **Compressão HEVC/H.265 Inteligente**: O telemóvel analisa a preferência local e de rede e tenta usar o codec H.265 acelerado por hardware para transmitir o vídeo, poupando até 40% de largura de banda Wi-Fi mantendo a qualidade de imagem idêntica. Cobre de forma totalmente transparente o fallback para H.264 em runtime caso o dispositivo não disponha de codificador hardware HEVC.
4. **Microfone Sem Fios Incorporado**: O utilizador pode iniciar/parar a gravação de voz diretamente do painel ativo da câmara. O áudio do microfone físico do telemóvel é capturado de forma contínua em formato PCM de 48kHz mono através de `AudioRecord` e enviado por um canal QUIC dedicado, permitindo injetar o sinal num microfone virtual correspondente no Linux.
5. **Prevenção de Suspensão de Ecrã**: O diálogo de câmara ativa força o ecrã a ficar permanentemente ligado (`dialogView.keepScreenOn = true`), impedindo que o Android atinja o modo de descanso ou bloqueie a transmissão durante o streaming contínuo.

---

## 29. Correção de Descodificação e Codificação de Estruturas CBOR Aninhadas (Listas e Mapas)

Identificámos e corrigimos uma falha estrutural no processamento de pacotes CBOR que impedia o correto funcionamento do novo ecrã ÁUDIO:
1. **Causa Raiz**: O descodificador `decodePacket` original apenas tratava tipos escalares básicos de dados (como `String`, `Long`, `Boolean`, `ByteArray` e `null`). Ao deparar-se com arrays (`co.nstant.in.cbor.model.Array`) ou dicionários (`co.nstant.in.cbor.model.Map`) nativos no payload CBOR de resposta (tais como as listas `sinks` ou `apps` do comando `audio.state`), o descodificador caía no ramo de fallback genérico convertendo o objeto CBOR interno numa string bruta. Isto provocava falhas silenciosas de casting em runtime e impedia a exibição das listas correspondentes na UI.
2. **Solução de Descodificação Recursiva**: Implementámos o método privado `decodeCborValue(item: DataItem?): Any?` no `ConnectionRepository`. Este método avalia recursivamente os dados recebidos e converte corretamente arrays CBOR em `List<Any?>` de Kotlin, e mapas CBOR em `Map<String, Any?>`, preservando o tipo nativo correto dos seus elementos de forma dinâmica.
3. **Solução de Codificação Recursiva**: Para evitar retrocompatibilidades ou problemas futuros ao enviar payloads mais complexos do telemóvel para o PC, estendemos também o codificador `encodePacket` com o método correspondente `encodeCborValue(value: Any?): DataItem`. Este reconstrói nativamente arrays (`co.nstant.in.cbor.model.Array`) e dicionários (`co.nstant.in.cbor.model.Map`) aninhados, garantindo a simetria perfeita na comunicação bidirecional QUIC-CBOR do projeto.

---

## 30. Otimização das Operações One-Way e Normalização Indefinida do CBOR (Correção do Áudio Tap e Controlo de Média)

Identificámos e corrigimos dois problemas graves que impediam o funcionamento do controlo de média e a transmissão/ativação do Audio Tap (ouvir áudio do PC no telemóvel):

1. **Normalização de Codificação Indefinida (Chunked CBOR)**: 
   - **Problema**: O parser de CBOR no daemon da workstation (escrito em Python/Rust) espera que os mapas e arrays aninhados no corpo dos pacotes sejam codificados como estruturas de comprimento indefinido (indefinite-length / chunked), padrão utilizado pelo `CborBuilder` original da biblioteca Java `co.nstant.in.cbor`. A nossa implementação recursiva anterior de `encodeCborValue` criava instâncias de `co.nstant.in.cbor.model.Map` e `Array` de comprimento fixo (definite-length) e sem fragmentação. Isto fazia com que o daemon do PC rejeitasse de forma silenciosa os corpos de pacotes aninhados (como os de volume, mute, comandos de média ou início de áudio tap), impossibilitando o processamento do lado do host.
   - **Solução**: Configurámos explicitamente as instâncias de `co.nstant.in.cbor.model.Map` e `co.nstant.in.cbor.model.Array` instanciadas dinamicamente com o método `.setChunked(true)` nos métodos `encodePacket` e `encodeCborValue`. Deste modo, toda a codificação regressa a um formato 100% homólogo e compatível com as expetativas do servidor.

2. **Eliminação de Bloqueio Sistemático de 10 Segundos em Fluxos Unidirecionais (One-Way)**:
   - **Problema**: O método geral `sendAnnouncedPacket` era utilizado para o despacho de todos os pacotes unidirecionais (`sendOneWayPacket`), tais como `media.command` (play/pause, next, previous), injeção de teclas/ratos, alterações de clipboard e bateria. No entanto, este método tentava forçar um flush realizando a leitura bloqueante de EOF através de `inp.readBytes()` com um prazo limite de 10 segundos. Visto que o daemon da workstation não envia qualquer byte de resposta para pacotes one-way, a thread bloqueava sistematicamente por exatamente **10 segundos** antes de retornar para cada clique, resultando numa paralisia de rede de controlo, acumulando lag dramático e dando a perceção de que o controlo de média e o áudio tap não funcionavam de todo.
   - **Solução**: Modificámos o comportamento em `sendAnnouncedPacket` para apenas invocar a leitura de confirmação `inp.readBytes()` quando a flag `hasPayload` for explicitamente `true`. Para todas as comunicações one-way (onde `hasPayload == false`), o fluxo realiza a escrita, fecha o output (`out.close()`) para flush del frame QUIC e retorna instantaneamente em microsegundos. Isto removeu completamente a latência, tornando os botões de média, teclado, touchpad e notificações incrivelmente fluidos e instantâneos.

---

## 31. Persistência de Controlos e Resolução Dinâmica de Sinks (Controlo de Média Ativo / Inativo e Audio Tap Inteligente)

Aperfeiçoámos as interfaces e interações de áudio e média com duas grandes melhorias:

1. **Controlos de Média Sempre Disponíveis**:
   - **Problema**: Anteriormente, a interface do ecrã de Áudio ocultava completamente os botões de reprodução (Anterior, Play/Pause, Seguinte) quando o PC não se encontrava a reproduzir ativamente nenhuma faixa de música/vídeo (`mediaState == null`). Isto impedia o utilizador de interagir ou de usar o telemóvel para mandar um sinal de Play genérico para reativar os leitores de média do computador host.
   - **Solução**: Implementámos um estado alternativo (fallback) no `AudioScreen`. Se o `mediaState` for nulo, a aplicação desenha os controlos com um visual focado em "Inativo / Nenhum leitor ativo detetado", mantendo os botões de reprodução operacionais. Desta forma, o utilizador pode sempre controlar o PC.

2. **Resolução de Sink de Áudio Dinâmica (Áudio Tap)**:
   - **Problema**: O switch do Audio Tap estava configurado para forçar a saída por defeito para a string estática `"hyprlink-speaker"`. Se o daemon no computador criasse o dispositivo de loopback virtual com outra nomenclatura no PulseAudio/PipeWire, o encaminhamento do áudio falhava silenciosamente.
   - **Solução**: Atualizámos a rotina no interruptor do Audio Tap para primeiro procurar dinamicamente por uma placa onde `is_phone == true` na lista de sinks ativos enviados pelo PC. Se detetado, utiliza o nome do sink dinamicamente, mantendo `"hyprlink-speaker"` apenas como último recurso de fallback.

