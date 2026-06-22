#!/usr/bin/env bash
#
# lua_install_smoke.sh — clean-install smoke gate for the PUBLISHED LuaRocks packages.
#
# Why this exists: a real bug shipped where `luarocks` installed ZERO Lua modules
# because the rockspec coupled module installation to copying a native binary that
# was not present on the build platform — when that copy aborted, the whole `build`
# aborted and left an empty tree. This gate proves the opposite: from the exact
# source tarball the release workflow ships, a fresh `luarocks make` of every
# rockspec installs the .lua modules AND the platform native, into an empty tree,
# and the installed modules actually load (including the prebuilt core .so).
#
# It deliberately mirrors the release pipeline:
#   1. Assemble the `polyplug-lua-<VER>/` source tree EXACTLY as
#      .github/workflows/release.yml's "Build LuaRocks source tarball" step does
#      (linux-x64 native only — enough for a linux smoke), tar it.
#   2. Unpack the tarball into a fresh workdir, drop the matching rockspecs into the
#      unpacked source.dir, and `luarocks make` abi -> host -> 5 loaders into a
#      fresh --tree. This is what `luarocks install` runs after fetching source.url.
#   3. Smoke with luajit: require every installed module + load the core native.
#      Assert installed module count > 0 (the original bug was zero).
#
# It does NOT run cargo: it reuses the prebuilt target/release/*.so. If a required
# .so is missing it STOPS with a clear error (build the workspace in release first).

set -euo pipefail

# ---------------------------------------------------------------------------
# Paths and version
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

LUA_SDK="$REPO_ROOT/sdks/lua"
RELEASE_DIR="$REPO_ROOT/target/release"

# Read VERSION from the host rockspec filename (e.g. polyplug-0.1.1-1.rockspec).
HOST_ROCKSPEC="$(ls "$LUA_SDK"/host/polyplug-*-*.rockspec 2>/dev/null | head -1 || true)"
if [ -z "$HOST_ROCKSPEC" ]; then
    echo "ERROR: cannot find host rockspec under $LUA_SDK/host/ (polyplug-<VER>-1.rockspec)" >&2
    exit 1
fi
# Strip "polyplug-" prefix and "-1.rockspec" suffix to get the bare version (e.g. 0.1.1).
ROCK_BASENAME="$(basename "$HOST_ROCKSPEC")"
VER="${ROCK_BASENAME#polyplug-}"
VER="${VER%-1.rockspec}"
if [ -z "$VER" ]; then
    echo "ERROR: failed to parse version from rockspec name '$ROCK_BASENAME'" >&2
    exit 1
fi
echo "polyplug Lua version: $VER"

# ---------------------------------------------------------------------------
# Temp dirs (cleaned on exit)
# ---------------------------------------------------------------------------
STAGE_DIR="$(mktemp -d)"   # where the tarball is assembled + the .tar.gz written
WORK_DIR="$(mktemp -d)"    # where the tarball is unpacked + the install --tree lives

cleanup() {
    rm -rf "$STAGE_DIR" "$WORK_DIR"
}
trap cleanup EXIT

# ===========================================================================
# Step 1 — assemble the source tarball EXACTLY as release.yml does (linux only)
# ===========================================================================
TB="polyplug-lua-$VER"
TB_ROOT="$STAGE_DIR/$TB"

mkdir -p "$TB_ROOT/polyplug/loaders" "$TB_ROOT/_native/linux-x64"

# Flat top-level modules: abi, polyplug_abi, polyplug_guest, polyplug.
cp "$LUA_SDK/abi/abi.lua" "$LUA_SDK/abi/polyplug_abi.lua" "$TB_ROOT/"
cp "$LUA_SDK/guest/polyplug_guest.lua" "$TB_ROOT/"
cp "$LUA_SDK/host/polyplug.lua" "$TB_ROOT/"

# polyplug/ submodules: native, runtime, reload_phase.
cp "$LUA_SDK/host/polyplug/native.lua" \
   "$LUA_SDK/host/polyplug/runtime.lua" \
   "$LUA_SDK/host/polyplug/reload_phase.lua" \
   "$TB_ROOT/polyplug/"

# polyplug/loaders/<n>.lua for all 5 loaders.
for n in native python lua js dotnet; do
    cp "$LUA_SDK/loaders/$n/polyplug/loaders/$n.lua" "$TB_ROOT/polyplug/loaders/"
done

# _native/linux-x64/ — populated from PREBUILT target/release/*.so.
# release.yml copies dist/native/linux-x64/*.so; the natives the Lua SDK needs are
# the core (libpolyplug.so) and the per-loader cdylibs (libpolyplug_<loader>.so).
REQUIRED_SOS=(
    libpolyplug.so
    libpolyplug_native.so
    libpolyplug_python.so
    libpolyplug_lua.so
    libpolyplug_js.so
    libpolyplug_dotnet.so
)
for so in "${REQUIRED_SOS[@]}"; do
    if [ ! -f "$RELEASE_DIR/$so" ]; then
        echo "ERROR: prebuilt native missing: $RELEASE_DIR/$so" >&2
        echo "       This gate does NOT run cargo. Build the workspace first:" >&2
        echo "         cargo build --release" >&2
        exit 1
    fi
    cp "$RELEASE_DIR/$so" "$TB_ROOT/_native/linux-x64/"
done

mkdir -p "$STAGE_DIR/lua-src"
TARBALL="$STAGE_DIR/lua-src/$TB.tar.gz"
tar czf "$TARBALL" -C "$STAGE_DIR" "$TB"

echo "=== LuaRocks tarball ==="
tar tzf "$TARBALL" | sort

# ===========================================================================
# Step 2 — unpack, drop rockspecs into source.dir, luarocks make in dep order
# ===========================================================================
tar xzf "$TARBALL" -C "$WORK_DIR"
SRC_DIR="$WORK_DIR/$TB"   # matches source.dir in every rockspec

if [ ! -d "$SRC_DIR" ]; then
    echo "ERROR: unpacked source dir not found: $SRC_DIR" >&2
    exit 1
fi

# Copy the matching rockspecs into the unpacked source.dir. `luarocks make` runs
# the build relative to the rockspec's directory, so the rockspec must sit next to
# the assembled abi.lua / polyplug/ / _native/ tree — exactly the layout an
# `install` produces after fetching+unpacking source.url.
ABI_ROCKSPEC="$LUA_SDK/abi/polyplug-abi-$VER-1.rockspec"
HOST_ROCKSPEC_SRC="$LUA_SDK/host/polyplug-$VER-1.rockspec"
declare -a LOADER_ROCKSPECS
for n in native python lua js dotnet; do
    LOADER_ROCKSPECS+=("$LUA_SDK/loaders/$n/polyplug-loader-$n-$VER-1.rockspec")
done

# Verify all rockspecs exist before copying anything.
for rs in "$ABI_ROCKSPEC" "$HOST_ROCKSPEC_SRC" "${LOADER_ROCKSPECS[@]}"; do
    if [ ! -f "$rs" ]; then
        echo "ERROR: rockspec missing: $rs" >&2
        exit 1
    fi
    cp "$rs" "$SRC_DIR/"
done

TREE="$WORK_DIR/tree"
mkdir -p "$TREE"

# Dependency order: abi (no deps) -> host (deps polyplug-abi) -> loaders (dep polyplug).
# --deps-mode=one keeps resolution confined to this fresh --tree so nothing leaks
# in from a system rocks tree.
make_rock() {
    local rockspec_name="$1"
    echo ">>> luarocks make $rockspec_name"
    ( cd "$SRC_DIR" && luarocks make "$rockspec_name" --tree="$TREE" --no-doc --deps-mode=one )
}

make_rock "polyplug-abi-$VER-1.rockspec"
make_rock "polyplug-$VER-1.rockspec"
for n in native python lua js dotnet; do
    make_rock "polyplug-loader-$n-$VER-1.rockspec"
done

# ===========================================================================
# Step 3 — smoke: require every module + load the core native, assert count > 0
# ===========================================================================
eval "$(luarocks --tree="$TREE" path)"

luajit -e '
local function must_require(name)
    local ok, mod = pcall(require, name)
    if not ok then
        io.stderr:write("FAIL require " .. name .. ": " .. tostring(mod) .. "\n")
        os.exit(1)
    end
    return mod
end

-- All installed pure-Lua modules must load.
must_require("polyplug_abi")
must_require("polyplug")
must_require("polyplug.native")
must_require("polyplug.runtime")
must_require("polyplug.reload_phase")
must_require("polyplug.loaders.native")
must_require("polyplug.loaders.python")
must_require("polyplug.loaders.lua")
must_require("polyplug.loaders.js")
must_require("polyplug.loaders.dotnet")

-- Prove the native actually loads via the installed resolver. The staged
-- _native/linux-x64/libpolyplug.so was installed by the host rockspec; with no
-- env override set, native.resolve falls through to that co-located binary.
local n = require("polyplug.native")
local lib = n.load("POLYPLUG_CORE_LIB", "polyplug")
if lib == nil then
    io.stderr:write("FAIL: core native loaded as nil\n")
    os.exit(1)
end
'

# Assert the installed module COUNT is > 0 — the original bug installed ZERO.
MODULE_COUNT="$(find "$TREE" -name '*.lua' -type f | wc -l | tr -d ' ')"
echo "Installed Lua modules: $MODULE_COUNT"
if [ "$MODULE_COUNT" -le 0 ]; then
    echo "ERROR: zero modules installed — the clean-install bug has regressed." >&2
    exit 1
fi

echo "LUA SMOKE OK"
