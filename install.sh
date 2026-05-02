#!/usr/bin/env bash
#
# cc-flytrap installer
# Usage: curl -fsSL https://raw.githubusercontent.com/adiled/cc-flytrap/main/install.sh | bash
# Or: curl -fsSL https://raw.githubusercontent.com/adiled/cc-flytrap/main/install.sh | INSTALL_DIR=~/.local/share/ccft SKIP_INSTALL=1 bash

set -e
REPO="adiled/cc-flytrap"
DIR="${INSTALL_DIR:-$HOME/.local/share/ccft}"
SKIP="${SKIP_INSTALL:-0}"

mkdir -p "$DIR"
echo "Downloading cc-flytrap..."

TAG=$(curl -sS "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
curl -sSL "https://github.com/$REPO/archive/refs/tags/$TAG.tar.gz" | tar -xz -C "$DIR" --strip-components=1

if [ "$SKIP" = "1" ]; then
    echo "Downloaded to $DIR. Run '$DIR/bin/ccft install' to install service."
else
    "$DIR/bin/ccft" install
fi