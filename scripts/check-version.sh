#!/usr/bin/env bash
# check-version.sh — verify Lope's product v2 and Python compat version groups
# are each internally synchronized and plugin/package metadata URLs match makakoo/lope.
#
# Usage: ./scripts/check-version.sh
#
# Exits 0 if both version groups and URL checks pass. Exits 1 on drift.
# Bash 3.2 compatible (stock macOS).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

extract_toml_section_version() {
  local file="$1"
  local section="$2"
  awk -v target="[$section]" '
    /^\[/ { in_sec = ($0 == target); next }
    in_sec && /^version = "/ {
      sub(/^version = "/, "");
      sub(/".*/, "");
      print;
      count++;
    }
    END { if (count != 1) exit 2 }
  ' "$file" 2>/dev/null || true
}

extract_single_regex() {
  local file="$1"
  local grep_re="$2"
  local sed_re="$3"
  local matches
  matches="$(grep -E "$grep_re" "$file" || true)"
  if [ -z "$matches" ]; then
    return 0
  fi
  local line_count
  line_count="$(printf '%s\n' "$matches" | wc -l | tr -d ' ')"
  if [ "$line_count" -ne 1 ]; then
    return 0
  fi
  printf '%s\n' "$matches" | sed -E "$sed_re"
}

# Product v2 group (native Rust binary + skills/plugins/installer)
v_cargo="$(extract_toml_section_version Cargo.toml "workspace.package")"
v_install="$(extract_single_regex install '^echo "Lope v' 's/echo "Lope v([^ ]+) .*/\1/')"
v_claude="$(extract_single_regex .claude-plugin/plugin.json '^[[:space:]]*"version": "' 's/.*"version": "([^"]+)".*/\1/')"
v_cursor="$(extract_single_regex .cursor-plugin/plugin.json '^[[:space:]]*"version": "' 's/.*"version": "([^"]+)".*/\1/')"
v_gemini="$(extract_single_regex gemini-extension.json '^[[:space:]]*"version": "' 's/.*"version": "([^"]+)".*/\1/')"

# Python compatibility & rollback distribution group (PL-1 / parity pin)
v_pyproject="$(extract_toml_section_version pyproject.toml "project")"
v_init="$(extract_single_regex lope/__init__.py '^__version__ = "' 's/__version__ = "([^"]+)".*/\1/')"
v_rust_compat="$(extract_single_regex crates/lope-core/src/lib.rs '^pub const COMPAT_VERSION: &str = "' 's/pub const COMPAT_VERSION: &str = "([^"]+)".*/\1/')"
v_grammar="$(extract_single_regex crates/lope-compat/grammar/argparse-python-lope.json '^[[:space:]]*"lope_version": "' 's/.*"lope_version": "([^"]+)".*/\1/')"

echo "Product v2 surfaces:"
printf '  %-48s %s\n' "Cargo.toml ([workspace.package]):"               "$v_cargo"
printf '  %-48s %s\n' "install (banner):"                               "$v_install"
printf '  %-48s %s\n' ".claude-plugin/plugin.json:"                     "$v_claude"
printf '  %-48s %s\n' ".cursor-plugin/plugin.json:"                     "$v_cursor"
printf '  %-48s %s\n' "gemini-extension.json:"                          "$v_gemini"

echo
echo "Python compat surfaces (frozen parity/rollback baseline):"
printf '  %-48s %s\n' "pyproject.toml ([project]):"                     "$v_pyproject"
printf '  %-48s %s\n' "lope/__init__.py:"                               "$v_init"
printf '  %-48s %s\n' "crates/lope-core/src/lib.rs (COMPAT_VERSION):"   "$v_rust_compat"
printf '  %-48s %s\n' "argparse-python-lope.json (lope_version):"       "$v_grammar"

for pair in \
  "Cargo.toml:$v_cargo" \
  "install:$v_install" \
  ".claude-plugin/plugin.json:$v_claude" \
  ".cursor-plugin/plugin.json:$v_cursor" \
  "gemini-extension.json:$v_gemini" \
  "pyproject.toml:$v_pyproject" \
  "lope/__init__.py:$v_init" \
  "crates/lope-core/src/lib.rs:$v_rust_compat" \
  "crates/lope-compat/grammar/argparse-python-lope.json:$v_grammar"
do
  name="${pair%%:*}"
  value="${pair#*:}"
  if [ -z "$value" ]; then
    echo
    echo "ERROR: could not unambiguously extract version from $name" >&2
    exit 1
  fi
done

status=0

if [ "$v_cargo" != "$v_install" ] || \
   [ "$v_cargo" != "$v_claude" ] || \
   [ "$v_cargo" != "$v_cursor" ] || \
   [ "$v_cargo" != "$v_gemini" ]; then
  echo
  echo "✗ Product v2 version drift detected." >&2
  echo "  Fix with: ./scripts/bump-version.sh <product_version>" >&2
  status=1
fi

if [ "$v_pyproject" != "$v_init" ] || \
   [ "$v_pyproject" != "$v_rust_compat" ] || \
   [ "$v_pyproject" != "$v_grammar" ]; then
  echo
  echo "✗ Python compatibility version pin drift detected." >&2
  status=1
fi

# Guard against stale traylinx URLs in active package/plugin metadata.
for meta in pyproject.toml .claude-plugin/plugin.json .cursor-plugin/plugin.json; do
  if grep -q "traylinx/lope" "$meta"; then
    echo
    echo "✗ Stale traylinx/lope URL found in $meta (expected makakoo/lope)." >&2
    status=1
  fi
done

if [ "$status" -ne 0 ]; then
  exit "$status"
fi

echo
echo "✓ Product v2 surfaces in sync at $v_cargo (Python compat pin at $v_pyproject)"
exit 0
