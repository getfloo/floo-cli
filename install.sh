#!/usr/bin/env bash
# Floo CLI installer
# Usage: curl -fsSL https://getfloo.com/install.sh | bash

set -euo pipefail

REPO="${FLOO_INSTALL_REPO:-getfloo/floo-cli}"
INSTALL_DIR="${FLOO_INSTALL_DIR:-/usr/local/bin}"
REQUESTED_VERSION="${FLOO_INSTALL_VERSION:-}"
BINARY_NAME="floo"
TMP_DIR=""

log() {
    echo "$*" >&2
}

fail() {
    echo "Error: $*" >&2
    exit 1
}

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        fail "Required command '$1' is not installed."
    fi
}

sha256_file() {
    local file="$1"

    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print tolower($1)}'
        return
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print tolower($1)}'
        return
    fi

    if command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$file" | awk '{print tolower($NF)}'
        return
    fi

    fail "No SHA256 tool found (need shasum, sha256sum, or openssl)."
}

detect_target() {
    local uname_s="${FLOO_INSTALL_OS:-$(uname -s)}"
    local uname_m="${FLOO_INSTALL_ARCH:-$(uname -m)}"
    local os=""
    local arch=""

    case "$uname_s" in
        Darwin) os="apple-darwin" ;;
        Linux) os="unknown-linux-musl" ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT)
            fail "Windows install is not supported by this script. Download floo.exe from https://github.com/${REPO}/releases"
            ;;
        *)
            fail "Unsupported operating system: ${uname_s}. Supported: macOS, Linux."
            ;;
    esac

    case "$uname_m" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *)
            fail "Unsupported architecture: ${uname_m}. Supported: x86_64, arm64."
            ;;
    esac

    echo "${arch}-${os}"
}

release_binary_url() {
    local asset="$1"

    if [[ -n "${FLOO_INSTALL_BINARY_URL:-}" ]]; then
        echo "${FLOO_INSTALL_BINARY_URL}"
        return
    fi

    if [[ -n "$REQUESTED_VERSION" ]]; then
        echo "https://github.com/${REPO}/releases/download/${REQUESTED_VERSION}/${asset}"
    else
        echo "https://github.com/${REPO}/releases/latest/download/${asset}"
    fi
}

release_checksum_url() {
    local binary_url="$1"

    if [[ -n "${FLOO_INSTALL_CHECKSUM_URL:-}" ]]; then
        echo "${FLOO_INSTALL_CHECKSUM_URL}"
    else
        echo "${binary_url}.sha256"
    fi
}

install_binary() {
    local source_path="$1"
    local destination_path="${INSTALL_DIR}/${BINARY_NAME}"

    mkdir -p "$INSTALL_DIR" 2>/dev/null || true

    if [[ -w "$INSTALL_DIR" ]]; then
        mv "$source_path" "$destination_path"
        chmod 755 "$destination_path"
        return
    fi

    if ! command -v sudo >/dev/null 2>&1; then
        fail "Write permission denied for ${INSTALL_DIR}. Re-run with a writable FLOO_INSTALL_DIR."
    fi

    sudo mkdir -p "$INSTALL_DIR" || fail "Failed to create install directory ${INSTALL_DIR} with sudo."
    sudo mv "$source_path" "$destination_path"
    sudo chmod 755 "$destination_path"
}

cleanup() {
    if [[ -n "$TMP_DIR" && -d "$TMP_DIR" ]]; then
        rm -rf "$TMP_DIR"
    fi
}

main() {
    need_cmd curl
    need_cmd uname
    need_cmd mktemp
    need_cmd awk

    local target
    target="$(detect_target)"
    local asset="floo-${target}"

    log "Detected platform: ${target}"

    local binary_url
    binary_url="$(release_binary_url "$asset")"
    local checksum_url
    checksum_url="$(release_checksum_url "$binary_url")"

    TMP_DIR="$(mktemp -d)"
    trap cleanup EXIT

    local binary_path="${TMP_DIR}/${asset}"
    local checksum_path="${TMP_DIR}/${asset}.sha256"

    log "Downloading ${asset}..."
    curl -fsSL -o "$binary_path" "$binary_url" || fail "Failed to download binary from ${binary_url}"

    log "Downloading checksum..."
    curl -fsSL -o "$checksum_path" "$checksum_url" || fail "Failed to download checksum from ${checksum_url}"

    local expected_checksum
    expected_checksum="$(awk 'NF {print tolower($1); exit}' "$checksum_path")"
    [[ -n "$expected_checksum" ]] || fail "Checksum file is empty or invalid."

    local actual_checksum
    actual_checksum="$(sha256_file "$binary_path")"

    if [[ "$actual_checksum" != "$expected_checksum" ]]; then
        fail "Checksum mismatch for ${asset}. Expected ${expected_checksum}, got ${actual_checksum}."
    fi

    chmod +x "$binary_path"

    log "Installing ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}..."
    install_binary "$binary_path"

    local version_output
    version_output="$(${INSTALL_DIR}/${BINARY_NAME} --version 2>/dev/null || true)"
    [[ -n "$version_output" ]] || fail "Installation completed but '${INSTALL_DIR}/${BINARY_NAME} --version' failed."

    echo
    echo "Floo CLI installed successfully."
    echo "Version: ${version_output}"
    echo
    echo "Get started:"
    echo "  floo login"
    echo "  floo deploy"
    echo "  floo update"
}

main "$@"
