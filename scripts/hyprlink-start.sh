#!/usr/bin/env bash
# Arranca o daemon do HyprLink e, quando o socket local estiver pronto, a GUI.
# Uso: scripts/hyprlink-start.sh [--restart] [--debug]
#   --restart  encerra primeiro o que estiver a correr (daemon e GUI)
#   --debug    usa target/debug em vez de target/release
set -u

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PERFIL=release
REINICIAR=0
for a in "$@"; do
  case "$a" in
    --restart) REINICIAR=1 ;;
    --debug) PERFIL=debug ;;
    -h|--help) sed -n '2,5p' "$0"; exit 0 ;;
    *) echo "Opção desconhecida: $a" >&2; exit 2 ;;
  esac
done

DAEMON="$RAIZ/target/$PERFIL/hyprlink-daemon"
GUI="$RAIZ/target/$PERFIL/hyprlink-gui"
SOCKET="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/hyprlink.sock"
LOGS="${XDG_STATE_HOME:-$HOME/.local/state}/hyprlink"
mkdir -p "$LOGS"

for b in "$DAEMON" "$GUI"; do
  if [ ! -x "$b" ]; then
    echo "Não encontro $b — compila primeiro (cargo build --release -p hyprlinkd -p hyprlink-gui)." >&2
    exit 1
  fi
done

if [ "$REINICIAR" = 1 ]; then
  "$RAIZ/scripts/hyprlink-stop.sh"
fi

# 1) Daemon
if pgrep -x hyprlink-daemon >/dev/null; then
  echo "Daemon já está a correr — não arranco outro."
else
  echo "A arrancar o daemon…"
  setsid nohup "$DAEMON" >>"$LOGS/daemon.log" 2>&1 < /dev/null &
  # Espera até 8 s pelo socket local (é por ele que a GUI fala com o daemon).
  for _ in $(seq 1 80); do
    [ -S "$SOCKET" ] && break
    if ! pgrep -x hyprlink-daemon >/dev/null; then
      echo "O daemon terminou ao arrancar. Últimas linhas de $LOGS/daemon.log:" >&2
      tail -n 15 "$LOGS/daemon.log" >&2
      exit 1
    fi
    sleep 0.1
  done
  if [ ! -S "$SOCKET" ]; then
    echo "Aviso: o socket $SOCKET ainda não existe; arranco a GUI na mesma." >&2
  fi
fi

# 2) GUI
if pgrep -x hyprlink-gui >/dev/null; then
  echo "A GUI já está a correr."
else
  echo "A arrancar a GUI…"
  setsid nohup "$GUI" >>"$LOGS/gui.log" 2>&1 < /dev/null &
fi

echo "Pronto. Registos em $LOGS/ (daemon.log, gui.log)."
