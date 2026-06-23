#!/usr/bin/env bash
#
# js_install_smoke.sh — prove the PUBLISHED npm packages work when installed CLEAN.
#
# Why this exists: a real bug shipped where the npm packages contained raw `.ts`
# and Node refused to run them (ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING).
# The fix transpiles each package to `dist/` JS. This gate replicates the npm
# publish path from .github/workflows/release.yml EXACTLY (tsc per package +
# embed the prebuilt linux native into @polyplug/host), packs the tarballs,
# installs them into a fresh consumer dir, and imports/uses them — catching
# raw-.ts shipping and broken cross-package specifiers.
#
# It does NOT run cargo (reuses target/release/libpolyplug.so) and NEVER runs
# any git command. All staged build output is cleaned up on exit.
#
set -euo pipefail

# --------------------------------------------------------------------------
# Locations
# --------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
JS_DIR="$REPO_ROOT/sdks/js"
HOST_DIR="$JS_DIR/host"
PREBUILT_SO="$REPO_ROOT/target/release/libpolyplug.so"

# npm package directories, in dependency order (matches release.yml):
#   abi first (host imports it) -> host (loaders import its types) -> guest
#   -> the 5 loaders (depend on host). guest is pure .js (no transpile).
PKG_DIRS=(abi host guest loaders/native loaders/python loaders/lua loaders/js loaders/dotnet)
# Subset that transpiles via tsc (guest is already pure .js).
TSC_DIRS=(abi host loaders/native loaders/python loaders/lua loaders/js loaders/dotnet)

# The single native file we copy INTO the source tree (host dist). Cleaned on exit.
EMBED_NATIVE_DIR="$HOST_DIR/dist/polyplug/_native/linux-x64"
EMBED_NATIVE_FILE="$EMBED_NATIVE_DIR/libpolyplug.so"

# Temp dirs (created later, removed on exit).
TARBALL_DIR=""
CONSUMER_DIR=""

fail() {
  echo "JS SMOKE FAILED: $*" >&2
  exit 1
}

cleanup() {
  # NEVER use git here. Only remove what we created/copied.
  # 1. The native we embedded into the host source dist.
  rm -f "$EMBED_NATIVE_FILE" 2>/dev/null || true
  rmdir "$EMBED_NATIVE_DIR" 2>/dev/null || true
  rmdir "$HOST_DIR/dist/polyplug/_native" 2>/dev/null || true
  # 2. Temp tarball + consumer dirs.
  [ -n "$TARBALL_DIR" ] && rm -rf "$TARBALL_DIR" 2>/dev/null || true
  [ -n "$CONSUMER_DIR" ] && rm -rf "$CONSUMER_DIR" 2>/dev/null || true
}
trap cleanup EXIT

# --------------------------------------------------------------------------
# Preconditions
# --------------------------------------------------------------------------
command -v node >/dev/null 2>&1 || fail "node not found on PATH"
command -v npm  >/dev/null 2>&1 || fail "npm not found on PATH"
command -v npx  >/dev/null 2>&1 || fail "npx not found on PATH"
command -v deno >/dev/null 2>&1 || fail "deno not found on PATH"
if command -v bun >/dev/null 2>&1; then HAVE_BUN=1; else HAVE_BUN=0; fi

# DO NOT run cargo. The prebuilt native must already exist.
if [ ! -f "$PREBUILT_SO" ]; then
  fail "prebuilt native missing: $PREBUILT_SO (build it with 'cargo build --release -p polyplug'; this script will NOT run cargo)"
fi

if [ "$HAVE_BUN" = "1" ]; then BUN_VER="$(bun --version)"; else BUN_VER="(absent)"; fi
echo "==> node $(node --version), npm $(npm --version), deno $(deno --version | head -1), bun $BUN_VER"
echo "==> using prebuilt native: $PREBUILT_SO"

# --------------------------------------------------------------------------
# 1. Transpile each JS package EXACTLY as release.yml does (tsc per package).
#    Build order: abi -> host -> loaders. guest is pure .js (no transpile).
# --------------------------------------------------------------------------
echo "==> transpiling JS SDKs to dist/ (.js + .d.ts) via tsc"
tsc() { npx -y -p typescript@5.7 tsc "$@"; }
for d in "${TSC_DIRS[@]}"; do
  echo "    tsc -p $d/tsconfig.json"
  ( cd "$JS_DIR/$d" && tsc -p tsconfig.json )
done

# Sanity: the transpiled entry points must exist as .js (proves tsc ran).
[ -f "$JS_DIR/abi/dist/polyplug_abi.js" ]      || fail "abi did not transpile (dist/polyplug_abi.js missing)"
[ -f "$HOST_DIR/dist/mod.js" ]                 || fail "host did not transpile (dist/mod.js missing)"
[ -f "$JS_DIR/loaders/native/dist/mod.js" ]    || fail "loaders-native did not transpile (dist/mod.js missing)"

# --------------------------------------------------------------------------
#    Embed the prebuilt linux native into @polyplug/host's dist (mirror
#    release.yml "Embed natives into @polyplug/host"). native-loader.js resolves
#    "./_native/<platform>/" relative to dist/polyplug/, so stage it there.
# --------------------------------------------------------------------------
echo "==> embedding native into @polyplug/host dist"
mkdir -p "$EMBED_NATIVE_DIR"
cp "$PREBUILT_SO" "$EMBED_NATIVE_FILE"
[ -f "$EMBED_NATIVE_FILE" ] || fail "failed to embed native at $EMBED_NATIVE_FILE"

# --------------------------------------------------------------------------
# 2. npm pack each package into a temp dir, then install ONLY the tarballs into
#    a fresh consumer dir.
# --------------------------------------------------------------------------
TARBALL_DIR="$(mktemp -d)"
CONSUMER_DIR="$(mktemp -d)"
echo "==> packing tarballs into $TARBALL_DIR"

TARBALLS=()
for d in "${PKG_DIRS[@]}"; do
  # `npm pack --pack-destination` writes the .tgz and prints its filename.
  tgz_name="$( cd "$JS_DIR/$d" && npm pack --pack-destination "$TARBALL_DIR" --silent )"
  tgz_path="$TARBALL_DIR/$tgz_name"
  [ -f "$tgz_path" ] || fail "npm pack did not produce $tgz_path for $d"
  echo "    packed $d -> $tgz_name"
  TARBALLS+=("$tgz_path")
done

# Confirm the host tarball actually contains the embedded native + transpiled JS
# (catches a tarball that shipped raw .ts or dropped the native).
echo "==> verifying host tarball contents"
host_tgz="$(printf '%s\n' "${TARBALLS[@]}" | grep -E 'polyplug-host-' || true)"
[ -n "$host_tgz" ] || fail "could not identify @polyplug/host tarball"
host_listing="$(tar -tzf "$host_tgz")"
echo "$host_listing" | grep -q 'package/dist/mod.js' \
  || fail "host tarball missing dist/mod.js (transpile/pack regression)"
echo "$host_listing" | grep -q 'package/dist/polyplug/_native/linux-x64/libpolyplug.so' \
  || fail "host tarball missing embedded linux native"
# Raw-.ts guard: native-loader.ts is intentionally shipped as .ts and imported
# from mod.js, but the ABI/runtime surface must be .js. Fail if any top-level
# dist .ts (other than the known native-loader and .d.ts) is the entry surface.
if echo "$host_listing" | grep -E 'package/dist/mod\.ts$' >/dev/null; then
  fail "host tarball ships raw dist/mod.ts (ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING bug class)"
fi

echo "==> creating fresh consumer at $CONSUMER_DIR"
( cd "$CONSUMER_DIR" && npm init -y >/dev/null )
# Install ONLY from the local tarballs. All cross-package deps (@polyplug/abi,
# @polyplug/host) must resolve from the tarballs already in the install set.
# Saved to package.json (no --no-save) so Deno's --node-modules-dir resolution
# sees the bare "@polyplug/*" specifiers as declared dependencies.
echo "==> npm install <tarballs>"
( cd "$CONSUMER_DIR" && npm install "${TARBALLS[@]}" )

# Confirm the installed @polyplug/abi + @polyplug/guest are transpiled .js, not
# raw .ts (the exact bug this gate guards).
[ -f "$CONSUMER_DIR/node_modules/@polyplug/abi/dist/polyplug_abi.js" ] \
  || fail "installed @polyplug/abi has no dist/polyplug_abi.js (raw .ts regression)"
[ -f "$CONSUMER_DIR/node_modules/@polyplug/guest/polyplug_guest.js" ] \
  || fail "installed @polyplug/guest has no polyplug_guest.js"
[ -f "$CONSUMER_DIR/node_modules/@polyplug/host/dist/polyplug/_native/linux-x64/libpolyplug.so" ] \
  || fail "installed @polyplug/host has no embedded linux native"

# --------------------------------------------------------------------------
# 3a. Smoke under NODE: import the runtime-agnostic packages and read exported
#     symbols. This proves they are real transpiled JS, not raw .ts (Node would
#     throw ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING on raw .ts).
# --------------------------------------------------------------------------
echo "==> NODE smoke: import @polyplug/abi + @polyplug/guest"
cat > "$CONSUMER_DIR/node_smoke.mjs" <<'NODE_EOF'
import * as abi from "@polyplug/abi";
import * as guest from "@polyplug/guest";

// abi exports the ABI struct offset/size constants (e.g. HostApi layout).
if (typeof abi.HOST_API_RUNTIME_OFFSET !== "number") {
  throw new Error("@polyplug/abi did not export HOST_API_RUNTIME_OFFSET");
}
if (typeof abi.GUEST_CONTRACT_INTERFACE_SIZE !== "number") {
  throw new Error("@polyplug/abi did not export GUEST_CONTRACT_INTERFACE_SIZE");
}

// guest exports POLYPLUG_ABI_VERSION + AbiErrorCode (Rule 17 canonical enum).
if (guest.POLYPLUG_ABI_VERSION !== 1) {
  throw new Error("@polyplug/guest POLYPLUG_ABI_VERSION !== 1, got " + guest.POLYPLUG_ABI_VERSION);
}
if (!guest.AbiErrorCode || guest.AbiErrorCode.Ok !== 0) {
  throw new Error("@polyplug/guest AbiErrorCode.Ok !== 0");
}

console.log("NODE OK: abi.HOST_API_RUNTIME_OFFSET=" + abi.HOST_API_RUNTIME_OFFSET +
            " guest.POLYPLUG_ABI_VERSION=" + guest.POLYPLUG_ABI_VERSION);
NODE_EOF
( cd "$CONSUMER_DIR" && node node_smoke.mjs )

# --------------------------------------------------------------------------
# 3b. Smoke under DENO: import @polyplug/host (+ @polyplug/loaders-native) FROM
#     the installed node_modules, resolve the embedded linux native, dlopen it
#     via the host SDK, and construct + destroy a real Runtime. This proves the
#     embedded native actually loads through Deno.dlopen.
# --------------------------------------------------------------------------
echo "==> DENO smoke: load embedded native + construct Runtime"
cat > "$CONSUMER_DIR/deno_smoke.ts" <<'DENO_EOF'
import {
  loadNativeLibrary,
  openPolyplug,
  runtimeNew,
} from "@polyplug/host";
// Importing the loader package proves it transpiled and its "@polyplug/host"
// specifier resolves from the installed node_modules.
import { registerNativeLoader } from "@polyplug/loaders-native";

if (typeof registerNativeLoader !== "function") {
  throw new Error("@polyplug/loaders-native did not export registerNativeLoader");
}

// Resolve the embedded native (POLYPLUG_LIB env override honored if set).
const resolved = loadNativeLibrary();
console.log("resolved native: " + resolved.path + " (embedded=" + resolved.isEmbedded +
            ", platform=" + resolved.platform + ")");

// dlopen the core native and create a real runtime — this is the load that the
// shipped-raw-.ts bug and any broken native embed would fail.
const lib = openPolyplug(resolved.path);
const rt = runtimeNew(lib);
if (rt.host() === null) {
  throw new Error("polyplug_runtime_create returned a null HostApi pointer");
}
rt.destroy();
lib.close();
console.log("DENO OK: native loaded + Runtime created/destroyed");
DENO_EOF

# Resolve the host package's module path so Deno can use a Node-style import map.
# Deno resolves bare "@polyplug/*" specifiers from node_modules when run with
# --node-modules-dir against the consumer dir.
(
  cd "$CONSUMER_DIR"
  # If embedded resolution somehow fails, the script honors POLYPLUG_LIB; we do
  # NOT set it here so the embedded-native path is what gets proven. Deno needs
  # FFI + read + env + the node_modules resolution against the consumer install.
  # --node-modules-dir=manual: use the EXISTING node_modules from the clean npm
  # install as-is (the consumer's package.json references the tarballs via
  # file: specifiers, which Deno only honors in manual mode). This is precisely
  # the "import from the installed node_modules" path we want to prove.
  deno run \
    --allow-ffi \
    --allow-read \
    --allow-env \
    --node-modules-dir=manual \
    deno_smoke.ts
)

# --------------------------------------------------------------------------
# 3c/3d. Smoke under NODE and BUN: import @polyplug/host (+ @polyplug/loaders-native)
#     FROM the installed node_modules, resolve the embedded linux native, dlopen
#     it via the host SDK, and construct + destroy a real Runtime. This is the
#     key proof that the PUBLISHED Node (koffi) and Bun (bun:ffi) FFI backends
#     load through the transpiled dist/ package — getBackend() picks the right
#     backend per runtime, and ffi/index.js lazy-requires dist/ffi/node.js /
#     bun.js. A broken dist transpile or a stale require specifier fails here.
#
# Both runtimes execute the SAME ESM file (.mjs): the backend is runtime-detected,
# so the only difference is which interpreter runs it. The native loads through
# koffi.load under Node and bun:ffi's dlopen under Bun.
# --------------------------------------------------------------------------
cat > "$CONSUMER_DIR/ffi_smoke.mjs" <<'FFI_EOF'
import {
  loadNativeLibrary,
  openPolyplug,
  runtimeNew,
} from "@polyplug/host";
// Importing the loader package proves it transpiled and its "@polyplug/host"
// specifier resolves from the installed node_modules.
import { registerNativeLoader } from "@polyplug/loaders-native";

const runtime = typeof Bun !== "undefined" ? "BUN" : "NODE";

if (typeof registerNativeLoader !== "function") {
  throw new Error("@polyplug/loaders-native did not export registerNativeLoader");
}

// Resolve the embedded native (POLYPLUG_LIB env override honored if set).
const resolved = loadNativeLibrary();
console.log(runtime + " resolved native: " + resolved.path + " (embedded=" +
            resolved.isEmbedded + ", platform=" + resolved.platform + ")");

// dlopen the core native and create a real runtime — this drives getBackend()
// under the current runtime, exercising the published koffi/bun:ffi FFI path.
const lib = openPolyplug(resolved.path);
const rt = runtimeNew(lib);
if (rt.host() === null) {
  throw new Error("polyplug_runtime_create returned a null HostApi pointer");
}
rt.destroy();
lib.close();
console.log(runtime + " OK: native loaded via FFI backend + Runtime created/destroyed");
FFI_EOF

echo "==> NODE-FFI smoke: load embedded native + construct Runtime (koffi backend)"
( cd "$CONSUMER_DIR" && node ffi_smoke.mjs )

if [ "$HAVE_BUN" = "1" ]; then
  echo "==> BUN-FFI smoke: load embedded native + construct Runtime (bun:ffi backend)"
  ( cd "$CONSUMER_DIR" && bun ffi_smoke.mjs )
else
  echo "==> BUN-FFI smoke: SKIPPED — bun not on PATH (CI installs it; install bun to run this leg locally)"
fi

echo "JS SMOKE OK"
