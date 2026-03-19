#!/bin/bash
set -e

if [ -z "$1" ]; then
    echo "Error: Version argument required"
    echo "Usage: bash download-native.sh 0.1.0"
    exit 1
fi

VERSION="$1"
BASE_URL="https://github.com/polyplug/polyplug/releases/download/v${VERSION}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

declare -a LIBRARIES=(
    "libpolyplug-linux-x64.so:runtimes/linux-x64/native/libpolyplug.so:Linux x64"
    "libpolyplug-macos-x64.dylib:runtimes/osx-x64/native/libpolyplug.dylib:macOS x64"
    "libpolyplug-macos-arm64.dylib:runtimes/osx-arm64/native/libpolyplug.dylib:macOS ARM64"
    "polyplug-windows-x64.dll:runtimes/win-x64/native/polyplug.dll:Windows x64"
)

echo "Downloading native libraries for polyplug v${VERSION}..."
echo ""

for LIB_ENTRY in "${LIBRARIES[@]}"; do
    IFS=':' read -r FILENAME RELATIVE_PATH LIB_NAME <<< "$LIB_ENTRY"
    
    URL="${BASE_URL}/${FILENAME}"
    DEST_PATH="${SCRIPT_DIR}/${RELATIVE_PATH}"
    
    echo "Downloading ${LIB_NAME}..."
    echo "  URL: ${URL}"
    echo "  Path: ${DEST_PATH}"
    
    DEST_DIR="$(dirname "${DEST_PATH}")"
    mkdir -p "${DEST_DIR}"
    
    PLACEHOLDER_PATH="${DEST_DIR}/README.txt"
    if [ -f "${PLACEHOLDER_PATH}" ]; then
        rm -f "${PLACEHOLDER_PATH}"
    fi
    
    if curl -L -o "${DEST_PATH}" "${URL}" --fail --silent --show-error; then
        echo "  ✓ Success"
    else
        echo "  ✗ Failed"
        echo "  Continuing with other platforms..."
    fi
    
    echo ""
done

echo "Native library download complete."
echo ""
echo "Note: Some downloads may have failed if the release doesn't include all platforms."
echo "This is expected for development builds. CI should ensure all platforms are available."
