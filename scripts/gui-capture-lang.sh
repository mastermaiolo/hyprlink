#!/usr/bin/env bash
# Capas e Definições da GUI em cada idioma (mock, Xvfb + tiny-skia), para ver
# que nada parte nem fica em quadrados: docs/screenshots/lang-{capa,definicoes}-<idioma>.png
#
#   scripts/gui-capture-lang.sh [BIN] [OUT]
#     BIN  binário da GUI   (por omissão target/release/hyprlink-gui)
#     OUT  pasta de saída   (por omissão crates/hyprlink-gui/docs/screenshots)
#
# O idioma vem de um gui.json temporário (XDG_CONFIG_HOME próprio): não toca na
# configuração real.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${1:-$ROOT/target/release/hyprlink-gui}"
OUT="${2:-$ROOT/crates/hyprlink-gui/docs/screenshots}"
DISP=":$((90 + RANDOM % 100))"
mkdir -p "$OUT"

shoot() { # idioma(variante) etiqueta secção altura nome
    local lang=$1 tag=$2 sec=$3 h=$4 name=$5
    local cfg; cfg="$(mktemp -d)"; mkdir -p "$cfg/hyprlink"
    printf '{"lang":"%s"}\n' "$lang" > "$cfg/hyprlink/gui.json"
    Xvfb "$DISP" -screen 0 "1480x${h}x24" -nolisten tcp >/dev/null 2>&1 &
    local xp=$!; sleep 0.8
    env -u WAYLAND_DISPLAY DISPLAY="$DISP" ICED_BACKEND=tiny-skia \
        HYPRLINK_MOCK=1 HYPRLINK_SECTION="$sec" HYPRLINK_WINDOW_H="$h" \
        XDG_CONFIG_HOME="$cfg" XDG_RUNTIME_DIR="$(mktemp -d)" "$BIN" >/dev/null 2>&1 &
    local gp=$!; sleep "${SETTLE:-4}"
    DISPLAY="$DISP" import -window root "$OUT/lang-$name-$tag.png"
    kill $gp 2>/dev/null || true; wait $gp 2>/dev/null || true
    kill $xp 2>/dev/null || true; wait $xp 2>/dev/null || true
    rm -rf "$cfg"
}

for pair in PtPt:pt-PT PtBr:pt-BR EnGb:en EsEs:es Zh:zh; do
    shoot "${pair%%:*}" "${pair##*:}" 1 2060 capa
    shoot "${pair%%:*}" "${pair##*:}" 11 1200 definicoes
done
