#!/usr/bin/env python3
"""Diferenças locais entre os ficheiros de design da GUI e o workspace.

Aplicadas pelo sync-gui.sh depois de copiar do repositório de design. Cada
uma é idempotente e falha alto se o ponto de inserção desaparecer (o design
mudou e é preciso rever à mão).

    local_patches.py FICHEIRO...           (decide pelo nome do ficheiro)
    local_patches.py --as app.rs FICHEIRO  (para cópias temporárias)
"""
import re
import sys

TRANSPORT_OLD = "            link: Box::new(link::mock::Simulator::new()),\n"
TRANSPORT_NEW = "            link: hyprlink_gui::transport(),\n"
TRAY_SUB = "            tray::subscription(),\n"
INSTANCE_SUB = (
    "            hyprlink_gui::instance::subscription().map(|_| Message::Tray(tray::Action::Open)),\n"
)
MAIN_FN = "fn main() -> iced::Result {\n"
MAIN_GUARD = (
    '    if hyprlink_gui::instance::forward_to_running(!std::env::args().any(|a| a == "--hidden")) {\n'
    "        return Ok(());\n"
    "    }\n"
)


def need(s, what, name):
    if what not in s:
        sys.exit(f"local_patches: {name}: ponto de inserção não encontrado: {what.strip()!r}")


def app(s):
    if TRANSPORT_NEW not in s:
        need(s, TRANSPORT_OLD, "app.rs")
        s = s.replace(TRANSPORT_OLD, TRANSPORT_NEW, 1)
    if INSTANCE_SUB not in s:
        need(s, TRAY_SUB, "app.rs")
        s = s.replace(TRAY_SUB, TRAY_SUB + INSTANCE_SUB, 1)
    return s


def main_rs(s):
    if MAIN_GUARD not in s:
        need(s, MAIN_FN, "main.rs")
        s = s.replace(MAIN_FN, MAIN_FN + MAIN_GUARD, 1)
    return s


def link_mod(s):
    return re.sub(
        r'(?m)^(?<!\))pub mod (mock|mock_more);',
        r'#[cfg(any(test, feature = "mock"))]\npub mod \1;',
        re.sub(r'#\[cfg\(any\(test, feature = "mock"\)\)\]\n(pub mod (mock|mock_more);)', r"\1", s),
    )


PATCHES = {"app.rs": app, "main.rs": main_rs, "mod.rs": link_mod}

args = sys.argv[1:]
forced = None
if args[:1] == ["--as"]:
    forced, args = args[1], args[2:]
for path in args:
    name = forced or path.rsplit("/", 1)[-1]
    with open(path) as f:
        s = f.read()
    s = PATCHES[name](s)
    with open(path, "w") as f:
        f.write(s)
