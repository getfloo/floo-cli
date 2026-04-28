#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
INSTALLER="${REPO_ROOT}/install.sh"

pass() {
    echo "[pass] $*"
}

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

make_mock_uname() {
    local dir="$1"
    mkdir -p "$dir"
    cat >"${dir}/uname" <<'SH'
#!/usr/bin/env bash
case "${1:-}" in
    -s) echo "${MOCK_UNAME_S:-Linux}" ;;
    -m) echo "${MOCK_UNAME_M:-x86_64}" ;;
    *) /usr/bin/uname "$@" ;;
esac
SH
    chmod +x "${dir}/uname"
}

make_signing_keypair() {
    local dir="$1"
    openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "${dir}/signing-private.pem" >/dev/null 2>&1
    openssl rsa -pubout -in "${dir}/signing-private.pem" -out "${dir}/signing-public.pem" >/dev/null 2>&1
}

sign_asset() {
    local fixture_dir="$1"
    local asset="$2"
    local signature="${asset}.sig"
    openssl dgst -sha256 -sign "${fixture_dir}/signing-private.pem" -out "$signature" "$asset"
    echo "$signature"
}

run_installer_expect_success() {
    local name="$1"
    local mock_os="$2"
    local mock_arch="$3"
    local binary_url="$4"
    local checksum_url="$5"
    local signature_url="$6"
    local verify_key_path="$7"

    local tmp
    tmp="$(mktemp -d)"
    local install_dir="${tmp}/bin"
    local mock_dir="${tmp}/mockbin"
    local log_path="${tmp}/run.log"

    make_mock_uname "$mock_dir"

    if ! env \
        MOCK_UNAME_S="$mock_os" \
        MOCK_UNAME_M="$mock_arch" \
        PATH="${mock_dir}:${PATH}" \
        FLOO_INSTALL_BINARY_URL="$binary_url" \
        FLOO_INSTALL_CHECKSUM_URL="$checksum_url" \
        FLOO_INSTALL_SIGNATURE_URL="$signature_url" \
        FLOO_INSTALL_VERIFY_KEY_PATH="$verify_key_path" \
        FLOO_INSTALL_DIR="$install_dir" \
        bash "$INSTALLER" >"$log_path" 2>&1; then
        cat "$log_path" >&2
        fail "${name}: installer failed unexpectedly"
    fi

    [[ -x "${install_dir}/floo" ]] || fail "${name}: installed binary missing"
    "${install_dir}/floo" --version >/dev/null 2>&1 || fail "${name}: installed binary not executable"

    pass "${name}"
    rm -rf "$tmp"
}

run_installer_expect_failure() {
    local name="$1"
    local mock_os="$2"
    local mock_arch="$3"
    local binary_url="$4"
    local checksum_url="$5"
    local signature_url="$6"
    local verify_key_path="$7"
    local expected_text="$8"

    local tmp
    tmp="$(mktemp -d)"
    local install_dir="${tmp}/bin"
    local mock_dir="${tmp}/mockbin"
    local log_path="${tmp}/run.log"

    make_mock_uname "$mock_dir"

    if env \
        MOCK_UNAME_S="$mock_os" \
        MOCK_UNAME_M="$mock_arch" \
        PATH="${mock_dir}:${PATH}" \
        FLOO_INSTALL_BINARY_URL="$binary_url" \
        FLOO_INSTALL_CHECKSUM_URL="$checksum_url" \
        FLOO_INSTALL_SIGNATURE_URL="$signature_url" \
        FLOO_INSTALL_VERIFY_KEY_PATH="$verify_key_path" \
        FLOO_INSTALL_DIR="$install_dir" \
        bash "$INSTALLER" >"$log_path" 2>&1; then
        cat "$log_path" >&2
        fail "${name}: installer succeeded unexpectedly"
    fi

    grep -q "$expected_text" "$log_path" || {
        cat "$log_path" >&2
        fail "${name}: expected error text not found: ${expected_text}"
    }

    pass "${name}"
    rm -rf "$tmp"
}

main() {
    command -v openssl >/dev/null 2>&1 || fail "openssl is required for install script signature tests"

    local fixture_dir
    fixture_dir="$(mktemp -d)"
    make_signing_keypair "$fixture_dir"
    local verify_key_path="${fixture_dir}/signing-public.pem"

    # Mock binary output must match the stricter version regex in install.sh
    # (calver like 2026.04.12[.N] or the dev tag 0.0.0-dev). The real floo
    # binary prints the bare tag on stdout — `0.0.0-dev` for unreleased
    # builds, `YYYY.MM.DD[.N]` for releases — so the mock uses the dev
    # format to exercise exactly that code path.
    local asset="${fixture_dir}/floo-x86_64-unknown-linux-musl"
    cat >"$asset" <<'SH'
#!/usr/bin/env bash
echo "0.0.0-dev"
SH
    chmod +x "$asset"

    local checksum
    checksum="$(sha256_file "$asset")"
    local checksum_file="${asset}.sha256"
    echo "${checksum}  $(basename "$asset")" >"$checksum_file"
    local signature_file
    signature_file="$(sign_asset "$fixture_dir" "$asset")"

    local bad_checksum_file="${asset}.bad.sha256"
    echo "0000000000000000000000000000000000000000000000000000000000000000  $(basename "$asset")" >"$bad_checksum_file"
    local bad_signature_file="${asset}.bad.sig"
    printf 'not-a-valid-signature' >"$bad_signature_file"

    run_installer_expect_success \
        "linux x86_64 success" \
        "Linux" \
        "x86_64" \
        "file://${asset}" \
        "file://${checksum_file}" \
        "file://${signature_file}" \
        "$verify_key_path"

    run_installer_expect_failure \
        "unsupported os" \
        "FreeBSD" \
        "x86_64" \
        "file://${asset}" \
        "file://${checksum_file}" \
        "file://${signature_file}" \
        "$verify_key_path" \
        "Unsupported operating system"

    run_installer_expect_failure \
        "unsupported arch" \
        "Linux" \
        "mips64" \
        "file://${asset}" \
        "file://${checksum_file}" \
        "file://${signature_file}" \
        "$verify_key_path" \
        "Unsupported architecture"

    run_installer_expect_failure \
        "checksum mismatch" \
        "Linux" \
        "x86_64" \
        "file://${asset}" \
        "file://${bad_checksum_file}" \
        "file://${signature_file}" \
        "$verify_key_path" \
        "Checksum mismatch"

    run_installer_expect_failure \
        "signature mismatch" \
        "Linux" \
        "x86_64" \
        "file://${asset}" \
        "file://${checksum_file}" \
        "file://${bad_signature_file}" \
        "$verify_key_path" \
        "Signature verification failed"

    # Regression guard for the v2026.04.12.1 install failure: a binary
    # that exits 0 but prints no recognizable version tag on stdout must
    # be rejected by install.sh, not silently accepted. Uses a mock that
    # emits only stderr (mirroring the exact bug) plus a correct checksum.
    local stderr_only_asset="${fixture_dir}/floo-x86_64-unknown-linux-musl.stderr-only"
    cat >"$stderr_only_asset" <<'SH'
#!/usr/bin/env bash
# Emits the version to stderr instead of stdout — the exact v2026.04.12.1
# regression. install.sh must catch this and fail, not accept it.
echo "✓ floo 0.0.0-dev" >&2
SH
    chmod +x "$stderr_only_asset"
    local stderr_only_checksum
    stderr_only_checksum="$(sha256_file "$stderr_only_asset")"
    local stderr_only_checksum_file="${stderr_only_asset}.sha256"
    echo "${stderr_only_checksum}  $(basename "$stderr_only_asset")" >"$stderr_only_checksum_file"
    local stderr_only_signature_file
    stderr_only_signature_file="$(sign_asset "$fixture_dir" "$stderr_only_asset")"

    run_installer_expect_failure \
        "stderr-only --version output rejected" \
        "Linux" \
        "x86_64" \
        "file://${stderr_only_asset}" \
        "file://${stderr_only_checksum_file}" \
        "file://${stderr_only_signature_file}" \
        "$verify_key_path" \
        "did not print a recognizable version tag"

    rm -rf "$fixture_dir"
    pass "install_script_test suite"
}

main "$@"
