#!/usr/bin/env bash
# plainapp-cli installer
# Usage:  bash <(curl -fsSL https://raw.githubusercontent.com/plainhub/plainapp-cli/main/install.sh)
set -euo pipefail

REPO="plainhub/plainapp-cli"
BIN="plainapp-cli"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# ── detect OS and architecture ─────────────────────────────────────────────────

OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)  ASSET="${BIN}-macos-arm64" ;;
      x86_64) ASSET="${BIN}-macos-x86_64" ;;
      *) echo "Unsupported macOS arch: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64) ASSET="${BIN}-linux-x86_64" ;;
      *) echo "Unsupported Linux arch: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS  (Windows users: download from GitHub Releases)" >&2
    exit 1
    ;;
esac

# ── find the latest release ────────────────────────────────────────────

echo "Fetching latest release..."

if command -v curl &>/dev/null; then
  FETCH="curl -fsSL"
elif command -v wget &>/dev/null; then
  FETCH="wget -qO-"
else
  echo "curl or wget is required" >&2; exit 1
fi

TAG=$(
  $FETCH "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' \
  | sed 's/.*"tag_name": *"//' \
  | sed 's/".*//'
)

if [[ -z "$TAG" ]]; then
  echo "Could not find a release. Is there a published release?" >&2
  exit 1
fi

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
echo "Downloading ${ASSET} from ${TAG}..."

# ── download ───────────────────────────────────────────────────────────────────

TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT

$FETCH "$DOWNLOAD_URL" > "$TMP"
chmod +x "$TMP"

# ── install ────────────────────────────────────────────────────────────────────

if [[ -w "$INSTALL_DIR" ]]; then
  mv "$TMP" "${INSTALL_DIR}/${BIN}"
else
  echo "Installing to ${INSTALL_DIR} (sudo required)..."
  sudo mv "$TMP" "${INSTALL_DIR}/${BIN}"
fi

echo "Installed: $(which ${BIN}) — $(${BIN} --version)"
echo ""
echo "Next steps:"
echo "  ${BIN} --init          # create example config at ~/.config/plainapp-cli/config.toml"
echo "  ${BIN} --schema        # fetch GraphQL schema"
echo "  ${BIN} -e '{\"query\":\"{ app { battery } }\",\"variables\":null}'"
