# Histórico Completo de Versões, Mensagens e Desenvolvimento — HyprLink

Este documento compila a linha do tempo exata de todas as fases, versões desenvolvidas, intenções de mensagens, problemas enfrentados e as soluções técnicas implementadas no projeto **HyprLink**.

---

## Índice das Versões e Iterações

* [Fase 1: Configuração Inicial de Dependências e Descoberta do Runtime](#fase-1-configuração-inicial-de-dependências-e-descoberta-do-runtime)
* [Fase 2: Engenharia Reversa da API Interna do Kwik QUIC Client](#fase-2-engenharia-reversa-da-api-interna-do-kwik-quic-client)
* [Fase 3: Handshake mTLS e Resolução do Hostname Verifier (Fingerprint Pinning)](#fase-3-handshake-mtls-e-resolução-do-hostname-verifier-fingerprint-pinning)
* [Fase 4: Persistência com Room Database e Troca Rápida de Workstation](#fase-4-persistência-com-room-database-e-troca-rápida-de-workstation)
* [Fase 5: Timeouts Reais de Leitura e Prevenção de Races de Reconexão](#fase-5-timeouts-reais-de-leitura-e-prevenção-de-races-de-reconexão)
* [Fase 6: Correção do Scanner de QR Code de Emparelhamento](#fase-6-correção-do-scanner-de-qr-code-de-emparelhamento)
* [Fase 7: Resolução do Crash de Permissões FGS (Android 14+ / API 34-35)](#fase-7-resolução-do-crash-de-permissões-fgs-android-14--api-34-35)
* [Fase 8: Separação Arquitetural da Porta de Pareamento vs. Porta de Serviço (7443)](#fase-8-separação-arquitetural-da-porta-de-pareamento-vs-porta-de-serviço-7443)
* [Fase 9: Definição dos Pacotes e Protocolo Hyprland (hypr.*)](#fase-9-definição-dos-pacotes-e-protocolo-hyprland-hypr)
* [Fase 10: Criação do Ecrã Desktop / Mission Control](#fase-10-criação-do-ecrã-desktop--mission-control)
* [Fase 11: Auto-Refresh e Resiliência no Ecrã Desktop](#fase-11-auto-refresh-e-resiliência-no-ecrã-desktop)
* [Fase 12: Auditoria de Segurança e Conformidade de Permissões de Rede](#fase-12-auditoria-de-segurança-e-conformidade-de-permissões-de-rede)
* [Fase 13: Grelha Modular de Recursos do Dashboard (Ativas vs. Em Breve)](#fase-13-grelha-modular-de-recursos-do-dashboard-ativas-vs-em-breve)
* [Fase 14: Resposta Remota a Notificações a partir do PC (RemoteInput)](#fase-14-resposta-remota-a-notificações-a-partir-do-pc-remoteinput)
* [Fase 15: Telemetria e Envio de Estado da Bateria (battery.state)](#fase-15-telemetria-e-envio-de-estado-da-bateria-batterystate)
* [Fase 16: Higiene Técnica, Keepalive Ping/Pong e Histórico de Transferências em Disco](#fase-16-higiene-técnica-keepalive-pingpong-e-histórico-de-transferências-em-disco)
* [Fase 17: Push em Tempo Real do Hyprland e Ecrã Touchpad/Teclado (Feature 6)](#fase-17-push-em-tempo-real-do-hyprland-e-ecrã-touchpadteclado-feature-6)
* [Fase 18: Correções Críticas de Input (Inteiros Negativos CBOR e Stream Leak QUIC)](#fase-18-correções-críticas-de-input-inteiros-negativos-cbor-e-stream-leak-quic)
* [Fase 19: Correção do IME Virtual e Tratamento de Espaços do Teclado](#fase-19-correção-do-ime-virtual-e-tratamento-de-espaços-do-teclado)
* [Fase 20: Webcam Sem Fios do PC com MediaCodec e CameraX (Feature 8)](#fase-20-webcam-sem-fios-do-pc-com-mediacodec-e-camerax-feature-8)
* [Fase 21: Correção do Pipeline da Webcam (Fps Dinâmico e Unificação de Use Cases)](#fase-21-correção-do-pipeline-da-webcam-fps-dinâmico-e-unificação-de-use-cases)
* [Fase 22: Integração Reativa da Webcam no Dashboard Principal](#fase-22-integração-reativa-da-webcam-no-dashboard-principal)
* [Fase 23: Diagnósticos Persistentes em Disco (diagnostics.log) com Rotação de 5MB](#fase-23-diagnósticos-persistentes-em-disco-diagnosticslog-com-rotação-de-5mb)
* [Fase 24: Reversão da Reconexão na Rotação da Câmara (Pillarbox no PC)](#fase-24-reversão-da-reconexão-na-rotação-da-câmara-pillarbox-no-pc)
* [Fase 25: Alinhamento e Espelhamento Fixo das Câmaras Frontal e Traseira](#fase-25-alinhamento-e-espelhamento-fixo-das-câmaras-frontal-e-traseira)
* [Fase 26: Ecrã ÁUDIO Completo (Mixer PipeWire, Media Player e Audio Tap)](#fase-26-ecrã-áudio-completo-mixer-pipewire-media-player-e-audio-tap)
* [Fase 27: Polimento da Webcam (Multi-Lentes, HEVC/H.265 e Microfone Virtual Sem Fios)](#fase-27-polimento-da-webcam-multi-lentes-hevch265-e-microfone-virtual-sem-fios)
* [Fase 28: Correção Estrutural de CBOR Recursivo (Listas e Mapas Aninhados)](#fase-28-correção-estrutural-de-cbor-recursivo-listas-e-mapas-aninhados)
* [Fase 29: Normalização de CBOR Indefinido (Chunked) e Remoção do Delay de 10s One-Way](#fase-29-normalização-de-cbor-indefinido-chunked-e-remoção-do-delay-de-10s-one-way)
* [Fase 30: Persistência de Controles de Mídia Inativos e Resolução Dinâmica de Sinks](#fase-30-persistência-de-controles-de-mídia-inativos-e-resolução-dinâmica-de-sinks)
* [Fase 31: Documentação Integral e Implementações do Daemon Linux (Python e Rust)](#fase-31-documentação-integral-e-implementações-do-daemon-linux-python-e-rust)

---

### Fase 1: Configuração Inicial de Dependências e Descoberta do Runtime
* **Mensagem/Objetivo do Utilizador:** Inicializar o projeto HyprLink no Android Studio com stack moderna Kotlin/Compose e compatibilidade direta com QUIC/CBOR para Linux.
* **O que foi feito/trabalhado:**
  - Resolução das coordenadas reais no Maven Central em `gradle/libs.versions.toml` e `app/build.gradle.kts`.
  - Inclusão do **Kwik** (`tech.kwik:kwik:0.10.9`), **Bouncy Castle** (`bcprov-jdk18on:1.78.1`, `bcpkix-jdk18on:1.78.1`), **CBOR** (`co.nstant.in:cbor:0.9`), **AndroidX Security Crypto** (`security-crypto:1.1.0-alpha06`) e **ML Kit Barcode Scanning** (`barcode-scanning:17.3.0`).

### Fase 2: Engenharia Reversa da API Interna do Kwik QUIC Client
* **Mensagem/Objetivo do Utilizador:** Garantir que a biblioteca de QUIC funciona sem falhas de carregamento de classes ou de nativos no Android ART.
* **O que foi feito/trabalhado:**
  - Descoberta das classes públicas via reflexão no teste de unidade (`tech.kwik.core.QuicClientConnection` e seu Builder interno).
  - Mapeamento dos métodos: `host()`, `port()`, `applicationProtocol("hyprlink/1")`, `noServerCertificateCheck()`, `createStream()`, `getOutputStream()`, `getInputStream()`.
  - Construção do código mínimo comprovado de ligação e teste de transmissão.

### Fase 3: Handshake mTLS e Resolução do Hostname Verifier (Fingerprint Pinning)
* **Mensagem/Objetivo do Utilizador:** Resolver o erro fatal `CertificateUnknownAlert: servername does not match` durante a ligação por IP ao PC.
* **O que foi feito/trabalhado:**
  - O motor TLS interno do Kwik (`agent15`) validava apenas SANs de DNS ignorando SANs de IP.
  - Solução: Desativar a verificação de CAs clássicas com `.noServerCertificateCheck()` e injetar autenticação de cliente mTLS (`clientCertificate` + `clientCertificateKey`).
  - Implementação da validação estrita pós-ligação por **SHA-256 Fingerprint Pinning** antes de transmitir qualquer byte da aplicação em `ConnectionUtils.connectPinned`.

### Fase 4: Persistência com Room Database e Troca Rápida de Workstation
* **Mensagem/Objetivo do Utilizador:** Salvar computadores pareados para não precisar escanear o QR Code a cada reinicialização do app.
* **O que foi feito/trabalhado:**
  - Criação das entidades do Room: `Workstation` com ID, nome, host, porta, fingerprint e token de pareamento.
  - Implementação de um dropdown/seletor visual na interface para troca rápida da estação de trabalho ativa.

### Fase 5: Timeouts Reais de Leitura e Prevenção de Races de Reconexão
* **Mensagem/Objetivo do Utilizador:** O aplicativo travava as threads de rede se o PC fosse desligado subitamente ou a conexão caísse.
* **O que foi feito/trabalhado:**
  - A leitura síncrona `InputStream.read()` do Kwik não respondia a cancelamentos de corrotinas.
  - Criação da infraestrutura `ConnectionUtils.readWithDeadline` combinando `runInterruptible` com `stream.abortReading(0)` nativo.
  - Criação de um contador monotónico de geração (`connectionGeneration`) para descartar conexões obsoletas e evitar sobreposição de sockets ativos.

### Fase 6: Correção do Scanner de QR Code de Emparelhamento
* **Mensagem/Objetivo do Utilizador:** O scanner de câmera falhava dizendo que faltavam campos no QR Code gerado pelo PC.
* **O que foi feito/trabalhado:**
  - Remoção de um parser inline incorreto que esperava 4 campos fixos.
  - Unificação na função canônica `parsePairingQr(qrText)` com suporte a IPv4, IPv6 e divisão de porta no formato `<fingerprint_hex>|<host>:<porta>|<token_hex>`.

### Fase 7: Resolução do Crash de Permissões FGS (Android 14+ / API 34-35)
* **Mensagem/Objetivo do Utilizador:** Corrigir crash `SecurityException: Starting FGS with type connectedDevice requires at least one permission`.
* **O que foi feito/trabalhado:**
  - O Android 14+ exige que serviços de primeiro plano do tipo `connectedDevice` declarem permissões qualificadas.
  - Adição de `android.permission.CHANGE_NETWORK_STATE` e `android.permission.CHANGE_WIFI_STATE` (permissões de nível normal concedidas na instalação, sem popups intrusivos).

### Fase 8: Separação Arquitetural da Porta de Pareamento vs. Porta de Serviço (7443)
* **Mensagem/Objetivo do Utilizador:** O app não conseguia reconectar ao PC após o pareamento inicial.
* **O que foi feito/trabalhado:**
  - Identificação de portas distintas: a porta do QR Code é temporária (apenas para handshake inicial).
  - Criação da constante unificada `HYPRLINK_SERVICE_PORT = 7443`. Ao gravar no Room, o app fixa a porta do serviço em 7443.

### Fase 9: Definição dos Pacotes e Protocolo Hyprland (hypr.*)
* **Mensagem/Objetivo do Utilizador:** Estruturar os comandos de controle remoto das janelas e áreas de trabalho do compositor Wayland.
* **O que foi feito/trabalhado:**
  - Mapeamento dos pacotes: `hypr.get_workspaces` (retorna JSON de `hyprctl workspaces -j`), `hypr.get_clients` (retorna `hyprctl clients -j`) e `hypr.dispatch` (despacha ações como focar e fechar janelas).

### Fase 10: Criação do Ecrã Desktop / Mission Control
* **Mensagem/Objetivo do Utilizador:** Construir a interface gráfica no Android para visualizar e gerenciar as janelas do PC.
* **O que foi feito/trabalhado:**
  - Interface com chips dinâmicos para cada workspace ativo, cartões com os nomes das janelas/aplicações abertas, botão de focar janela e botão de fechar com confirmação.
  - Implementação de um modo de demonstração interativo quando desconectado.

### Fase 11: Auto-Refresh e Resiliência no Ecrã Desktop
* **Mensagem/Objetivo do Utilizador:** As janelas mostradas no telemóvel ficavam desatualizadas se o usuário abrisse ou fechasse algo diretamente no computador.
* **O que foi feito/trabalhado:**
  - Implementação de polling automático com intervalo de 5 segundos rodando sob `LaunchedEffect`.
  - Tratamento de sobreposição de requisições (`isLoading`) e silenciamento de erros transitórios para evitar telas piscando.

### Fase 12: Auditoria de Segurança e Conformidade de Permissões de Rede
* **Mensagem/Objetivo do Utilizador:** Avaliar se as permissões declaradas no manifesto violam as políticas da Google Play Store.
* **O que foi feito/trabalhado:**
  - Auditoria completa: manutenção estrita das permissões de rede normais, evitando a necessidade de pedir localização precisa por GPS ou Bluetooth invasivo.

### Fase 13: Grelha Modular de Recursos do Dashboard (Ativas vs. Em Breve)
* **Mensagem/Objetivo do Utilizador:** Organizar todos os módulos do aplicativo numa tela inicial bonita e intuitiva.
* **O que foi feito/trabalhado:**
  - Criação de uma grelha M3 de 2 colunas com 10 módulos: Desktop, Partilha, Clipboard, Media, Transferências, Touchpad/Teclado, Notificações, Webcam, Chamadas e Espelhar Ecrã.
  - Distinção visual entre recursos prontos e selos "EM BREVE", com controle de opacidade e feedbacks táteis.

### Fase 14: Resposta Remota a Notificações a partir do PC (RemoteInput)
* **Mensagem/Objetivo do Utilizador:** Permitir que o usuário responda a mensagens (WhatsApp, Telegram, etc.) recebidas no celular diretamente do teclado do PC.
* **O que foi feito/trabalhado:**
  - Atualização do `HyprNotificationListenerService` para exportar a flag `is_reply: true` e o índice `idx`.
  - Tratamento do pacote `notification.reply`: preenchimento dos bundles do `RemoteInput` e disparo assíncrono via `actionIntent.send()`.

### Fase 15: Telemetria e Envio de Estado da Bateria (battery.state)
* **Mensagem/Objetivo do Utilizador:** Mostrar a porcentagem de bateria do celular na barra de status do Linux (Waybar).
* **O que foi feito/trabalhado:**
  - Registro de um `BroadcastReceiver` dinâmico em `HyprLinkConnectionService` escutando `ACTION_BATTERY_CHANGED`.
  - Envio filtrado de pacotes CBOR `battery.state { level, charging }` apenas em mudanças de estado reais, além de disparo imediato após o handshake `core.hello`.

### Fase 16: Higiene Técnica, Keepalive Ping/Pong e Histórico de Transferências em Disco
* **Mensagem/Objetivo do Utilizador:** Garantir estabilidade a longo prazo e registrar histórico persistente de envios de arquivos.
* **O que foi feito/trabalhado:**
  - Conversão de `connectionGeneration` para `AtomicInteger`.
  - Implementação do keepalive em segundo plano disparando `core.ping` a cada 30 segundos, com reconexão automática após falhas consecutivas.
  - Migração do Room (`MIGRATION_1_2`) criando a tabela `transfer_records` para manter o histórico de transferências após fechar o app.

### Fase 17: Push em Tempo Real do Hyprland e Ecrã Touchpad/Teclado (Feature 6)
* **Mensagem/Objetivo do Utilizador:** Atualização instantânea sem depender de poll no Desktop e uso do celular como mouse/touchpad do Linux.
* **O que foi feito/trabalhado:**
  - Recebimento de eventos push `hypr.event` para refresh imediato no Mission Control.
  - Criação do `TouchpadScreen`: área tátil com drag a 60Hz (16ms throttle), tap para botão esquerdo, tap com 2 dedos para botão direito, scroll com 2 dedos, botões físicos e barra de teclas especiais (`ESC`, `TAB`, `ENTER`, etc.).
  - Integração com o daemon `ydotool` no Linux.

### Fase 18: Correções Críticas de Input (Inteiros Negativos CBOR e Stream Leak QUIC)
* **Mensagem/Objetivo do Utilizador:** O mouse não se movia para a esquerda ou para cima e o teclado parava de responder após alguns cliques.
* **O que foi feito/trabalhado:**
  - Suporte a `NegativeInteger` no codificador CBOR (deltas negativos como `dx = -10` travavam o codificador).
  - Correção de vazamento de stream QUIC em `sendOneWayPacket`: fechamento garantido no bloco `finally` para evitar esgotamento de créditos de stream.
  - Remoção de código concorrente duplicado no listener do teclado virtual.

### Fase 19: Correção do IME Virtual e Tratamento de Espaços do Teclado
* **Mensagem/Objetivo do Utilizador:** O teclado virtual do celular digitava espaços antes das letras.
* **O que foi feito/trabalhado:**
  - Implementação da sentinela imutável `sentinelKeyboardValue` (`TextFieldValue(" ", TextRange(1))`), mantendo o cursor sempre à direita do caractere sentinela.
  - Uso de `.removePrefix(" ")` e reset consistente do buffer.

### Fase 20: Webcam Sem Fios do PC com MediaCodec e CameraX (Feature 8)
* **Mensagem/Objetivo do Utilizador:** Usar o smartphone como webcam de alta definição para o computador (OBS, Zoom, Discord, etc.).
* **O que foi feito/trabalhado:**
  - Adição da permissão `android.permission.CAMERA` e atualização do serviço para `foregroundServiceType="camera|connectedDevice"`.
  - Criação da classe `WebcamStreamer.kt`: codificação contínua H.264 acelerada por hardware via `MediaCodec` conectada ao CameraX.
  - Transmissão em uni-stream QUIC: cabeçalho com 8 bytes do `packet.id` seguido das NALUs brutas (Annex-B).
  - Criação do overlay de visualização com botão explícito de cancelamento e suporte a `webcam.start`/`webcam.stop`.

### Fase 21: Correção do Pipeline da Webcam (Fps Dinâmico e Unificação de Use Cases)
* **Mensagem/Objetivo do Utilizador:** O vídeo chegava preto ou congelado ao PC devido a conflito de instâncias de câmera.
* **O que foi feito/trabalhado:**
  - Unificação do ciclo de vida: o `WebcamStreamer` passou a ser o único a controlar o CameraX, associando o Preview da tela e o encoder num único `bindToLifecycle`.
  - Suporte ao framerate alvo requisitado pelo daemon (24, 30, 60, 90 fps) usando `Camera2Interop.CONTROL_AE_TARGET_FPS_RANGE`.
  - Adição do botão "TROCAR CÂMARA" para alternar entre frontal e traseira em pleno streaming.

### Fase 22: Integração Reativa da Webcam no Dashboard Principal
* **Mensagem/Objetivo do Utilizador:** O botão Webcam na tela principal não refletia o estado da gravação.
* **O que foi feito/trabalhado:**
  - Atualização do botão na grelha: selo visual em ciano quando ativo, feedback em tempo real de "CÂMARA ATIVA" e capacidade de desligar a transmissão diretamente da tela inicial.

### Fase 23: Diagnósticos Persistentes em Disco (diagnostics.log) com Rotação de 5MB
* **Mensagem/Objetivo do Utilizador:** Ter acesso a logs completos mesmo após fechar o app, para auditoria de erros de rede.
* **O que foi feito/trabalhado:**
  - Gravação contínua no arquivo `diagnostics.log` no armazenamento privado.
  - Carregamento das últimas 1000 linhas ao reabrir o app.
  - Rotação automática cortando logs antigos quando o arquivo atinge 5MB.
  - Botão de exportação total e botão de limpeza total (lixeira).

### Fase 24: Reversão da Reconexão na Rotação da Câmara (Pillarbox no PC)
* **Mensagem/Objetivo do Utilizador:** O vídeo sofria interrupção ao girar o celular.
* **O que foi feito/trabalhado:**
  - Reversão do restart do codificador local: o PC passou a lidar com a rotação via GStreamer (adicionando barras pretas / pillarbox nas resoluções verticais).
  - O celular passou a apenas enviar o pacote de controle leve `webcam.transform { rotation, mirror }`, tornando a rotação instantânea em menos de 1 segundo.

### Fase 25: Alinhamento e Espelhamento Fixo das Câmaras Frontal e Traseira
* **Mensagem/Objetivo do Utilizador:** A câmera frontal aparecia deitada e a traseira rodada em 180° no Linux.
* **O que foi feito/trabalhado:**
  - Câmera frontal: configurada por padrão com rotação de 270° e espelhamento (`true`), chegando vertical no PC.
  - Câmera traseira: rotação de 90° no Preview local para manter conformidade exata com o que o computador recebe.

### Fase 26: Ecrã ÁUDIO Completo (Mixer PipeWire, Media Player e Audio Tap)
* **Mensagem/Objetivo do Utilizador:** Construir uma central completa de som: tocar o áudio do PC no celular e regular o volume das aplicações do computador.
* **O que foi feito/trabalhado:**
  - Controle de mídia MPRIS (título, artista, capa, play/pause/prev/next).
  - **Audio Tap de Baixa Latência**: envio de `audio.tap_start`, recepção de PCM cru (16-bit, 48kHz, Estéreo) via uni-stream QUIC tocado pelo `AudioTrack` em modo streaming.
  - Mixer de Sinks com controle de volume mestre (0-150%) e Mute.
  - Mixer por aplicação (Spotify, Chrome, Discord) com atualização reativa a cada 3 segundos via `audio.state`.

### Fase 27: Polimento da Webcam (Multi-Lentes, HEVC/H.265 e Microfone Virtual Sem Fios)
* **Mensagem/Objetivo do Utilizador:** Adicionar suporte a lentes ultra-wide/teleobjetiva, compressão moderna H.265 e uso do microfone do celular no PC.
* **O que foi feito/trabalhado:**
  - Comutador multi-lentes dinâmico analisando `availableCameraInfos`.
  - Suporte ao codec **HEVC/H.265** acelerado por hardware para economizar 40% de banda Wi-Fi, com fallback transparente para H.264.
  - **Microfone Sem Fios**: captura contínua de PCM 16-bit 48kHz mono via `AudioRecord` enviada em uni-stream QUIC dedicado para um microfone virtual no Linux.
  - Ativação de `keepScreenOn = true` para impedir o descanso de tela durante a transmissão de vídeo.

### Fase 28: Correção Estrutural de CBOR Recursivo (Listas e Mapas Aninhados)
* **Mensagem/Objetivo do Utilizador:** A tela de Áudio não exibia as aplicações ou as saídas de som do PC.
* **O que foi feito/trabalhado:**
  - O descodificador CBOR original tratava apenas tipos primitivos, convertendo arrays e dicionários aninhados em strings brutas.
  - Criação dos métodos recursivos `decodeCborValue` e `encodeCborValue` em `ConnectionRepository`, permitindo converter listas e mapas aninhados de forma dinâmica e bidirecional.

### Fase 29: Normalização de CBOR Indefinido (Chunked) e Remoção do Delay de 10s One-Way
* **Mensagem/Objetivo do Utilizador:** Os comandos de volume, mídia e Audio Tap demoravam exatamente 10 segundos para responder no PC.
* **O que foi feito/trabalhado:**
  - O parser CBOR do daemon Linux espera estruturas de comprimento indefinido (chunked). Configuração explícita de `.setChunked(true)` nos mapas e arrays.
  - O método `sendAnnouncedPacket` tentava ler EOF até o timeout de 10 segundos em pacotes unidirecionais. Modificado para retornar imediatamente quando `hasPayload == false`. Comandos de teclado, touchpad e som tornaram-se instantâneos.

### Fase 30: Persistência de Controles de Mídia Inativos e Resolução Dinâmica de Sinks
* **Mensagem/Objetivo do Utilizador:** A interface de mídia sumia se nenhum reprodutor estivesse aberto no PC, e o áudio falhava se a placa de som virtual tivesse outro nome.
* **O que foi feito/trabalhado:**
  - Implementação de estado de fallback na interface de áudio, mantendo os botões de reprodução operacionais mesmo sem metadados ativos.
  - Resolução dinâmica do sink do Audio Tap: busca por placas marcadas com `is_phone == true`, com fallback transparente para `"hyprlink-speaker"`.

### Fase 31: Documentação Integral e Implementações do Daemon Linux (Python e Rust)
* **Mensagem/Objetivo do Utilizador:** Recuperar e documentar o ecossistema completo do daemon no computador após perda de arquivos locais.
* **O que foi feito/trabalhado:**
  - Criação de `DOCUMENTACAO_COMPLETA_E_DAEMON.md` contendo todas as especificações técnicas de transporte QUIC, mTLS P-256, enquadramento de dados e tabela de pacotes CBOR.
  - Criação de `hyprlink_daemon.py` funcional na raiz do projeto.
  - Compilação dos requisitos e pipeline em Rust (`quinn`, `gstreamer`, `pipewire`, `v4l2loopback`) para reconstituição do servidor desktop de alta performance.
