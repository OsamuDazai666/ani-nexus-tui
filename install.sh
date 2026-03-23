#!/usr/bin/env bash
# ────────────────────────────────────────────────────────────────────────────
# nexus-tui installer — Linux / macOS
# Usage: bash <(curl -sSf https://raw.githubusercontent.com/OsamuDazai666/nexus-tui/main/install.sh)
# ────────────────────────────────────────────────────────────────────────────
set -euo pipefail

REPO_URL="https://github.com/OsamuDazai666/nexus-tui.git"
INSTALL_DIR="${HOME}/.local/share/nexus-tui"
BIN_DIR="${HOME}/.local/bin"
BINARY="${INSTALL_DIR}/target/release/nexus"

# ── Colors ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; DIM='\033[2m'; RESET='\033[0m'

# ── Helpers ───────────────────────────────────────────────────────────────────
println()  { echo -e "$*"; }
header()   { println "\n  ${YELLOW}◆${RESET} ${BOLD}NEXUS-TUI INSTALLER${RESET}\n  ${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}\n"; }
step()     { println "  ${CYAN}▶${RESET} ${BOLD}${1}${RESET}"; }
ok()       { println "    ${GREEN}✓${RESET} ${1}"; }
fail()     { println "    ${RED}✗${RESET} ${1}"; exit 1; }
info()     { println "    ${DIM}${1}${RESET}"; }
warn()     { println "    ${YELLOW}⚠${RESET}  ${1}"; }
sep()      { println "  ${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"; }

ask() {
    printf "  ${CYAN}?${RESET} %s ${DIM}[Y/n]${RESET} " "$1"
    read -r ans </dev/tty
    [[ -z "$ans" || "$ans" =~ ^[Yy] ]]
}

spin() {
    local pid=$1 msg=$2
    local frames=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')
    local i=0
    while kill -0 "$pid" 2>/dev/null; do
        printf "\r    ${CYAN}%s${RESET}  %s" "${frames[$((i % 10))]}" "$msg"
        sleep 0.08
        ((i++)) || true
    done
    printf "\r%-60s\r" ""
}

# ── Start ─────────────────────────────────────────────────────────────────────
clear
header

# ── Existing install? ─────────────────────────────────────────────────────────
if [[ -d "${INSTALL_DIR}/.git" ]]; then
    println "  ${YELLOW}Existing install detected${RESET}"
    println ""
    step "Checking for updates"

    cd "$INSTALL_DIR"
    git fetch origin --quiet 2>/dev/null || true

    LOCAL=$(git rev-parse HEAD 2>/dev/null || echo "none")
    REMOTE=$(git rev-parse origin/main 2>/dev/null || echo "unknown")

    if [[ "$LOCAL" == "$REMOTE" ]]; then
        ok "Already up to date"
        # Binary missing even though repo is current — rebuild
        if [[ ! -f "${BIN_DIR}/nexus" ]]; then
            warn "Binary not found — rebuilding"
            println ""
        else
            println ""
            info "Run nexus to launch"
            println ""
            exit 0
        fi
    fi

    COMMITS=$(git log --oneline "${LOCAL}..${REMOTE}" 2>/dev/null | wc -l | tr -d ' ')
    info "${COMMITS} new commit(s) available"
    println ""

    if ! ask "Update nexus-tui?"; then
        println "\n  Skipped.\n"
        exit 0
    fi

    println ""
    step "Pulling latest"
    git pull origin main --quiet
    ok "Pulled $(git rev-parse --short HEAD)"
    println ""

else
    # ── Fresh install ─────────────────────────────────────────────────────────
    info "Install location: ${INSTALL_DIR}"
    println ""

    if ! ask "Install nexus-tui?"; then
        println "\n  Cancelled.\n"
        exit 0
    fi

    println ""

    # ── Dependencies ─────────────────────────────────────────────────────────
    step "Checking dependencies"

    command -v git  &>/dev/null && ok "git"  || fail "git is required — install it and re-run"
    command -v curl &>/dev/null && ok "curl" || fail "curl is required — install it and re-run"
    command -v mpv  &>/dev/null && ok "mpv"  || warn "mpv not found — install it to play anime"

    if command -v rustc &>/dev/null; then
        ok "rust $(rustc --version 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
    else
        println ""
        warn "Rust not found"
        if ask "Install Rust via rustup?"; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
            sh /tmp/rustup-init.sh -y --quiet &
            spin $! "Installing Rust…"
            wait $!
            source "${HOME}/.cargo/env" 2>/dev/null || true
            command -v rustc &>/dev/null && ok "Rust installed" || fail "Rust install failed — visit https://rustup.rs"
        else
            fail "Rust is required to build nexus-tui"
        fi
    fi

    println ""

    # ── Clone ─────────────────────────────────────────────────────────────────
    step "Cloning repository"
    [[ -d "$INSTALL_DIR" ]] && rm -rf "$INSTALL_DIR"
    mkdir -p "$(dirname "$INSTALL_DIR")"

    git clone --quiet "$REPO_URL" "$INSTALL_DIR" &
    spin $! "Cloning nexus-tui…"
    wait $!
    ok "Cloned to ${INSTALL_DIR}"
    println ""
fi

# ── Build ──────────────────────────────────────────────────────────────────────
step "Building nexus-tui"
info "First build takes 2–5 minutes"
println ""

cd "$INSTALL_DIR"
START=$(date +%s)

source "${HOME}/.cargo/env" 2>/dev/null || true

CARGO_INCREMENTAL=0 cargo build --release 2>/tmp/nexus_build_err &
BUILD_PID=$!
spin $BUILD_PID "Compiling…"
wait $BUILD_PID
BUILD_EXIT=$?

END=$(date +%s)

if [[ $BUILD_EXIT -ne 0 ]]; then
    println ""
    println "  ${RED}Build failed:${RESET}"
    tail -20 /tmp/nexus_build_err | sed 's/^/    /'
    println ""
    fail "Fix the errors above and re-run the installer"
fi

ok "Built in $((END - START))s"
println ""

# ── Install binary ─────────────────────────────────────────────────────────────
step "Installing binary"
mkdir -p "$BIN_DIR"
cp "$BINARY" "${BIN_DIR}/nexus"
chmod +x "${BIN_DIR}/nexus"
ok "Installed to ${BIN_DIR}/nexus"

# ── PATH hint ──────────────────────────────────────────────────────────────────
if [[ ":${PATH}:" != *":${BIN_DIR}:"* ]]; then
    println ""
    warn "${BIN_DIR} is not in your PATH"
    info "Add this to ~/.bashrc or ~/.zshrc:"
    println ""
    println "    export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

# ── Done ───────────────────────────────────────────────────────────────────────
println ""
sep
println "  ${YELLOW}◆${RESET} ${BOLD}Done!${RESET}  Run ${CYAN}${BOLD}nexus${RESET} to launch"
println ""