#!/bin/sh
# Floo CLI installer
# Usage: curl -fsSL https://getfloo.com/install.sh | sh

set -e

REPO="getfloo/floo-cli"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="floo"

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Darwin) OS="apple-darwin" ;;
    Linux)  OS="unknown-linux-musl" ;;
    MINGW*|MSYS*|CYGWIN*)
        echo "Error: This install script does not support Windows."
        echo "Download floo.exe from https://github.com/${REPO}/releases"
        exit 1
        ;;
    *)
        echo "Error: Unsupported operating system: $OS"
        echo "Floo supports macOS, Linux, and Windows."
        exit 1
        ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    arm64|aarch64) ARCH="aarch64" ;;
    *)
        echo "Error: Unsupported architecture: $ARCH"
        echo "Floo supports x86_64 and arm64."
        exit 1
        ;;
esac

TARGET="${ARCH}-${OS}"
BINARY="floo-${TARGET}"

echo "Detected platform: ${TARGET}"

# Get latest release URL
LATEST_URL="https://api.github.com/repos/${REPO}/releases/latest"
DOWNLOAD_URL=$(curl -fsSL "$LATEST_URL" | grep "browser_download_url.*${BINARY}\"" | head -1 | cut -d '"' -f 4)

if [ -z "$DOWNLOAD_URL" ]; then
    echo "Error: Could not find a release for ${TARGET}"
    echo "Check https://github.com/${REPO}/releases for available binaries."
    exit 1
fi

CHECKSUM_URL="${DOWNLOAD_URL}.sha256"

echo "Downloading ${BINARY}..."
TMP_DIR=$(mktemp -d)
curl -fsSL -o "${TMP_DIR}/${BINARY}" "$DOWNLOAD_URL"

# Verify checksum if available
if curl -fsSL -o "${TMP_DIR}/${BINARY}.sha256" "$CHECKSUM_URL" 2>/dev/null; then
    echo "Verifying checksum..."
    cd "$TMP_DIR"
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -c "${BINARY}.sha256"
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum -c "${BINARY}.sha256"
    else
        echo "Warning: No checksum tool found, skipping verification."
    fi
    cd - >/dev/null
fi

# Install
echo "Installing to ${INSTALL_DIR}/${BINARY_NAME}..."
chmod +x "${TMP_DIR}/${BINARY}"

if [ -w "$INSTALL_DIR" ]; then
    mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY_NAME}"
else
    sudo mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY_NAME}"
fi

rm -rf "$TMP_DIR"

echo ""
echo "Floo CLI installed successfully!"
echo ""
echo "Get started:"
echo "  floo login          # Authenticate"
echo "  floo deploy         # Deploy your project"
echo "  floo --help         # See all commands"
echo ""
echo "Docs: https://docs.getfloo.com"
