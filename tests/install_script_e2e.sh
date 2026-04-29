#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
INSTALLER="${REPO_ROOT}/install.sh"
TMP_DIR=""

fail() {
    echo "[fail] $*" >&2
    exit 1
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
    fail "No SHA256 tool available for tests"
}

detect_target() {
    local os
    os="$(uname -s)"
    local arch
    arch="$(uname -m)"

    case "$os" in
        Darwin) os="apple-darwin" ;;
        Linux) os="unknown-linux-musl" ;;
        *) fail "Unsupported OS for e2e test: ${os}" ;;
    esac

    case "$arch" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *) fail "Unsupported architecture for e2e test: ${arch}" ;;
    esac

    echo "${arch}-${os}"
}

cleanup() {
    if [[ -n "$TMP_DIR" && -d "$TMP_DIR" ]]; then
        rm -rf "$TMP_DIR"
    fi
}

main() {
    cd "$REPO_ROOT"
    command -v openssl >/dev/null 2>&1 || fail "openssl is required for install script e2e signature verification"

    local target
    target="$(detect_target)"
    local asset_name="floo-${target}"
    local binary_source="${FLOO_E2E_BINARY:-${REPO_ROOT}/target/debug/floo}"

    if [[ ! -x "$binary_source" ]]; then
        cargo build >/dev/null
    fi

    [[ -x "$binary_source" ]] || fail "Could not find executable floo binary for e2e test"

    TMP_DIR="$(mktemp -d)"
    trap cleanup EXIT

    local asset_path="${TMP_DIR}/${asset_name}"
    local checksum_path="${asset_path}.sha256"
    local signature_path="${asset_path}.sig"
    local signing_key_path="${TMP_DIR}/signing-private.pem"
    local verify_key_path="${TMP_DIR}/signing-public.pem"
    local install_dir="${TMP_DIR}/install"

    cp "$binary_source" "$asset_path"
    chmod +x "$asset_path"

    local checksum
    checksum="$(sha256_file "$asset_path")"
    echo "${checksum}  ${asset_name}" >"$checksum_path"
    openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$signing_key_path" >/dev/null 2>&1
    openssl rsa -RSAPublicKey_out -in "$signing_key_path" -out "$verify_key_path" >/dev/null 2>&1
    openssl dgst -sha256 -sign "$signing_key_path" -out "$signature_path" "$asset_path"

    FLOO_INSTALL_BINARY_URL="file://${asset_path}" \
    FLOO_INSTALL_CHECKSUM_URL="file://${checksum_path}" \
    FLOO_INSTALL_SIGNATURE_URL="file://${signature_path}" \
    FLOO_INSTALL_VERIFY_KEY_PATH="$verify_key_path" \
    FLOO_INSTALL_DIR="$install_dir" \
    bash "$INSTALLER" >/dev/null

    [[ -x "${install_dir}/floo" ]] || fail "Installed binary missing"

    "${install_dir}/floo" --help | grep -q "Manage and observe web apps. Deploys are git-driven." \
        || fail "Installed binary failed help output check"

    echo "[pass] install_script_e2e"
}

main "$@"
