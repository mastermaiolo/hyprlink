#!/usr/bin/env bash
# Widget Waybar do HyprLink — lê o status.json que o daemon publica em
# $XDG_RUNTIME_DIR/hyprlink/ (ver src/ctl.rs). Waybar chama isto a cada
# `interval` e consome o JSON devolvido (text/tooltip/class).
#
# Adicione ao seu waybar config ("modules-right" ou "modules-left"):
#   "hyprlink"
# e à secção de módulos:
#   "hyprlink": {
#       "exec": "~/.config/waybar/hyprlink.sh",
#       "interval": 5,
#       "return-type": "json",
#       "on-click": "hyprlinkctl tap on",
#       "on-click-middle": "hyprlinkctl speaker on",
#       "on-click-right": "hyprlinkctl status | notify-send HyprLink"
#   }
# Classes CSS: `connected`, `disconnected`, `unknown` — estilize à vontade.

S="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/hyprlink/status.json"

if [ ! -f "$S" ]; then
    echo '{"text":"📱✗","tooltip":"daemon do HyprLink não está a correr","class":"disconnected"}'
    exit 0
fi

if ! command -v jq >/dev/null 2>&1; then
    echo '{"text":"📱?","tooltip":"instale jq pra ver o estado do HyprLink","class":"unknown"}'
    exit 0
fi

JSON=$(cat "$S")

if [ "$(jq -r '.connected' <<<"$JSON")" != "true" ]; then
    echo '{"text":"📱✗","tooltip":"sem telemóvel conectado","class":"disconnected"}'
    exit 0
fi

DEV=$(jq -r '.device // "?"' <<<"$JSON")
BATT=$(jq -r '.phone_battery_pct // empty' <<<"$JSON")
TAP=$(jq -r '.audio_tap' <<<"$JSON")
SPK=$(jq -r '.speaker' <<<"$JSON")
MIC=$(jq -r '.mic' <<<"$JSON")
MEDIA=$(jq -r '.media // empty' <<<"$JSON")

TXT="📱"
[ -n "$BATT" ] && TXT+=" ${BATT}%"
[ "$SPK" = "true" ] && TXT+=" 🔊"
[ "$TAP" = "true" ] && TXT+=" 🎧"
[ "$MIC" = "true" ] && TXT+=" 🎙"

TIP="$DEV"
[ -n "$MEDIA" ] && TIP+=" · ${MEDIA}"
[ "$SPK" = "true" ] && TIP+=" · som no telemóvel"

jq -n --arg t "$TXT" --arg tt "$TIP" '{text:$t, tooltip:$tt, class:"connected"}'
