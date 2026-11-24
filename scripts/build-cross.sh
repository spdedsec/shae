#!/usr/bin/env bash
# ==============================================================================
#  scripts/build-cross.sh
#  Multi-target cross-compilation pipeline for Shae
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Version detection from Cargo.toml
VERSION="$(grep -m1 '^version = ' "${REPO_ROOT}/Cargo.toml" | cut -d '"' -f 2 || echo "0.8.0")"

SUPPORTED_TARGETS=(
    "x86_64-unknown-linux-gnu"
    "x86_64-unknown-linux-musl"
    "aarch64-unknown-linux-gnu"
    "aarch64-unknown-linux-musl"
    "aarch64-apple-darwin"
    "x86_64-apple-darwin"
    "x86_64-pc-windows-msvc"
)

# Defaults
BUILD_MODE="build"
PROFILE=""
DRY_RUN=0
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/cargo-target}"
SELECTED_TARGETS=()

print_help() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS] [TARGET...]

Multi-target cross-compilation tool for Shae.

Arguments:
  TARGET                  One or more compilation targets, or 'all' / 'matrix'

Supported Targets:
  x86_64-unknown-linux-gnu    Linux 64-bit glibc (standard desktop/server)
  x86_64-unknown-linux-musl   Linux 64-bit musl (fully static binary)
  aarch64-unknown-linux-gnu   Linux ARM64 glibc (Debian multiarch)
  aarch64-unknown-linux-musl  Linux ARM64 musl (fully static ARM64)
  aarch64-apple-darwin        macOS Apple Silicon (M1/M2/M3/M4)
  x86_64-apple-darwin         macOS Intel 64-bit
  x86_64-pc-windows-msvc      Windows 64-bit PE

Options:
  -h, --help              Show this help message and exit
  -v, --version           Show version information and exit
  --release               Build with --release optimizations
  --check                 Run 'cargo check' instead of 'cargo build'
  --dry-run               Display commands and environment without executing
  --target-dir <DIR>      Override target directory (default: \$CARGO_TARGET_DIR or /tmp/cargo-target)
  --target <TARGET>       Explicit target specification
  all, matrix             Build all supported targets

Examples:
  $(basename "$0") --check x86_64-unknown-linux-musl
  $(basename "$0") --release x86_64-unknown-linux-musl
  $(basename "$0") --dry-run all
EOF
}

print_version() {
    echo "shae-build-cross ${VERSION}"
}

# Parse command line options
while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            print_help
            exit 0
            ;;
        -v|--version)
            print_version
            exit 0
            ;;
        --release)
            PROFILE="--release"
            shift
            ;;
        --check)
            BUILD_MODE="check"
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --target-dir)
            if [[ $# -lt 2 ]]; then
                echo "error: --target-dir requires a directory path" >&2
                exit 1
            fi
            TARGET_DIR="$2"
            shift 2
            ;;
        --target)
            if [[ $# -lt 2 ]]; then
                echo "error: --target requires a target triple" >&2
                exit 1
            fi
            SELECTED_TARGETS+=("$2")
            shift 2
            ;;
        all|matrix)
            SELECTED_TARGETS=("${SUPPORTED_TARGETS[@]}")
            shift
            ;;
        -*)
            echo "error: unrecognized option '$1'" >&2
            echo "Run '$(basename "$0") --help' for usage." >&2
            exit 1
            ;;
        *)
            SELECTED_TARGETS+=("$1")
            shift
            ;;
    esac
done

if [[ ${#SELECTED_TARGETS[@]} -eq 0 ]]; then
    CURRENT_HOST="$(rustc -vV 2>/dev/null | grep '^host:' | cut -d ' ' -f 2 || echo "x86_64-unknown-linux-gnu")"
    SELECTED_TARGETS=("$CURRENT_HOST")
fi

# Validate all selected targets
for target in "${SELECTED_TARGETS[@]}"; do
    valid=0
    for supported in "${SUPPORTED_TARGETS[@]}"; do
        if [[ "$target" == "$supported" ]]; then
            valid=1
            break
        fi
    done
    if [[ $valid -eq 0 ]]; then
        echo "error: unsupported target '$target'" >&2
        echo "Supported targets are: ${SUPPORTED_TARGETS[*]}" >&2
        exit 1
    fi
done

# Execute builds for each target
for target in "${SELECTED_TARGETS[@]}"; do
    echo "==> Configuring cross-compilation environment for target: ${target}"

    ENV_VARS=()
    ENV_VARS+=("PKG_CONFIG_ALLOW_CROSS=1")
    ENV_VARS+=("ALSA_NO_PKG_CONFIG=1")

    case "$target" in
        x86_64-unknown-linux-musl)
            if command -v musl-gcc >/dev/null 2>&1; then
                ENV_VARS+=("CC_x86_64_unknown_linux_musl=musl-gcc")
            elif command -v x86_64-linux-musl-gcc >/dev/null 2>&1; then
                ENV_VARS+=("CC_x86_64_unknown_linux_musl=x86_64-linux-musl-gcc")
            fi
            ;;
        aarch64-unknown-linux-gnu)
            if command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
                ENV_VARS+=("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc")
                ENV_VARS+=("CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc")
            fi
            ;;
        aarch64-unknown-linux-musl)
            if command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
                ENV_VARS+=("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc")
                ENV_VARS+=("CC_aarch64_unknown_linux_musl=aarch64-linux-gnu-gcc")
            fi
            ;;
        *)
            ;;
    esac

    CARGO_CMD=("cargo" "$BUILD_MODE" "--target" "$target" "--target-dir" "$TARGET_DIR")
    if [[ -n "$PROFILE" ]]; then
        CARGO_CMD+=("$PROFILE")
    fi

    echo "Environment:"
    for ev in "${ENV_VARS[@]}"; do
        echo "  export $ev"
    done
    echo "Command:"
    echo "  ${CARGO_CMD[*]}"

    if [[ $DRY_RUN -eq 1 ]]; then
        echo "==> [DRY RUN] Skipping actual cargo invocation for ${target}"
        continue
    fi

    if command -v rustup >/dev/null 2>&1; then
        if ! rustup target list --installed | grep -q "^${target}\$"; then
            echo "==> Installing rustup target ${target}..."
            rustup target add "$target" || {
                echo "warning: could not add target $target via rustup" >&2
            }
        fi
    fi

    (
        cd "$REPO_ROOT"
        for ev in "${ENV_VARS[@]}"; do
            export "$ev"
        done
        "${CARGO_CMD[@]}"
    )
    echo "==> Target ${target} completed successfully."
done
