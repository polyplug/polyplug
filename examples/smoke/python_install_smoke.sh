#!/usr/bin/env bash
#
# python_install_smoke.sh — prove the PUBLISHED PyPI packages work when
# installed CLEAN.
#
# This catches the class of bug where the SOURCE TREE imports fine but the
# built WHEEL does not. Two real instances have shipped broken:
#   1. the wheel's polyplug_abi/abi.py was a shim re-exporting an unpackaged
#      `abi.abi` module — `import polyplug_abi` failed after install.
#   2. loader native resolution pointed at the wrong path — the .so was never
#      found at runtime even though the module imported.
#
# Strategy: build the publishable wheels EXACTLY as .github/workflows/release.yml
# does (linux-only native, reusing the PREBUILT target/release/*.so — never
# cargo), install ONLY those wheels into a throwaway venv, then run the smoke
# import from a directory OUTSIDE the repo so the source tree cannot shadow the
# installed packages. The native is actually LOADED, not merely imported.
#
# Touches only temp dirs and the sdks/python/**/_native/linux-x64/ copy targets,
# all of which are removed on exit (rm only — never git).

set -euo pipefail

# ---------------------------------------------------------------------------
# Locate the repo root (this script lives at examples/smoke/).
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

RELEASE_DIR="$REPO_ROOT/target/release"

# ---------------------------------------------------------------------------
# Temp working dirs + the in-tree native copy targets we must clean up.
# ---------------------------------------------------------------------------
DIST_DIR="$(mktemp -d)"
VENV_DIR="$(mktemp -d)/venv"
COPIED_NATIVES=()

# The checked-in polyplug_abi/abi.py is a tracked source-tree SHIM. The build
# overwrites it in place (as release.yml does); we must restore its original
# contents on exit, not delete it. Backed up below before the first overwrite.
ABI_SHIM="$REPO_ROOT/sdks/python/polyplug_abi/polyplug_abi/abi.py"
ABI_SHIM_BACKUP="$(mktemp)"

fail() {
    echo "ERROR: $*" >&2
    exit 1
}

cleanup() {
    local status=$?
    # Remove every native we copied into the source tree's _native dirs.
    for f in "${COPIED_NATIVES[@]:-}"; do
        [ -n "$f" ] && rm -f "$f"
    done
    # Restore the tracked abi.py shim from its backup (the build overwrote it).
    if [ -s "$ABI_SHIM_BACKUP" ]; then
        cp "$ABI_SHIM_BACKUP" "$ABI_SHIM"
    fi
    rm -f "$ABI_SHIM_BACKUP"
    rm -rf "$DIST_DIR" "$(dirname "$VENV_DIR")"
    if [ "$status" -ne 0 ]; then
        echo "FAILED: python install smoke did not complete (exit $status)" >&2
    fi
}
trap cleanup EXIT

# Record a copy target so cleanup can remove it, then perform the copy.
copy_native() {
    local src="$1" dst="$2"
    [ -f "$src" ] || fail "missing prebuilt native: $src (build it first; this script will NOT run cargo)"
    mkdir -p "$(dirname "$dst")"
    COPIED_NATIVES+=("$dst")
    cp "$src" "$dst"
}

command -v uv >/dev/null 2>&1 || fail "uv is required but not found on PATH"

echo "=== polyplug Python install smoke ==="
echo "dist dir : $DIST_DIR"
echo "venv dir : $VENV_DIR"
echo

# ---------------------------------------------------------------------------
# 1. Build the publishable wheels (mirrors release.yml publish-pypi, linux-only).
# ---------------------------------------------------------------------------

echo "--- building abi wheel ---"
# The checked-in polyplug_abi/abi.py is a source-tree shim re-exporting the
# unpackaged `abi.abi`; the wheel must instead CONTAIN the generated types.
# Back up the tracked shim first so cleanup can restore it verbatim.
[ -f "$ABI_SHIM" ] || fail "missing tracked abi shim: $ABI_SHIM"
cp "$ABI_SHIM" "$ABI_SHIM_BACKUP"
cp "$REPO_ROOT/sdks/python/abi/abi.py" "$ABI_SHIM"
( cd "$REPO_ROOT/sdks/python/polyplug_abi" && uv build --out-dir "$DIST_DIR" )

echo "--- building host wheel (embeds libpolyplug.so) ---"
copy_native "$RELEASE_DIR/libpolyplug.so" \
    "$REPO_ROOT/sdks/python/host/polyplug/_native/linux-x64/libpolyplug.so"
( cd "$REPO_ROOT/sdks/python/host" && uv build --out-dir "$DIST_DIR" )

echo "--- building guest wheel ---"
( cd "$REPO_ROOT/sdks/python/guest" && uv build --out-dir "$DIST_DIR" )

echo "--- building loader wheels (embed libpolyplug_<loader>.so) ---"
for loader in native python lua js dotnet; do
    loader_dir="$REPO_ROOT/sdks/python/loaders/$loader"
    [ -d "$loader_dir" ] || fail "missing loader dir: $loader_dir"
    copy_native "$RELEASE_DIR/libpolyplug_${loader}.so" \
        "$loader_dir/polyplug_loaders_${loader}/_native/linux-x64/libpolyplug_${loader}.so"
    ( cd "$loader_dir" && uv build --out-dir "$DIST_DIR" )
done

echo
echo "--- built distributions ---"
ls -1 "$DIST_DIR"/*.whl || fail "no wheels were built"
echo

# ---------------------------------------------------------------------------
# 2. Fresh venv; install ONLY the built wheels (no PyPI, no source tree).
# ---------------------------------------------------------------------------
echo "--- creating clean venv ---"
uv venv "$VENV_DIR"

echo "--- installing built wheels into venv ---"
# --no-index: prove the wheels are self-contained; nothing is fetched from PyPI.
uv pip install --python "$VENV_DIR" --no-index "$DIST_DIR"/*.whl

# ---------------------------------------------------------------------------
# 3. Smoke import FROM OUTSIDE THE REPO so the source tree cannot shadow the
#    installed packages — verify the native actually LOADS.
# ---------------------------------------------------------------------------
echo "--- running smoke import from /tmp (outside repo) ---"
PY_BIN="$VENV_DIR/bin/python"
[ -x "$PY_BIN" ] || PY_BIN="$VENV_DIR/Scripts/python.exe"
[ -x "$PY_BIN" ] || fail "could not locate venv python interpreter"

( cd /tmp && "$PY_BIN" - <<'PYEOF'
from polyplug_abi import POLYPLUG_ABI_VERSION
from polyplug import Runtime
from polyplug._native import load_native_lib

# Actually load the native — not just import the module.
load_native_lib()

import polyplug_loaders_native
import polyplug_loaders_python
import polyplug_loaders_lua
import polyplug_loaders_js
import polyplug_loaders_dotnet

print("PY SMOKE OK")
PYEOF
) || fail "smoke import failed — a published wheel is broken when installed clean"

echo
echo "=== python install smoke PASSED ==="
