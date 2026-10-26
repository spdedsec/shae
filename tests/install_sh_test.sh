#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_SH="$SCRIPT_DIR/install.sh"

echo "=========================================================="
echo "          SHAE UNIVERSAL INSTALLER TEST SUITE            "
echo "=========================================================="

# 1. Syntax Validation
echo "[TEST 1] Validating install.sh syntax..."
bash -n "$INSTALL_SH"
echo "  [PASS] bash -n passed cleanly."

# 2. Source and Test Functions in Isolation
echo "[TEST 2] Testing detection and asset resolution..."

# Test OS Detection
OS_RESULT=$(bash -c '
    source "$1"
    detect_os
    echo "$OS:$LIBC"
' _ "$INSTALL_SH")
echo "  Detected OS:LIBC -> $OS_RESULT"
if [[ "$OS_RESULT" != linux:* ]]; then
    echo "  [FAIL] Expected linux OS, got $OS_RESULT"
    exit 1
fi
echo "  [PASS] OS detection verified."

# Test Architecture Detection
ARCH_RESULT=$(bash -c '
    source "$1"
    detect_arch
    echo "$ARCH"
' _ "$INSTALL_SH")
echo "  Detected Architecture -> $ARCH_RESULT"
if [[ "$ARCH_RESULT" != "x86_64" && "$ARCH_RESULT" != "arm64" ]]; then
    echo "  [FAIL] Expected x86_64 or arm64, got $ARCH_RESULT"
    exit 1
fi
echo "  [PASS] Architecture detection verified."

# Test Version Normalization
TAG_RESULT=$(bash -c '
    source "$1"
    SHAE_VERSION="0.4.0"
    resolve_version
    echo "$TAG"
' _ "$INSTALL_SH")
if [ "$TAG_RESULT" != "v0.4.0" ]; then
    echo "  [FAIL] Expected v0.4.0, got $TAG_RESULT"
    exit 1
fi
echo "  [PASS] Version normalization (0.4.0 -> v0.4.0) verified."

# Test Asset Determination for macOS and Windows
MACOS_ASSET=$(bash -c '
    source "$1"
    OS="macos"
    ARCH="arm64"
    TAG="v0.4.0"
    determine_asset
    echo "$ASSET_NAME"
' _ "$INSTALL_SH")
if [ "$MACOS_ASSET" != "shae-v0.4.0-macos-arm64.tar.gz" ]; then
    echo "  [FAIL] Expected shae-v0.4.0-macos-arm64.tar.gz, got $MACOS_ASSET"
    exit 1
fi
echo "  [PASS] macOS arm64 asset determination verified."

WIN_ASSET=$(bash -c '
    source "$1"
    OS="windows"
    ARCH="x86_64"
    TAG="v0.4.0"
    determine_asset
    echo "$ASSET_NAME"
' _ "$INSTALL_SH")
if [ "$WIN_ASSET" != "shae-v0.4.0-windows-x86_64.zip" ]; then
    echo "  [FAIL] Expected shae-v0.4.0-windows-x86_64.zip, got $WIN_ASSET"
    exit 1
fi
echo "  [PASS] Windows x86_64 asset determination verified."

# 3. End-to-End Mock Download & Checksum Verification Test
echo "[TEST 3] Testing Mock Download, Checksum Verification, and Installation..."
TMP_TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_TEST_DIR"' EXIT

MOCK_BIN="$TMP_TEST_DIR/shae"
cat << 'MOCK_SHAE' > "$MOCK_BIN"
#!/usr/bin/env bash
echo "shae 0.4.0"
MOCK_SHAE
chmod +x "$MOCK_BIN"

# Package into tar.gz matching asset name
MOCK_ASSET="shae-v0.4.0-linux-x86_64.tar.gz"
tar -czvf "$TMP_TEST_DIR/$MOCK_ASSET" -C "$TMP_TEST_DIR" shae >/dev/null

# Generate Checksums
cd "$TMP_TEST_DIR"
sha256sum "$MOCK_ASSET" > SHA256SUMS.txt
cd - >/dev/null

# Custom Target Install Directory
MOCK_INSTALL_DIR="$TMP_TEST_DIR/installed_bin"

bash -c '
    INSTALL_SCRIPT="$1"
    MOCK_DIR="$2"
    TARGET_DIR="$3"
    
    source "$INSTALL_SCRIPT"

    # Mock download_file
    download_file() {
        local url="$1"
        local dest="$2"
        local filename="$(basename "$url")"
        if [ -f "$MOCK_DIR/$filename" ]; then
            cp "$MOCK_DIR/$filename" "$dest"
            return 0
        else
            return 1
        fi
    }

    print_banner() { :; }
    export SHAE_VERSION="v0.4.0"
    export SHAE_INSTALL_DIR="$TARGET_DIR"
    export LIBC="gnu"

    main
' _ "$INSTALL_SH" "$TMP_TEST_DIR" "$MOCK_INSTALL_DIR"

if [ ! -x "$MOCK_INSTALL_DIR/shae" ]; then
    echo "  [FAIL] Expected binary at $MOCK_INSTALL_DIR/shae"
    exit 1
fi
INSTALLED_OUT=$("$MOCK_INSTALL_DIR/shae")
if [ "$INSTALLED_OUT" != "shae 0.4.0" ]; then
    echo "  [FAIL] Unexpected output: $INSTALLED_OUT"
    exit 1
fi
echo "  [PASS] Mock download, checksum validation, extraction, and verification passed."

# 4. Tamper Checksum Test (Ensures installer rejects corrupt / tampered downloads)
echo "[TEST 4] Testing Tampered Checksum Detection..."
echo "0000000000000000000000000000000000000000000000000000000000000000  $MOCK_ASSET" > "$TMP_TEST_DIR/SHA256SUMS.txt"

set +e
bash -c '
    INSTALL_SCRIPT="$1"
    MOCK_DIR="$2"
    TARGET_DIR="$3"
    
    source "$INSTALL_SCRIPT"

    download_file() {
        local url="$1"
        local dest="$2"
        local filename="$(basename "$url")"
        if [ -f "$MOCK_DIR/$filename" ]; then
            cp "$MOCK_DIR/$filename" "$dest"
            return 0
        else
            return 1
        fi
    }

    print_banner() { :; }
    export SHAE_VERSION="v0.4.0"
    export SHAE_INSTALL_DIR="$TARGET_DIR"
    export LIBC="gnu"

    main
' _ "$INSTALL_SH" "$TMP_TEST_DIR" "$MOCK_INSTALL_DIR" 2>/dev/null
TAMPER_STATUS=$?
set -e

if [ "$TAMPER_STATUS" -ne 0 ]; then
    echo "  [PASS] Installer correctly detected checksum mismatch and aborted with status $TAMPER_STATUS."
else
    echo "  [FAIL] Installer did not reject mismatched checksum!"
    exit 1
fi

echo "=========================================================="
echo "          ALL INSTALLER TESTS PASSED 100%!                "
echo "=========================================================="
