#!/usr/bin/env bash
# package-release.sh — build a deterministic release tarball + .sha256 sidecar
# from a compiled `lope` binary and checked-in host assets.
#
# Single authoritative archive manifest shared between CI smoke tests and
# `.github/workflows/publish.yml`.
#
# Reproducibility: normalizes file order, uid/gid/uname/gname, mtimes (via
# SOURCE_DATE_EPOCH or HEAD commit timestamp), and gzip header timestamp using
# Python stdlib `tarfile` + `gzip`, while preserving modes and symlinks.
#
# Layout of `<out-dir>/lope-<target>.tar.gz`:
#   lope-<target>/bin/lope
#   lope-<target>/share/lope/install
#   lope-<target>/share/lope/skills/...
#   lope-<target>/share/lope/commands/...
#   lope-<target>/share/lope/docs/reference.md
#   lope-<target>/install.sh
#   lope-<target>/README.md
#   lope-<target>/CHANGELOG.md
#   lope-<target>/LICENSE

set -euo pipefail

TARGET=""
BIN_PATH=""
OUT_DIR=""

while [ $# -gt 0 ]; do
  case "$1" in
    --target)
      [ $# -ge 2 ] || { echo "ERROR: --target requires a value" >&2; exit 2; }
      TARGET="$2"
      shift 2
      ;;
    --bin)
      [ $# -ge 2 ] || { echo "ERROR: --bin requires a value" >&2; exit 2; }
      BIN_PATH="$2"
      shift 2
      ;;
    --out-dir)
      [ $# -ge 2 ] || { echo "ERROR: --out-dir requires a value" >&2; exit 2; }
      OUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 --target <triple> --bin <path/to/lope> --out-dir <dir>"
      exit 0
      ;;
    *)
      echo "ERROR: unknown argument '$1'" >&2
      exit 2
      ;;
  esac
done

[ -n "$TARGET" ] && [ -n "$BIN_PATH" ] && [ -n "$OUT_DIR" ] || {
  echo "Usage: $0 --target <triple> --bin <path/to/lope> --out-dir <dir>" >&2
  exit 2
}

if ! printf '%s\n' "$TARGET" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$' || printf '%s\n' "$TARGET" | grep -q '\.\.'; then
  echo "ERROR: invalid --target '$TARGET' (expected target triple characters only)" >&2
  exit 2
fi

[ -f "$BIN_PATH" ] || { echo "ERROR: binary not found at $BIN_PATH" >&2; exit 1; }

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PYTHON_BIN="${LOPE_PYTHON:-python3}"
EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$REPO_ROOT" log -1 --format=%ct 2>/dev/null || echo 0)}"
PRODUCT_VERSION="$(awk '
  /^\[/ { in_sec = ($0 == "[workspace.package]"); next }
  in_sec && /^version = "/ { sub(/^version = "/, ""); sub(/".*/, ""); print; exit }
' "$REPO_ROOT/Cargo.toml")"
[ -n "$PRODUCT_VERSION" ] || { echo "ERROR: could not read workspace version from Cargo.toml" >&2; exit 1; }
ARCHIVE_NAME="lope-${TARGET}"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

STAGE="$WORK_DIR/$ARCHIVE_NAME"
mkdir -p "$STAGE/bin" "$STAGE/share/lope/docs" "$OUT_DIR"

cp "$BIN_PATH" "$STAGE/bin/lope"
chmod 0755 "$STAGE/bin/lope"

printf '%s\n' "$PRODUCT_VERSION" > "$STAGE/share/lope/VERSION"
cp "$REPO_ROOT/install" "$STAGE/share/lope/install"
chmod 0755 "$STAGE/share/lope/install"

cp -R "$REPO_ROOT/skills" "$STAGE/share/lope/skills"
cp -R "$REPO_ROOT/commands" "$STAGE/share/lope/commands"
cp "$REPO_ROOT/docs/reference.md" "$STAGE/share/lope/docs/reference.md"

cp "$REPO_ROOT/scripts/install-release.sh" "$STAGE/install.sh"
chmod 0755 "$STAGE/install.sh"

for meta in README.md CHANGELOG.md LICENSE; do
  [ -f "$REPO_ROOT/$meta" ] && cp "$REPO_ROOT/$meta" "$STAGE/$meta"
done

TARBALL="lope-${TARGET}.tar.gz"
"$PYTHON_BIN" - "$WORK_DIR" "$ARCHIVE_NAME" "$OUT_DIR/$TARBALL" "$EPOCH" <<'PYEOF'
import gzip
import os
import stat
import sys
import tarfile

work_dir, root_name, out_path, epoch_s = sys.argv[1:]
epoch = int(epoch_s)
stage_root = os.path.join(work_dir, root_name)

def collect(path):
    entries = [path]
    if os.path.isdir(path) and not os.path.islink(path):
        for name in sorted(os.listdir(path)):
            entries.extend(collect(os.path.join(path, name)))
    return entries

with open(out_path, "wb") as raw:
    with gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=epoch) as gz:
        with tarfile.open(fileobj=gz, mode="w|", format=tarfile.GNU_FORMAT) as tf:
            for full_path in collect(stage_root):
                arcname = os.path.relpath(full_path, work_dir)
                st = os.lstat(full_path)
                ti = tf.gettarinfo(full_path, arcname=arcname)
                ti.mtime = epoch
                ti.uid = 0
                ti.gid = 0
                ti.uname = ""
                ti.gname = ""
                if stat.S_ISREG(st.st_mode):
                    ti.mode = 0o755 if (st.st_mode & 0o111) else 0o644
                elif stat.S_ISDIR(st.st_mode):
                    ti.mode = 0o755
                if ti.isreg():
                    with open(full_path, "rb") as fh:
                        tf.addfile(ti, fh)
                else:
                    tf.addfile(ti)
PYEOF

if command -v sha256sum >/dev/null 2>&1; then
  ( cd "$OUT_DIR" && sha256sum "$TARBALL" > "$TARBALL.sha256" )
else
  ( cd "$OUT_DIR" && shasum -a 256 "$TARBALL" > "$TARBALL.sha256" )
fi

echo "Packaged: $OUT_DIR/$TARBALL"
cat "$OUT_DIR/$TARBALL.sha256"
