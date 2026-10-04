#!/usr/bin/env bash
# Instala as peças de ecossistema do HyprLink pra o usuário atual:
#   1. compila os binários (release) e copia pra ~/.local/bin
#   2. instala o .desktop "Enviar via HyprLink" (menu de contexto dos
#      gestores de ficheiros — Nautilus/Nemo/Thunar via %f)
#   3. instala o widget de Waybar em ~/.config/waybar/hyprlink.sh
#
# Reexecutável à vontade. Desinstalar: remover ~/.local/bin/hyprlinkctl,
# ~/.local/bin/hyprlink-daemon, ~/.local/share/applications/hyprlink-send.desktop
# e ~/.config/waybar/hyprlink.sh.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "[1/3] a compilar (release)…"
cargo build --release -p hyprlinkd -p hyprlinkctl

BIN_DIR="${HOME}/.local/bin"
mkdir -p "$BIN_DIR"
install -m755 target/release/hyprlinkctl "$BIN_DIR/hyprlinkctl"
install -m755 target/release/hyprlink-daemon "$BIN_DIR/hyprlink-daemon"

echo "[2/3] a instalar o .desktop (Enviar via HyprLink)…"
DESKTOP_DIR="${HOME}/.local/share/applications"
mkdir -p "$DESKTOP_DIR"
sed "s|@BIN@|${BIN_DIR}/hyprlinkctl|g" "$ROOT/contrib/hyprlink-send.desktop" > "$DESKTOP_DIR/hyprlink-send.desktop"
update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true

echo "[3/3] a instalar o widget de Waybar…"
mkdir -p "${HOME}/.config/waybar"
install -m755 "$ROOT/contrib/waybar-hyprlink.sh" "${HOME}/.config/waybar/hyprlink.sh"

cat <<FIM

Pronto. Teste agora:
  hyprlinkctl ping
  hyprlinkctl status

Waybar — adicione ao seu config ("modules-left/right" + secção de módulos):
  "hyprlink": { "exec": "~/.config/waybar/hyprlink.sh", "interval": 5, "return-type": "json" }

Clicar com o botão direito num ficheiro no gestor → "Enviar via HyprLink".
FIM
