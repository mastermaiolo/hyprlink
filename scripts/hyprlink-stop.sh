#!/usr/bin/env bash
# Encerra a GUI e depois o daemon do HyprLink (SIGTERM; SIGKILL só se não saírem em 5 s).
set -u

parar() {
  local nome="$1"
  if ! pgrep -x "$nome" >/dev/null; then
    echo "$nome: não estava a correr."
    return 0
  fi
  pkill -TERM -x "$nome"
  for _ in $(seq 1 50); do
    pgrep -x "$nome" >/dev/null || { echo "$nome: encerrado."; return 0; }
    sleep 0.1
  done
  echo "$nome: não saiu a tempo, a forçar." >&2
  pkill -KILL -x "$nome"
  echo "$nome: encerrado à força."
}

# GUI primeiro, para não ficar a mostrar um daemon que já não existe.
parar hyprlink-gui
parar hyprlink-daemon

# O socket local fica órfão se o daemon for morto à força.
SOCKET="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/hyprlink.sock"
if [ -S "$SOCKET" ] && ! pgrep -x hyprlink-daemon >/dev/null; then
  rm -f "$SOCKET"
fi
