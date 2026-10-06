#!/usr/bin/env python3
"""Gera o subconjunto do Noto Sans CJK SC (OFL 1.1) embutido na GUI para 中文.

  python3 scripts/font-zh-subset.py [DIR_NOTO_CJK] [DESTINO]

Lê as faces «Noto Sans CJK SC» Regular e Bold de NotoSansCJK-*.ttc (pacote
noto-fonts-cjk) e guarda só: hanzi GB2312 nível 1 (3755), pontuação CJK, formas
largas, ASCII, os símbolos da UI e todos os caracteres usados em
crates/hyprlink-proto/src/i18n/zh.rs. Texto fora disto (nomes de ficheiros,
notificações do telemóvel) cai para as fontes CJK do sistema, se existirem.
Precisa de fonttools (pip install fonttools).
"""
import os, sys
from fontTools import subset
from fontTools.ttLib import TTCollection

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
src = sys.argv[1] if len(sys.argv) > 1 else "/usr/share/fonts/noto-cjk"
dst = sys.argv[2] if len(sys.argv) > 2 else os.path.join(root, "crates/hyprlink-gui/assets/fonts")

chars = set()
for hi in range(0xB0, 0xD8):  # GB2312 nível 1
    for lo in range(0xA1, 0xFF):
        try:
            chars.add(bytes([hi, lo]).decode("gb2312"))
        except UnicodeDecodeError:
            pass
chars |= {chr(c) for c in range(0x3000, 0x3040)} | {chr(c) for c in range(0xFF00, 0xFFF0)}
chars |= {chr(c) for c in range(0x20, 0x7F)} | set("—–…·‘’“”→←↑↓⇄■●▶❚＋×÷±°")
zh = open(os.path.join(root, "crates/hyprlink-proto/src/i18n/zh.rs"), encoding="utf8").read()
chars |= {c for c in zh if ord(c) > 0x7F}

for w in ("Regular", "Bold"):
    face = TTCollection(os.path.join(src, f"NotoSansCJK-{w}.ttc")).fonts[2]  # SC
    assert "SC" in face["name"].getDebugName(1), face["name"].getDebugName(1)
    tmp = os.path.join(dst, f".sc-{w}.otf")
    face.save(tmp)
    opts = subset.Options()
    opts.layout_features = ["kern", "vert", "locl"]
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    font = subset.load_font(tmp, opts)
    s = subset.Subsetter(opts)
    s.populate(text="".join(sorted(chars)))
    s.subset(font)
    out = os.path.join(dst, f"NotoSansSC-{w}.otf")
    subset.save_font(font, out, opts)
    os.remove(tmp)
    print(out, os.path.getsize(out))
