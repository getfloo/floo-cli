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

run_installer_expect_success() {
    local name="$1"
    local mock_os="$2"
    local mock_arch="$3"
    local binary_url="$4"
    local checksum_url="$5"

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
    local expected_text="$6"

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
    local fixture_dir
    fixture_dir="$(mktemp -d)"

    local asset="${fixture_dir}/floo-x86_64-unknown-linux-musl"
    cat >"$asset" <<'SH'
#!/usr/bin/env bash
echo "floo test-0.0.0"
SH
    chmod +x "$asset"

    local checksum
    checksum="$(sha256_file "$asset")"
    local checksum_file="${asset}.sha256"
    echo "${checksum}  $(basename "$asset")" >"$checksum_file"

    local bad_checksum_file="${asset}.bad.sha256"
    echo "0000000000000000000000000000000000000000000000000000000000000000  $(basename "$asset")" >"$bad_checksum_file"

    run_installer_expect_success \
        "linux x86_64 success" \
        "Linux" \
        "x86_64" \
        "file://${asset}" \
        "file://${checksum_file}"

    run_installer_expect_failure \
        "unsupported os" \
        "FreeBSD" \
        "x86_64" \
        "file://${asset}" \
        "file://${checksum_file}" \
        "Unsupported operating system"

    run_installer_expect_failure \
        "unsupported arch" \
        "Linux" \
        "mips64" \
        "file://${asset}" \
        "file://${checksum_file}" \
        "Unsupported architecture"

    run_installer_expect_failure \
        "checksum mismatch" \
        "Linux" \
        "x86_64" \
        "file://${asset}" \
        "file://${bad_checksum_file}" \
        "Checksum mismatch"

    rm -rf "$fixture_dir"
    pass "install_script_test suite"
}

main "$@"
