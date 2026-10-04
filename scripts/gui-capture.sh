#!/usr/bin/env bash
# Regressão visual da GUI em mock: captura cada secção em Xvfb (tiny-skia) e
# compara com docs/screenshots/.
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

for s in 1 2 3 5 6 7 8; do shoot "$(printf %02d "$s")" "$s" 940; done
shoot 04 4 940 mirror   # a referência está em direto
shoot 09-pair 2 940 pair
shoot 01-capa-completa 1 2060
cat "$OUT/report.txt"
