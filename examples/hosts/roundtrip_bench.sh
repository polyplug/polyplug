#!/usr/bin/env bash
# roundtrip_bench.sh — measure the full cross-language round trip as a MATRIX:
#   every HOST language → runtime (FFI) → a GUEST plugin of language Y → return.
#
# Each cell is one host language calling the same `decode("name,value,42")`
# contract, where the guest is implemented in language Y. Sweeping host × guest
# gives the full picture: "if I write my app in Rust and my plugin in Lua, what
# does a call cost?" — every combination, not just one axis.
#
#   hosts  : rust, cpp, csharp, python, lua, js   (examples/hosts/*)
#   guests : rust, cpp, lua, js, python, csharp   (examples/plugins/<lang>_*;
#            csharp bundles live in examples/plugins-csharp — they need the
#            .NET loader, which every example host now registers — so the
#            full 6×6 matrix is measured, no N/A column.)
#
# Local-only, like every benchmark here. The measured numbers are piped straight
# into scripts/gen_bench_charts.py, which renders cross_lang_matrix.svg. Nothing
# is committed except the SVG — there is no intermediate data file in the repo.
#
# Each host enters a timed loop only when POLYPLUG_BENCH_ITERS is set, so normal
# (parity) runs are unaffected.
#
# --hostcall mode: instead of the matrix, measure the BARE host → runtime call
# (one find_guest_contract lookup, no guest dispatch) per host language. The
# hosts print an additional `HOSTCALL_NS=<n> LANG=<host>` line in the same
# POLYPLUG_BENCH_ITERS-gated run; this mode collects those into a
# `<host> <ns>` data file and renders cross_lang_host.svg.
set -uo pipefail

MODE="matrix"
if [ "${1:-}" = "--hostcall" ]; then
    MODE="hostcall"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKSPACE_DIR="$(cd "$EXAMPLES_DIR/.." && pwd)"
cd "$EXAMPLES_DIR"

DEPS_DIR="$WORKSPACE_DIR/target/release/deps"
ITERS="${POLYPLUG_BENCH_ITERS:-200000}"

# Guest languages with a built `*_decoder` bundle. Native-process hosts can load
# any of them. The csharp guest lives in examples/plugins-csharp (it needs the
# .NET loader, now registered by every example host) — see the per-guest source
# directory selection in the sweep loop below.
GUEST_LANGS=(rust cpp lua js python csharp)

# Shared environment every host needs to find the core lib + loader cdylibs and
# the language SDKs. Mirrors examples/verify_hosts.sh.
export LD_LIBRARY_PATH="$DEPS_DIR:${LD_LIBRARY_PATH:-}"
export POLYPLUG_LIB="$DEPS_DIR/libpolyplug.so"
export POLYPLUG_NATIVE_LIB="$DEPS_DIR/libpolyplug_native.so"
export POLYPLUG_LUA_LIB="$DEPS_DIR/libpolyplug_lua.so"
export POLYPLUG_JS_LIB="$DEPS_DIR/libpolyplug_js.so"
export POLYPLUG_PYTHON_LIB="$DEPS_DIR/libpolyplug_python.so"
export POLYPLUG_DOTNET_LIB="$DEPS_DIR/libpolyplug_dotnet.so"
export PYTHONFAULTHANDLER=1
export POLYPLUG_BENCH_ITERS="$ITERS"

PYTHON_HOST_PATH="$WORKSPACE_DIR/sdks/python/host:$WORKSPACE_DIR/sdks/python/polyplug_abi:$WORKSPACE_DIR/sdks/python:$WORKSPACE_DIR/sdks/python/loaders/native:$WORKSPACE_DIR/sdks/python/loaders/python:$WORKSPACE_DIR/sdks/python/loaders/lua:$WORKSPACE_DIR/sdks/python/loaders/js:$WORKSPACE_DIR/sdks/python/loaders/dotnet"
LUA_HOST_PATH="$WORKSPACE_DIR/sdks/lua/host/?.lua;$WORKSPACE_DIR/sdks/lua/abi/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/native/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/lua/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/js/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/python/?.lua;$WORKSPACE_DIR/sdks/lua/loaders/dotnet/?.lua;$SCRIPT_DIR/lua/?.lua;;"

# Measured data, written as `<host> <guest> <ns>` lines (matrix mode) or
# `<host> <ns>` lines (hostcall mode). A fresh temp file each run — never
# committed; the chart is the only artifact that lands in the repo.
MATRIX="$(mktemp)"
trap 'rm -f "$MATRIX"' EXIT

# run_host <host> — run one host against the current POLYPLUG_PLUGIN_PATH and
# echo its raw stdout (which includes a `ROUNDTRIP_NS=<n> LANG=<host>` line when
# the host reaches its timed loop). Returns non-zero if the host is unavailable.
run_host() {
    case "$1" in
        rust)
            [ -x "$WORKSPACE_DIR/target/release/pipeline_host" ] || return 1
            "$WORKSPACE_DIR/target/release/pipeline_host" 2>/dev/null ;;
        cpp)
            [ -x "$SCRIPT_DIR/cpp/host" ] || return 1
            "$SCRIPT_DIR/cpp/host" 2>/dev/null ;;
        python)
            command -v python3 >/dev/null || return 1
            env PYTHONPATH="$PYTHON_HOST_PATH" python3 "$SCRIPT_DIR/python/main.py" 2>/dev/null ;;
        lua)
            command -v luajit >/dev/null || return 1
            env LUA_PATH="$LUA_HOST_PATH" luajit "$SCRIPT_DIR/lua/host.lua" 2>/dev/null ;;
        js)
            command -v deno >/dev/null || return 1
            deno run --allow-read --allow-ffi --allow-env "$SCRIPT_DIR/js/host.js" 2>/dev/null ;;
        csharp)
            command -v dotnet >/dev/null && [ -f "$SCRIPT_DIR/csharp/Host.csproj" ] || return 1
            ( cd "$SCRIPT_DIR/csharp" && dotnet run -c Release 2>/dev/null ) ;;
        *) return 1 ;;
    esac
}

HOSTS=(rust cpp csharp python lua js)

# ─── hostcall mode: bare host → runtime call, per host language ───────────────
if [ "$MODE" = "hostcall" ]; then
    echo "host-call sweep: $ITERS iters/host, $DEPS_DIR" >&2

    # Any loaded bundle that provides pipeline.decoder makes the lookup hit;
    # native (rust) guests load under every host, and the lookup cost does not
    # depend on the guest language (no guest code runs).
    guest_dir="$(mktemp -d)"
    for d in "$EXAMPLES_DIR/plugins"/rust_*; do
        [ -d "$d" ] && cp -rL "$d" "$guest_dir/"
    done
    export POLYPLUG_PLUGIN_PATH="$guest_dir"

    for host in "${HOSTS[@]}"; do
        ns="$(run_host "$host" | grep -oE 'HOSTCALL_NS=[0-9.]+' | head -1 | cut -d= -f2)"
        if [ -n "$ns" ]; then
            printf '%s %s\n' "$host" "$ns" | tee -a "$MATRIX" >&2
        else
            echo "  $host: failed/unavailable" >&2
        fi
    done

    rm -rf "$guest_dir"

    echo "" >&2
    if command -v python3 >/dev/null && [ -s "$MATRIX" ]; then
        # criterion_dir is unused in --hostcall mode but the positional is required.
        python3 "$WORKSPACE_DIR/scripts/gen_bench_charts.py" --hostcall "$MATRIX" \
            "$WORKSPACE_DIR/target/criterion" "$WORKSPACE_DIR/docs/assets/benches" >&2
        exit 0
    else
        echo "no host-call data collected (no hosts available?)" >&2
        exit 1
    fi
fi

# ─── matrix mode (default): full host × guest round-trip sweep ────────────────
echo "round-trip matrix: $ITERS iters/cell, hosts × guests, $DEPS_DIR" >&2

for guest in "${GUEST_LANGS[@]}"; do
    # Build a plugin set containing ONLY this guest language's bundles, so the
    # contract resolves deterministically to the language under test. Real copies
    # (cp -rL) because the Deno scanner does not follow symlinked plugin dirs.
    guest_dir="$(mktemp -d)"
    # C# guest bundles are published separately (they need the .NET loader); every
    # other guest lives in examples/plugins.
    src_root="$EXAMPLES_DIR/plugins"
    [ "$guest" = "csharp" ] && src_root="$EXAMPLES_DIR/plugins-csharp"
    for d in "$src_root"/"${guest}"_*; do
        [ -d "$d" ] && cp -rL "$d" "$guest_dir/"
    done
    export POLYPLUG_PLUGIN_PATH="$guest_dir"

    echo "--- guest=$guest ---" >&2
    for host in "${HOSTS[@]}"; do
        ns="$(run_host "$host" | grep -oE 'ROUNDTRIP_NS=[0-9.]+' | head -1 | cut -d= -f2)"
        if [ -n "$ns" ]; then
            printf '%s %s %s\n' "$host" "$guest" "$ns" | tee -a "$MATRIX" >&2
        else
            echo "  $host × $guest: failed/unavailable" >&2
        fi
    done

    rm -rf "$guest_dir"
done

echo "" >&2
if command -v python3 >/dev/null && [ -s "$MATRIX" ]; then
    # criterion_dir is unused in --matrix mode but the positional is required.
    python3 "$WORKSPACE_DIR/scripts/gen_bench_charts.py" --matrix "$MATRIX" \
        "$WORKSPACE_DIR/target/criterion" "$WORKSPACE_DIR/docs/assets/benches" >&2
else
    echo "no matrix data collected (no hosts available?)" >&2
    exit 1
fi
