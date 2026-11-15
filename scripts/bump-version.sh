#!/usr/bin/env bash
# bump-version.sh — atomically bump Lope product v2 version strings (default)
# or all Python compat distribution surfaces (--compat).
#
# Usage:
#   ./scripts/bump-version.sh NEW_VERSION
#   ./scripts/bump-version.sh --compat NEW_COMPAT_VERSION
#
# Default (product v2) updates in lockstep:
#   1. Cargo.toml                    ([workspace.package] version = "X.Y.Z")
#   2. install                       (echo "Lope vX.Y.Z — ...")
#   3. .claude-plugin/plugin.json    ("version": "X.Y.Z")
#   4. .cursor-plugin/plugin.json    ("version": "X.Y.Z")
#   5. gemini-extension.json         ("version": "X.Y.Z")
#
# Compat mode (--compat) updates all 4 compat surfaces in lockstep:
#   1. pyproject.toml                ([project] version = "X.Y.Z")
#   2. lope/__init__.py              (__version__ = "X.Y.Z")
#   3. crates/lope-core/src/lib.rs   (pub const COMPAT_VERSION: &str = "X.Y.Z";)
#   4. crates/lope-compat/grammar/argparse-python-lope.json ("lope_version": "X.Y.Z")
#
# Bash 3.2 compatible (stock macOS).

set -euo pipefail

MODE="product"
if [ "${1:-}" = "--compat" ]; then
  MODE="compat"
  shift
fi

if [ $# -ne 1 ]; then
  echo "Usage: $0 [--compat] NEW_VERSION" >&2
  echo "Example: $0 2.0.1" >&2
  exit 1
fi

NEW="$1"

# Strict release SemVer subset: X.Y.Z or X.Y.Z-prerelease (no leading zeros or +build; matches git tags).
PRE_ID='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
SEMVER_RE="^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-${PRE_ID}(\\.${PRE_ID})*)?\$"
if ! echo "$NEW" | grep -Eq "$SEMVER_RE"; then
  echo "ERROR: '$NEW' is not release SemVer (expected X.Y.Z or X.Y.Z-prerelease without numeric leading zeros)" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

BACKUP_DIR="$(mktemp -d)"
BUMP_OK=0
TMP_FILE=""

backup_files() {
  local f
  for f in "$@"; do
    mkdir -p "$BACKUP_DIR/$(dirname "$f")"
    cp -p "$f" "$BACKUP_DIR/$f"
  done
}

cleanup_bump() {
  local rc=$?
  [ -n "$TMP_FILE" ] && rm -f "$TMP_FILE"
  if [ "$BUMP_OK" != "1" ] && [ -d "$BACKUP_DIR" ]; then
    (
      cd "$BACKUP_DIR"
      find . -type f | while IFS= read -r rel; do
        rel="${rel#./}"
        cp -p "$BACKUP_DIR/$rel" "$REPO_ROOT/$rel" 2>/dev/null || true
      done
    )
    echo "Rolled back version bump due to failure." >&2
  fi
  rm -rf "$BACKUP_DIR"
  exit "$rc"
}
trap cleanup_bump EXIT

if sed --version >/dev/null 2>&1; then
  SED_I=(-i)
else
  SED_I=(-i '')
fi

sed_escape() {
  printf '%s' "$1" | sed -e 's/[][\/.^$*]/\\&/g'
}

if [ "$MODE" = "product" ]; then
  CURRENT="$(awk '
    /^\[/ { in_sec = ($0 == "[workspace.package]"); next }
    in_sec && /^version = "/ { sub(/^version = "/, ""); sub(/".*/, ""); print; exit }
  ' Cargo.toml)"
  [ -n "$CURRENT" ] || { echo "ERROR: could not read [workspace.package] version from Cargo.toml" >&2; exit 1; }
  [ "$CURRENT" != "$NEW" ] || { echo "Product version is already $NEW — nothing to do."; BUMP_OK=1; exit 0; }

  backup_files Cargo.toml Cargo.lock install .claude-plugin/plugin.json .cursor-plugin/plugin.json gemini-extension.json
  echo "Bumping product v2: $CURRENT → $NEW"
  echo
  CURRENT_ESC="$(sed_escape "$CURRENT")"

  # 1. Cargo.toml ([workspace.package] section only) + refresh Cargo.lock
  TMP_FILE="$(mktemp "$REPO_ROOT/Cargo.toml.XXXXXX")"
  awk -v cur="$CURRENT" -v nxt="$NEW" '
    /^\[/ { in_sec = ($0 == "[workspace.package]") }
    in_sec && $0 == "version = \"" cur "\"" && !done {
      $0 = "version = \"" nxt "\""
      done = 1
    }
    { print }
    END { if (!done) exit 2 }
  ' Cargo.toml > "$TMP_FILE" && mv "$TMP_FILE" Cargo.toml
  TMP_FILE=""
  echo "  ✓ Cargo.toml"

  if [ -x "$REPO_ROOT/scripts/cargo" ] && { command -v cargo >/dev/null 2>&1 || [ -x "${HOME}/.cargo/bin/cargo" ]; }; then
    "$REPO_ROOT/scripts/cargo" update --workspace --offline >/dev/null
    echo "  ✓ Cargo.lock (offline workspace sync)"
  else
    echo "WARNING: cargo not found; update Cargo.lock manually before running --locked checks" >&2
  fi

  # 2. install (bash banner)
  sed "${SED_I[@]}" -E "s/Lope v${CURRENT_ESC} —/Lope v${NEW} —/" install
  echo "  ✓ install"

  # 3. .claude-plugin/plugin.json
  sed "${SED_I[@]}" -E "s/\"version\": \"${CURRENT_ESC}\"/\"version\": \"${NEW}\"/" .claude-plugin/plugin.json
  echo "  ✓ .claude-plugin/plugin.json"

  # 4. .cursor-plugin/plugin.json
  sed "${SED_I[@]}" -E "s/\"version\": \"${CURRENT_ESC}\"/\"version\": \"${NEW}\"/" .cursor-plugin/plugin.json
  echo "  ✓ .cursor-plugin/plugin.json"

  # 5. gemini-extension.json
  sed "${SED_I[@]}" -E "s/\"version\": \"${CURRENT_ESC}\"/\"version\": \"${NEW}\"/" gemini-extension.json
  echo "  ✓ gemini-extension.json"
else
  CURRENT="$(awk '
    /^\[/ { in_sec = ($0 == "[project]"); next }
    in_sec && /^version = "/ { sub(/^version = "/, ""); sub(/".*/, ""); print; exit }
  ' pyproject.toml)"
  [ -n "$CURRENT" ] || { echo "ERROR: could not read [project] version from pyproject.toml" >&2; exit 1; }
  [ "$CURRENT" != "$NEW" ] || { echo "Compat version is already $NEW — nothing to do."; BUMP_OK=1; exit 0; }

  backup_files pyproject.toml lope/__init__.py crates/lope-core/src/lib.rs crates/lope-compat/grammar/argparse-python-lope.json
  echo "Bumping Python compat baseline across all 4 compat surfaces: $CURRENT → $NEW"
  echo
  CURRENT_ESC="$(sed_escape "$CURRENT")"

  TMP_FILE="$(mktemp "$REPO_ROOT/pyproject.toml.XXXXXX")"
  awk -v cur="$CURRENT" -v nxt="$NEW" '
    /^\[/ { in_sec = ($0 == "[project]") }
    in_sec && $0 == "version = \"" cur "\"" && !done {
      $0 = "version = \"" nxt "\""
      done = 1
    }
    { print }
    END { if (!done) exit 2 }
  ' pyproject.toml > "$TMP_FILE" && mv "$TMP_FILE" pyproject.toml
  TMP_FILE=""
  echo "  ✓ pyproject.toml"

  sed "${SED_I[@]}" -E "s/^__version__ = \"${CURRENT_ESC}\"/__version__ = \"${NEW}\"/" lope/__init__.py
  echo "  ✓ lope/__init__.py"

  sed "${SED_I[@]}" -E "s/^pub const COMPAT_VERSION: &str = \"${CURRENT_ESC}\";/pub const COMPAT_VERSION: \&str = \"${NEW}\";/" crates/lope-core/src/lib.rs
  echo "  ✓ crates/lope-core/src/lib.rs"

  sed "${SED_I[@]}" -E "s/\"lope_version\": \"${CURRENT_ESC}\"/\"lope_version\": \"${NEW}\"/" crates/lope-compat/grammar/argparse-python-lope.json
  echo "  ✓ crates/lope-compat/grammar/argparse-python-lope.json"
fi

echo
echo "Verifying sync..."
"$REPO_ROOT/scripts/check-version.sh"
BUMP_OK=1

echo
echo "Next steps:"
if [ "$MODE" = "compat" ]; then
  echo "  1. Run ./tools/parity/dump-python --update if CLI schema changed"
  echo "  2. Run ./scripts/cargo test --workspace && pytest -q"
else
  echo "  1. Add a CHANGELOG entry at the top of CHANGELOG.md: ## $NEW — <tagline>"
  echo "  2. Run workspace tests:  ./scripts/cargo test --workspace"
  echo "  3. Build binary:         ./scripts/cargo build --release"
  echo "  4. Smoke test installer: ./install --host all"
  echo "  5. Stage, commit, tag, push per docs/RELEASING.md"
fi
