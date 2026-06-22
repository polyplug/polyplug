#!/usr/bin/env bash
#
# nuget_install_smoke.sh — clean-install smoke test for the PUBLISHED NuGet packages.
#
# Proves the Polyplug.* NuGet packages work when restored CLEAN from a local feed,
# catching packaging regressions that `dotnet build` inside the repo cannot:
#   - the prebuilt native (libpolyplug.so) not landing in runtimes/<rid>/native/
#   - a missing transitive package dependency (Abi -> Host -> Loaders.Native)
#   - the RID-conventional native not being resolved at runtime
#
# It replicates the `dotnet pack` steps of .github/workflows/release.yml's NuGet
# publish job EXACTLY (RID embedding into runtimes/linux-x64/native/), packs into a
# throwaway local feed, then restores a fresh console app that points ONLY at that
# feed and runs a minimal native-touching program.
#
# It does NOT run cargo and NEVER touches git. It reuses the prebuilt
# target/release/*.so binaries; if any is missing it stops with an error.
#
# Usage: examples/smoke/nuget_install_smoke.sh

set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────────────
RID="linux-x64"

# Repo root = two levels up from this script (examples/smoke/ -> repo root).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Package version = the workspace Cargo.toml version (the single source release.yml
# derives every package version from), so this gate never drifts on a version bump.
PKG_VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
[ -n "$PKG_VERSION" ] || { echo "NUGET SMOKE FAILED: could not read version from Cargo.toml" >&2; exit 1; }

CSHARP_DIR="$REPO_ROOT/sdks/csharp"
RELEASE_DIR="$REPO_ROOT/target/release"

# The csproj `runtimes/` dirs we populate (so we can clean ONLY those afterward).
HOST_RUNTIME_DIR="$CSHARP_DIR/host/runtimes/$RID/native"
declare -a LOADER_RUNTIME_DIRS=()

# Temp dirs and obj/bin we create, cleaned up unconditionally on exit.
FEED_DIR=""
APP_DIR=""
declare -a CREATED_OBJBIN=()

# ── Helpers ────────────────────────────────────────────────────────────────────
fail() {
  echo "NUGET SMOKE FAILED: $*" >&2
  exit 1
}

cleanup() {
  local status=$?
  # Remove native files copied into the csproj runtimes/ trees, restoring the source tree.
  rm -rf "$CSHARP_DIR/host/runtimes" 2>/dev/null || true
  for d in "${LOADER_RUNTIME_DIRS[@]}"; do
    # d is .../loaders/<Name>/runtimes/<rid>/native ; strip back to .../runtimes
    local loader_runtimes="${d%/runtimes/*}/runtimes"
    rm -rf "$loader_runtimes" 2>/dev/null || true
  done
  # Remove obj/bin dirs we caused in the source csproj trees.
  for ob in "${CREATED_OBJBIN[@]}"; do
    rm -rf "$ob" 2>/dev/null || true
  done
  # Remove temp dirs.
  [ -n "$FEED_DIR" ] && rm -rf "$FEED_DIR" 2>/dev/null || true
  [ -n "$APP_DIR" ] && rm -rf "$APP_DIR" 2>/dev/null || true
  if [ "$status" -ne 0 ]; then
    echo "NUGET SMOKE FAILED (exit $status)" >&2
  fi
  exit "$status"
}
trap cleanup EXIT

require_so() {
  local path="$1"
  [ -f "$path" ] || fail "missing prebuilt native '$path' — build it first (this script does NOT run cargo)"
}

# Track obj/bin a pack/build creates under a project dir so cleanup can remove them.
track_objbin() {
  local proj_dir="$1"
  CREATED_OBJBIN+=("$proj_dir/obj" "$proj_dir/bin")
}

# ── Preconditions ──────────────────────────────────────────────────────────────
command -v dotnet >/dev/null 2>&1 || fail "dotnet not found on PATH"

require_so "$RELEASE_DIR/libpolyplug.so"
for loader in native python lua js dotnet; do
  require_so "$RELEASE_DIR/libpolyplug_${loader}.so"
done

FEED_DIR="$(mktemp -d)"
APP_DIR="$(mktemp -d)"

echo "== polyplug NuGet clean-install smoke =="
echo "repo:    $REPO_ROOT"
echo "feed:    $FEED_DIR"
echo "app:     $APP_DIR"
echo "version: $PKG_VERSION  rid: $RID"
echo

# ── 1. Pack every publishable C# project into the local feed ────────────────────
#    Replicates .github/workflows/release.yml NuGet publish steps, but sources the
#    prebuilt natives from target/release/ instead of dist/native/.

echo "-- pack Polyplug.Abi --"
track_objbin "$CSHARP_DIR/abi"
dotnet pack "$CSHARP_DIR/abi/Polyplug.Abi.csproj" -c Release -o "$FEED_DIR" \
  || fail "dotnet pack Polyplug.Abi failed"

echo "-- pack Polyplug.Host (embedding core native) --"
mkdir -p "$HOST_RUNTIME_DIR"
cp "$RELEASE_DIR/libpolyplug.so" "$HOST_RUNTIME_DIR/" \
  || fail "failed to copy libpolyplug.so into host runtimes/"
track_objbin "$CSHARP_DIR/host"
dotnet pack "$CSHARP_DIR/host/Polyplug.Host.csproj" -c Release -o "$FEED_DIR" \
  || fail "dotnet pack Polyplug.Host failed"

echo "-- pack Polyplug.Guest --"
track_objbin "$CSHARP_DIR/guest"
dotnet pack "$CSHARP_DIR/guest" -c Release -o "$FEED_DIR" \
  || fail "dotnet pack Polyplug.Guest failed"

echo "-- pack Polyplug.Loaders.* (embedding loader natives) --"
for loader in Native Python Lua Js Dotnet; do
  loader_dir="$CSHARP_DIR/loaders/$loader"
  loader_lower="$(echo "$loader" | tr '[:upper:]' '[:lower:]')"
  loader_native_dir="$loader_dir/runtimes/$RID/native"
  LOADER_RUNTIME_DIRS+=("$loader_native_dir")
  mkdir -p "$loader_native_dir"
  cp "$RELEASE_DIR/libpolyplug_${loader_lower}.so" "$loader_native_dir/" \
    || fail "failed to copy libpolyplug_${loader_lower}.so into $loader runtimes/"
  track_objbin "$loader_dir"
  dotnet pack "$loader_dir" -c Release -o "$FEED_DIR" \
    || fail "dotnet pack Polyplug.Loaders.$loader failed"
done

echo
echo "-- packed packages --"
ls -1 "$FEED_DIR"/*.nupkg || fail "no .nupkg produced into local feed"
echo

# ── 2. Fresh console app restoring ONLY from the local feed ─────────────────────
echo "-- create fresh console app --"
dotnet new console -o "$APP_DIR" -f net10.0 >/dev/null \
  || fail "dotnet new console failed"

# nuget.config pointing ONLY at the local feed — <clear/> drops nuget.org so the
# packed .nupkg files are the only restore source. Any missing transitive
# dependency therefore fails the restore instead of silently pulling from nuget.org.
cat > "$APP_DIR/nuget.config" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <packageSources>
    <clear />
    <add key="local" value="$FEED_DIR" />
  </packageSources>
</configuration>
EOF

echo "-- add Polyplug.Host + Polyplug.Loaders.Native (v$PKG_VERSION) --"
dotnet add "$APP_DIR" package Polyplug.Host --version "$PKG_VERSION" \
  || fail "dotnet add package Polyplug.Host failed (packaging or transitive-dep regression)"
dotnet add "$APP_DIR" package Polyplug.Loaders.Native --version "$PKG_VERSION" \
  || fail "dotnet add package Polyplug.Loaders.Native failed (packaging or transitive-dep regression)"

# ── 3. Minimal native-touching program ──────────────────────────────────────────
#    RuntimeBuilder().Build() calls polyplug_runtime_create in libpolyplug.so;
#    RegisterNativeLoader() calls polyplug_native_loader_create in
#    libpolyplug_native.so. Both natives must resolve from the restored
#    runtimes/linux-x64/native/ for this to succeed.
cat > "$APP_DIR/Program.cs" <<'EOF'
using Polyplug.Host;
using Polyplug.Loaders.Native;

// Construct the runtime (resolves libpolyplug.so from runtimes/<rid>/native/).
var runtime = new RuntimeBuilder().Build();

// Register the native loader (resolves libpolyplug_native.so the same way).
runtime.RegisterNativeLoader();

Console.WriteLine("NUGET SMOKE OK");
EOF

# ── 4. Run it from the restored package graph ───────────────────────────────────
echo
echo "-- dotnet run --"
RUN_OUTPUT="$(dotnet run --project "$APP_DIR" -c Release 2>&1)" || {
  echo "$RUN_OUTPUT" >&2
  fail "dotnet run failed — native likely not resolved from restored runtimes/$RID/native/"
}
echo "$RUN_OUTPUT"

if ! grep -q "NUGET SMOKE OK" <<<"$RUN_OUTPUT"; then
  fail "program did not print 'NUGET SMOKE OK'"
fi

echo
echo "NUGET SMOKE PASSED"
# cleanup() runs via the EXIT trap and restores the tree.
