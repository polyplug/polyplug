#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PLUGINS_DIR="${SCRIPT_DIR}/plugins"
GUESTS_DIR="${SCRIPT_DIR}/guests"
POLYPLUGC="${REPO_ROOT}/target/debug/polyplugc"

if [[ -t 1 ]]; then
    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[0;33m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    GREEN='' RED='' YELLOW='' BOLD='' RESET=''
fi

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
declare -a RESULTS=()

pass() {
    printf "${GREEN}PASS${RESET}\n"
    RESULTS+=("PASS  $1")
    (( PASS_COUNT += 1 ))
}

fail() {
    printf "${RED}FAIL${RESET}\n"
    RESULTS+=("FAIL  $1")
    (( FAIL_COUNT += 1 ))
}

skip() {
    printf "${YELLOW}SKIP${RESET} (%s)\n" "$2"
    RESULTS+=("SKIP  $1 ($2)")
    (( SKIP_COUNT += 1 ))
}

printf "\n${BOLD}polyplug — build all examples${RESET}\n"
printf "%-50s\n\n" "$(printf '%.0s─' {1..50})"

printf "${BOLD}Step 1: Build polyplugc${RESET}\n"
printf "  %-40s " "cargo build -p polyplugc ..."
if cargo build -p polyplugc 2>/dev/null; then
    pass "polyplugc"
else
    fail "polyplugc"
    printf "\n${RED}Cannot continue without polyplugc.${RESET}\n"
    exit 1
fi

printf "\n${BOLD}Step 2: Build polyplug runtime + loaders${RESET}\n"
printf "  %-40s " "cargo build (workspace) ..."
if cargo build 2>/dev/null; then
    pass "workspace"
else
    fail "workspace"
    printf "\n${RED}Cannot continue without runtime libraries.${RESET}\n"
    exit 1
fi

rm -rf "${PLUGINS_DIR:?}"/*
mkdir -p "${PLUGINS_DIR}"

install_plugin() {
    local bundle_name="$1"
    local guest_dir="$2"
    local manifest_file="${guest_dir}/manifest.toml"
    local dest="${PLUGINS_DIR}/${bundle_name}"

    mkdir -p "${dest}"
    cp "${manifest_file}" "${dest}/manifest.toml"

    local file_field
    file_field=$(grep '^file ' "${manifest_file}" | sed 's/file.*=.*"\(.*\)"/\1/' || true)
    if [[ -n "${file_field}" ]] && [[ -f "${guest_dir}/${file_field}" ]]; then
        cp "${guest_dir}/${file_field}" "${dest}/"
    fi
}

generate_bindings() {
    local label="$1"
    local bundle_toml="$2"
    local lang="$3"
    local out_dir="$4"

    printf "  %-40s " "[codegen] ${label} ..."
    mkdir -p "${out_dir}"
    if "${POLYPLUGC}" generate --bundle "${bundle_toml}" --lang "${lang}" --out "${out_dir}" >/dev/null 2>&1; then
        pass "codegen/${label}"
    else
        fail "codegen/${label}"
        return 1
    fi
    return 0
}

printf "\n${BOLD}Step 3: Build guest plugins → examples/plugins/${RESET}\n"

printf "\n  ${BOLD}[rust]${RESET}\n"
for guest in decoder reporter; do
    guest_dir="${GUESTS_DIR}/rust/${guest}"
    label="rust/${guest}"
    printf "  %-40s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        skip "${label}" "directory not found"
        continue
    fi

    generate_bindings "${label}" "${guest_dir}/bundle.toml" rust "${guest_dir}/generated" || continue

    if cargo build --release --manifest-path "${guest_dir}/Cargo.toml" 2>/dev/null; then
        bundle_name=$(grep '^bundle_name' "${guest_dir}/manifest.toml" | sed 's/bundle_name.*=.*"\(.*\)"/\1/')
        install_plugin "${bundle_name}" "${guest_dir}"
        so_file=$(grep '^file ' "${guest_dir}/manifest.toml" | sed 's/file.*=.*"\(.*\)"/\1/')
        release_so="${guest_dir}/target/release/${so_file}"
        if [[ -f "${release_so}" ]]; then
            cp "${release_so}" "${PLUGINS_DIR}/${bundle_name}/"
        fi
        pass "${label}"
    else
        fail "${label}"
    fi
done

printf "\n  ${BOLD}[cpp]${RESET}\n"
for guest in transformer reporter; do
    guest_dir="${GUESTS_DIR}/cpp/${guest}"
    label="cpp/${guest}"
    printf "  %-40s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        skip "${label}" "directory not found"
        continue
    fi

    generate_bindings "${label}" "${guest_dir}/bundle.toml" cpp "${guest_dir}/generated" || continue

    if make -C "${guest_dir}" 2>/dev/null; then
        bundle_name=$(grep '^bundle_name' "${guest_dir}/manifest.toml" | sed 's/bundle_name.*=.*"\(.*\)"/\1/')
        install_plugin "${bundle_name}" "${guest_dir}"
        pass "${label}"
    else
        fail "${label}"
    fi
done

printf "\n  ${BOLD}[csharp]${RESET}\n"
for guest in encoder reporter; do
    guest_dir="${GUESTS_DIR}/csharp/${guest}"
    label="csharp/${guest}"
    printf "  %-40s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        skip "${label}" "directory not found"
        continue
    fi

    if ! command -v dotnet &>/dev/null; then
        skip "${label}" "dotnet not found"
        continue
    fi

    generate_bindings "${label}" "${guest_dir}/bundle.toml" csharp "${guest_dir}/generated" || continue

    if dotnet build "${guest_dir}" --configuration Release 2>/dev/null; then
        bundle_name=$(grep '^bundle_name' "${guest_dir}/manifest.toml" | sed 's/bundle_name.*=.*"\(.*\)"/\1/')
        install_plugin "${bundle_name}" "${guest_dir}"
        pass "${label}"
    else
        fail "${label}"
    fi
done

printf "\n  ${BOLD}[python]${RESET}\n"
for guest in decoder reporter; do
    guest_dir="${GUESTS_DIR}/python/${guest}"
    label="python/${guest}"
    printf "  %-40s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        skip "${label}" "directory not found"
        continue
    fi

    generate_bindings "${label}" "${guest_dir}/bundle.toml" python "${guest_dir}/generated" || continue

    bundle_name=$(grep '^bundle_name' "${guest_dir}/manifest.toml" | sed 's/bundle_name.*=.*"\(.*\)"/\1/')
    install_plugin "${bundle_name}" "${guest_dir}"
    py_file=$(grep '^file ' "${guest_dir}/manifest.toml" | sed 's/file.*=.*"\(.*\)"/\1/')
    if [[ -n "${py_file}" ]] && [[ -f "${guest_dir}/${py_file}" ]]; then
        cp "${guest_dir}/${py_file}" "${PLUGINS_DIR}/${bundle_name}/"
    fi
    pass "${label}"
done

printf "\n  ${BOLD}[lua]${RESET}\n"
for guest in transformer reporter; do
    guest_dir="${GUESTS_DIR}/lua/${guest}"
    label="lua/${guest}"
    printf "  %-40s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        skip "${label}" "directory not found"
        continue
    fi

    generate_bindings "${label}" "${guest_dir}/bundle.toml" lua "${guest_dir}/generated" || continue

    bundle_name=$(grep '^bundle_name' "${guest_dir}/manifest.toml" | sed 's/bundle_name.*=.*"\(.*\)"/\1/')
    install_plugin "${bundle_name}" "${guest_dir}"
    lua_file=$(grep '^file ' "${guest_dir}/manifest.toml" | sed 's/file.*=.*"\(.*\)"/\1/')
    if [[ -n "${lua_file}" ]] && [[ -f "${guest_dir}/${lua_file}" ]]; then
        cp "${guest_dir}/${lua_file}" "${PLUGINS_DIR}/${bundle_name}/"
    fi
    pass "${label}"
done

printf "\n  ${BOLD}[js_quickjs]${RESET}\n"
for guest in transformer reporter; do
    guest_dir="${GUESTS_DIR}/js_quickjs/${guest}"
    label="js_quickjs/${guest}"
    printf "  %-40s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        skip "${label}" "directory not found"
        continue
    fi

    generate_bindings "${label}" "${guest_dir}/bundle.toml" js-quickjs "${guest_dir}/generated" || continue

    bundle_name=$(grep '^bundle_name' "${guest_dir}/manifest.toml" | sed 's/bundle_name.*=.*"\(.*\)"/\1/')
    install_plugin "${bundle_name}" "${guest_dir}"
    js_file=$(grep '^file ' "${guest_dir}/manifest.toml" | sed 's/file.*=.*"\(.*\)"/\1/')
    if [[ -n "${js_file}" ]] && [[ -f "${guest_dir}/${js_file}" ]]; then
        cp "${guest_dir}/${js_file}" "${PLUGINS_DIR}/${bundle_name}/"
    fi
    pass "${label}"
done

printf "\n  ${BOLD}[js_deno]${RESET}\n"
for guest in transformer reporter; do
    guest_dir="${GUESTS_DIR}/js_deno/${guest}"
    label="js_deno/${guest}"
    printf "  %-40s " "${label} ..."

    if [[ ! -d "${guest_dir}" ]]; then
        skip "${label}" "directory not found"
        continue
    fi

    generate_bindings "${label}" "${guest_dir}/bundle.toml" js-deno "${guest_dir}/generated" || continue

    bundle_name=$(grep '^bundle_name' "${guest_dir}/manifest.toml" | sed 's/bundle_name.*=.*"\(.*\)"/\1/')
    install_plugin "${bundle_name}" "${guest_dir}"
    ts_file=$(grep '^file ' "${guest_dir}/manifest.toml" | sed 's/file.*=.*"\(.*\)"/\1/')
    if [[ -n "${ts_file}" ]] && [[ -f "${guest_dir}/${ts_file}" ]]; then
        cp "${guest_dir}/${ts_file}" "${PLUGINS_DIR}/${bundle_name}/"
    fi
    pass "${label}"
done

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

if [[ -d "${PLUGINS_DIR}" ]]; then
    BUNDLE_COUNT=$(find "${PLUGINS_DIR}" -name "manifest.toml" -maxdepth 2 | wc -l)
    printf "\n  Plugins installed: ${BOLD}%d${RESET} bundles in examples/plugins/\n" "${BUNDLE_COUNT}"
fi

printf "\n"
if [[ ${FAIL_COUNT} -gt 0 ]]; then
    printf "${RED}${BOLD}BUILD FAILED${RESET} — %d step(s) failed.\n\n" "${FAIL_COUNT}"
    exit 1
else
    printf "${GREEN}${BOLD}BUILD PASSED${RESET} — all examples built and installed to examples/plugins/.\n"
    printf "Hosts can now discover plugins via: ${BOLD}POLYPLUG_PLUGIN_PATH=examples/plugins/${RESET}\n\n"
    exit 0
fi
