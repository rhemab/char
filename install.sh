#!/bin/sh
set -e

# this script installs 'char' on mac and linux

REPO="rhemab/char"
BIN_NAME="char"
INSTALL_DIR="/usr/local/bin"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
  x86_64) ARCH="x86_64" ;;
  aarch64 | arm64) ARCH="aarch64" ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
  linux)  TARGET="${ARCH}-unknown-linux-gnu" ;;
  darwin) TARGET="${ARCH}-apple-darwin" ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

# Get latest release tag from GitHub
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

ARCHIVE="${BIN_NAME}-${TARGET}.tar.xz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"

echo "Downloading ${BIN_NAME} ${TAG} for ${TARGET}..."
curl -fsSL "$URL" -o "/tmp/${ARCHIVE}"

echo "Extracting..."
tar -xJf "/tmp/${ARCHIVE}" -C /tmp

echo "Installing to ${INSTALL_DIR}..."
sudo mv "/tmp/${BIN_NAME}" "${INSTALL_DIR}/"
sudo chmod +x "${INSTALL_DIR}/${BIN_NAME}"

rm "/tmp/${ARCHIVE}"

echo "Done! Run '${BIN_NAME}' to verify."
