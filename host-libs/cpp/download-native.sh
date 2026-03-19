#!/usr/bin/env bash
# download-native.sh
# Download the polyplug native library for the current platform from GitHub Releases
#
# Usage:
#   ./download-native.sh [VERSION]
#
# Arguments:
#   VERSION   Optional. Version to download (default: "0.1.0")
#
# Output:
#   Downloads the library to _native/{platform}-{arch}/

set -euo pipefail

# Default version
VERSION="${1:-0.1.0}"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
    Linux)
        PLATFORM="linux-x64"
        LIB_NAME="libpolyplug.so"
        ;;
    Darwin)
        if [[ "${ARCH}" == "arm64" ]]; then
            PLATFORM="darwin-arm64"
        else
            PLATFORM="darwin-x64"
        fi
        LIB_NAME="libpolyplug.dylib"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        PLATFORM="win32-x64"
        LIB_NAME="polyplug-windows-x64.dll"
        ;;
    *)
        echo "ERROR: Unsupported platform: ${OS}" >&2
        exit 1
        ;;
esac

# Output directory
OUTPUT_DIR="_native/${PLATFORM}"
mkdir -p "${OUTPUT_DIR}"

# Download URL
DOWNLOAD_URL="https://github.com/polyplug/polyplug/releases/download/v${VERSION}/${LIB_NAME}"
OUTPUT_PATH="${OUTPUT_DIR}/${LIB_NAME}"

echo "Downloading polyplug v${VERSION} for ${PLATFORM}..."
echo "URL: ${DOWNLOAD_URL}"
echo "Output: ${OUTPUT_PATH}"

# Download with curl
if command -v curl &> /dev/null; then
    curl -L -o "${OUTPUT_PATH}" "${DOWNLOAD_URL}"
elif command -v wget &> /dev/null; then
    wget -O "${OUTPUT_PATH}" "${DOWNLOAD_URL}"
else
    echo "ERROR: Neither curl nor wget found. Please install one of them." >&2
    exit 1
fi

# Verify download
if [[ -f "${OUTPUT_PATH}" ]]; then
    FILE_SIZE="$(stat -f%z "${OUTPUT_PATH}" 2>/dev/null || stat -c%s "${OUTPUT_PATH}" 2>/dev/null || echo "unknown")"
    echo "SUCCESS: Downloaded ${LIB_NAME} (${FILE_SIZE} bytes) to ${OUTPUT_PATH}"
    
    # Make executable on Unix-like systems
    if [[ "${OS}" != "MINGW"* && "${OS}" != "MSYS"* && "${OS}" != "CYGWIN"* ]]; then
        chmod +x "${OUTPUT_PATH}"
    fi
else
    echo "ERROR: Download failed. File not found at ${OUTPUT_PATH}" >&2
    exit 1
fi
