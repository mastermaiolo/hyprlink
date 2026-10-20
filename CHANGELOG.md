# Changelog

Todas as alterações relevantes deste projeto ficam aqui. O formato segue o
[Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e as versões o
[Semantic Versioning](https://semver.org/lang/pt-BR/).

## [0.1.1] — 2026-10-DD

Versão de correções, sobretudo de segurança. **Atualização recomendada.** A app
Android 0.1.0 continua a funcionar com esta versão. A app 0.1.1 tem outro
identificador (`io.maggio.hyprlink`): instala-se ao lado da antiga, é preciso
emparelhar de novo e a antiga desinstala-se à mão.

### Segurança
- O token e o QR de emparelhamento só aparecem quando o daemon corre num
  terminal; nos registos do serviço ficam só o IP e a impressão digital.
- Abrir um link vindo do telemóvel (ou `hyprlinkctl url`) só aceita
  endereços `http://` e `https://`.
- Receber ficheiros tem um tamanho máximo e confirma o espaço livre antes de
  aceitar; o número de anúncios pendentes é limitado.
- Os ficheiros recebidos são escritos primeiro num `.hyprlink-part` escondido
  e só ganham o nome final quando chegam completos; uma transferência cortada
  ou mais curta do que o anunciado conta como falha e é apagada.
- Um ficheiro recebido nunca substitui outro com o mesmo nome: fica como
  `nome (1).ext`, `nome (2).ext`, etc. Partes órfãs com mais de 1 h são
  limpas ao arrancar.
- Ligações ainda por autenticar têm limites: apresentação até 64 KiB e 10 s,
  um só canal, no máximo 8 em simultâneo (2 por IP).
- Um IP que erra o token 5 vezes fica bloqueado 60 s, com o tempo a dobrar a
  cada bloqueio até 1 h; os aparelhos já emparelhados continuam a entrar.
- Uma ligação desconhecida recusada já não derruba a sessão do telemóvel
  emparelhado.
- Arrancar o daemon já não envia comandos ao Hyprland; só uma ação pedida
  (telemóvel, GUI ou `hyprlinkctl`) o faz.

### Corrigido
- Depois de reiniciar o Hyprland, o daemon volta a encontrar a instância nova
  e os eventos (workspaces, janelas) religam sozinhos.
- O daemon, a GUI e o `hyprlinkctl doctor` encontram o Hyprland mesmo quando
  arrancam sem as variáveis da sessão gráfica (por exemplo, pelo
  `systemd --user`).
- As respostas longas do Hyprland são lidas até ao fim.
- Copiar para a área de transferência, abrir ficheiros e correr ações a
  partir do telemóvel funcionam também com o daemon a correr como serviço.
- «Enviar via HyprLink» e `hyprlinkctl send` aceitam caminhos com espaços,
  URIs `file://` e caminhos relativos.
- `hyprlink-daemon --help` já não arranca o daemon; um argumento desconhecido
  dá erro, e um segundo daemon recusa arrancar quando já há um a correr.
- Área de transferência do PC para o telemóvel: depois de o telemóvel voltar
  a ligar, o daemon já não perde a ligação nova (deixava de lhe enviar
  texto, imagens e notificações); a última cópia feita sem telemóvel segue
  quando ele liga (até 60 s); a primeira captura de ecrã depois do arranque
  já não é ignorada.
- Câmara: a imagem já não sai de lado (a rotação que chegava antes do vídeo
  perdia-se); com o telemóvel ao alto a câmara virtual fica ao alto (ex.
  720×1280) e nunca esticada; mudar a resolução com um programa a usar a
  câmara já não derruba o vídeo (o Diário pede para a reabrir no programa);
  mudar de codec, resolução ou lente não perde a sessão.
- Câmara, saída PipeWire (**experimental**): o nó passa a oferecer um formato
  fixo (I420, depois NV12/YUY2 se um programa o recusar) com frame rate fixo;
  sem programa a ler, ou com um que falha, a sessão já não bloqueia (antes o
  vídeo parava ao fim de ~200 KB); os erros do `pipewiresink` aparecem no
  Diário e o nó usa o nome mostrado do telemóvel.

### Alterado
- `hyprlinkctl --json` fala com o daemon real; o simulador só se usa com
  `--mock` ou `HYPRLINK_MOCK=1`.
- A GUI adapta-se a janelas estreitas (tiling a meio ecrã do Hyprland), com
  uma barra lateral compacta.
- A fonte chinesa da GUI leva só os caracteres usados na interface (binário
  cerca de 3 MB mais pequeno).
- A GUI escolhe a classe de resolução da câmara pelo lado menor: no vertical,
  720p.

### Adicionado
- Ligação por cabo USB (lado do PC): com a ancoragem USB do telemóvel ligada, o
  `core.hello` passa a levar `pc_addrs` (IPs do PC por Wi-Fi/cabo/USB) e o
  daemon empurra `pc.addrs` quando mudam; a GUI e o `hyprlinkctl status`
  mostram por onde vai a ligação (chip USB / WI-FI); o `doctor` avisa se a
  internet do PC passa pelo telemóvel ou se a firewall fecha a porta, e
  `hyprlinkctl usb setup` aplica o `never-default` (só se o pedires). Precisa
  da app Android 0.1.1; a 0.1.0 continua a ligar-se por Wi-Fi.
- `hyprlinkctl doctor --no-dispatch` (ou `HYPRLINK_NO_DISPATCH=1`): diagnóstico
  sem enviar comandos ao Hyprland; o modo (Lua ou clássico) é inferido pela
  versão e pela configuração.
- `hyprlinkctl status --waybar`: estado em JSON para um módulo `custom` da
  Waybar.
- «Onde está o telemóvel?»: nas ações rápidas da GUI e
  `hyprlinkctl find [sound|vibrate|torch|all] [--secs N]` / `find stop`; o
  telemóvel toca, vibra e/ou acende a lanterna. Precisa de uma app com
  suporte; a app 0.1.0 não responde e a GUI diz isso.
- `hyprlink-daemon --version`.
- Transferências: velocidade e tempo restante na GUI («12,4 MB/s · faltam
  2 min 10 s») e duração e média no histórico; campo `transfer` no
  `hyprlinkctl status --json` / `watch --json` (aditivo, continua v1).
- O `watch --json` (e o `status --json`) traz o estado do «Onde está o
  telemóvel?» (`find_phone`) e as versões do daemon e do `hyprlinkctl`
  (`daemon_version`, `ctl_version`), para os plugins detetarem um binário
  antigo (aditivo, continua v1).
- Câmara: a GUI diz quando o telemóvel não suporta H.265 e usa H.264.
- Câmara: descodificação H.264/H.265 por VA-API (GPU), com recuo para o CPU
  (`webcam_decoder` no `config.json`: `auto`, `gpu` ou `cpu`). Para a GPU são
  precisos o plugin `gst-plugin-va` e o driver (mesa em AMD, intel-media-driver
  em Intel).
- Câmara: saída PipeWire sem v4l2loopback (`webcam_output` no `config.json`:
  `v4l2loopback`, o valor por omissão; `pipewire`; ou `auto`, que usa o
  v4l2loopback se o módulo estiver disponível e senão o PipeWire). É
  **experimental**.
- Apoiar o projeto: [Ko-fi](https://ko-fi.com/mastermaggio).

## [0.1.0] — 2026-10-07

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
