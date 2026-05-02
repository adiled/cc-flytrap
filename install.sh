#!/usr/bin/env bash
#
# cc-flytrap installer
# Usage: curl -fsSL https://raw.githubusercontent.com/adiled/cc-flytrap/main/install.sh | bash

set -e
REPO="adiled/cc-flytrap"
DIR="$HOME/.local/share/ccft"
SKIP="${SKIP_INSTALL:-0}"

mkdir -p "$DIR"
echo "Installing cc-flytrap to $DIR..."

TAG=$(curl -sS "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
curl -sSL "https://github.com/$REPO/archive/refs/tags/$TAG.tar.gz" | tar -xz -C "$DIR" --strip-components=1

if [ "$SKIP" = "1" ]; then
    echo "Downloaded. Run '$DIR/bin/ccft install' to configure."
else
    "$DIR/bin/ccft" install
fi