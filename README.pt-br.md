<!-- MAIOLO / SYSTEMS LAB — HYPRLINK -->

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="HyprLink — Integração de Ecossistema Android ⇄ Hyprland">
</p>

<p align="center"><sub><strong>ANDROID ⇄ LINUX/HYPRLAND · QUIC + mTLS · CBOR · CONTINUIDADE · UINPUT</strong></sub></p>

<p align="center">
  <a href="README.md">🇬🇧 English (UK)</a>
  · 🇧🇷 <strong>Português (BR)</strong>
  · <a href="README.pt-pt.md">🇵🇹 Português (PT)</a>
  · <a href="README.es-es.md">🇪🇸 Español (ES)</a>
  · <a href="README.zh-cn.md">🇨🇳 简体中文</a>
  · <a href="CHANGELOG.md">CHANGELOG</a>
  · <a href="PROTOCOL.md">PROTOCOL</a>
</p>

> **Integração de ecossistema entre Android e Linux/Hyprland** — no espírito do Continuity/Handoff da Apple, projetado especificamente para o Hyprland. Área de transferência, notificações, mídia, bateria, touchpad e teclado remotos, transferência de arquivos, webcam, áudio e controle do compositor sincronizados em tempo real via conexão local direta QUIC + mTLS, sem depender de nenhum serviço na nuvem.

> [!NOTE]
> **Versão alfa (0.1.0).** Funciona no dia a dia para quem desenvolve o projeto, mas o protocolo ainda pode mudar entre versões e alguns módulos continuam em validação (veja [03 / Matriz de Subsistemas](#03--matriz-de-subsistemas)). Relatórios de erros e contribuições são muito bem-vindos.

<p align="center">
  <img src="assets/hyprlink_tour.gif" width="720" alt="Tour pelo aplicativo desktop do HyprLink: capa, dispositivos, mesa, câmera, áudio, notificações, compartilhamento, mídia, sensores e diário">
</p>

---

## 01 / VISÃO GERAL

<p align="center">
  <img src="assets/readme/at-a-glance.svg" width="100%" alt="Perfil de sistema do HyprLink em resumo">
</p>

A conexão é direta na sua rede local via **QUIC + mTLS** (autenticação mútua com certificados autoassinados e *fingerprint pinning*); o protocolo de controle é serializado em **CBOR**. Sem servidores intermediários: o celular conversa direto com o computador na porta **7443/UDP**.

---

## 02 / RECURSOS

<p align="center">
  <img src="assets/readme/capabilities.svg" width="100%" alt="Matriz de recursos do HyprLink">
</p>

### O que o sistema faz na prática?

- **Notificações**: notificações do celular exibidas no desktop com ações remotas, descarte e restauração da lista de alertas ativos ao reconectar.
- **Área de transferência bidirecional**: sincronização rápida de texto UTF-8 e imagens PNG sem compressão.
- **Transferência de arquivos**: fila de envios em ambos os sentidos, progresso em tempo real, histórico e pasta de downloads configurável.
- **Controle de mídia**: controle os players MPRIS do PC diretamente do celular (com posição da faixa e capa do álbum); o player do PC também aparece como notificação no celular.
- **Celular como webcam**: use a câmera do celular como webcam virtual (`/dev/video42` via `v4l2loopback`) em H.264 ou H.265 acelerado por hardware.
- **Ponte de áudio**: microfone do celular como entrada de áudio do PC, celular como alto-falante do PC e transmissão do som do PC para o celular.
- **Touchpad e teclado remotos**: integração com `/dev/uinput` com suporte a movimento de cursor, cliques, gestos, digitação ao vivo, Enter e atalhos, com liberação de segurança em caso de queda de sinal.
- **Ações rápidas**: bloquear tela, suspender computador, captura de tela (área ou tela cheia, copiada para a área de transferência e salva), volume e comandos arbitrários via `hyprctl dispatch`.
- **Hyprland IPC**: alternância de áreas de trabalho (*workspaces*), janelas ativas e monitoramento de eventos ao vivo; gestos do celular mapeados para dispatches (compatível com `hyprland.lua` e formato clássico).
- **Telemetria e Wake-on-LAN**: nível de bateria em ambas as direções; CPU, memória RAM, temperaturas (hwmon), espaço livre em disco e GPU do PC exibidos no celular; transmissão de MAC para ligar o PC remotamente.
- **Interface desktop**: aplicativo reativo independente em `iced`, com ícone na bandeja do sistema e suporte a 5 idiomas (EN-GB, PT-BR, PT-PT, ES-ES, ZH-CN).

---

## 03 / MATRIZ DE SUBSISTEMAS

<p align="center">
  <img src="assets/readme/showcase.svg" width="100%" alt="Matriz de subsistemas integrados do HyprLink">
</p>

| Subsistema | Estado | Observações |
|---|---|---|
| **Conectividade** (QUIC/mTLS, pareamento QR) | ✅ Pronto | Autenticação mútua de certificados na porta 7443/UDP |
| **Interface desktop** (iced, processo separado, bandeja) | ✅ Pronto | Processo dedicado, ícone de bandeja via ksni |
| **Área de transferência bidirecional** | ✅ Pronto | Texto UTF-8 e imagens PNG |
| **Hyprland IPC** (workspaces, janelas, dispatch) | ✅ Pronto | Integração com socket2 do compositor |
| **Bateria** (PC ⇄ Celular) | ✅ Pronto | Telemetria bidirecional e alertas |
| **Mídia** (MPRIS: controle, tocando agora, capa) | ✅ Pronto | Integração D-Bus via zbus |
| **Touchpad e teclado remotos** | ✅ Pronto | Emulação direta de dispositivo via /dev/uinput |
| **Ações rápidas** (bloqueio, suspensão, screenshot) | ✅ Pronto | Integração com hyprlock, noctalia, grim |
| **Notificações** (espelhar, ações, dispensar) | ✅ Pronto | Restaura alertas ativos na reconexão |
| **Transferência de arquivos** (fila, histórico, pasta) | ✅ Pronto | Fluxo binário unidirecional QUIC |
| **Telemetria no celular** (CPU, RAM, temp, GPU, disco) | 🚧 Em testes | Estados de suspensão GPU NVIDIA/hwmon em validação |
| **Botões do mouse e arrastar** (`input.button`) | 🚧 Em testes | Daemon e app Android prontos; testes no celular pendentes |
| **Gestos no celular** (`gesture`) | 🚧 Em testes | Daemon, GUI e app prontos; testes no celular pendentes |
| **Ponte de áudio** (microfone, alto-falante, streaming) | 🚧 Em testes | Validação de pipeline PipeWire e GStreamer |
| **Webcam virtual** (H.264 / H.265 em /dev/video42) | 🚧 Em testes | Carregamento do v4l2loopback via pkexec |

Mais detalhes do protocolo estão disponíveis no [`PROTOCOL.md`](PROTOCOL.md) e o histórico de alterações no [`CHANGELOG.md`](CHANGELOG.md).

---

## 04 / INSTALAÇÃO

### Requisitos

Testado no **Arch Linux / CachyOS** com **Hyprland** (Wayland). Os nomes dos pacotes abaixo referem-se ao repositório do Arch Linux:

| Domínio | Pacotes necessários (Arch Linux) |
|---|---|
| **Base / Núcleo** | `hyprland`, `pipewire`, `wireplumber`, `libpulse` (`pactl`), `wl-clipboard` |
| **Áudio e Câmera (GStreamer)** | `gst-plugins-base-libs`, `gst-plugins-good`, `gst-plugins-bad-libs` (H.265), `gst-libav` (H.264/H.265), `gst-plugin-pipewire` |
| **Webcam Virtual** | `v4l2loopback-dkms` (carregado em `/dev/video42` via `pkexec`) |
| **Seletor de arquivos da GUI** | `xdg-desktop-portal` + `xdg-desktop-portal-gtk` |
| **Ações rápidas (Opcional)** | Bloqueio: `noctalia`, `hyprlock`, ou `loginctl`; Captura: `grimblast` ou `grim` + `slurp` |
| **Compilação** | `rust`, `clang`, `lld`, `pkgconf` |

> [!IMPORTANT]
> O touchpad e teclado remotos escrevem direto em `/dev/uinput`. Certifique-se de que seu usuário possui permissão de escrita (verifique com `getfacl /dev/uinput`).

### Método rápido: Pacote binário pré-compilado (x86_64)

Baixe `hyprlink-0.1.0-linux-x86_64.tar.gz` da página de [Releases no GitHub](https://github.com/mastermaiolo/hyprlink/releases):

```bash
tar -xzf hyprlink-0.1.0-linux-x86_64.tar.gz
cd hyprlink-0.1.0-linux-x86_64
./install.sh                                     # Instala em ~/.local (não requer root)
systemctl --user enable --now hyprlink-bridge   # Ativa o serviço do daemon
hyprlink-gui                                     # Abre a interface gráfica
```

O `./install.sh` aceita `--prefix DIR` (padrão: `~/.local`), `--dry-run` e verifica dependências em tempo de execução. Use `./uninstall.sh` para remover os arquivos instalados.

### Arch Linux (PKGBUILD)

Um PKGBUILD local está disponível em `packaging/arch/` com o nome `hyprlink-bridge`:

```bash
git clone https://github.com/mastermaiolo/hyprlink.git
cd hyprlink/packaging/arch
makepkg -si
systemctl --user enable --now hyprlink-bridge
hyprlink-gui
```

Isso instala `hyprlink-daemon`, `hyprlink-gui` e `hyprlinkctl` em `/usr/bin`, registra o serviço `hyprlink-bridge.service` e cria lançadores `.desktop`. Para testar o código local antes de uma tag: `HYPRLINK_LOCAL=1 makepkg -si`.

<details>
<summary><strong>Inicialização da sessão e integração com o uwsm</strong></summary>

O serviço do daemon é associado ao `graphical-session.target`.
- **Com `uwsm`**: a inicialização ocorre de forma automática.
- **Sem `uwsm`**: adicione ao seu `hyprland.conf` para importar as variáveis de ambiente da sessão:
  ```ini
  exec-once = dbus-update-activation-environment --systemd --all
  exec-once = systemctl --user start hyprlink-bridge
  ```
- **Inicializar a GUI na bandeja ao fazer login**:
  ```bash
  cp /usr/share/hyprlink-bridge/hyprlink-bridge-autostart.desktop ~/.config/autostart/
  ```
  Ou adicione `exec-once = hyprlink-gui` ao `hyprland.conf`.

</details>

---

## 05 / SUPERFÍCIE DE CONTROLE

<p align="center">
  <img src="assets/readme/control-surface.svg" width="100%" alt="Superfície de controle do HyprLink: GUI e CLI">
</p>

### Pareando o celular

1. Instale o APK do app Android disponível na página de [Releases](https://github.com/mastermaiolo/hyprlink/releases) (permita a instalação de fontes desconhecidas no celular).
2. Com o daemon em execução, abra a GUI em **Dispositivos → Parear novo** (ou visualize o QR code no terminal se executou o daemon manualmente).
3. Escaneie o QR code no aplicativo. Ele contém a impressão digital SHA-256 do certificado, o endereço `<IP-DO-PC>:7443` e um token de uso único.
4. O link se restabelece de forma automática sempre que os dois aparelhos estiverem na mesma rede local.

> [!NOTE]
> Verifique se a porta **7443/UDP** está liberada no firewall para a rede local. As chaves criptográficas do computador ficam em `~/.config/hyprlink/`; apagar essa pasta reinicia todos os pareamentos.

### Navegação na interface gráfica

O app desktop foca primeiro no celular:
- Teclas `1` a `9`: Salto direto de tela (Capa, Dispositivos, Mesa, Câmera e Tela, Áudio, Notificações, Compartilhamento, Multimídia, Sensores e Presença, Diário).
- Tecla `0`: Abre as Configurações (*Settings*).

### Linha de comando (`hyprlinkctl`)

```bash
hyprlinkctl status             # Estado da conexão e detalhes do dispositivo
hyprlinkctl ping               # Mede latência de ida e volta até o celular
hyprlinkctl watch --json       # Transmissão contínua de telemetria em JSON
hyprlinkctl clipboard          # Envia a área de transferência do PC para o celular
hyprlinkctl pair               # Abre janela de pareamento de 120 segundos
hyprlinkctl mirror toggle      # Alterna o espelhamento da tela do celular
hyprlinkctl mic toggle         # Alterna microfone do celular (on | off | toggle)
hyprlinkctl tap toggle         # Alterna envio de áudio do PC para o celular
hyprlinkctl speaker toggle     # Alterna celular como alto-falante do PC
hyprlinkctl ws 3               # Alterna para o workspace 3 do Hyprland
hyprlinkctl open               # Abre ou foca a janela da aplicação gráfica
hyprlinkctl doctor             # Diagnóstico de ambiente e relatório de compatibilidade
```

Use `hyprlinkctl --help` para ver mais opções. A pasta `contrib/` oferece um widget para Waybar e uma ação para o menu de contexto do gerenciador de arquivos (instale via `contrib/install.sh`).

---

## 06 / CONFIGURAÇÃO

As opções ficam armazenadas em `~/.config/hyprlink/config.json` e podem ser alteradas direto na interface:

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
  "lang": "pt-BR"
}
```

* `download_dir`: Pasta onde os arquivos enviados pelo celular são salvos.
* `track`: Sensibilidade do touchpad, rolagem natural, aceleração e alternador do teclado.
* `shortcuts` e `gestures`: Comandos associados aos gestos no celular via `hyprctl dispatch`.
* `battery_alerts`: Limites de porcentagem para alertas de bateria baixa e carregada.
* `disk_path`: Caminho inspecionado para medir o espaço em disco (padrão: `$HOME`).
* `gpu_nvidia_wake`: Quando `false` (padrão), evita que as leituras de telemetria acordem placas NVIDIA dedicadas em suspensão.
* `lang`: Idioma da interface (`en-GB`, `pt-BR`, `pt-PT`, `es-ES`, `zh-CN`).

Para abrir a interface em modo de demonstração (sem daemon e sem celular):
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
│   ├── hyprlink-gui/         Interface desktop (iced, processo independente com ícone na bandeja)
│   ├── hyprlink-proto/       Contrato compartilhado, enquadramento CBOR, tipos de rede, fmt.rs
│   └── hyprlinkctl/          Utilitário de linha de comando para atalhos, Waybar e scripts
├── contrib/                  Widget para Waybar, entradas .desktop para gerenciador de arquivos
├── packaging/arch/           PKGBUILD local para Arch Linux (pacote hyprlink-bridge)
├── scripts/                  hyprlink-start.sh / stop, gui-capture.sh, release.sh
├── CHANGELOG.md              Histórico de versões e notas de atualização
└── PROTOCOL.md               Especificação formal do protocolo de rede
```

O daemon não possui janela; a interface gráfica se comunica com ele por meio de um socket UNIX local em `$XDG_RUNTIME_DIR/hyprlink.sock`.

Na rede local, uma única conexão QUIC na porta **7443/UDP** multiplexa:
- **Mensagens de controle**: Fluxo bidirecional serializado em CBOR.
- **Dados em massa**: Fluxos unidirecionais para arquivos e imagens da área de transferência.
- **Transmissão de áudio**: Quadros QUIC DATAGRAM de baixa latência e alta tolerância a perdas.

**Tecnologias centrais**: [`quinn`](https://github.com/quinn-rs/quinn) (QUIC), `rustls` (TLS), `ciborium` (CBOR), [`iced`](https://github.com/iced-rs/iced) (interface desktop), `ksni` (bandeja do sistema), `zbus` (D-Bus para MPRIS e notificações), `uinput` (emulação de periféricos no kernel), GStreamer / PipeWire (processamento multimídia).

---

## 08 / MODOS DE FALHA

<p align="center">
  <img src="assets/readme/failure-modes.svg" width="100%" alt="Modos de falha e matriz de recuperação do HyprLink">
</p>

<details>
<summary><strong>Procedimentos detalhados para solução de problemas</strong></summary>

1. **O celular não consegue se conectar**:
   - Verifique se os dois aparelhos estão exatamente na mesma sub-rede da rede local.
   - Certifique-se de que a porta **7443/UDP** está liberada no firewall.
   - Veja os logs do serviço: `systemctl --user status hyprlink-bridge`.
   - Remova o dispositivo na interface gráfica e refaça o pareamento via QR code.
2. **Touchpad ou teclado não respondem**:
   - Verifique as permissões de escrita em `/dev/uinput`: `getfacl /dev/uinput`.
   - Adicione regras udev para conceder acesso ao usuário caso necessário.
3. **Webcam virtual não aparece**:
   - Instale o pacote `v4l2loopback-dkms`.
   - O daemon solicita permissão de administrador (`pkexec`) para inicializar `/dev/video42`.
4. **O serviço não inicia ao fazer login**:
   - Se não estiver usando `uwsm`, verifique a inicialização do `graphical-session.target`.
   - Adicione `exec-once = dbus-update-activation-environment --systemd --all` no `hyprland.conf`.
5. **Comandos de bloqueio de tela ou screenshot falham**:
   - Instale utilitários de bloqueio (`noctalia`, `hyprlock`, `loginctl`).
   - Garanta que ferramentas de captura (`grimblast` ou `grim` + `slurp`) estão no `$PATH`.
6. **Restauração completa de configurações**:
   - Pare o serviço e apague a pasta de configurações: `rm -rf ~/.config/hyprlink/`.
   - Um novo par de chaves e banco de dados serão criados na inicialização seguinte.

</details>

---

<details>
<summary><strong>Informações complementares e compilação a partir do código-fonte</strong></summary>

### Compilando do código-fonte

```bash
# Inicialização automatizada (compila e executa daemon + GUI usando binários de target/release)
scripts/hyprlink-start.sh            # Use --restart para reiniciar; hyprlink-stop.sh para parar

# Inicialização manual em dois terminais separados
cargo run --release -p hyprlinkd     # Daemon (exibe o QR code no terminal)
cargo run --release -p hyprlink-gui  # Interface gráfica
```

O repositório disponibiliza um `.cargo/config.toml` configurado para usar o vinculador **`clang` + `lld`** (`pacman -S clang lld`), reduzindo o tempo de compilação e vinculação do daemon de ~5s para ~2s.

Perfis de compilação:
```bash
cargo build --profile fast -p hyprlinkd   # Iteração rápida: sem LTO, 16 codegen units
cargo build --release -p hyprlinkd        # Distribuição: LTO ativado, checagens ativas
```

O perfil `fast` herda de `release` com `lto = false`, `codegen-units = 16` e `overflow-checks = false`; **não deve ser usado para distribuição**.

### Aplicativo móvel para Android

O aplicativo para celular Android (desenvolvido em Kotlin com Jetpack Compose) é gerenciado em um repositório dedicado. APKs assinados e prontos são disponibilizados na página de [Releases](https://github.com/mastermaiolo/hyprlink/releases). Para desenvolvedores interessados em criar outros clientes, o protocolo completo está documentado em [`PROTOCOL.md`](PROTOCOL.md).

</details>

---

## 09 / PROVENIÊNCIA

<p align="center">
  <img src="assets/readme/provenance.svg" width="100%" alt="Proveniência e créditos do HyprLink">
</p>

A base de engenharia utiliza [`quinn`](https://github.com/quinn-rs/quinn), [`rustls`](https://github.com/rustls/rustls), [`iced`](https://github.com/iced-rs/iced), `ciborium`, `ksni` e `zbus`.

As fontes tipográficas incluídas no aplicativo desktop e no app móvel — Anton, Instrument Serif, Inter, IBM Plex Mono e Noto Sans SC — são distribuídas sob a licença SIL Open Font License 1.1; as declarações completas estão em `crates/hyprlink-gui/assets/fonts/OFL-*.txt`.

---

## 10 / LICENÇA

**GPL-3.0-or-later** — veja [`LICENSE`](LICENSE).

**Aviso de relicenciamento**: Até a versão 0.1.0, o código foi publicado sob os termos da licença MIT. Por ser um projeto de autor único, a partir da versão 0.1.0 ele passa a ser distribuído sob a licença GNU GPL v3.0 ou posterior. Qualquer usuário que obteve versões anteriores mantém seus direitos sob os termos da licença MIT.

---

<p align="center"><sub>MAIOLO / SYSTEMS LAB · HYPRLINK · HL / 01 · 食</sub></p>
