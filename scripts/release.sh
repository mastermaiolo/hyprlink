#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prepara uma release do HyprLink SEM publicar nada: testes, clippy, tarball
# do código-fonte, SHA-256 e notas extraídas do CHANGELOG.md.
#
# Uso: scripts/release.sh [VERSÃO] [--ref REF] [--skip-checks]
#   VERSÃO         por omissão, a de [workspace.package] no Cargo.toml
#   --ref REF      o que empacotar (por omissão: a tag vVERSÃO se existir, senão HEAD)
#   --skip-checks  não corre cargo test/clippy (só para repetir o empacotamento)
#
# Saída em dist/: hyprlink-VERSÃO.tar.gz, .sha256 e notas-VERSÃO.md.
# Publicar é à mão: tag, push e gh release create com os ficheiros de dist/.
set -euo pipefail

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$RAIZ"

VERSAO=""
REF=""
VERIFICAR=1
while [ $# -gt 0 ]; do
  case "$1" in
    --ref) REF="${2:?--ref precisa de um valor}"; shift ;;
    --skip-checks) VERIFICAR=0 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    -*) echo "Opção desconhecida: $1" >&2; exit 2 ;;
    *) VERSAO="$1" ;;
  esac
  shift
done

if [ -z "$VERSAO" ]; then
  VERSAO="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml)"
fi
[ -n "$VERSAO" ] || { echo "Não encontrei a versão no Cargo.toml." >&2; exit 1; }

if [ -z "$REF" ]; then
  if git rev-parse -q --verify "refs/tags/v$VERSAO" >/dev/null; then
    REF="v$VERSAO"
  else
    REF="HEAD"
    echo "[!] A tag v$VERSAO ainda não existe: empacoto o HEAD ($(git rev-parse --short HEAD))."
  fi
fi

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "[!] Há alterações por commitar; o tarball usa só o que está em $REF."
fi

if [ "$VERIFICAR" = 1 ]; then
  echo "[1/4] cargo test --workspace"
  cargo test --workspace --locked
  echo "[2/4] cargo clippy --workspace -- -D warnings"
  cargo clippy --workspace --all-targets --locked -- -D warnings
else
  echo "[1-2/4] verificações saltadas (--skip-checks)"
fi

DIST="$RAIZ/dist"
mkdir -p "$DIST"
NOME="hyprlink-$VERSAO"

# Mesmo prefixo que o tarball automático do GitHub (/archive/vX.tar.gz), que é
# o que o PKGBUILD descarrega. O SHA-256 do GitHub pode diferir deste; para o
# PKGBUILD, recalcular com updpkgsums depois de a tag existir.
echo "[3/4] tarball $NOME.tar.gz a partir de $REF"
git archive --format=tar.gz --prefix="$NOME/" -o "$DIST/$NOME.tar.gz" "$REF"
(cd "$DIST" && sha256sum "$NOME.tar.gz" > "$NOME.tar.gz.sha256")

echo "[4/4] notas a partir do CHANGELOG.md"
NOTAS="$DIST/notas-$VERSAO.md"
awk -v v="$VERSAO" '
  $0 ~ "^## \\[" v "\\]" { dentro = 1; next }
  dentro && /^## \[/ { exit }
  dentro { print }
' CHANGELOG.md > "$NOTAS"
[ -s "$NOTAS" ] || { echo "Não há entrada [$VERSAO] no CHANGELOG.md." >&2; exit 1; }

echo
echo "Pronto (nada foi publicado):"
ls -l "$DIST/$NOME.tar.gz" "$DIST/$NOME.tar.gz.sha256" "$NOTAS"
cat "$DIST/$NOME.tar.gz.sha256"
