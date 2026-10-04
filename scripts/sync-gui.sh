#!/usr/bin/env bash
# Traz as alterações do repositório de design da GUI para o workspace.
#
#   scripts/sync-gui.sh [--check]     (--check: só mostra o que difere)
#
# Mapa (o resto do repositório de design não entra):
#   src/{app,graphics,main,pages,theme,tray,ui,views}.rs → crates/hyprlink-gui/src/
#   src/{fmt,host,snapshot}.rs, src/link/*.rs      → crates/hyprlink-proto/src/
#   src/bin/hyprlinkctl.rs                         → (manual: crates/hyprlinkctl/src/main.rs)
#   assets/, docs/screenshots/, README.md          → crates/hyprlink-gui/
#
# Diferenças locais que o script repõe depois de copiar (local_patches.py):
#   - app.rs: o transporte vem de hyprlink_gui::transport() (socket real;
#     HYPRLINK_MOCK=1 = simulador) e a subscrição da instância única;
#   - main.rs: instância única (um segundo arranque só foca a janela);
#   - link/mod.rs: `pub mod mock` e `pub mod mock_more` atrás de
#     cfg(any(test, feature = "mock")).
set -euo pipefail

SRC="${HYPRLINK_GUI_SRC:-$HOME/Projectos/iced/hyprlink-gui}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GUI="$ROOT/crates/hyprlink-gui"
PROTO="$ROOT/crates/hyprlink-proto"

pairs=()
for f in app graphics main pages theme tray ui views; do pairs+=("src/$f.rs:$GUI/src/$f.rs"); done
for f in fmt host snapshot; do pairs+=("src/$f.rs:$PROTO/src/$f.rs"); done
for f in mod mock mock_more packets; do pairs+=("src/link/$f.rs:$PROTO/src/link/$f.rs"); done

patch_local() {
    python3 "$ROOT/scripts/local_patches.py" "$GUI/src/app.rs" "$GUI/src/main.rs" "$PROTO/src/link/mod.rs"
}

if [[ "${1:-}" == "--check" ]]; then
    tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
    for p in "${pairs[@]}"; do
        from="$SRC/${p%%:*}"; to="${p#*:}"
        cp "$from" "$tmp/x.rs"
        case "$to" in
            */app.rs|*/main.rs|*/link/mod.rs) python3 "$ROOT/scripts/local_patches.py" --as "$(basename "$to")" "$tmp/x.rs" ;;
        esac
        cmp -s "$tmp/x.rs" "$to" || echo "difere: ${p%%:*}"
    done
    diff -rq "$SRC/assets" "$GUI/assets" || true
    diff -rq "$SRC/docs/screenshots" "$GUI/docs/screenshots" || true
    cmp -s "$SRC/src/bin/hyprlinkctl.rs" "$ROOT/.sync/hyprlinkctl.base.rs" 2>/dev/null \
        || echo "rever à mão: src/bin/hyprlinkctl.rs (→ crates/hyprlinkctl/src/main.rs)"
    exit 0
fi

for p in "${pairs[@]}"; do cp "$SRC/${p%%:*}" "${p#*:}"; done
rsync -a --delete "$SRC/assets/" "$GUI/assets/"
rsync -a --delete "$SRC/docs/screenshots/" "$GUI/docs/screenshots/"
cp "$SRC/README.md" "$GUI/README.md"
patch_local
mkdir -p "$ROOT/.sync" && cp "$SRC/src/bin/hyprlinkctl.rs" "$ROOT/.sync/hyprlinkctl.base.rs"
echo "sincronizado de $SRC. Correr: cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace"
