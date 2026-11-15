#!/usr/bin/env bash
# install-release.sh — transactionally install a prebuilt Lope v2 bundle.
# Shipped at the root of release tarballs as `install.sh`.
#
# Layout inside extracted tarball:
#   bin/lope
#   share/lope/{VERSION,install,skills,commands,docs}
#
# Defaults to an unprivileged user prefix ($HOME/.local) so running `./install.sh`
# never requires `sudo` or writes host skills to /root/. For system-wide
# installs into /usr/local, install binaries with `--prefix /usr/local --skip-install`
# as root, then run `lope install --host all` as your normal user.
#
# Phase 1 transactionally swaps bin/lope and share/lope, verifying `lope --version`
# against share/lope/VERSION (when present) and rolling back if verification fails.
# Phase 2 optionally runs host skill registration (`lope install --host <host>`).
#
# Usage:
#   ./install.sh [--prefix "$HOME/.local"] [--host all|claude|codex|...] [--skip-install]

set -euo pipefail

PREFIX="${HOME}/.local"
HOST="all"
SKIP_HOST_INSTALL=0

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix)
      [ $# -ge 2 ] || { echo "ERROR: --prefix requires a non-empty absolute path" >&2; exit 2; }
      PREFIX="$2"
      shift 2
      ;;
    --prefix=*)
      PREFIX="${1#--prefix=}"
      shift
      ;;
    --host)
      [ $# -ge 2 ] || { echo "ERROR: --host requires a value" >&2; exit 2; }
      HOST="$2"
      shift 2
      ;;
    --host=*)
      HOST="${1#--host=}"
      shift
      ;;
    --skip-install)
      SKIP_HOST_INSTALL=1
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Usage: ./install.sh [--prefix "$HOME/.local"] [--host all|claude|codex|...] [--skip-install]

Transactionally installs bin/lope and share/lope/* under an absolute PREFIX,
verifies `lope --version`, rolls back if verification fails, and registers host
skills/commands via `lope install --host <host>`.
EOF
      exit 0
      ;;
    *)
      echo "ERROR: unknown option '$1'" >&2
      exit 2
      ;;
  esac
done

case "$PREFIX" in
  /*) ;;
  *)
    echo "ERROR: --prefix must be a non-empty absolute path (got '$PREFIX')" >&2
    exit 2
    ;;
esac

# Strip redundant trailing slashes except root '/'.
while [ "${#PREFIX}" -gt 1 ] && [ "${PREFIX%/}" != "$PREFIX" ]; do
  PREFIX="${PREFIX%/}"
done

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SRC_BIN="$SCRIPT_DIR/bin/lope"
SRC_SHARE="$SCRIPT_DIR/share/lope"

if [ ! -f "$SRC_BIN" ] || [ ! -f "$SRC_SHARE/install" ] || [ ! -d "$SRC_SHARE/skills" ]; then
  echo "ERROR: incomplete release bundle under $SCRIPT_DIR (expected bin/lope and share/lope/)" >&2
  exit 1
fi

BIN_DIR="$PREFIX/bin"
BIN_TARGET="$BIN_DIR/lope"
SHARE_PARENT="$PREFIX/share"
SHARE_TARGET="$SHARE_PARENT/lope"

mkdir -p "$BIN_DIR" "$SHARE_PARENT"

# Allocate unique same-filesystem staging and backup paths to prevent collisions.
SHARE_STAGE="$(mktemp -d "$SHARE_PARENT/.lope.stage.XXXXXX")"
SHARE_PREV="$(mktemp -d "$SHARE_PARENT/.lope.prev.XXXXXX")"
BIN_STAGE="$(mktemp "$BIN_DIR/.lope.stage.XXXXXX")"
BIN_PREV="$(mktemp "$BIN_DIR/.lope.prev.XXXXXX")"
rm -rf "$SHARE_PREV" "$BIN_PREV"
COMMITTED=0
SHARE_SWAPPED=0
BIN_SWAPPED=0

rollback() {
  rm -rf "$SHARE_STAGE" "$BIN_STAGE"
  if [ "$COMMITTED" = "0" ]; then
    if [ "$BIN_SWAPPED" = "1" ]; then
      if [ -e "$BIN_PREV" ] || [ -L "$BIN_PREV" ]; then
        mv -f "$BIN_PREV" "$BIN_TARGET" >/dev/null 2>&1 || true
      elif [ -e "$BIN_TARGET" ] || [ -L "$BIN_TARGET" ]; then
        rm -f "$BIN_TARGET" >/dev/null 2>&1 || true
      fi
    fi
    if [ "$SHARE_SWAPPED" = "1" ]; then
      if [ -e "$SHARE_PREV" ] || [ -L "$SHARE_PREV" ]; then
        rm -rf "$SHARE_TARGET" >/dev/null 2>&1 || true
        mv "$SHARE_PREV" "$SHARE_TARGET" >/dev/null 2>&1 || true
      elif [ -e "$SHARE_TARGET" ] || [ -L "$SHARE_TARGET" ]; then
        rm -rf "$SHARE_TARGET" >/dev/null 2>&1 || true
      fi
    fi
  fi
}
trap rollback EXIT

# Prepare staged files cleanly with world-readable/executable permissions.
cp -R "$SRC_SHARE"/. "$SHARE_STAGE"/
chmod 0755 "$SHARE_STAGE" "$SHARE_STAGE/install"
cp "$SRC_BIN" "$BIN_STAGE"
chmod 0755 "$BIN_STAGE"

EXPECTED_VERSION=""
if [ -f "$SRC_SHARE/VERSION" ]; then
  EXPECTED_VERSION="$(tr -d '\r\n' < "$SRC_SHARE/VERSION")"
fi

check_version_line() {
  local line="$1"
  local allow_any_major="$2"
  if [ -n "$EXPECTED_VERSION" ]; then
    local esc
    esc="$(printf '%s' "$EXPECTED_VERSION" | sed -e 's/[][(){}.+*?^$|\\]/\\&/g')"
    printf '%s\n' "$line" | grep -Eq "^lope ${esc}( \\(compat [^)]+\\))?\$"
  elif [ "$allow_any_major" = "1" ]; then
    printf '%s\n' "$line" | grep -Eq '^lope (0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?( \(compat [^)]+\))?$'
  else
    printf '%s\n' "$line" | grep -Eq '^lope 2\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?( \(compat [^)]+\))?$'
  fi
}

# Verify staged executable works directly before swapping live paths.
STAGED_VER="$("$BIN_STAGE" --version 2>/dev/null | head -n 1 || true)"
if ! check_version_line "$STAGED_VER" 1; then
  echo "ERROR: staged binary failed version verification (got '$STAGED_VER')" >&2
  exit 1
fi

# Preserve both previous targets until both swaps and final verification pass.
SHARE_SWAPPED=1
if [ -e "$SHARE_TARGET" ] || [ -L "$SHARE_TARGET" ]; then
  mv "$SHARE_TARGET" "$SHARE_PREV"
fi
mv "$SHARE_STAGE" "$SHARE_TARGET"

BIN_SWAPPED=1
if [ -e "$BIN_TARGET" ] || [ -L "$BIN_TARGET" ]; then
  mv "$BIN_TARGET" "$BIN_PREV"
fi
mv "$BIN_STAGE" "$BIN_TARGET"

VER="$("$BIN_TARGET" --version | head -n 1)"
if ! check_version_line "$VER" 0; then
  echo "ERROR: installed binary returned unexpected version '$VER'; rolled back" >&2
  exit 1
fi

COMMITTED=1
rm -rf "$SHARE_PREV" "$BIN_PREV"

echo "Installed $VER to $BIN_TARGET (assets: $SHARE_TARGET)"

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Note: add $BIN_DIR to your PATH (e.g. export PATH=$(shell_quote "$BIN_DIR"):\"\$PATH\")" ;;
esac

if [ "$SKIP_HOST_INSTALL" = "0" ]; then
  # Unset caller overrides so packaged assets under PREFIX/share/lope are exercised.
  env -u LOPE_SOURCE_DIR -u LOPE_DOCS_PATH -u LOPE_BIN "$BIN_TARGET" install --host "$HOST"
fi
