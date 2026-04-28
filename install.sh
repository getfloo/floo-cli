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

release_signature_url() {
    local binary_url="$1"

    if [[ -n "${FLOO_INSTALL_SIGNATURE_URL:-}" ]]; then
        echo "${FLOO_INSTALL_SIGNATURE_URL}"
    else
        echo "${binary_url}.sig"
    fi
}

write_release_public_key() {
    local destination="$1"

    if [[ -n "${FLOO_INSTALL_VERIFY_KEY_PATH:-}" ]]; then
        cp "${FLOO_INSTALL_VERIFY_KEY_PATH}" "$destination" || fail "Failed to read release verification public key."
        return
    fi

    if [[ -n "${FLOO_INSTALL_VERIFY_KEY_PEM:-}" ]]; then
        printf '%s\n' "${FLOO_INSTALL_VERIFY_KEY_PEM}" >"$destination"
        return
    fi

    cat >"$destination" <<'PEM'
-----BEGIN RSA PUBLIC KEY-----
MIIBigKCAYEAoxTcA648/UUTEcmZbiZbwGsQJIjI/CwEda/3Zwky26hOdu3ccQKD
U3lXj7c/cAvr0Y+ISnf23YvBr68q0kI0IhihYE74MoOKe0QjRv7aK0cYgIWKj5SZ
xcw0CLvMm36rNG7iZBHJb3Jbew5ebMpaRyCZBnruHQocHQammzUkuDjeJ753ZFmu
Y8Fyr/CLO+F2V7Bou/qh4DA0tJ8Ams4HLTUGAfXHgj3Q5L9DIZC6iDzGqg70DblC
wNrr/n+zx6TCjonKraYxDUXruR6Za6XrKSbTrq6Bh1DFYK5DM3m9OIdiMx2EC+yD
3iY/CZRC/auqq4CQeXLQyxTsExxnvG3O4Ci77MTZH4NSnngkkw5KrcvqCC9KVI9J
IViei4zB3GoTGDm9+FC02cCozhKiTvAqzdb+ieszMNsavQNdOy1qO9bQfObWWvay
Z4rrRM3hE+rKyk5WHrPZcR77YiqZ6cwXVl7g8gJ0JIQi2a8oHzmjQc7n+j1Nmglh
Wk6BmNyJThezAgMBAAE=
-----END RSA PUBLIC KEY-----
PEM
}

verify_release_signature() {
    local binary_path="$1"
    local signature_path="$2"
    local public_key_path="$3"
    local asset="$4"

    if ! openssl dgst -sha256 -verify "$public_key_path" -signature "$signature_path" "$binary_path" >/dev/null 2>&1; then
        fail "Signature verification failed for ${asset}. Do not run this binary."
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
    need_cmd openssl

    log "Detected platform: ${target}"

    local binary_url
    binary_url="$(release_binary_url "$asset")"
    local checksum_url
    checksum_url="$(release_checksum_url "$binary_url")"
    local signature_url
    signature_url="$(release_signature_url "$binary_url")"

    TMP_DIR="$(mktemp -d)"
    trap cleanup EXIT

    local binary_path="${TMP_DIR}/${asset}"
    local checksum_path="${TMP_DIR}/${asset}.sha256"
    local signature_path="${TMP_DIR}/${asset}.sig"
    local public_key_path="${TMP_DIR}/release-public-key.pem"

    log "Downloading ${asset}..."
    curl -fsSL -o "$binary_path" "$binary_url" || fail "Failed to download binary from ${binary_url}"

    log "Downloading checksum..."
    curl -fsSL -o "$checksum_path" "$checksum_url" || fail "Failed to download checksum from ${checksum_url}"

    log "Downloading signature..."
    curl -fsSL -o "$signature_path" "$signature_url" || fail "Failed to download signature from ${signature_url}"

    local expected_checksum
    expected_checksum="$(awk 'NF {print tolower($1); exit}' "$checksum_path")"
    [[ -n "$expected_checksum" ]] || fail "Checksum file is empty or invalid."

    local actual_checksum
    actual_checksum="$(sha256_file "$binary_path")"

    if [[ "$actual_checksum" != "$expected_checksum" ]]; then
        fail "Checksum mismatch for ${asset}. Expected ${expected_checksum}, got ${actual_checksum}."
    fi

    write_release_public_key "$public_key_path"
    verify_release_signature "$binary_path" "$signature_path" "$public_key_path" "$asset"

    chmod +x "$binary_path"

    log "Installing ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}..."
    install_binary "$binary_path"

    # Verify the installed binary. Previously this was:
    #   version_output="$(floo --version 2>/dev/null || true)"
    #   [[ -n "$version_output" ]] || fail ...
    # which swallowed BOTH the exit code (|| true) and stderr (2>/dev/null),
    # and only checked "is stdout non-empty" — meaning a binary that
    # panicked, wrote garbage to stdout, and exited 1 would still pass.
    #
    # v2026.04.12.1 also shipped with `floo --version` writing its output
    # only to stderr (human status line), not stdout, so the old
    # non-empty-stdout check failed for every fresh install. That's what
    # forced the rewrite.
    #
    # The stricter contract:
    #   1. Run with FLOO_NO_UPDATE_CHECK=1 so the verification doesn't
    #      hit the network and doesn't try to auto-update mid-install.
    #   2. Capture stdout and stderr to separate files so we can report
    #      either one on failure.
    #   3. Require exit code 0.
    #   4. Require stdout to contain a line that matches the floo tag
    #      format (calver `2026.04.12`, `2026.04.12.1`, or the dev tag
    #      `0.0.0-dev`). Catches both "empty stdout" AND "stdout has
    #      unrelated text but no version tag."
    local version_stdout_file="${TMP_DIR}/version.stdout"
    local version_stderr_file="${TMP_DIR}/version.stderr"
    local version_exit=0
    FLOO_NO_UPDATE_CHECK=1 "${INSTALL_DIR}/${BINARY_NAME}" --version \
        >"$version_stdout_file" 2>"$version_stderr_file" || version_exit=$?

    if [[ "$version_exit" -ne 0 ]]; then
        log "stdout: $(cat "$version_stdout_file" 2>/dev/null || true)"
        log "stderr: $(cat "$version_stderr_file" 2>/dev/null || true)"
        fail "Installation completed but '${INSTALL_DIR}/${BINARY_NAME} --version' exited with ${version_exit}."
    fi

    # Regex covers:
    #   - Calver releases: `2026.04.12`, `2026.04.12.1`, ...
    #   - Calver with SemVer-style suffixes: `2026.04.12-rc1`,
    #     `2026.04.12+build.7`, `2026.04.12-3-gabc1234`. Not used today
    #     but cheap to allow — the alternative is that a future
    #     pre-release channel silently breaks every install.
    #   - The dev tag: `0.0.0-dev`
    # LC_ALL=C ensures grep's ASCII character classes don't misbehave
    # under a user's broken locale (e.g. LANG=garbage).
    local version_output
    version_output="$(LC_ALL=C grep -oE '^([0-9]{4}\.[0-9]{2}\.[0-9]{2}(\.[0-9]+)?([-+][0-9A-Za-z.-]+)?|0\.0\.0-dev)$' "$version_stdout_file" | head -n1 || true)"
    if [[ -z "$version_output" ]]; then
        log "stdout: $(cat "$version_stdout_file" 2>/dev/null || true)"
        log "stderr: $(cat "$version_stderr_file" 2>/dev/null || true)"
        fail "Installation completed but '${INSTALL_DIR}/${BINARY_NAME} --version' did not print a recognizable version tag on stdout. Expected a calver tag like '2026.04.12' or the dev tag '0.0.0-dev'."
    fi

    echo
    echo "floo installed successfully (${version_output})"
    echo
    echo "Get started:"
    echo "  floo auth login                        log in or create an account"
    echo "  floo docs                              see how floo works"
    echo "  floo --help                            explore all commands"
    echo
    echo "Agent integration:"
    echo "  floo skills install --path <dir>       install agent skill to a directory"
    echo "  floo skills install --print            print skill to stdout"
}

main "$@"
