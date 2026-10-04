#!/usr/bin/env bash
# Regressão visual da GUI em mock: captura cada secção em Xvfb (tiny-skia) e
# compara com docs/screenshots/ (nomes e alturas iguais às de referência).
#
#   scripts/gui-capture.sh [BIN] [OUT]
#     BIN  binário da GUI   (por omissão target/release/hyprlink-gui)
#     OUT  pasta de saída   (por omissão target/gui-capture)
#
# Em OUT ficam NN.png (captura), NN-diff.png (diferenças a vermelho) e
# report.txt com a percentagem de píxeis diferentes (fuzz 6 %) por secção.
# Diferenças em zonas com dados vivos (relógio, gráficos, valores do mock)
# são esperadas; o resto tem de ser explicado.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${1:-$ROOT/target/release/hyprlink-gui}"
OUT="${2:-$ROOT/target/gui-capture}"
REF="$ROOT/crates/hyprlink-gui/docs/screenshots"
DISP=":$((90 + RANDOM % 100))"
mkdir -p "$OUT"; : > "$OUT/report.txt"

shoot() { # nome secção altura demo
    local name=$1 sec=$2 h=$3 demo=${4:-}
    Xvfb "$DISP" -screen 0 "1480x${h}x24" -nolisten tcp >/dev/null 2>&1 &
    local xp=$!; sleep 0.8
    env -u WAYLAND_DISPLAY DISPLAY="$DISP" ICED_BACKEND=tiny-skia \
        HYPRLINK_SECTION="$sec" HYPRLINK_WINDOW_H="$h" ${demo:+HYPRLINK_DEMO=$demo} \
        XDG_RUNTIME_DIR="$(mktemp -d)" "$BIN" >/dev/null 2>&1 &
    local gp=$!; sleep "${SETTLE:-4}"
    DISPLAY="$DISP" import -window root "$OUT/$name.png"
    kill $gp 2>/dev/null || true; wait $gp 2>/dev/null || true
    kill $xp 2>/dev/null || true; wait $xp 2>/dev/null || true
    if [[ -f "$REF/$name.png" ]]; then
        local px total
        px=$(compare -metric AE -fuzz 6% "$REF/$name.png" "$OUT/$name.png" "$OUT/$name-diff.png" 2>&1 >/dev/null || true)
        total=$(( 1480 * h ))
        awk -v n="$name" -v p="${px%% *}" -v t="$total" 'BEGIN{printf "%-16s %6.2f %%  (%d px)\n", n, 100*p/t, p}' >> "$OUT/report.txt"
    fi
}

# nome                 secção altura demo — igual às capturas de referência
shoot 00-definicoes        11 1200
shoot 01-capa               1 2060
shoot 02-dispositivos       2 1700
shoot 02b-emparelhar        2  940 pair
shoot 03-secretaria         3 1400
shoot 04a-camara            4 1100 webcam
shoot 04b-ecra              4 1100 screen
shoot 05-audio              5 1800
shoot 06-notificacoes       6  940
shoot 07-partilha           7 1100 transfer
shoot 08-multimedia         8 1000
shoot 09-sensores-presenca  9 1750
shoot 10-diario            10  940
cat "$OUT/report.txt"
