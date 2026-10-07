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
- **Atalhos da GUI no telemóvel**: `shortcuts.list` (só `id` e rótulo, nunca o
  comando) ao ligar e a cada alteração; `shortcut.run` com a identificação do
  atalho, que o daemon executa se existir na config (máx. 5 por segundo por
  ligação; o Diário leva só o rótulo). Os atalhos ganham um `id` estável (config
  antiga migra sozinha).
- **Responder a notificações** a partir do PC: a página Notificações mostra um
  campo de texto e «Responder» (Enter envia) nas notificações com ação de
  resposta; o daemon valida e envia `notification.reply`.
- **Câmara com formato sincronizado**: `webcam.state` e `webcam.configure`
  sincronizam resolução, FPS e codec entre o telemóvel e o PC; arranque a frio
  protegido e câmara pedida pelo telemóvel (`webcam.request`).
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

### Compatibilidade de ambiente (hyprlink-env)
- **`hyprlinkctl doctor`** (`--json`, `--report [ficheiro]`): relatório só de
  leitura do que o daemon detetou (Hyprland e sintaxe do `dispatch`, shell,
  CPU/GPU/temperatura, baterias, áudio, câmara virtual, ferramentas) e do que
  vai fazer; sem nome de utilizador, máquina, MAC, IP, SSID nem pasta pessoal.
- **EasyEffects e `tap_source`**: deteção automática do grafo PipeWire e do
  `easyeffects_sink`; escolha configurável entre som antes dos efeitos
  (`easyeffects_pre`), depois dos efeitos (`easyeffects_post`) ou da saída
  predefinida (`default`).
- **Opções de correção no `config.json`**: `lock_command`, `screenshot_tool`,
  `temp_sensor`, `gpu_source`, `v4l2_device_nr`, `audio_backend`,
  `hypr_dispatch_mode`, `tap_source`; valores em uso nas Definições da GUI.
- `hyprctl dispatch`: o modo (clássico ou Lua) deteta-se uma vez; mapeamento
  clássico→Lua completado e corrigido (`closewindow`, `fullscreen`, setas); o
  prefixo `lua:` nos atalhos envia uma expressão Lua direta.
- Bloqueio pela shell detetada (Noctalia v5, Ryoku, Caelestia) antes do
  `hyprlock`; volume por `pactl` quando não há `wpctl`; baterias de periféricos
  ignoradas; GPU Intel opcional (`intel_gpu_top`, só `i915`); mensagens claras
  para o v4l2loopback e para o PulseAudio.

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
