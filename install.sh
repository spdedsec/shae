#!/usr/bin/env bash
# ==============================================================================
#  Shae Universal Installer
#  Usage: curl -fsSL https://shae-lang.org/install.sh | bash
#         curl -fsSL https://raw.githubusercontent.com/spdedsec/shae/main/install.sh | bash
#
#  Environment variables:
#    SHAE_VERSION          Target version to install (e.g. v0.4.0; defaults to latest)
#    SHAE_INSTALL_DIR      Target directory for binary (defaults to ~/.cargo/bin or ~/.shae/bin)
#    SHAE_NO_MODIFY_PATH   Set to 1 or true to skip shell profile path suggestions
# ==============================================================================

set -euo pipefail

# ANSI styling
BOLD="$(tput bold 2>/dev/null || printf "")"
GREEN="$(tput setaf 2 2>/dev/null || printf "")"
BLUE="$(tput setaf 4 2>/dev/null || printf "")"
YELLOW="$(tput setaf 3 2>/dev/null || printf "")"
RED="$(tput setaf 1 2>/dev/null || printf "")"
RESET="$(tput sgr0 2>/dev/null || printf "")"

info() {
    printf "%s==>%s %s%s%s\n" "${BLUE}" "${RESET}" "${BOLD}" "$1" "${RESET}" >&2
}

success() {
    printf "%s==>%s %s%s%s\n" "${GREEN}" "${RESET}" "${BOLD}" "$1" "${RESET}" >&2
}

warn() {
    printf "%swarning:%s %s\n" "${YELLOW}" "${RESET}" "$1" >&2
}

error() {
    printf "%serror:%s %s\n" "${RED}" "${RESET}" "$1" >&2
    exit 1
}

# Print Shae Banner
print_banner() {
    cat << 'BANNER'
   ____  _                 
  / ___|| |__   __ _  ___  
  \___ \| '_ \ / _` |/ _ \ 
   ___) | | | | (_| |  __/ 
  |____/|_| |_|\__,_|\___| 
BANNER
    printf "%sShae Universal Toolchain Installer%s\n\n" "${BOLD}" "${RESET}"
}

# 1. Detect Operating System
detect_os() {
    local os
    os="$(uname -s)"
    case "$os" in
        Linux*)
            OS="linux"
            # Detect glibc vs musl
            if [ -f /etc/alpine-release ] || ldd --version 2>&1 | grep -qi musl || ls /lib/ld-musl* >/dev/null 2>&1; then
                LIBC="musl"
            else
                LIBC="gnu"
            fi
            ;;
        Darwin*)
            OS="macos"
            LIBC=""
            ;;
        MINGW*|MSYS*|CYGWIN*)
            OS="windows"
            LIBC=""
            ;;
        *)
            error "Unsupported operating system: $os. Shae supports Linux, macOS, and Windows."
            ;;
    esac
}

# 2. Detect CPU Architecture
detect_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64)
            ARCH="x86_64"
            ;;
        aarch64|arm64)
            ARCH="arm64"
            ;;
        *)
            error "Unsupported architecture: $arch. Shae supports x86_64 and arm64/aarch64."
            ;;
    esac
}

# 3. Determine Asset Name
determine_asset() {
    case "$OS" in
        linux)
            if [ "$ARCH" = "arm64" ]; then
                ASSET_NAME="shae-${TAG}-linux-aarch64.tar.gz"
                EXT="tar.gz"
            elif [ "$LIBC" = "musl" ]; then
                ASSET_NAME="shae-${TAG}-linux-x86_64-musl.tar.gz"
                EXT="tar.gz"
            else
                ASSET_NAME="shae-${TAG}-linux-x86_64.tar.gz"
                EXT="tar.gz"
            fi
            BINARY_NAME="shae"
            ;;
        macos)
            if [ "$ARCH" = "arm64" ]; then
                ASSET_NAME="shae-${TAG}-macos-arm64.tar.gz"
            else
                ASSET_NAME="shae-${TAG}-macos-x86_64.tar.gz"
            fi
            EXT="tar.gz"
            BINARY_NAME="shae"
            ;;
        windows)
            if [ "$ARCH" = "x86_64" ]; then
                ASSET_NAME="shae-${TAG}-windows-x86_64.zip"
                EXT="zip"
                BINARY_NAME="shae.exe"
            else
                error "Windows on $ARCH is not currently supported."
            fi
            ;;
    esac
}

# 4. Resolve Release Version
resolve_version() {
    REPO="spdedsec/shae"
    FALLBACK_VERSION="v0.4.0"

    if [ -n "${SHAE_VERSION:-}" ]; then
        if [[ "$SHAE_VERSION" != v* ]]; then
            TAG="v${SHAE_VERSION}"
        else
            TAG="$SHAE_VERSION"
        fi
        info "Using requested version ${TAG}"
        return
    fi

    info "Resolving latest release from GitHub..."
    local latest_tag=""
    if command -v curl >/dev/null 2>&1; then
        latest_tag="$(curl -sSfL -H "Accept: application/vnd.github.v3+json" "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | head -n1 | cut -d '"' -f 4 || true)"
    elif command -v wget >/dev/null 2>&1; then
        latest_tag="$(wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | head -n1 | cut -d '"' -f 4 || true)"
    fi

    if [ -n "$latest_tag" ]; then
        TAG="$latest_tag"
        info "Latest release resolved: ${TAG}"
    else
        TAG="$FALLBACK_VERSION"
        warn "Could not query GitHub Releases API; falling back to ${TAG}"
    fi
}

# 5. Determine Target Install Directory
determine_install_dir() {
    if [ -n "${SHAE_INSTALL_DIR:-}" ]; then
        INSTALL_DIR="$SHAE_INSTALL_DIR"
    elif [ -d "$HOME/.cargo/bin" ] && [[ ":$PATH:" == *":$HOME/.cargo/bin:"* ]]; then
        INSTALL_DIR="$HOME/.cargo/bin"
    else
        INSTALL_DIR="$HOME/.shae/bin"
    fi
}

# 6. Download Helper
download_file() {
    local url="$1"
    local dest="$2"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$url" -O "$dest"
    else
        error "Neither curl nor wget is installed. Please install curl or wget to continue."
    fi
}

# 7. Compute Hash Helper
compute_sha256() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$file" | awk '{print $NF}'
    else
        echo ""
    fi
}

# Main Execution Flow
main() {
    print_banner
    detect_os
    detect_arch
    resolve_version
    determine_asset
    determine_install_dir

    info "Platform: ${OS} (${ARCH}${LIBC:+-$LIBC})"
    info "Target Asset: ${ASSET_NAME}"
    info "Install Path: ${INSTALL_DIR}/${BINARY_NAME}"

    TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'shae-install')"
    trap 'rm -rf "$TMP_DIR"' EXIT

    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET_NAME}"
    CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${TAG}/SHA256SUMS.txt"

    info "Downloading ${ASSET_NAME}..."
    if ! download_file "$DOWNLOAD_URL" "$TMP_DIR/$ASSET_NAME"; then
        # If musl asset download failed on linux, attempt glibc fallback
        if [ "$OS" = "linux" ] && [ "$LIBC" = "musl" ]; then
            warn "Musl asset not available. Attempting glibc fallback..."
            ASSET_NAME="shae-${TAG}-linux-x86_64.tar.gz"
            DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET_NAME}"
            download_file "$DOWNLOAD_URL" "$TMP_DIR/$ASSET_NAME" || error "Failed to download ${DOWNLOAD_URL}"
        else
            error "Failed to download release asset from ${DOWNLOAD_URL}"
        fi
    fi

    # Checksum Verification
    info "Verifying cryptographic SHA-256 checksum..."
    if download_file "$CHECKSUMS_URL" "$TMP_DIR/SHA256SUMS.txt" 2>/dev/null; then
        EXPECTED_HASH="$(grep "$ASSET_NAME" "$TMP_DIR/SHA256SUMS.txt" | head -n1 | awk '{print $1}' || true)"
        if [ -n "$EXPECTED_HASH" ]; then
            ACTUAL_HASH="$(compute_sha256 "$TMP_DIR/$ASSET_NAME")"
            if [ -n "$ACTUAL_HASH" ]; then
                if [ "$EXPECTED_HASH" != "$ACTUAL_HASH" ]; then
                    error "Checksum verification failed! Expected ${EXPECTED_HASH}, but got ${ACTUAL_HASH}."
                fi
                success "Checksum verified: ${ACTUAL_HASH}"
            else
                warn "No SHA-256 utility (sha256sum/shasum/openssl) found; skipping verification."
            fi
        else
            warn "Asset '${ASSET_NAME}' not listed in SHA256SUMS.txt; proceeding without verification."
        fi
    else
        warn "SHA256SUMS.txt not available for this release; skipping checksum verification."
    fi

    # Extraction
    info "Extracting ${ASSET_NAME}..."
    if [ "$EXT" = "tar.gz" ]; then
        tar -xzf "$TMP_DIR/$ASSET_NAME" -C "$TMP_DIR"
    elif [ "$EXT" = "zip" ]; then
        if command -v unzip >/dev/null 2>&1; then
            unzip -q -o "$TMP_DIR/$ASSET_NAME" -d "$TMP_DIR"
        else
            error "'unzip' command not found. Please install unzip to extract Windows release packages."
        fi
    fi

    # Find the binary
    EXTRACTED_BIN=""
    if [ -f "$TMP_DIR/$BINARY_NAME" ]; then
        EXTRACTED_BIN="$TMP_DIR/$BINARY_NAME"
    else
        EXTRACTED_BIN="$(find "$TMP_DIR" -type f -name "$BINARY_NAME" | head -n1 || true)"
    fi

    if [ -z "$EXTRACTED_BIN" ] || [ ! -f "$EXTRACTED_BIN" ]; then
        error "Extracted binary '$BINARY_NAME' not found in release archive."
    fi

    # Install binary
    mkdir -p "$INSTALL_DIR"
    chmod +x "$EXTRACTED_BIN"
    mv "$EXTRACTED_BIN" "$INSTALL_DIR/$BINARY_NAME"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"

    success "Successfully installed Shae to ${INSTALL_DIR}/${BINARY_NAME}"

    # Verification
    if [ -x "$INSTALL_DIR/$BINARY_NAME" ]; then
        INSTALLED_VERSION="$("$INSTALL_DIR/$BINARY_NAME" --version 2>&1 || true)"
        success "Verified: ${INSTALLED_VERSION}"
    fi

    # PATH checks and guidance
    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
            # Already in PATH
            ;;
        *)
            printf "\n%sAttention:%s %s is not in your \$PATH.\n" "${YELLOW}" "${RESET}" "${INSTALL_DIR}"
            printf "To run 'shae' from anywhere, add it to your PATH by adding this to your profile:\n\n"
            printf "    export PATH=\"%s:\$PATH\"\n\n" "${INSTALL_DIR}"
            printf "Run this command to configure your current session:\n"
            printf "    export PATH=\"%s:\$PATH\"\n" "${INSTALL_DIR}"
            ;;
    esac

    printf "\n%sHappy hacking with Shae!%s Run 'shae --help' to explore the toolchain.\n" "${GREEN}${BOLD}" "${RESET}"
}

if [ "${BASH_SOURCE[0]:-$0}" = "$0" ]; then
    main "$@"
fi
