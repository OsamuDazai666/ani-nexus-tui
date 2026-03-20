#!/usr/bin/env bash
# nexus-tui installer — Linux & macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/OsamuDazai666/nexus-tui/main/install.sh | bash

set -e

REPO="OsamuDazai666/nexus-tui"
BOLD="\033[1m"
RED="\033[31m"
GREEN="\033[32m"
YELLOW="\033[33m"
CYAN="\033[36m"
DIM="\033[2m"
RESET="\033[0m"

# ── Detect OS ─────────────────────────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux*)  PLATFORM="linux" ;;
  Darwin*) PLATFORM="macos" ;;
  *)       echo -e "${RED}Unsupported OS: $OS${RESET}"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH_TAG="x86_64" ;;
  aarch64|arm64) ARCH_TAG="aarch64" ;;
  *) echo -e "${RED}Unsupported arch: $ARCH${RESET}"; exit 1 ;;
esac

INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"

echo ""
echo -e "${CYAN}${BOLD}◆ nexus-tui installer${RESET}"
echo -e "${DIM}  platform: $PLATFORM/$ARCH_TAG${RESET}"
echo ""

# ── Package manager helpers ───────────────────────────────────────────────────

has() { command -v "$1" >/dev/null 2>&1; }

install_pkg() {
  local pkg="$1"
  if [ "$PLATFORM" = "macos" ]; then
    brew install "$pkg"
  elif has apt-get; then
    sudo apt-get install -y "$pkg"
  elif has pacman; then
    sudo pacman -S --noconfirm "$pkg"
  elif has dnf; then
    sudo dnf install -y "$pkg"
  elif has zypper; then
    sudo zypper install -y "$pkg"
  else
    echo -e "${YELLOW}⚠  Cannot auto-install $pkg — please install it manually${RESET}"
    return 1
  fi
}

# ── build_from_source — defined here so it's available when called below ──────

build_from_source() {
  if ! has cargo; then
    echo -e "${CYAN}Installing Rust...${RESET}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
  fi

  if [ "$PLATFORM" = "linux" ]; then
    has apt-get && sudo apt-get install -y build-essential pkg-config libssl-dev
  fi

  TMP=$(mktemp -d)
  echo -e "${CYAN}Cloning nexus-tui...${RESET}"
  git clone "https://github.com/${REPO}.git" "$TMP/nexus-tui"
  cd "$TMP/nexus-tui"
  cargo build --release
  cp target/release/nexus "$INSTALL_DIR/nexus"
  cd - && rm -rf "$TMP"
  echo -e "${GREEN}✓ Built and installed${RESET}"
}

# ── Homebrew (macOS) ──────────────────────────────────────────────────────────

if [ "$PLATFORM" = "macos" ] && ! has brew; then
  echo -e "${CYAN}Installing Homebrew...${RESET}"
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
fi

# ── Kitty terminal ────────────────────────────────────────────────────────────

install_kitty() {
  echo -e "${CYAN}Installing Kitty terminal...${RESET}"
  if [ "$PLATFORM" = "macos" ]; then
    brew install --cask kitty
  else
    curl -L https://sw.kovidgoyal.net/kitty/installer.sh | sh /dev/stdin
    # Symlink to PATH
    ln -sf "$HOME/.local/kitty.app/bin/kitty" "$INSTALL_DIR/kitty" 2>/dev/null || true
    ln -sf "$HOME/.local/kitty.app/bin/kitten" "$INSTALL_DIR/kitten" 2>/dev/null || true
    # Desktop entry
    cp "$HOME/.local/kitty.app/share/applications/kitty.desktop" \
       "$HOME/.local/share/applications/" 2>/dev/null || true
    sed -i "s|Icon=kitty|Icon=$HOME/.local/kitty.app/share/icons/hicolor/256x256/apps/kitty.png|g" \
       "$HOME/.local/share/applications/kitty.desktop" 2>/dev/null || true
  fi
  echo -e "${GREEN}✓ Kitty installed${RESET}"
}

if has kitty; then
  echo -e "${GREEN}✓ Kitty $(kitty --version | head -1)${RESET}"
else
  echo -e "${YELLOW}Kitty not found (recommended for best image quality)${RESET}"
  read -rp "  Install Kitty? [Y/n] " ans
  case "$ans" in [Nn]*) ;; *) install_kitty ;; esac
fi

# ── mpv ───────────────────────────────────────────────────────────────────────

if has mpv; then
  echo -e "${GREEN}✓ mpv $(mpv --version | head -1 | cut -d' ' -f1-2)${RESET}"
else
  echo -e "${CYAN}Installing mpv...${RESET}"
  install_pkg mpv
fi

# ── yt-dlp ────────────────────────────────────────────────────────────────────

if has yt-dlp; then
  echo -e "${GREEN}✓ yt-dlp $(yt-dlp --version)${RESET}"
else
  echo -e "${CYAN}Installing yt-dlp...${RESET}"
  if [ "$PLATFORM" = "macos" ]; then
    brew install yt-dlp
  else
    sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp \
      -o /usr/local/bin/yt-dlp
    sudo chmod +x /usr/local/bin/yt-dlp
  fi
fi

# ── TMDB API key ──────────────────────────────────────────────────────────────

if [ -z "$TMDB_API_KEY" ]; then
  echo ""
  echo -e "${YELLOW}TMDB API key not set (needed for Movies & TV)${RESET}"
  echo -e "${DIM}  Get a free key at: https://www.themoviedb.org/settings/api${RESET}"
  read -rp "  Enter TMDB API key (Enter to skip): " KEY
  if [ -n "$KEY" ]; then
    export TMDB_API_KEY="$KEY"
    # Detect shell rc file
    SHELL_RC=""
    [ -f "$HOME/.zshrc" ]  && SHELL_RC="$HOME/.zshrc"
    [ -f "$HOME/.bashrc" ] && SHELL_RC="$HOME/.bashrc"
    [ -f "$HOME/.profile" ] && [ -z "$SHELL_RC" ] && SHELL_RC="$HOME/.profile"
    if [ -n "$SHELL_RC" ]; then
      # Remove old entry if present
      grep -v "TMDB_API_KEY" "$SHELL_RC" > "${SHELL_RC}.tmp" && mv "${SHELL_RC}.tmp" "$SHELL_RC"
      echo "export TMDB_API_KEY=\"$KEY\"" >> "$SHELL_RC"
      echo -e "${GREEN}✓ Saved to $SHELL_RC${RESET}"
    fi
  fi
fi

# ── Download nexus binary ─────────────────────────────────────────────────────

echo ""
echo -e "${CYAN}Downloading nexus-tui...${RESET}"

BINARY_URL="https://github.com/${REPO}/releases/latest/download/nexus-${PLATFORM}-${ARCH_TAG}"

if curl -fsSL "$BINARY_URL" -o "$INSTALL_DIR/nexus"; then
  chmod +x "$INSTALL_DIR/nexus"
  echo -e "${GREEN}✓ nexus installed to $INSTALL_DIR/nexus${RESET}"
else
  # Binary not available — build from source
  echo -e "${YELLOW}Pre-built binary not found — building from source...${RESET}"
  build_from_source
fi

# ── PATH check ────────────────────────────────────────────────────────────────

if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
  echo ""
  echo -e "${YELLOW}Add this to your shell profile to use 'nexus' from anywhere:${RESET}"
  echo -e "  ${BOLD}export PATH=\"\$HOME/.local/bin:\$PATH\"${RESET}"
fi

echo ""
echo -e "${GREEN}${BOLD}Done! Run nexus in your terminal.${RESET}"
echo -e "${DIM}  Tip: run inside Kitty for best image quality${RESET}"
echo ""