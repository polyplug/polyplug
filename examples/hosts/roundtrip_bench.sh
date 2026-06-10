#!/usr/bin/env bash
# roundtrip_bench.sh — measure the full cross-language round trip:
#   host language → runtime (FFI) → NATIVE guest plugin → return data → host.
#
# This is the end-to-end companion to the per-direction charts in PERFORMANCE.md
# (host-call overhead and guest dispatch). It times each host calling the native
# `decoder` plugin's `decode("name,value,42")` in a loop and reads the string
# back. The guest is held constant (a native Rust cdylib) so the only thing that
# varies between bars is the HOST language's binding cost — an honest comparison.
#
# Local-only, like every benchmark here. Writes `<lang> <ns_per_call>` lines to
# docs/assets/benches/roundtrip.txt, then renders cross_lang_roundtrip.svg from
# them in the same run (scripts/gen_bench_charts.py --roundtrip-only).
#
# Each host enters a timed loop only when POLYPLUG_BENCH_ITERS is set, so normal
# (parity) runs are unaffected.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKSPACE_DIR="$(cd "$EXAMPLES_DIR/.." && pwd)"
cd "$EXAMPLES_DIR"

DEPS_DIR="$WORKSPACE_DIR/target/release/deps"
ITERS="${POLYPLUG_BENCH_ITERS:-500000}"
OUT="$WORKSPACE_DIR/docs/assets/benches/roundtrip.txt"

# A native-guest-only plugin set: copy the rust_* example bundles into a fresh
# dir so find_guest_contract always resolves the NATIVE decoder (the full
# examples/plugins dir has 5 implementations of each contract).
NATIVE_PLUGINS="$(mktemp -d)"
for d in "$EXAMPLES_DIR"/plugins/rust_*; do
    cp -rL "$d" "$NATIVE_PLUGINS/"
done
trap 'rm -rf "$NATIVE_PLUGINS"' EXIT

export POLYPLUG_PLUGIN_PATH="$NATIVE_PLUGINS"
export POLYPLUG_BENCH_ITERS="$ITERS"
export LD_LIBRARY_PATH="$DEPS_DIR:${LD_LIBRARY_PATH:-}"
export POLYPLUG_LIB="$DEPS_DIR/libpolyplug.so"
export POLYPLUG_NATIVE_LIB="$DEPS_DIR/libpolyplug_native.so"
export POLYPLUG_LUA_LIB="$DEPS_DIR/libpolyplug_lua.so"
export POLYPLUG_JS_LIB="$DEPS_DIR/libpolyplug_js.so"
export POLYPLUG_PYTHON_LIB="$DEPS_DIR/libpolyplug_python.so"
export PYTHONFAULTHANDLER=1

PYTHON_HOST_PATH="$WORKSPACE_DIR/sdks/python/host:$WORKSPACE_DIR/sdks/python/polyplug_abi:$WORKSPACE_DIR/sdks/python:$WORKSPACE_DIR/sdks/python/loaders/native"
LUA_HOST_PATH="$WORKSPACE_DIR/sdks/lua/host/?.lua;$WORKSPACE_DIR/sdks/lua/abi/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/native/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/lua/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/js/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/python/?.lua;$SCRIPT_DIR/lua/?.lua;;"

echo "round-trip bench: $ITERS iters/host, native guest, $DEPS_DIR" >&2
: > "$OUT"

emit() {  # emit <raw host output>
    grep -oE 'ROUNDTRIP_NS=[0-9.]+ LANG=[a-z]+' | while read -r line; do
        ns="${line#ROUNDTRIP_NS=}"; ns="${ns%% *}"
        lang="${line##*LANG=}"
        printf '%s %s\n' "$lang" "$ns" | tee -a "$OUT" >&2
    done
}

run() {  # run <label> <command...>
    echo "--- $1 ---" >&2
    "${@:2}" 2>/dev/null | emit || echo "  $1: failed/unavailable" >&2
}

# Rust (pipeline_host binary, bench mode)
[ -x "$WORKSPACE_DIR/target/release/pipeline_host" ] && \
    run rust "$WORKSPACE_DIR/target/release/pipeline_host"

# Python
command -v python3 >/dev/null && \
    run python env PYTHONPATH="$PYTHON_HOST_PATH" python3 "$SCRIPT_DIR/python/host.py"

# Lua (LuaJIT)
command -v luajit >/dev/null && \
    run lua env LUA_PATH="$LUA_HOST_PATH" luajit "$SCRIPT_DIR/lua/host.lua"

# JavaScript (Deno)
command -v deno >/dev/null && \
    run js deno run --allow-read --allow-ffi --allow-env "$SCRIPT_DIR/js/host.js"

# C++ (prebuilt host binary, bench mode)
[ -x "$SCRIPT_DIR/cpp/host" ] && run cpp "$SCRIPT_DIR/cpp/host"

# C# (dotnet run)
if command -v dotnet >/dev/null && [ -f "$SCRIPT_DIR/csharp/Host.csproj" ]; then
    run csharp bash -c "cd '$SCRIPT_DIR/csharp' && dotnet run -c Release"
fi

echo "" >&2
echo "wrote $OUT:" >&2
cat "$OUT" >&2

# Render the SVG from the freshly-measured numbers so this is a single command.
# --roundtrip-only skips the criterion charts (no cargo bench needed here).
if command -v python3 >/dev/null; then
    echo "" >&2
    python3 "$WORKSPACE_DIR/scripts/gen_bench_charts.py" --roundtrip-only \
        "$WORKSPACE_DIR/target/criterion" "$WORKSPACE_DIR/docs/assets/benches" >&2
fi
