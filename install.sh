#!/usr/bin/env bash
# ────────────────────────────────────────────────────────────────────────────
# nexus-tui installer — Linux / macOS
# ────────────────────────────────────────────────────────────────────────────
set -euo pipefail

# ── Colors & symbols ─────────────────────────────────────────────────────────
RED='\033[0;31m';  GREEN='\033[0;32m';  YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m';      DIM='\033[2m';  RESET='\033[0m'
CHECK="${GREEN}✓${RESET}"; CROSS="${RED}✗${RESET}"; ARROW="${CYAN}▶${RESET}"
DIAMOND="${YELLOW}◆${RESET}"

INSTALL_DIR="${HOME}/.local/share/nexus-tui"
BIN_DIR="${HOME}/.local/bin"
REPO_URL="https://github.com/OsamuDazai666/nexus-tui.git"

# ── Helpers ───────────────────────────────────────────────────────────────────
header() {
  echo ""
  echo -e "  ${DIAMOND} ${BOLD}NEXUS-TUI INSTALLER${RESET}"
  echo -e "  ${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
  echo ""
}

step() { echo -e "  ${ARROW} ${BOLD}$1${RESET}"; }
ok()   { echo -e "    ${CHECK} $1"; }
fail() { echo -e "    ${CROSS} $1"; exit 1; }
info() { echo -e "    ${DIM}$1${RESET}"; }
warn() { echo -e "    ${YELLOW}⚠${RESET}  $1"; }

ask() {
  echo -en "  ${CYAN}?${RESET} $1 ${DIM}[Y/n]${RESET} "
  read -r ans
  [[ -z "$ans" || "$ans" =~ ^[Yy] ]]
}

spinner() {
  local pid=$1 msg=$2
  local frames=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')
  local i=0
  while kill -0 "$pid" 2>/dev/null; do
    printf "\r    ${CYAN}%s${RESET}  %s" "${frames[$((i % 10))]}" "$msg"
    sleep 0.1
    ((i++))
  done
  printf "\r"
}

check_cmd() {
  if command -v "$1" &>/dev/null; then
    ok "$1 $(${2:-$1 --version 2>&1 | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1})"
    return 0
  fi
  return 1
}

# ── Main ──────────────────────────────────────────────────────────────────────
clear
header

# ── Detect existing install ───────────────────────────────────────────────────
if [[ -d "$INSTALL_DIR/.git" ]]; then
  echo -e "  ${YELLOW}Existing install found at ${INSTALL_DIR}${RESET}"
  echo ""

  step "Checking for updates"
  cd "$INSTALL_DIR"
  git fetch origin --quiet 2>/dev/null || true
  LOCAL=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
  REMOTE=$(git rev-parse origin/main 2>/dev/null || echo "unknown")

  if [[ "$LOCAL" == "$REMOTE" ]]; then
    ok "Already up to date"
    echo ""
    info "Binary: ${BIN_DIR}/nexus"
    echo ""
    echo -e "  ${DIAMOND} Nothing to do. Run ${BOLD}nexus${RESET} to launch."
    echo ""
    exit 0
  fi

  echo ""
  COMMITS=$(git log --oneline "${LOCAL}..${REMOTE}" 2>/dev/null | wc -l | tr -d ' ')
  info "${COMMITS} new commit(s) available"
  echo ""
  if ! ask "Update nexus-tui?"; then
    echo -e "\n  ${DIM}Skipped. Run ${BOLD}nexus${RESET} to launch.${RESET}\n"
    exit 0
  fi

  step "Pulling latest"
  git pull origin main --quiet &
  spinner $! "Pulling changes…"
  ok "Updated to $(git rev-parse --short HEAD)"
  SKIP_CLONE=true
else
  SKIP_CLONE=false
  echo -e "  ${DIM}Install directory: ${INSTALL_DIR}${RESET}"
  echo ""
  if ! ask "Install nexus-tui?"; then
    echo -e "\n  ${DIM}Cancelled.${RESET}\n"; exit 0
  fi
fi

echo ""

# ── Check dependencies ────────────────────────────────────────────────────────
step "Checking dependencies"

HAS_GIT=false; HAS_RUST=false; HAS_MPV=false; HAS_CURL=false

check_cmd git  && HAS_GIT=true  || true
check_cmd curl && HAS_CURL=true || true
check_cmd mpv  && HAS_MPV=true  || warn "mpv not found — install it to play anime"

if command -v rustc &>/dev/null; then
  RUSTV=$(rustc --version 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
  ok "rust ${RUSTV}"
  HAS_RUST=true
fi

if [[ "$HAS_GIT" == false ]]; then
  fail "git is required. Install it and re-run this script."
fi

# ── Install Rust if missing ───────────────────────────────────────────────────
if [[ "$HAS_RUST" == false ]]; then
  echo ""
  step "Installing Rust via rustup"
  if [[ "$HAS_CURL" == false ]]; then
    fail "curl is required to install Rust. Please install curl first."
  fi
  if ask "Install Rust (rustup)?"; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet &
    spinner $! "Installing Rust…"
    source "${HOME}/.cargo/env" 2>/dev/null || true
    ok "Rust installed"
    HAS_RUST=true
  else
    fail "Rust is required to build nexus-tui."
  fi
fi

echo ""

# ── Clone repo ────────────────────────────────────────────────────────────────
if [[ "$SKIP_CLONE" == false ]]; then
  step "Cloning repository"
  mkdir -p "$(dirname "$INSTALL_DIR")"
  if [[ -d "$INSTALL_DIR" ]]; then
    rm -rf "$INSTALL_DIR"
  fi
  git clone --quiet "$REPO_URL" "$INSTALL_DIR" &
  spinner $! "Cloning nexus-tui…"
  ok "Cloned to ${INSTALL_DIR}"
  echo ""
fi

# ── Build ─────────────────────────────────────────────────────────────────────
step "Building nexus-tui"
info "This takes 1–3 minutes on first build"
echo ""

cd "$INSTALL_DIR"
START_TS=$(date +%s)

CARGO_INCREMENTAL=0 cargo build --release --quiet 2>&1 &
BUILD_PID=$!
spinner $BUILD_PID "Compiling…"
wait $BUILD_PID
BUILD_EXIT=$?

END_TS=$(date +%s)
ELAPSED=$((END_TS - START_TS))

if [[ $BUILD_EXIT -ne 0 ]]; then
  fail "Build failed. Run 'cargo build --release' in ${INSTALL_DIR} to see errors."
fi
ok "Built in ${ELAPSED}s"
echo ""

# ── Install binary ────────────────────────────────────────────────────────────
step "Installing binary"
mkdir -p "$BIN_DIR"
cp "${INSTALL_DIR}/target/release/nexus" "${BIN_DIR}/nexus"
chmod +x "${BIN_DIR}/nexus"
ok "Installed to ${BIN_DIR}/nexus"

# ── PATH check ────────────────────────────────────────────────────────────────
if [[ ":$PATH:" != *":${BIN_DIR}:"* ]]; then
  echo ""
  warn "${BIN_DIR} is not in your PATH"
  info "Add this to your shell config (~/.bashrc, ~/.zshrc, etc.):"
  echo ""
  echo -e "    ${DIM}export PATH=\"\$HOME/.local/bin:\$PATH\"${RESET}"
fi

# ── Done ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "  ${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "  ${DIAMOND} ${BOLD}Done!${RESET}  Run ${CYAN}${BOLD}nexus${RESET} to launch"
echo ""