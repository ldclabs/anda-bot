#!/bin/sh
# anda-bot installer — detects OS/Arch and downloads the right binary
# Usage: curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh

set -e

REPO="ldclabs/anda-bot"
BINARY_NAME="anda"
BANNER_ART='        _     _   _   ____      _
       / \   | \ | | |  _ \    / \
      / _ \  |  \| | | | | |  / _ \
     / ___ \ | |\  | | |_| | / ___ \
    /_/   \_\|_| \_| |____/ /_/   \_\  '

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

info() { printf "${CYAN}$1${NC}\n"; }
success() { printf "${GREEN}$1${NC}\n"; }
error() { printf "${RED}Error: $1${NC}\n" >&2; exit 1; }

print_banner() {
    printf "%s\n" "$BANNER_ART"
    printf '\n'
}

path_contains() {
    case ":${PATH}:" in
        *":$1:"*) return 0 ;;
        *) return 1 ;;
    esac
}

windows_home_dir() {
    if [ -n "${USERPROFILE:-}" ] && command -v cygpath >/dev/null 2>&1; then
        cygpath -u "$USERPROFILE"
    elif [ -n "${HOME:-}" ]; then
        printf '%s\n' "$HOME"
    elif [ -n "${USERPROFILE:-}" ]; then
        printf '%s\n' "$USERPROFILE"
    else
        return 1
    fi
}

windows_path_from_posix() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$1"
    elif [ -n "${USERPROFILE:-}" ]; then
        printf '%s\\bin\n' "$USERPROFILE"
    else
        printf '%s\n' "$1"
    fi
}

detect_profile() {
    SHELL_NAME=$(basename "${SHELL:-sh}" 2>/dev/null || printf 'sh')
    case "$SHELL_NAME" in
        zsh)  printf '%s\n' "$HOME/.zshrc" ;;
        bash) printf '%s\n' "$HOME/.bashrc" ;;
        fish) printf '%s\n' "$HOME/.config/fish/config.fish" ;;
        *)
            if [ "$OS" = "macos" ]; then
                printf '%s\n' "$HOME/.zshrc"
            else
                printf '%s\n' "$HOME/.profile"
            fi
            ;;
    esac
}

profile_has_install_dir() {
    PROFILE_PATH="$1"

    if [ ! -f "$PROFILE_PATH" ]; then
        return 1
    fi

    if grep -F "$INSTALL_DIR" "$PROFILE_PATH" >/dev/null 2>&1; then
        return 0
    fi

    if [ "$INSTALL_DIR" = "${HOME}/.local/bin" ]; then
        grep -F '.local/bin' "$PROFILE_PATH" >/dev/null 2>&1
    else
        return 1
    fi
}

append_unix_path_profile() {
    PROFILE_PATH=$(detect_profile)
    PROFILE_DIR=$(dirname "$PROFILE_PATH")
    SHELL_NAME=$(basename "${SHELL:-sh}" 2>/dev/null || printf 'sh')

    if profile_has_install_dir "$PROFILE_PATH"; then
        return 0
    fi

    mkdir -p "$PROFILE_DIR" 2>/dev/null || return 1

    if [ "$SHELL_NAME" = "fish" ]; then
        if [ "$INSTALL_DIR" = "${HOME}/.local/bin" ]; then
            PATH_LINE='fish_add_path -g "$HOME/.local/bin"'
        else
            PATH_LINE="fish_add_path -g \"$INSTALL_DIR\""
        fi
    elif [ "$INSTALL_DIR" = "${HOME}/.local/bin" ]; then
        PATH_LINE='export PATH="$HOME/.local/bin:$PATH"'
    else
        PATH_LINE="export PATH=\"$INSTALL_DIR:\$PATH\""
    fi

    {
        if [ -s "$PROFILE_PATH" ]; then
            printf '\n'
        fi
        printf '# anda-bot\n'
        printf '%s\n' "$PATH_LINE"
    } >> "$PROFILE_PATH"
}

ensure_unix_path() {
    if path_contains "$INSTALL_DIR"; then
        return 0
    fi

    export PATH="$INSTALL_DIR:$PATH"

    if append_unix_path_profile; then
        PROFILE_PATH=$(detect_profile)
        success "Ensured ${INSTALL_DIR} is in PATH via ${PROFILE_PATH}"
        info "Open a new terminal for the PATH change to take effect."
    else
        info "Add ${INSTALL_DIR} to your PATH to run ${BINARY_NAME} from any terminal."
    fi
}

ensure_windows_path() {
    if ! path_contains "$INSTALL_DIR"; then
        export PATH="$INSTALL_DIR:$PATH"
    fi

    WINDOWS_INSTALL_DIR=$(windows_path_from_posix "$INSTALL_DIR")

    if command -v powershell.exe >/dev/null 2>&1; then
        PS_INSTALL_DIR=$(printf '%s' "$WINDOWS_INSTALL_DIR" | sed "s/'/''/g")
        if powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "\$installDir = '${PS_INSTALL_DIR}'; \$userPath = [Environment]::GetEnvironmentVariable('Path', 'User'); \$parts = @(); if (-not [string]::IsNullOrWhiteSpace(\$userPath)) { \$parts = \$userPath -split ';' | Where-Object { \$_ } }; \$normalizedInstallDir = [Environment]::ExpandEnvironmentVariables(\$installDir).TrimEnd('\\'); \$exists = \$false; foreach (\$part in \$parts) { if ([Environment]::ExpandEnvironmentVariables(\$part).TrimEnd('\\').Equals(\$normalizedInstallDir, [StringComparison]::OrdinalIgnoreCase)) { \$exists = \$true; break } }; if (-not \$exists) { [Environment]::SetEnvironmentVariable('Path', ((\$parts + \$installDir) -join ';'), 'User') }" >/dev/null 2>&1; then
            success "Ensured ${WINDOWS_INSTALL_DIR} is in your Windows user PATH."
            info "Open a new terminal for the PATH change to take effect."
        else
            info "Could not update Windows user PATH automatically. Add ${WINDOWS_INSTALL_DIR} manually."
        fi
    else
        info "Add ${WINDOWS_INSTALL_DIR} to your Windows user PATH."
    fi
}

install_binary() {
    INSTALL_PATH="${INSTALL_DIR}/${INSTALL_NAME}"
    INSTALL_TMP="${INSTALL_DIR}/.${INSTALL_NAME}.$$"

    rm -f "$INSTALL_TMP" 2>/dev/null || true

    if ! mv "${TMPDIR}/${ASSET_NAME}" "$INSTALL_TMP"; then
        error "Could not stage binary in ${INSTALL_DIR}"
    fi

    chmod +x "$INSTALL_TMP" 2>/dev/null || true

    if mv -f "$INSTALL_TMP" "$INSTALL_PATH" 2>/dev/null; then
        return 0
    fi

    rm -f "$INSTALL_TMP" 2>/dev/null || true

    if [ "$OS" = "windows" ]; then
        error "Could not replace ${INSTALL_PATH}. If ${INSTALL_NAME} is running, stop it and rerun the installer."
    fi

    error "Could not replace ${INSTALL_PATH}"
}

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

if [ -n "${ANDA_INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$ANDA_INSTALL_DIR"
elif [ -n "${INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$INSTALL_DIR"
elif [ "$OS" = "windows" ]; then
    WINDOWS_HOME=$(windows_home_dir) || error "Could not detect USERPROFILE. Set ANDA_INSTALL_DIR and rerun."
    INSTALL_DIR="${WINDOWS_HOME}/bin"
else
    if [ -z "${HOME:-}" ]; then
        error "Could not detect HOME. Set ANDA_INSTALL_DIR and rerun."
    fi
    INSTALL_DIR="${HOME}/.local/bin"
fi

EXE_EXT=""
if [ "$OS" = "windows" ]; then
    EXE_EXT=".exe"
fi

ASSET_NAME="${BINARY_NAME}-${TARGET}${EXE_EXT}"
CHECKSUM_NAME="${ASSET_NAME}.sha256"
INSTALL_NAME="${BINARY_NAME}${EXE_EXT}"

print_banner

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
mkdir -p "$INSTALL_DIR" || error "Could not create install directory: ${INSTALL_DIR}"
install_binary

if [ "$OS" = "windows" ]; then
    ensure_windows_path
else
    ensure_unix_path
fi

# Verify
if [ -x "${INSTALL_DIR}/${INSTALL_NAME}" ]; then
    INSTALLED_VERSION=$("${INSTALL_DIR}/${INSTALL_NAME}" --version 2>/dev/null || echo "unknown")
    success "✓ ${INSTALL_NAME} installed successfully! (${INSTALLED_VERSION})"
    echo ""
    echo "  Get started:"
    echo "    ${BINARY_NAME} --help"
else
    success "✓ Installed to ${INSTALL_DIR}/${INSTALL_NAME}"
    echo "  Make sure ${INSTALL_DIR} is in your PATH."
fi
