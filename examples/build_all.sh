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

printf "${BOLD}Step 1: Build workspace (polyplugc + runtime + loaders)${RESET}\n"
printf "  %-40s " "cargo build ..."
if cargo build 2>/dev/null; then
    pass "workspace"
else
    fail "workspace"
    printf "\n${RED}Cannot continue without workspace build.${RESET}\n"
    exit 1
fi

rm -rf "${PLUGINS_DIR:?}"/*
mkdir -p "${PLUGINS_DIR}"

get_bundle_name() {
    local manifest="$1"
    grep '^bundle_name' "${manifest}" | sed 's/bundle_name.*=.*"\(.*\)"/\1/'
}

get_file_field() {
    local manifest="$1"
    local flat
    flat=$(grep '^file ' "${manifest}" | sed 's/file.*=.*"\(.*\)"/\1/' || true)
    if [[ -n "${flat}" ]]; then
        echo "${flat}"
        return
    fi
    local os arch key
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    case "${os}" in
        darwin) os="macos" ;;
    esac
    arch=$(uname -m)
    case "${arch}" in
        arm64) arch="aarch64" ;;
    esac
    key="${os}.${arch}"
    grep "^${key}" "${manifest}" | sed 's/.*=.*"\(.*\)"/\1/' || true
}

install_plugin() {
    local bundle_name="$1"
    local guest_dir="$2"
    local dest="${PLUGINS_DIR}/${bundle_name}"
    local generated_manifest="${guest_dir}/generated/manifest.toml"

    if [[ ! -f "${generated_manifest}" ]]; then
        echo "ERROR: generated manifest not found: ${generated_manifest}" >&2
        return 1
    fi

    mkdir -p "${dest}"
    cp "${generated_manifest}" "${dest}/manifest.toml"

    local file_field
    file_field=$(get_file_field "${generated_manifest}")

    if [[ -n "${file_field}" ]]; then
        if [[ -f "${guest_dir}/${file_field}" ]]; then
            cp "${guest_dir}/${file_field}" "${dest}/"
        elif [[ -f "${guest_dir}/bin/Release/net10.0/${file_field}" ]]; then
            # C# output location
            cp "${guest_dir}/bin/Release/net10.0/${file_field}" "${dest}/"
        fi
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

printf "\n${BOLD}Step 2: Build guest plugins → examples/plugins/${RESET}\n"

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
        bundle_name=$(get_bundle_name "${guest_dir}/generated/manifest.toml")
        install_plugin "${bundle_name}" "${guest_dir}"
        so_file=$(get_file_field "${guest_dir}/generated/manifest.toml")
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
        bundle_name=$(get_bundle_name "${guest_dir}/generated/manifest.toml")
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
        bundle_name=$(get_bundle_name "${guest_dir}/generated/manifest.toml")
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

    bundle_name=$(get_bundle_name "${guest_dir}/generated/manifest.toml")
    install_plugin "${bundle_name}" "${guest_dir}"
    py_file=$(get_file_field "${guest_dir}/generated/manifest.toml")
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

    bundle_name=$(get_bundle_name "${guest_dir}/generated/manifest.toml")
    install_plugin "${bundle_name}" "${guest_dir}"
    lua_file=$(get_file_field "${guest_dir}/generated/manifest.toml")
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

    bundle_name=$(get_bundle_name "${guest_dir}/generated/manifest.toml")
    install_plugin "${bundle_name}" "${guest_dir}"
    js_file=$(get_file_field "${guest_dir}/generated/manifest.toml")
    if [[ -n "${js_file}" ]] && [[ -f "${guest_dir}/${js_file}" ]]; then
        cp "${guest_dir}/${js_file}" "${PLUGINS_DIR}/${bundle_name}/"
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
