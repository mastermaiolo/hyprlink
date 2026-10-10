# contrib/ — integração de ecossistema do HyprLink

As peças que transformam o HyprLink de "app" em parte do sistema — a carta
que um ecossistema fechado não pode jogar. Tudo fala com o daemon pelo
socket Unix `$XDG_RUNTIME_DIR/hyprlink/cmd.sock` (ver `src/ctl.rs`).

## `hyprlinkctl` (CLI)

| Comando | O que faz |
|---|---|
| `hyprlinkctl status` | estado completo do daemon em JSON |
| `hyprlinkctl send <ficheiro>` | envia um ficheiro pro telemóvel |
| `hyprlinkctl dispatch "workspace 2"` | `hyprctl dispatch` de qualquer script |
| `hyprlinkctl lock` | bloqueia o PC (`exec hyprlock`) |
| `hyprlinkctl tap on\|off` | ouvir o som do PC no telemóvel |
| `hyprlinkctl speaker on\|off` | telemóvel como coluna do PC |
| `hyprlinkctl mic on\|off` | microfone do telemóvel como entrada do PC |
| `hyprlinkctl notif "Título" "corpo"` | notificação no telemóvel |
| `hyprlinkctl url <url>` | abre URL no PC |
| `hyprlinkctl phone-url <url>` | abre URL no telemóvel |
| `hyprlinkctl phone-app <package>` | abre app no telemóvel (ex: `com.whatsapp`) |
| `hyprlinkctl ping` | o daemon está vivo? |

Exit codes: `0` ok, `1` erro, `2` uso — pronto pra scripts e Makefiles.

## Widget de Waybar

`waybar-hyprlink.sh` lê o `status.json` que o daemon publica a cada 2s e
devolve o formato que o Waybar espera. Instalação:

```bash
./install.sh
# depois no config do waybar:
#   "hyprlink": { "exec": "~/.config/waybar/hyprlink.sh", "interval": 5, "return-type": "json" }
```

Classes CSS disponíveis: `connected`, `disconnected`, `unknown`.

## "Enviar via HyprLink" (gestor de ficheiros)

O `install.sh` regista um `.desktop` com `%f` — aparece no menu de contexto
de Nautilus/Nemo/Thunar/KDE pra qualquer tipo de ficheiro comum.

## Regras de janela do Hyprland

A GUI define o identificador Wayland `application_id = dev.hyprlink.gui` (mapeado para `class` no Hyprland e `StartupWMClass` no `.desktop`). Para que a janela abra flutuante e centrada quando acionada a partir da bandeja ou do lançador de aplicações:

### Sintaxe clássica (`hyprland.conf`)
```ini
# Janela do HyprLink flutuante e centrada
windowrulev2 = float, class:^(dev.hyprlink.gui)$
windowrulev2 = center, class:^(dev.hyprlink.gui)$
```

### Sintaxe moderna Lua (`hyprland.lua`)
```lua
-- Janela do HyprLink flutuante e centrada
hl.window_rule({
    match = { class = "dev.hyprlink.gui" },
    float = true,
    center = true,
})
```

## Dependências opcionais

| Pacote | Para quê |
|---|---|
| `xdg-desktop-portal-gtk` (ou `-kde`) | seletor de ficheiros do botão «ESCOLHER FICHEIROS…» da GUI; o `xdg-desktop-portal-hyprland` não o fornece |

Pacote Arch disponível em `packaging/arch/PKGBUILD` com todos os `optdepends` mapeados.

## Instalar tudo

```bash
./install.sh
```

Compila os dois binários em release, copia pra `~/.local/bin` e regista o
`.desktop` + widget. Reexecutável; instruções de remoção no próprio script.
