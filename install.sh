#!/usr/bin/env bash
#
# cc-flytrap installer - run via: curl -fsSL https://raw.githubusercontent.com/adiled/cc-flytrap/main/install.sh | bash

set -e
REPO="adiled/cc-flytrap"
DIR="${INSTALL_DIR:-$HOME/.local/share/ccft}"

echo "Installing cc-flytrap to $DIR..."
mkdir -p "$DIR"

TAG=$(curl -sS "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
curl -sSL "https://github.com/$REPO/archive/refs/tags/$TAG.tar.gz" | tar -xz -C "$DIR" --strip-components=1

"$DIR/bin/ccft" install
echo "Done."