#!/usr/bin/env bash
# examples/verify_hosts.sh — Run all polyplug host examples and diff against golden.txt
#
# For each host, captures its stdout and compares it against examples/hosts/golden.txt.
# Reports PASS/FAIL per host and exits 0 only if all hosts match.
#
# Usage:
#   bash examples/verify_hosts.sh
#   ./examples/verify_hosts.sh

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOSTS_DIR="${SCRIPT_DIR}/hosts"
GOLDEN="${HOSTS_DIR}/golden.txt"

# Tracking
PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
declare -a RESULTS=()

# Colours (disabled if not a terminal)
if [[ -t 1 ]]; then
    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[0;33m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    GREEN=''
    RED=''
    YELLOW=''
    BOLD=''
    RESET=''
fi

# ---------------------------------------------------------------------------
# check_golden — verify golden.txt exists
# ---------------------------------------------------------------------------
if [[ ! -f "${GOLDEN}" ]]; then
    printf "${RED}ERROR${RESET}: golden.txt not found at %s\n" "${GOLDEN}" >&2
    exit 2
fi

# ---------------------------------------------------------------------------
# run_host LABEL CMD...
#   Runs CMD, captures stdout, diffs against golden.txt.
#   Records PASS / FAIL in RESULTS.
# ---------------------------------------------------------------------------
run_host() {
    local label="$1"
    shift
    local cmd=("$@")

    printf "  %-32s " "${label} ..."

    local actual
    actual=$("${cmd[@]}" 2>/dev/null)
    local exit_code=$?

    if [[ ${exit_code} -ne 0 ]]; then
        printf "${RED}FAIL${RESET} (host exited with code %d)\n" "${exit_code}"
        RESULTS+=("FAIL  ${label} (exit code ${exit_code})")
        (( FAIL_COUNT += 1 ))
        return
    fi

    local diff_output
    diff_output=$(diff <(printf '%s\n' "${actual}") "${GOLDEN}" 2>&1)
    local diff_code=$?

    if [[ ${diff_code} -eq 0 ]]; then
        printf "${GREEN}PASS${RESET}\n"
        RESULTS+=("PASS  ${label}")
        (( PASS_COUNT += 1 ))
    else
        printf "${RED}FAIL${RESET} (output differs from golden.txt)\n"
        RESULTS+=("FAIL  ${label} (diff mismatch)")
        (( FAIL_COUNT += 1 ))
        # Print diff indented so it's visible but distinct
        printf '%s\n' "${diff_output}" | sed 's/^/        /' >&2
    fi
}

# ---------------------------------------------------------------------------
# skip_host LABEL REASON
# ---------------------------------------------------------------------------
skip_host() {
    local label="$1"
    local reason="$2"
    printf "  %-32s ${YELLOW}SKIP${RESET} (%s)\n" "${label} ..." "${reason}"
    RESULTS+=("SKIP  ${label} (${reason})")
    (( SKIP_COUNT += 1 ))
}

# ===========================================================================
# MAIN
# ===========================================================================

printf "\n${BOLD}polyplug — verify all hosts against golden.txt${RESET}\n"
printf "Golden: %s\n" "${GOLDEN}"
printf "%-50s\n" "$(printf '%.0s─' {1..50})"
printf "\n"

# ── Rust host ────────────────────────────────────────────────────────────────
RUST_HOST_DIR="${HOSTS_DIR}/rust_minimal"
if [[ -d "${RUST_HOST_DIR}" ]]; then
    run_host "rust  rust_minimal" \
        cargo run --quiet --manifest-path "${RUST_HOST_DIR}/Cargo.toml"
else
    skip_host "rust  rust_minimal" "directory not found: ${RUST_HOST_DIR}"
fi

# ── C++ host ─────────────────────────────────────────────────────────────────
CPP_HOST_BIN="${HOSTS_DIR}/cpp/host"
if [[ -f "${CPP_HOST_BIN}" ]]; then
    run_host "cpp   cpp/host" "${CPP_HOST_BIN}"
elif command -v make &>/dev/null && [[ -f "${HOSTS_DIR}/cpp/Makefile" ]]; then
    # Attempt to build first, then run
    make --quiet -C "${HOSTS_DIR}/cpp" 2>/dev/null || true
    if [[ -f "${CPP_HOST_BIN}" ]]; then
        run_host "cpp   cpp/host" "${CPP_HOST_BIN}"
    else
        skip_host "cpp   cpp/host" "binary not found after make; run 'make' in hosts/cpp"
    fi
else
    skip_host "cpp   cpp/host" "binary not found at hosts/cpp/host; build with 'make'"
fi

# ── Python host ──────────────────────────────────────────────────────────────
PYTHON_HOST="${HOSTS_DIR}/python/host.py"
if [[ -f "${PYTHON_HOST}" ]]; then
    if command -v python3 &>/dev/null; then
        run_host "python python/host.py" python3 "${PYTHON_HOST}"
    else
        skip_host "python python/host.py" "python3 not found"
    fi
else
    skip_host "python python/host.py" "file not found: ${PYTHON_HOST}"
fi

# ── Lua host ─────────────────────────────────────────────────────────────────
LUA_HOST="${HOSTS_DIR}/lua/host.lua"
if [[ -f "${LUA_HOST}" ]]; then
    # The Lua host may be backed by a Rust dylib — try lua / luajit
    LUA_BIN=""
    for candidate in luajit lua lua5.4 lua5.3; do
        if command -v "${candidate}" &>/dev/null; then
            LUA_BIN="${candidate}"
            break
        fi
    done
    if [[ -n "${LUA_BIN}" ]]; then
        run_host "lua   lua/host.lua" "${LUA_BIN}" "${LUA_HOST}"
    else
        skip_host "lua   lua/host.lua" "no lua interpreter found (tried luajit, lua, lua5.4, lua5.3)"
    fi
else
    skip_host "lua   lua/host.lua" "file not found: ${LUA_HOST}"
fi

# ── C# host ──────────────────────────────────────────────────────────────────
CSHARP_DIR="${HOSTS_DIR}/csharp"
CSHARP_PROJ=""
# Prefer Host.csproj, fall back to any .csproj
if [[ -f "${CSHARP_DIR}/Host.csproj" ]]; then
    CSHARP_PROJ="${CSHARP_DIR}/Host.csproj"
elif [[ -f "${CSHARP_DIR}/PolyplugHost.csproj" ]]; then
    CSHARP_PROJ="${CSHARP_DIR}/PolyplugHost.csproj"
fi

if [[ -n "${CSHARP_PROJ}" ]]; then
    if command -v dotnet &>/dev/null; then
        run_host "csharp csharp/Host.csproj" \
            dotnet run --project "${CSHARP_PROJ}" --configuration Release
    else
        skip_host "csharp csharp/Host.csproj" "dotnet not found"
    fi
else
    skip_host "csharp csharp/Host.csproj" "no .csproj found in hosts/csharp"
fi

# ── JS host (Deno) ───────────────────────────────────────────────────────────
JS_HOST="${HOSTS_DIR}/js/host.ts"
if [[ -f "${JS_HOST}" ]]; then
    if command -v deno &>/dev/null; then
        run_host "js    js/host.ts (deno)" \
            deno run --allow-read --allow-ffi "${JS_HOST}"
    else
        skip_host "js    js/host.ts (deno)" "deno not found"
    fi
else
    skip_host "js    js/host.ts (deno)" "file not found: ${JS_HOST}"
fi

# ── Rust host (rust_showcase/) ────────────────────────────────────────────────
# Full showcase host that runs all plugins and pipelines
RUST_ALT_DIR="${HOSTS_DIR}/rust_showcase"
if [[ -d "${RUST_ALT_DIR}" && -f "${RUST_ALT_DIR}/Cargo.toml" ]]; then
    run_host "rust  rust_showcase/host" \
        cargo run --quiet --manifest-path "${RUST_ALT_DIR}/Cargo.toml"
fi

# ===========================================================================
# SUMMARY
# ===========================================================================

printf "\n%-50s\n" "$(printf '%.0s─' {1..50})"
printf "${BOLD}Results:${RESET}\n\n"

for result in "${RESULTS[@]}"; do
    status="${result:0:4}"
    name="${result:6}"
    case "${status}" in
        PASS) printf "  ${GREEN}✔ PASS${RESET}  %s\n" "${name}" ;;
        FAIL) printf "  ${RED}✘ FAIL${RESET}  %s\n" "${name}" ;;
        SKIP) printf "  ${YELLOW}⚠ SKIP${RESET}  %s\n" "${name}" ;;
    esac
done

printf "\n"
printf "  Passed : ${GREEN}%d${RESET}\n" "${PASS_COUNT}"
printf "  Failed : ${RED}%d${RESET}\n"  "${FAIL_COUNT}"
printf "  Skipped: ${YELLOW}%d${RESET}\n" "${SKIP_COUNT}"
printf "\n"

if [[ ${FAIL_COUNT} -gt 0 ]]; then
    printf "${RED}${BOLD}VERIFY FAILED${RESET} — %d host(s) produced output that differs from golden.txt.\n\n" "${FAIL_COUNT}"
    exit 1
else
    printf "${GREEN}${BOLD}VERIFY PASSED${RESET} — all checked hosts match golden.txt.\n\n"
    exit 0
fi
