#!/usr/bin/env bash
# examples/build_everything.sh — Build all polyplug guest examples with PASS/FAIL reporting
#
# Compiles or verifies all 12 guest plugin examples across all languages:
#   Native   : polyplug_native crate      (cargo build --release -p polyplug_native)
#   Native   : native example plugin      (cargo build --release)
#   Rust     : decoder, encoder           (cargo build --release)
#   C++      : transformer, validator     (make)
#   C#       : encoder, reporter          (dotnet build --configuration Release)
#   Python   : decoder, reporter          (python3 -m py_compile — syntax check)
#   Lua      : transformer, validator     (luac -p — syntax check)
#   JS       : reporter, validator        (bundle.js existence check)
#
# Exits 0 if all examples pass, non-zero if any fail.
#
# Usage:
#   bash examples/build_everything.sh
#   ./examples/build_everything.sh

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUESTS_DIR="${SCRIPT_DIR}/guests"

# Tracking
PASS_COUNT=0
FAIL_COUNT=0
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
# run_example LANG GUEST CMD...
#   Runs CMD in the guest directory. Captures output. Records PASS/FAIL.
# ---------------------------------------------------------------------------
run_example() {
    local lang="$1"
    local guest="$2"
    shift 2
    local cmd=("$@")
    local label="${lang}/${guest}"
    local guest_dir="${GUESTS_DIR}/${lang}/${guest}"

    printf "  %-28s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        printf "${YELLOW}SKIP${RESET} (directory not found: %s)\n" "${guest_dir}"
        RESULTS+=("SKIP  ${label}")
        return
    fi

    local log
    log=$(
        cd "${guest_dir}" || exit 1
        "${cmd[@]}" 2>&1
    )
    local exit_code=$?

    if [[ ${exit_code} -eq 0 ]]; then
        printf "${GREEN}PASS${RESET}\n"
        RESULTS+=("PASS  ${label}")
        (( PASS_COUNT += 1 ))
    else
        printf "${RED}FAIL${RESET}\n"
        RESULTS+=("FAIL  ${label}")
        (( FAIL_COUNT += 1 ))
        # Print indented build output so errors are visible
        echo "${log}" | sed 's/^/        /' >&2
    fi
}

# ---------------------------------------------------------------------------
# Specialised check for Python (syntax-check all .py files in the guest dir)
# ---------------------------------------------------------------------------
run_python_example() {
    local guest="$1"
    local label="python/${guest}"
    local guest_dir="${GUESTS_DIR}/python/${guest}"

    printf "  %-28s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        printf "${YELLOW}SKIP${RESET} (directory not found: %s)\n" "${guest_dir}"
        RESULTS+=("SKIP  ${label}")
        return
    fi

    local log
    log=$(
        find "${guest_dir}" -name "*.py" -print0 \
            | xargs -0 -I{} python3 -m py_compile {} 2>&1
    )
    local exit_code=$?

    if [[ ${exit_code} -eq 0 ]]; then
        printf "${GREEN}PASS${RESET} (syntax ok)\n"
        RESULTS+=("PASS  ${label}")
        (( PASS_COUNT += 1 ))
    else
        printf "${RED}FAIL${RESET}\n"
        RESULTS+=("FAIL  ${label}")
        (( FAIL_COUNT += 1 ))
        echo "${log}" | sed 's/^/        /' >&2
    fi
}

# ---------------------------------------------------------------------------
# Specialised check for Lua (luac -p on each .lua file)
# ---------------------------------------------------------------------------
run_lua_example() {
    local guest="$1"
    local label="lua/${guest}"
    local guest_dir="${GUESTS_DIR}/lua/${guest}"

    printf "  %-28s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        printf "${YELLOW}SKIP${RESET} (directory not found: %s)\n" "${guest_dir}"
        RESULTS+=("SKIP  ${label}")
        return
    fi

    # luac may not be available — fall back gracefully
    if ! command -v luac &>/dev/null; then
        printf "${YELLOW}SKIP${RESET} (luac not found — install lua to syntax-check)\n"
        RESULTS+=("SKIP  ${label}")
        return
    fi

    local log
    log=$(
        find "${guest_dir}" -name "*.lua" -print0 \
            | xargs -0 -I{} luac -p {} 2>&1
    )
    local exit_code=$?

    if [[ ${exit_code} -eq 0 ]]; then
        printf "${GREEN}PASS${RESET} (syntax ok)\n"
        RESULTS+=("PASS  ${label}")
        (( PASS_COUNT += 1 ))
    else
        printf "${RED}FAIL${RESET}\n"
        RESULTS+=("FAIL  ${label}")
        (( FAIL_COUNT += 1 ))
        echo "${log}" | sed 's/^/        /' >&2
    fi
}

# ---------------------------------------------------------------------------
# Specialised check for JS (verify bundle.js exists)
# ---------------------------------------------------------------------------
run_js_example() {
    local guest="$1"
    local label="js/${guest}"
    local guest_dir="${GUESTS_DIR}/js/${guest}"
    local bundle="${guest_dir}/bundle.js"

    printf "  %-28s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        printf "${YELLOW}SKIP${RESET} (directory not found: %s)\n" "${guest_dir}"
        RESULTS+=("SKIP  ${label}")
        return
    fi

    if [[ -f "${bundle}" ]]; then
        printf "${GREEN}PASS${RESET} (bundle.js present)\n"
        RESULTS+=("PASS  ${label}")
        (( PASS_COUNT += 1 ))
    else
        printf "${RED}FAIL${RESET} (bundle.js missing: %s)\n" "${bundle}"
        RESULTS+=("FAIL  ${label}")
        (( FAIL_COUNT += 1 ))
    fi
}

# ---------------------------------------------------------------------------
# run_workspace_crate LABEL CMD...
#   Runs CMD from the repository root (for workspace-level crates).
# ---------------------------------------------------------------------------
run_workspace_crate() {
    local label="$1"
    shift
    local cmd=("$@")
    local repo_root
    repo_root="$(cd "${SCRIPT_DIR}/.." && pwd)"

    printf "  %-28s " "${label} ..."

    local log
    log=$(
        cd "${repo_root}" || exit 1
        "${cmd[@]}" 2>&1
    )
    local exit_code=$?

    if [[ ${exit_code} -eq 0 ]]; then
        printf "${GREEN}PASS${RESET}\n"
        RESULTS+=("PASS  ${label}")
        (( PASS_COUNT += 1 ))
    else
        printf "${RED}FAIL${RESET}\n"
        RESULTS+=("FAIL  ${label}")
        (( FAIL_COUNT += 1 ))
        echo "${log}" | sed 's/^/        /' >&2
    fi
}

# ---------------------------------------------------------------------------
# run_dir_example LABEL DIR CMD...
#   Runs CMD in an arbitrary directory (not constrained to GUESTS_DIR layout).
# ---------------------------------------------------------------------------
run_dir_example() {
    local label="$1"
    local dir="$2"
    shift 2
    local cmd=("$@")

    printf "  %-28s " "${label} ..."

    if [[ ! -d "${dir}" ]]; then
        printf "${YELLOW}SKIP${RESET} (directory not found: %s)\n" "${dir}"
        RESULTS+=("SKIP  ${label}")
        return
    fi

    local log
    log=$(
        cd "${dir}" || exit 1
        "${cmd[@]}" 2>&1
    )
    local exit_code=$?

    if [[ ${exit_code} -eq 0 ]]; then
        printf "${GREEN}PASS${RESET}\n"
        RESULTS+=("PASS  ${label}")
        (( PASS_COUNT += 1 ))
    else
        printf "${RED}FAIL${RESET}\n"
        RESULTS+=("FAIL  ${label}")
        (( FAIL_COUNT += 1 ))
        echo "${log}" | sed 's/^/        /' >&2
    fi
}

# ===========================================================================
# MAIN
# ===========================================================================

printf "\n${BOLD}polyplug — build all guest examples${RESET}\n"
printf "%-30s\n" "$(printf '%.0s─' {1..50})"

# ── Native loader ───────────────────────────────────────────────────────────
printf "\n${BOLD}[native loader]${RESET}  cargo build --release -p polyplug_native\n"
run_workspace_crate polyplug_native  cargo build --release -p polyplug_native

# ── Native example plugin ────────────────────────────────────────────────────
printf "\n${BOLD}[native example]${RESET}  cargo build --release\n"
run_dir_example "native/plugin" "${GUESTS_DIR}/native"  cargo build --release

# ── Rust ────────────────────────────────────────────────────────────────────
printf "\n${BOLD}[rust]${RESET}  cargo build --release\n"
run_example rust decoder  cargo build --release
run_example rust encoder  cargo build --release

# ── C++ ─────────────────────────────────────────────────────────────────────
printf "\n${BOLD}[cpp]${RESET}   make\n"
run_example cpp transformer  make
run_example cpp validator    make

# ── C# ──────────────────────────────────────────────────────────────────────
printf "\n${BOLD}[csharp]${RESET}  dotnet build --configuration Release\n"
run_example csharp encoder   dotnet build --configuration Release
run_example csharp reporter  dotnet build --configuration Release

# ── Python ──────────────────────────────────────────────────────────────────
printf "\n${BOLD}[python]${RESET}  python3 -m py_compile (syntax check)\n"
run_python_example decoder
run_python_example reporter

# ── Lua ─────────────────────────────────────────────────────────────────────
printf "\n${BOLD}[lua]${RESET}   luac -p (syntax check)\n"
run_lua_example transformer
run_lua_example validator

# ── JavaScript ──────────────────────────────────────────────────────────────
printf "\n${BOLD}[js]${RESET}    bundle.js existence check\n"
run_js_example reporter
run_js_example validator

# ===========================================================================
# SUMMARY
# ===========================================================================

printf "\n%-30s\n" "$(printf '%.0s─' {1..50})"
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
printf "\n"

if [[ ${FAIL_COUNT} -gt 0 ]]; then
    printf "${RED}${BOLD}BUILD FAILED${RESET} — %d example(s) did not pass.\n\n" "${FAIL_COUNT}"
    exit 1
else
    printf "${GREEN}${BOLD}BUILD PASSED${RESET} — all examples OK.\n\n"
    exit 0
fi
