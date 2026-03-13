#!/usr/bin/env bash
# examples/build.sh — Master build script for all polyplug example guests
#
# Delegates to per-language build scripts under examples/guests/<lang>/build.sh
# Run from the repository root or from within examples/.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUESTS_DIR="${SCRIPT_DIR}/guests"

LANGUAGES=(rust cpp csharp python lua js)

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS] [LANG...]

Build polyplug example guest plugins.

OPTIONS:
  -h, --help      Print this help message and exit
  --list          List available languages and exit

LANG:
  One or more language names to build. If omitted, all languages are built.
  Available: rust cpp csharp python lua js

EXAMPLES:
  $(basename "$0")              Build all guest languages
  $(basename "$0") rust         Build only the Rust guest
  $(basename "$0") rust cpp     Build Rust and C++ guests
  $(basename "$0") --help       Print this message
EOF
}

list_languages() {
    echo "Available languages:"
    for lang in "${LANGUAGES[@]}"; do
        echo "  ${lang}"
    done
}

build_language() {
    local lang="$1"
    local build_script="${GUESTS_DIR}/${lang}/build.sh"

    echo "==> Building guest: ${lang}"

    if [[ -f "${build_script}" ]]; then
        bash "${build_script}"
    else
        echo "  [SKIP] No build script found at ${build_script}" >&2
        return 1
    fi
}

main() {
    local targets=()

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            --list)
                list_languages
                exit 0
                ;;
            -*)
                echo "Error: Unknown option: $1" >&2
                usage >&2
                exit 1
                ;;
            *)
                targets+=("$1")
                ;;
        esac
        shift
    done

    # Default: build all languages
    if [[ ${#targets[@]} -eq 0 ]]; then
        targets=("${LANGUAGES[@]}")
    fi

    # Validate requested languages
    for target in "${targets[@]}"; do
        local valid=0
        for lang in "${LANGUAGES[@]}"; do
            if [[ "${target}" == "${lang}" ]]; then
                valid=1
                break
            fi
        done
        if [[ ${valid} -eq 0 ]]; then
            echo "Error: Unknown language '${target}'. Use --list to see available languages." >&2
            exit 1
        fi
    done

    # Build each requested language
    for target in "${targets[@]}"; do
        build_language "${target}"
    done

    echo ""
    echo "Build complete."
}

main "$@"
