#!/usr/bin/env bash
# ==============================================================================
#  scripts/release.sh
#  Packaging pipeline and cryptographic checksum generator for Shae
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Detect version from Cargo.toml
DETECTED_VER="$(grep -m1 '^version = ' "${REPO_ROOT}/Cargo.toml" | cut -d '"' -f 2 || echo "0.8.0")"
RAW_VERSION="$DETECTED_VER"
TAG="v${RAW_VERSION#v}"
DIST_DIR="${REPO_ROOT}/dist"
DRY_RUN=0

SUPPORTED_TARGETS=(
    "x86_64-unknown-linux-gnu"
    "x86_64-unknown-linux-musl"
    "aarch64-unknown-linux-gnu"
    "aarch64-unknown-linux-musl"
    "aarch64-apple-darwin"
    "x86_64-apple-darwin"
    "macos-universal"
    "universal-apple-darwin"
    "x86_64-pc-windows-msvc"
)

print_help() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS]

Packaging and release artifact pipeline for Shae.

Options:
  -h, --help                    Show this help message and exit
  -v, --version [VER]           Show version or set target release version (e.g. 0.8.0 or v0.8.0)
  --dist, --out-dir <DIR>       Distribution output directory (default: dist)
  --package <TARGET> <BIN_PATH> Package a compiled binary into its release archive
  --checksums                   Generate SHA256SUMS.txt, sha256sums.txt, and individual .sha256 files
  --verify [DIST_DIR]           Validate checksums and archive extraction integrity
  --dry-run                     Display packaging operations without writing files
  --all                         Prepare release assets for all targets

Supported Targets:
  x86_64-unknown-linux-gnu    -> shae-\${TAG}-linux-x86_64.tar.gz
  x86_64-unknown-linux-musl   -> shae-\${TAG}-linux-x86_64-musl.tar.gz
  aarch64-unknown-linux-gnu   -> shae-\${TAG}-linux-aarch64.tar.gz
  aarch64-unknown-linux-musl  -> shae-\${TAG}-linux-aarch64-musl.tar.gz
  aarch64-apple-darwin        -> shae-\${TAG}-macos-arm64.tar.gz
  x86_64-apple-darwin         -> shae-\${TAG}-macos-x86_64.tar.gz
  macos-universal             -> shae-\${TAG}-macos-universal.tar.gz
  x86_64-pc-windows-msvc      -> shae-\${TAG}-windows-x86_64.zip

Archive Contents:
  shae-v{VERSION}-{TARGET}/
    ├── shae (or shae.exe)
    ├── README.md
    ├── LICENSE
    └── GETTING_STARTED.md

Examples:
  $(basename "$0") --package x86_64-unknown-linux-musl target/x86_64-unknown-linux-musl/release/shae
  $(basename "$0") --checksums --dist dist
  $(basename "$0") --verify dist
  $(basename "$0") --dry-run --package x86_64-pc-windows-msvc target/release/shae.exe
EOF
}

resolve_asset() {
    local target="$1"
    case "$target" in
        x86_64-unknown-linux-gnu)
            ASSET_NAME="shae-${TAG}-linux-x86_64.tar.gz"
            ARCHIVE_TYPE="tar.gz"
            BIN_NAME="shae"
            ;;
        x86_64-unknown-linux-musl)
            ASSET_NAME="shae-${TAG}-linux-x86_64-musl.tar.gz"
            ARCHIVE_TYPE="tar.gz"
            BIN_NAME="shae"
            ;;
        aarch64-unknown-linux-gnu)
            ASSET_NAME="shae-${TAG}-linux-aarch64.tar.gz"
            ARCHIVE_TYPE="tar.gz"
            BIN_NAME="shae"
            ;;
        aarch64-unknown-linux-musl)
            ASSET_NAME="shae-${TAG}-linux-aarch64-musl.tar.gz"
            ARCHIVE_TYPE="tar.gz"
            BIN_NAME="shae"
            ;;
        aarch64-apple-darwin)
            ASSET_NAME="shae-${TAG}-macos-arm64.tar.gz"
            ARCHIVE_TYPE="tar.gz"
            BIN_NAME="shae"
            ;;
        x86_64-apple-darwin)
            ASSET_NAME="shae-${TAG}-macos-x86_64.tar.gz"
            ARCHIVE_TYPE="tar.gz"
            BIN_NAME="shae"
            ;;
        macos-universal|universal-apple-darwin)
            ASSET_NAME="shae-${TAG}-macos-universal.tar.gz"
            ARCHIVE_TYPE="tar.gz"
            BIN_NAME="shae"
            ;;
        x86_64-pc-windows-msvc)
            ASSET_NAME="shae-${TAG}-windows-x86_64.zip"
            ARCHIVE_TYPE="zip"
            BIN_NAME="shae.exe"
            ;;
        *)
            echo "error: unsupported target '$target'" >&2
            echo "Supported targets: ${SUPPORTED_TARGETS[*]}" >&2
            exit 1
            ;;
    esac
}

compute_file_sha256() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$file" | awk '{print $NF}'
    elif command -v python3 >/dev/null 2>&1; then
        python3 -c "import hashlib, sys; print(hashlib.sha256(open(sys.argv[1], 'rb').read()).hexdigest())" "$file"
    else
        echo "error: no SHA-256 computation utility available" >&2
        exit 1
    fi
}

package_target() {
    local target="$1"
    local bin_path="$2"

    resolve_asset "$target"

    echo "==> Packaging target: ${target}"
    echo "    Binary source:   ${bin_path}"
    echo "    Output archive:  ${DIST_DIR}/${ASSET_NAME}"

    if [[ $DRY_RUN -eq 1 ]]; then
        echo "==> [DRY RUN] Would package ${bin_path} into ${DIST_DIR}/${ASSET_NAME}"
        return 0
    fi

    if [[ ! -f "$bin_path" ]]; then
        echo "error: binary file '$bin_path' not found" >&2
        exit 1
    fi

    mkdir -p "$DIST_DIR"

    local stage_root
    stage_root="$(mktemp -d 2>/dev/null || mktemp -d -t 'shae-stage')"
    local folder_name="shae-${TAG}-${target}"
    local stage_dir="${stage_root}/${folder_name}"
    mkdir -p "$stage_dir"

    cp "$bin_path" "${stage_dir}/${BIN_NAME}"
    chmod 0755 "${stage_dir}/${BIN_NAME}"

    for doc in "README.md" "LICENSE" "GETTING_STARTED.md"; do
        if [[ -f "${REPO_ROOT}/${doc}" ]]; then
            cp "${REPO_ROOT}/${doc}" "${stage_dir}/${doc}"
            chmod 0644 "${stage_dir}/${doc}"
        else
            echo "warning: ${doc} not found at repository root" >&2
        fi
    done

    local out_archive="${DIST_DIR}/${ASSET_NAME}"
    rm -f "$out_archive"

    if [[ "$ARCHIVE_TYPE" == "tar.gz" ]]; then
        tar -czf "$out_archive" -C "$stage_root" "$folder_name"
    elif [[ "$ARCHIVE_TYPE" == "zip" ]]; then
        if command -v zip >/dev/null 2>&1; then
            (
                cd "$stage_root"
                zip -r -q "$out_archive" "$folder_name"
            )
        elif command -v python3 >/dev/null 2>&1; then
            python3 -c "
import zipfile, os, sys
stage_root = sys.argv[1]
folder_name = sys.argv[2]
out_zip = sys.argv[3]
with zipfile.ZipFile(out_zip, 'w', zipfile.ZIP_DEFLATED) as z:
    for root, dirs, files in os.walk(os.path.join(stage_root, folder_name)):
        for f in files:
            full = os.path.join(root, f)
            rel = os.path.relpath(full, stage_root)
            z.write(full, rel)
" "$stage_root" "$folder_name" "$out_archive"
        else
            echo "error: neither zip nor python3 available to create zip archive" >&2
            rm -rf "$stage_root"
            exit 1
        fi
    fi

    rm -rf "$stage_root"

    local hash
    hash="$(compute_file_sha256 "$out_archive")"
    echo "${hash}  ${ASSET_NAME}" > "${out_archive}.sha256"

    echo "==> Successfully created ${out_archive} (${hash})"
}

generate_checksums() {
    local target_dir="$1"

    echo "==> Generating release checksums in ${target_dir}..."

    if [[ $DRY_RUN -eq 1 ]]; then
        echo "==> [DRY RUN] Would generate SHA256SUMS.txt, sha256sums.txt, and individual .sha256 files in ${target_dir}"
        return 0
    fi

    if [[ ! -d "$target_dir" ]]; then
        echo "error: directory '$target_dir' does not exist" >&2
        exit 1
    fi

    local manifest_upper="${target_dir}/SHA256SUMS.txt"
    local manifest_lower="${target_dir}/sha256sums.txt"

    rm -f "$manifest_upper" "$manifest_lower"

    local entries=()
    for file in "$target_dir"/*; do
        [[ -f "$file" ]] || continue
        local fname
        fname="$(basename "$file")"
        if [[ "$fname" == "SHA256SUMS.txt" || "$fname" == "sha256sums.txt" || "$fname" == *.sha256 ]]; then
            continue
        fi
        if [[ "$fname" == *.tar.gz || "$fname" == *.zip || "$fname" == *.vsix ]]; then
            local hash
            hash="$(compute_file_sha256 "$file")"
            entries+=("${hash}  ${fname}")
            echo "${hash}  ${fname}" > "${file}.sha256"
        fi
    done

    if [[ ${#entries[@]} -eq 0 ]]; then
        echo "warning: no release archives found in ${target_dir}" >&2
        touch "$manifest_upper" "$manifest_lower"
        return 0
    fi

    IFS=$'
' sorted_entries=($(sort <<<"${entries[*]}"))
    unset IFS

    printf "%s
" "${sorted_entries[@]}" > "$manifest_upper"
    cp "$manifest_upper" "$manifest_lower"

    echo "==> Created SHA256SUMS.txt and sha256sums.txt with ${#sorted_entries[@]} entries:"
    cat "$manifest_upper"
}

verify_distribution() {
    local target_dir="$1"

    echo "==> Verifying distribution in ${target_dir}..."

    if [[ ! -d "$target_dir" ]]; then
        echo "error: distribution directory '$target_dir' does not exist" >&2
        exit 1
    fi

    local manifest=""
    if [[ -f "${target_dir}/SHA256SUMS.txt" ]]; then
        manifest="${target_dir}/SHA256SUMS.txt"
    elif [[ -f "${target_dir}/sha256sums.txt" ]]; then
        manifest="${target_dir}/sha256sums.txt"
    else
        echo "error: no checksum manifest found in ${target_dir}" >&2
        exit 1
    fi

    local verified_count=0
    while IFS= read -r line || [[ -n "$line" ]]; do
        line="$(echo "$line" | tr -d '')"
        [[ -z "$line" ]] && continue
        local expected_hash
        local file_rel
        expected_hash="$(echo "$line" | awk '{print $1}' | tr '[:upper:]' '[:lower:]')"
        file_rel="$(echo "$line" | awk '{$1=""; print $0}' | sed 's/^[ 	]*//')"

        local full_path="${target_dir}/${file_rel}"
        if [[ ! -f "$full_path" ]]; then
            echo "error: manifest file '${file_rel}' not found in ${target_dir}" >&2
            exit 1
        fi

        local actual_hash
        actual_hash="$(compute_file_sha256 "$full_path" | tr '[:upper:]' '[:lower:]')"
        if [[ "$expected_hash" != "$actual_hash" ]]; then
            echo "error: checksum mismatch for ${file_rel}!" >&2
            echo "  Expected: ${expected_hash}" >&2
            echo "  Actual:   ${actual_hash}" >&2
            exit 1
        fi
        echo "  [OK] ${file_rel}: ${actual_hash}"
        verified_count=$((verified_count + 1))

        local tmp_extract
        tmp_extract="$(mktemp -d 2>/dev/null || mktemp -d -t 'shae-extract')"
        if [[ "$file_rel" == *.tar.gz ]]; then
            tar -xzf "$full_path" -C "$tmp_extract"
        elif [[ "$file_rel" == *.zip ]]; then
            if command -v unzip >/dev/null 2>&1; then
                unzip -q "$full_path" -d "$tmp_extract"
            else
                python3 -c "import zipfile, sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$full_path" "$tmp_extract"
            fi
        fi

        local bin_found=0
        if find "$tmp_extract" -type f \( -name "shae" -o -name "shae.exe" \) | grep -q .; then
            bin_found=1
        fi
        if [[ $bin_found -eq 0 && "$file_rel" != *.vsix ]]; then
            echo "error: ${file_rel} did not contain shae or shae.exe binary!" >&2
            rm -rf "$tmp_extract"
            exit 1
        fi

        rm -rf "$tmp_extract"
    done < "$manifest"

    echo "==> All ${verified_count} distribution artifacts verified successfully."
}

ACTION=""
ARG_TARGET=""
ARG_BIN=""

if [[ $# -eq 0 ]]; then
    print_help
    exit 0
fi

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            print_help
            exit 0
            ;;
        -v|--version)
            if [[ $# -gt 1 && ! "$2" =~ ^- ]]; then
                RAW_VERSION="$2"
                TAG="v${RAW_VERSION#v}"
                shift 2
            else
                echo "shae-release ${TAG}"
                exit 0
            fi
            ;;
        --dist|--out-dir)
            if [[ $# -lt 2 ]]; then
                echo "error: --dist requires a directory argument" >&2
                exit 1
            fi
            DIST_DIR="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --package)
            if [[ $# -lt 3 ]]; then
                echo "error: --package requires <TARGET> and <BINARY_PATH>" >&2
                exit 1
            fi
            ACTION="package"
            ARG_TARGET="$2"
            ARG_BIN="$3"
            shift 3
            ;;
        --checksums)
            ACTION="checksums"
            shift
            ;;
        --verify)
            ACTION="verify"
            if [[ $# -gt 1 && ! "$2" =~ ^- ]]; then
                DIST_DIR="$2"
                shift 2
            else
                shift 1
            fi
            ;;
        --all)
            ACTION="all"
            shift
            ;;
        -*)
            echo "error: unrecognized option '$1'" >&2
            exit 1
            ;;
        *)
            echo "error: unexpected argument '$1'" >&2
            exit 1
            ;;
    esac
done

case "$ACTION" in
    package)
        package_target "$ARG_TARGET" "$ARG_BIN"
        ;;
    checksums)
        generate_checksums "$DIST_DIR"
        ;;
    verify)
        verify_distribution "$DIST_DIR"
        ;;
    all)
        if [[ $DRY_RUN -eq 1 ]]; then
            echo "==> [DRY RUN] Release pipeline for targets:"
            for t in "${SUPPORTED_TARGETS[@]}"; do
                resolve_asset "$t"
                echo "  ${t} -> ${ASSET_NAME}"
            done
            echo "  Manifests: SHA256SUMS.txt, sha256sums.txt"
        else
            echo "==> Use --package <TARGET> <BIN> to stage individual artifacts or run release workflow."
        fi
        ;;
    "")
        echo "error: no action specified. Use --package, --checksums, or --verify." >&2
        exit 1
        ;;
esac
