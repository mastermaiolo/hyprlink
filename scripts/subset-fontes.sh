#!/usr/bin/env bash
# Gera o subconjunto do Noto Sans CJK SC (OFL 1.1) embutido na GUI para 中文.
#
#   scripts/subset-fontes.sh [DIR_NOTO_CJK] [DESTINO]
#
# Lê as faces «Noto Sans CJK SC» Regular e Bold (índice 2 de NotoSansCJK-*.ttc, pacote
# noto-fonts-cjk) e guarda só os glifos de que a GUI precisa:
#   - ASCII, pontuação CJK (U+3000–303F), formas largas (U+FF00–FFEF) e os símbolos da UI;
#   - todos os caracteres não ASCII de crates/hyprlink-proto/src/i18n/zh.rs;
#   - qualquer carácter CJK no código da GUI e do proto (ex.: «中文» do seletor de
#     língua e «＋», que aparecem em todas as línguas).
# Texto que não é da UI (notificações e nomes de ficheiros do telemóvel) cai para as
# fontes CJK do sistema, se existirem.
#
# REGENERAR sempre que as traduções zh mudarem; o teste
# crates/hyprlink-gui/tests/fontes_zh.rs falha se faltar algum carácter.
# Precisa de pyftsubset (fonttools) e python3.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRC=${1:-/usr/share/fonts/noto-cjk}
DST=${2:-$ROOT/crates/hyprlink-gui/assets/fonts}
command -v pyftsubset >/dev/null || { echo "falta o pyftsubset (fonttools)" >&2; exit 1; }

TEXT=$(mktemp); trap 'rm -f "$TEXT"' EXIT
python3 -I - "$ROOT" > "$TEXT" <<'PY'
import os, sys
root = sys.argv[1]
def cjk(c):
    o = ord(c)
    return 0x2E80 <= o <= 0x9FFF or 0xF900 <= o <= 0xFAFF or 0x3000 <= o <= 0x303F or 0xFF00 <= o <= 0xFFEF
chars = {chr(c) for c in range(0x20, 0x7F)}
chars |= {chr(c) for c in range(0x3000, 0x3040)} | {chr(c) for c in range(0xFF00, 0xFFF0)}
chars |= set("—–…·‘’“”→←↑↓⇄■●▶❚＋×÷±°")
zh = open(os.path.join(root, "crates/hyprlink-proto/src/i18n/zh.rs"), encoding="utf8").read()
chars |= {c for c in zh if ord(c) > 0x7F}
for crate in ("hyprlink-gui", "hyprlink-proto"):
    for dp, _, fs in os.walk(os.path.join(root, "crates", crate, "src")):
        for f in fs:
            if f.endswith(".rs"):
                chars |= {c for c in open(os.path.join(dp, f), encoding="utf8").read() if cjk(c)}
sys.stdout.write("".join(sorted(chars)))
PY

for w in Regular Bold; do
  out="$DST/NotoSansSC-$w.otf"
  pyftsubset "$SRC/NotoSansCJK-$w.ttc" --font-number=2 \
    --text-file="$TEXT" \
    --layout-features='kern,vert,locl' --name-IDs='*' --notdef-outline \
    --output-file="$out"
  echo "$out $(stat -c %s "$out") bytes"
done
