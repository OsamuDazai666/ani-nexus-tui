#!/usr/bin/env bash
set -euo pipefail

REPO="OsamuDazai666/nexus-tui"
INSTALLER_URL="https://github.com/${REPO}/releases/latest/download/ani-nexus-tui-installer.sh"

tmp_file="$(mktemp)"
cleanup() {
    rm -f "$tmp_file"
}
trap cleanup EXIT

echo "Fetching latest ani-nexus installer from GitHub Releases..."
curl --proto '=https' --tlsv1.2 -LsSf "$INSTALLER_URL" -o "$tmp_file"
sh "$tmp_file" "$@"
