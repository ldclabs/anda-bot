#!/bin/sh
# anda-bot installer — detects OS/Arch and downloads the right binary
# Usage: curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh

set -e

REPO="ldclabs/anda-bot"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="anda"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

info() { printf "${CYAN}$1${NC}\n"; }
success() { printf "${GREEN}$1${NC}\n"; }
error() { printf "${RED}Error: $1${NC}\n" >&2; exit 1; }

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$1" | awk '{print $NF}'
    else
        return 1
    fi
}

verify_checksum() {
    FILE_PATH="$1"
    CHECKSUM_PATH="$2"
    EXPECTED_HASH=$(awk '{print $1}' "$CHECKSUM_PATH" | tr -d '\r\n')

    if [ -z "$EXPECTED_HASH" ]; then
        error "Checksum file is empty: ${CHECKSUM_PATH}"
    fi

    if ! ACTUAL_HASH=$(sha256_file "$FILE_PATH"); then
        info "No SHA-256 tool found; skipping checksum verification."
        return 0
    fi

    if [ "$EXPECTED_HASH" != "$ACTUAL_HASH" ]; then
        error "Checksum verification failed for $(basename "$FILE_PATH")"
    fi

    success "Checksum verified."
}

# Detect OS
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$OS" in
    linux*)  OS="linux" ;;
    darwin*) OS="macos" ;;
    mingw*|msys*|cygwin*) OS="windows" ;;
    *) error "Unsupported OS: $OS" ;;
esac

# Detect Arch
ARCH=$(uname -m)
case "$ARCH" in
    x86_64|amd64)  ARCH="x86_64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    *) error "Unsupported architecture: $ARCH" ;;
esac

TARGET="${OS}-${ARCH}"

case "$TARGET" in
    linux-x86_64|linux-arm64|windows-x86_64|macos-x86_64|macos-arm64) ;;
    *) error "Unsupported target: ${TARGET}. Available releases: linux-x86_64, linux-arm64, windows-x86_64, macos-x86_64, macos-arm64" ;;
esac

EXE_EXT=""
if [ "$OS" = "windows" ]; then
    EXE_EXT=".exe"
fi

ASSET_NAME="${BINARY_NAME}-${TARGET}${EXE_EXT}"
CHECKSUM_NAME="${ASSET_NAME}.sha256"
INSTALL_NAME="${BINARY_NAME}${EXE_EXT}"

# Get latest version (via redirect, avoids API rate limit)
info "Detecting latest version..."
VERSION=$(curl -fsSI "https://github.com/${REPO}/releases/latest" | grep -i "location:" | sed -E 's/.*\/tag\/(.*)/\1/' | tr -d '\r\n')
if [ -z "$VERSION" ]; then
    error "Could not detect latest version. Check https://github.com/${REPO}/releases"
fi
info "Latest version: ${VERSION}"

URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET_NAME}"
CHECKSUM_URL="https://github.com/${REPO}/releases/download/${VERSION}/${CHECKSUM_NAME}"

# Download
info "Downloading ${ASSET_NAME}..."
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

if ! curl -fsSL "$URL" -o "${TMPDIR}/${ASSET_NAME}"; then
    error "Download failed. Binary may not exist for ${TARGET}.\nCheck: https://github.com/${REPO}/releases/tag/${VERSION}"
fi

if curl -fsSL "$CHECKSUM_URL" -o "${TMPDIR}/${CHECKSUM_NAME}"; then
    verify_checksum "${TMPDIR}/${ASSET_NAME}" "${TMPDIR}/${CHECKSUM_NAME}"
else
    info "Checksum file not found; skipping checksum verification."
fi

# Install
chmod +x "${TMPDIR}/${ASSET_NAME}" 2>/dev/null || true
if [ -w "$INSTALL_DIR" ]; then
    mv "${TMPDIR}/${ASSET_NAME}" "${INSTALL_DIR}/${INSTALL_NAME}"
else
    info "Installing to ${INSTALL_DIR} (requires sudo)..."
    sudo mv "${TMPDIR}/${ASSET_NAME}" "${INSTALL_DIR}/${INSTALL_NAME}"
fi

chmod +x "${INSTALL_DIR}/${INSTALL_NAME}" 2>/dev/null || true

# Verify
BINARY_CMD="$BINARY_NAME"
if ! command -v "$BINARY_CMD" >/dev/null 2>&1 && command -v "$INSTALL_NAME" >/dev/null 2>&1; then
    BINARY_CMD="$INSTALL_NAME"
fi

if command -v "$BINARY_CMD" >/dev/null 2>&1; then
    INSTALLED_VERSION=$("$BINARY_CMD" --version 2>/dev/null || echo "unknown")
    success "✓ ${INSTALL_NAME} installed successfully! (${INSTALLED_VERSION})"
    echo ""
    echo "  Get started:"
    echo "    ${BINARY_CMD} --help"
else
    success "✓ Installed to ${INSTALL_DIR}/${INSTALL_NAME}"
    echo "  Make sure ${INSTALL_DIR} is in your PATH."
fi
