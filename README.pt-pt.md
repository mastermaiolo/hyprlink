<!-- MAIOLO / SYSTEMS LAB — HYPRLINK -->

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="HyprLink — Integração de Ecossistema Android ⇄ Hyprland">
</p>

<p align="center"><sub><strong>ANDROID ⇄ LINUX/HYPRLAND · QUIC + mTLS · CBOR · CONTINUIDADE · UINPUT</strong></sub></p>

<p align="center">
  <a href="README.md">🇬🇧 English (UK)</a>
  · <a href="README.pt-br.md">🇧🇷 Português (BR)</a>
  · 🇵🇹 <strong>Português (PT)</strong>
  · <a href="README.es-es.md">🇪🇸 Español (ES)</a>
  · <a href="README.zh-cn.md">🇨🇳 简体中文</a>
  · <a href="CHANGELOG.md">CHANGELOG</a>
  · <a href="PROTOCOL.md">PROTOCOL</a>
</p>

> **Integração de ecossistema entre Android e Linux/Hyprland** — no espírito do Continuity/Handoff da Apple, mas concebido especificamente para quem usa Hyprland. Área de transferência, notificações, multimédia, bateria, touchpad e teclado remotos, transferência de ficheiros, webcam, áudio e controlo do compositor sincronizados em tempo real através de ligação local direta QUIC + mTLS, sem qualquer servidor na nuvem.

> [!NOTE]
> **Versão alfa (0.1.0).** Funciona no dia a dia para quem o desenvolve, mas o protocolo ainda pode mudar entre lançamentos e alguns módulos continuam em validação (consultar [03 / Matriz de Subsistemas](#03--matriz-de-subsistemas)). Relatórios de erros e contributos são bem-vindos.

<p align="center">
  <img src="assets/hyprlink_tour.gif" width="720" alt="Visita à app de secretária do HyprLink: capa, dispositivos, secretária, câmara, áudio, notificações, partilha, multimédia, sensores e diário">
</p>

---

## 01 / À PRIMEIRA VISTA

<p align="center">
  <img src="assets/readme/at-a-glance.svg" width="100%" alt="Perfil de sistema do HyprLink à primeira vista">
</p>

A ligação é estabelecida diretamente na tua rede local através de **QUIC + mTLS** (autenticação mútua por certificados autoassinados, com *fingerprint pinning*); o protocolo de controlo é serializado em **CBOR**. Sem servidores intermédios: o telemóvel comunica diretamente com o PC no porto **7443/UDP**.

---

## 02 / CAPACIDADES

<p align="center">
  <img src="assets/readme/capabilities.svg" width="100%" alt="Matriz de capacidades do HyprLink">
</p>

### O que acontece na prática?

- **Notificações**: alertas do telemóvel espelhados no ecrã com ações remotas, descarte e lista de ativas reposta quando o dispositivo se volta a ligar.
- **Área de transferência bidirecional**: sincronização imediata de texto UTF-8 e imagens PNG sem perdas.
- **Transferência de ficheiros**: fila de envios nos dois sentidos, acompanhamento em tempo real, histórico e pasta de transferências configurável.
- **Multimédia**: controlo dos leitores MPRIS do PC a partir do telemóvel (com progresso e capa do álbum); o leitor do PC aparece também como notificação no telemóvel.
- **Telemóvel como webcam**: câmara do telemóvel como dispositivo de vídeo virtual (`/dev/video42` via `v4l2loopback`) em H.264 ou H.265 por hardware.
- **Ponte de áudio**: microfone do telemóvel como entrada do PC, telemóvel como coluna do PC e som do PC enviado para o telemóvel.
- **Touchpad e teclado remotos**: integração com `/dev/uinput` com suporte para cursor, clique/arrasto, escrita em tempo real, Enter e atalhos, com soltura de segurança em caso de queda de ligação.
- **Ações rápidas**: bloquear sessão, suspender, captura de ecrã (área ou completo, copiado e guardado), volume e despachos arbitrários via `hyprctl dispatch`.
- **Hyprland IPC**: mudança de áreas de trabalho (*workspaces*), janelas ativas e escuta de eventos ao vivo; gestos do telemóvel mapeados para despachos (compatível com `hyprland.lua` e formato clássico).
- **Telemetria e Wake-on-LAN**: bateria nos dois sentidos; CPU, RAM, temperaturas (hwmon), espaço livre em disco e GPU do PC mostrados no telemóvel; transmissão de endereço MAC para ligar o PC à distância.
- **GUI de secretária**: aplicação dedicada reativa em `iced`, com ícone na bandeja do sistema e suporte a 5 idiomas (EN-GB, PT-BR, PT-PT, ES-ES, ZH-CN).

---

## 03 / MATRIZ DE SUBSISTEMAS

<p align="center">
  <img src="assets/readme/showcase.svg" width="100%" alt="Matriz de subsistemas integrados do HyprLink">
</p>

| Subsistema | Estado | Notas |
|---|---|---|
| **Conectividade** (QUIC/mTLS, emparelhamento por QR) | ✅ Pronto | Autenticação mútua de certificados em 7443/UDP |
| **GUI de secretária** (iced, separada do daemon, bandeja) | ✅ Pronto | Processo dedicado, ícone via ksni |
| **Área de transferência bidirecional** | ✅ Pronto | Texto UTF-8 e imagens PNG |
| **Hyprland IPC** (workspaces, janelas, dispatch) | ✅ Pronto | Integração com socket2 do compositor |
| **Bateria** (PC ⇄ Telemóvel) | ✅ Pronto | Telemetria bidirecional e alertas |
| **Multimédia** (MPRIS: controlo, a tocar, capa) | ✅ Pronto | Integração D-Bus via zbus |
| **Touchpad e teclado remotos** | ✅ Pronto | Emulação direta de dispositivo por /dev/uinput |
| **Ações rápidas** (bloquear, suspender, captura) | ✅ Pronto | Integração com hyprlock, noctalia, grim |
| **Notificações** (espelhar, ações, dispensar) | ✅ Pronto | Repõe notificações ativas ao religar |
| **Transferência de ficheiros** (fila, histórico, pasta) | ✅ Pronto | Fluxo binário unidirecional QUIC |
| **Telemetria no telemóvel** (CPU, RAM, temp, GPU, disco) | 🚧 Em teste | Estados de suspensão GPU NVIDIA/hwmon em validação |
| **Botões do rato e arrastar** (`input.button`) | 🚧 Em teste | Daemon e app Android prontos; teste em telemóvel pendente |
| **Gestos no telemóvel** (`gesture`) | 🚧 Em teste | Daemon, GUI e app prontos; teste em telemóvel pendente |
| **Ponte de áudio** (microfone, coluna, escuta) | 🚧 Em teste | Validação do pipeline PipeWire e GStreamer |
| **Webcam virtual** (H.264 / H.265 em /dev/video42) | 🚧 Em teste | Carregamento de v4l2loopback via pkexec |

O detalhe completo do protocolo está no [`PROTOCOL.md`](PROTOCOL.md) e o histórico de alterações no [`CHANGELOG.md`](CHANGELOG.md).

---

## 04 / INSTALAÇÃO

### Requisitos

Testado em **Arch Linux / CachyOS** com **Hyprland** (Wayland). Os pacotes indicados referem-se ao Arch Linux:

| Domínio | Pacotes necessários (Arch Linux) |
|---|---|
| **Base / Núcleo** | `hyprland`, `pipewire`, `wireplumber`, `libpulse` (`pactl`), `wl-clipboard` |
| **Áudio e Câmara (GStreamer)** | `gst-plugins-base-libs`, `gst-plugins-good`, `gst-plugins-bad-libs` (H.265), `gst-libav` (H.264/H.265), `gst-plugin-pipewire` |
| **Webcam Virtual** | `v4l2loopback-dkms` (carregado em `/dev/video42` através de `pkexec`) |
| **Seletor de ficheiros da GUI** | `xdg-desktop-portal` + `xdg-desktop-portal-gtk` |
| **Ações rápidas (Opcional)** | Bloqueio: `noctalia`, `hyprlock`, ou `loginctl`; Capturas: `grimblast` ou `grim` + `slurp` |
| **Compilação** | `rust`, `clang`, `lld`, `pkgconf` |

> [!IMPORTANT]
> O touchpad e teclado remotos escrevem diretamente em `/dev/uinput`. Certifica-te de que o teu utilizador tem permissão de escrita (verifica com `getfacl /dev/uinput`).

### Método rápido: Pacote binário pré-compilado (x86_64)

Transfere `hyprlink-0.1.0-linux-x86_64.tar.gz` dos [Lançamentos do GitHub](https://github.com/mastermaiolo/hyprlink/releases):

```bash
tar -xzf hyprlink-0.1.0-linux-x86_64.tar.gz
cd hyprlink-0.1.0-linux-x86_64
./install.sh                                     # Instala em ~/.local (não requer root)
systemctl --user enable --now hyprlink-bridge   # Ativa o serviço de utilizador
hyprlink-gui                                     # Abre a interface gráfica
```

O `./install.sh` suporta `--prefix DIR` (predefinição: `~/.local`), `--dry-run`, e verifica dependências de execução. Executa `./uninstall.sh` para desinstalar os ficheiros.

### Arch Linux (PKGBUILD)

Existe um PKGBUILD local fornecido em `packaging/arch/` sob o nome `hyprlink-bridge`:

```bash
git clone https://github.com/mastermaiolo/hyprlink.git
cd hyprlink/packaging/arch
makepkg -si
systemctl --user enable --now hyprlink-bridge
hyprlink-gui
```

Isto instala `hyprlink-daemon`, `hyprlink-gui` e `hyprlinkctl` em `/usr/bin`, regista o serviço `hyprlink-bridge.service` e cria os lançadores `.desktop`. Para testar a árvore de trabalho antes de existir uma etiqueta: `HYPRLINK_LOCAL=1 makepkg -si`.

<details>
<summary><strong>Arranque de sessão e integração com uwsm</strong></summary>

O serviço de fundo é direcionado a `graphical-session.target`.
- **Com `uwsm`**: o arranque é automático.
- **Sem `uwsm`**: assegura a exportação das variáveis de ambiente de sessão no `hyprland.conf`:
  ```ini
  exec-once = dbus-update-activation-environment --systemd --all
  exec-once = systemctl --user start hyprlink-bridge
  ```
- **Arranque automático da GUI na bandeja**:
  ```bash
  cp /usr/share/hyprlink-bridge/hyprlink-bridge-autostart.desktop ~/.config/autostart/
  ```
  Ou adiciona `exec-once = hyprlink-gui` no `hyprland.conf`.

</details>

---

## 05 / SUPERFÍCIE DE CONTROLO

<p align="center">
  <img src="assets/readme/control-surface.svg" width="100%" alt="Superfície de controlo do HyprLink: GUI e CLI">
</p>

### Emparelhar o telemóvel

1. Instala a app Android companheira a partir da página de [Lançamentos](https://github.com/mastermaiolo/hyprlink/releases) (ativa "Instalar aplicações desconhecidas" no telemóvel).
2. Com o daemon em execução, abre a GUI em **Dispositivos → Emparelhar novo** (ou visualiza o código QR no terminal se executaste o daemon interativamente).
3. Lê o código QR na app. O código transmite a impressão digital SHA-256 do certificado, o endereço `<IP-DO-PC>:7443` e um token de uso único.
4. A partir deste ponto, a ligação é restabelecida automaticamente sempre que ambos os dispositivos estiverem na mesma rede local.

> [!NOTE]
> Garante que o porto **7443/UDP** está acessível na firewall da rede local. A identidade criptográfica do PC reside em `~/.config/hyprlink/`; apagar esta pasta reverte todos os emparelhamentos.

### Navegação na GUI de secretária

A interface gráfica prioriza o telemóvel sobre os dados locais:
- Teclas `1` a `9`: Salto direto de página (Capa, Dispositivos, Secretária, Câmara e Ecrã, Áudio, Notificações, Partilha, Multimédia, Sensores e Presença, Diário).
- Tecla `0`: Abre as Definições (*Settings*).

### Linha de comandos (`hyprlinkctl`)

```bash
hyprlinkctl status             # Estado da ligação e detalhes do dispositivo
hyprlinkctl ping               # Verifica a latência de ida e volta ao telemóvel
hyprlinkctl watch --json       # Transmissão contínua de telemetria em linhas JSON
hyprlinkctl clipboard          # Envia a área de transferência do PC para o telemóvel
hyprlinkctl pair               # Abre janela de descoberta e emparelhamento de 120s
hyprlinkctl mirror toggle      # Alterna o espelhamento do ecrã do telemóvel
hyprlinkctl mic toggle         # Alterna microfone do telemóvel (on | off | toggle)
hyprlinkctl tap toggle         # Alterna envio de áudio do PC para o telemóvel
hyprlinkctl speaker toggle     # Alterna o telemóvel como coluna do PC
hyprlinkctl ws 3               # Muda para a área de trabalho 3 do Hyprland
hyprlinkctl open               # Abre ou foca a janela da aplicação gráfica
hyprlinkctl doctor             # Diagnóstico de ambiente e relatório de compatibilidade
```

Consulta todas as opções com `hyprlinkctl --help`. A pasta `contrib/` disponibiliza um módulo para a Waybar e uma ação de menu contextual para o gestor de ficheiros (instala com `contrib/install.sh`).

---

## 06 / CONFIGURAÇÃO

As definições são guardadas em `~/.config/hyprlink/config.json` e podem ser editadas diretamente na interface:

```json
{
  "download_dir": "~/Downloads",
  "track": {
    "sensitivity": 1.0,
    "scroll_speed": 1.0,
    "natural_scroll": true
  },
  "shortcuts": [],
  "gestures": [],
  "battery_alerts": {
    "low": 20,
    "full": 90
  },
  "disk_path": "/home/user",
  "gpu_nvidia_wake": false,
  "lang": "pt-PT"
}
```

* `download_dir`: Diretoria onde são guardados os ficheiros recebidos.
* `track`: Sensibilidade, deslocamento natural, aceleração e alternador do teclado.
* `shortcuts` e `gestures`: Associações entre gestos do telemóvel e comandos `hyprctl dispatch`.
* `battery_alerts`: Percentagens mínima e máxima para avisos de bateria.
* `disk_path`: Ponto de montagem para medição do disco (predefinição: `$HOME`).
* `gpu_nvidia_wake`: Se `false` (predefinição), impede que as leituras de telemetria acordem placas NVIDIA dedicadas em suspensão.
* `lang`: Idioma da interface (`en-GB`, `pt-BR`, `pt-PT`, `es-ES`, `zh-CN`).

Para testar a interface gráfica sem telemóvel ou daemon ativo:
```bash
HYPRLINK_MOCK=1 hyprlink-gui
```

---

## 07 / ARQUITETURA

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="Arquitetura do sistema HyprLink e árvore de módulos">
</p>

```text
├── crates/
│   ├── hyprlinkd/            Daemon em segundo plano (binário hyprlink-daemon, sem janela)
│   ├── hyprlink-gui/         GUI de secretária (iced, processo autónomo com ícone na bandeja)
│   ├── hyprlink-proto/       Contrato partilhado, tramas CBOR, tipos de rede, fmt.rs
│   └── hyprlinkctl/          Utilitário de linha de comandos para atalhos, Waybar e scripts
├── contrib/                  Módulo para Waybar, entradas .desktop para o gestor de ficheiros
├── packaging/arch/           PKGBUILD local para Arch Linux (pacote hyprlink-bridge)
├── scripts/                  hyprlink-start.sh / stop, gui-capture.sh, release.sh
├── CHANGELOG.md              Histórico de versões e notas de lançamento
└── PROTOCOL.md               Especificação formal do protocolo de rede
```

O daemon não tem janela; a GUI comunica com este através do socket UNIX local `$XDG_RUNTIME_DIR/hyprlink.sock`.

Na rede local, uma ligação QUIC no porto **7443/UDP** multiplexa:
- **Mensagens de controlo**: Fluxo bidirecional serializado em CBOR.
- **Dados em bloco**: Fluxos unidirecionais para ficheiros e imagens da área de transferência.
- **Transmissão de áudio**: Tramas QUIC DATAGRAM de baixa latência e tolerantes a perda.

**Tecnologias principais**: [`quinn`](https://github.com/quinn-rs/quinn) (QUIC), `rustls` (TLS), `ciborium` (CBOR), [`iced`](https://github.com/iced-rs/iced) (GUI), `ksni` (bandeja do sistema), `zbus` (D-Bus para MPRIS e notificações), `uinput` (emulação de periféricos no kernel), GStreamer / PipeWire (processamento multimédia).

---

## 08 / MODOS DE FALHA

<p align="center">
  <img src="assets/readme/failure-modes.svg" width="100%" alt="Modos de falha e matriz de recuperação do HyprLink">
</p>

<details>
<summary><strong>Procedimentos detalhados de resolução de problemas</strong></summary>

1. **O telemóvel não liga**:
   - Confirma se ambos os dispositivos estão na mesma sub-rede da rede local.
   - Verifica se a firewall permite tráfego no porto **7443/UDP**.
   - Inspeciona o estado do serviço: `systemctl --user status hyprlink-bridge`.
   - Remove o dispositivo na interface gráfica e repete o emparelhamento por QR.
2. **Touchpad ou teclado não respondem**:
   - Confirma as permissões de escrita em `/dev/uinput`: `getfacl /dev/uinput`.
   - Concede acesso ao utilizador via regras udev se necessário.
3. **Webcam virtual ausente**:
   - Certifica-te de que `v4l2loopback-dkms` está instalado.
   - O daemon invoca `pkexec` para configurar o nó `/dev/video42`.
4. **O serviço não arranca ao iniciar a sessão**:
   - Sem `uwsm`, verifica a ativação de `graphical-session.target`.
   - Insere `exec-once = dbus-update-activation-environment --systemd --all` no `hyprland.conf`.
5. **Comandos de bloqueio ou captura de ecrã sem efeito**:
   - Instala uma ferramenta de bloqueio (`noctalia`, `hyprlock`, `loginctl`).
   - Garante que os utilitários de captura (`grimblast` ou `grim` + `slurp`) estão disponíveis no `$PATH`.
6. **Reposição total de configurações**:
   - Para o serviço e apaga a diretoria de configuração: `rm -rf ~/.config/hyprlink/`.
   - Um novo par de chaves e base de dados limpa serão gerados na execução seguinte.

</details>

---

<details>
<summary><strong>Documentação complementar e compilação a partir do código</strong></summary>

### Compilar a partir do código-fonte

```bash
# Arranque automatizado (compila e corre daemon + GUI com os binários de target/release)
scripts/hyprlink-start.sh            # Usa --restart para reiniciar; hyprlink-stop.sh para parar

# Arranque manual em terminais separados
cargo run --release -p hyprlinkd     # Daemon (mostra o código QR no terminal)
cargo run --release -p hyprlink-gui  # Aplicação gráfica
```

O repositório inclui `.cargo/config.toml` pré-configurado para utilizar o vinculador acelerado **`clang` + `lld`** (`pacman -S clang lld`), reduzindo o tempo de vinculação do daemon de testes de ~5s para ~2s.

Perfis de compilação:
```bash
cargo build --profile fast -p hyprlinkd   # Para iteração: sem LTO, 16 codegen units
cargo build --release -p hyprlinkd        # Para distribuição: LTO ativo, overflow checks ativados
```

O perfil `fast` herda de `release` com `lto = false`, `codegen-units = 16` e `overflow-checks = false`; **não se destina a distribuição**.

### Aplicação móvel Android

A aplicação para Android (desenvolvida em Kotlin e Jetpack Compose) é gerida num espaço de trabalho autónomo. Os ficheiros APK compilados e assinados são disponibilizados na página de [Lançamentos](https://github.com/mastermaiolo/hyprlink/releases). Para quem desejar criar clientes alternativos, o protocolo integral está documentado em [`PROTOCOL.md`](PROTOCOL.md).

</details>

---

## 09 / PROVENIÊNCIA

<p align="center">
  <img src="assets/readme/provenance.svg" width="100%" alt="Proveniência e atribuições do HyprLink">
</p>

O desenvolvimento assenta sobre [`quinn`](https://github.com/quinn-rs/quinn), [`rustls`](https://github.com/rustls/rustls), [`iced`](https://github.com/iced-rs/iced), `ciborium`, `ksni` e `zbus`.

Os tipos de letra incluídos na aplicação gráfica e móvel — Anton, Instrument Serif, Inter, IBM Plex Mono e Noto Sans SC — estão sob a licença SIL Open Font License 1.1; as declarações encontram-se em `crates/hyprlink-gui/assets/fonts/OFL-*.txt`.

---

## 10 / LICENÇA

**GPL-3.0-or-later** — ver [`LICENSE`](LICENSE).

**Nota de relicenciamento**: Até à versão 0.1.0, o código foi publicado sob a licença MIT. Sendo o projeto de autor único, a partir da versão 0.1.0 adota a licença GNU GPL v3.0 ou posterior. Quem obteve versões anteriores mantém os direitos ao abrigo dos termos da licença MIT.

---

<p align="center"><sub>MAIOLO / SYSTEMS LAB · HYPRLINK · HL / 01 · 食</sub></p>
