# Changelog

Todas as alterações relevantes deste projeto ficam aqui. O formato segue o
[Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e as versões o
[Semantic Versioning](https://semver.org/lang/pt-BR/).

## [0.1.0] — por publicar

Primeira versão instalável (alfa). Daemon, GUI e linha de comandos do PC;
a app Android tem versões próprias (`versionName`).

### Ligação
- QUIC (RFC 9000) sobre UDP, porta única 7443, com **mTLS 1.3**: certificados
  ECDSA P-256 autoassinados dos dois lados, validação por impressão digital
  SHA-256 e lista de aparelhos emparelhados.
- Emparelhamento por QR (impressão digital, endereço e token renovado a cada
  arranque e após cada emparelhamento); reconexão automática.
- Protocolo de controlo em CBOR; som do PC para o telemóvel por QUIC DATAGRAM
  (RFC 9221). Especificação completa em `PROTOCOL.md`.

### Funcionalidades
- **Notificações** do telemóvel no PC: espelhar, ações, dispensar e lista de
  ativas reposta ao religar.
- **Clipboard** bidirecional, incluindo imagens PNG.
- **Ficheiros** nos dois sentidos: fila de envios (um de cada vez), progresso,
  histórico e pasta de transferências configurável.
- **Multimédia**: leitores MPRIS do PC controlados do telemóvel, com posição
  e capa do álbum; leitor do PC na notificação do telemóvel.
- **Webcam**: câmara do telemóvel como `/dev/video42` (v4l2loopback), em
  H.264 ou H.265.
- **Áudio**: microfone do telemóvel como entrada do PC (PCM 48 kHz), telemóvel
  como coluna do PC e retorno do som do PC.
- **Touchpad e teclado remotos** por `/dev/uinput`: escrita ao vivo, Enter,
  teclas de atalho, botões do rato (premir/largar, com soltura de segurança).
- **Ações rápidas**: bloquear, suspender, captura de ecrã ou de área, volume,
  multimédia e `hyprctl dispatch`.
- **Hyprland**: workspaces, janelas e eventos ao vivo; gestos do telemóvel
  mapeados para despachos.
- **Telemetria**: bateria nos dois sentidos; CPU, RAM, temperatura (hwmon),
  disco livre e GPU do PC no telemóvel.
- Wake-on-LAN: o daemon envia o MAC do PC para o telemóvel o poder acordar.

### Paridade com a app Android
- **Responder a notificações** a partir do PC: a página Notificações mostra um
  campo de texto e «Responder» (Enter envia) nas notificações com ação de
  resposta; o daemon valida e envia `notification.reply`.
- **Câmara pedida pelo telemóvel** (`webcam.request` → `webcam.request_result`):
  o daemon arranca a câmara pelo mesmo caminho da GUI e recusa se já estiver
  ativa.
- **Bateria do PC** no telemóvel com `charging` (a carregar de facto) e
  `plugged` (fio ligado) separados, e `present:false` sem bateria; empurrada
  quando muda e de 5 em 5 minutos, só com o telemóvel ligado.
- **Atalhos editáveis** na página Secretária (adicionar, editar, remover) e
  **Modo auricular** (coluna + microfone) na página Áudio; o daemon valida
  os atalhos (vazios, 40/200 caracteres, máximo de 32).
- **Abrir no telemóvel pela GUI** (página Partilha): URL (só http/https) com
  «Abrir no telemóvel» e nome de pacote com «Abrir app».
- **Controlos por implementar** (espelhar ecrã, ponte de sensores, regras de
  presença) ficam desativados na GUI com a nota «em breve», em vez de darem
  erro ao carregar.

### PC
- `hyprlink-daemon` sem janela; a **GUI** (`hyprlink-gui`, iced) é um processo
  à parte que fala com o daemon pelo socket `$XDG_RUNTIME_DIR/hyprlink.sock`,
  com ícone na bandeja (StatusNotifierItem).
- GUI em 5 idiomas: pt-PT, pt-BR, inglês, espanhol e chinês (Noto Sans SC
  embutido).
- `hyprlinkctl` para scripts e atalhos (texto ou JSON v1 com `--json`);
  widget de Waybar e atalho `.desktop` de envio em `contrib/`.

### Problemas conhecidos
- Alfa: o protocolo pode mudar entre versões.
- Gestos e botões do rato estão prontos no daemon, na GUI e na app Android,
  mas ainda por testar num telemóvel.
- Temperatura e GPU na telemetria ainda por validar em mais hardware.
- Espelho do ecrã do telemóvel no PC e bloqueio por presença (afastamento do
  telemóvel): propostos, ainda não compilados no daemon.

### Licença
- Relicenciado de MIT para **GPL-3.0-or-later** (autor único). As versões
  anteriores continuam MIT para quem as recebeu.
