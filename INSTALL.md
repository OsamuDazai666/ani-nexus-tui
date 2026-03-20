# Installing nexus-tui

## One-line install

### Linux / macOS
```bash
curl -fsSL https://raw.githubusercontent.com/OsamuDazai666/nexus-tui/main/install.sh | bash
```

### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/OsamuDazai666/nexus-tui/main/install.ps1 | iex
```

---

## What the installer does

1. Installs **Kitty terminal** (best image quality — cover art renders in full color)
2. Installs **mpv** (video player)
3. Downloads and builds nexus-tui
4. Adds `nexus` to your PATH

No API keys required. Works out of the box.

### Linux
Uses your existing package manager — `apt`, `pacman`, `dnf`, or `zypper`.

### macOS
Installs Homebrew if not present, then uses `brew` for all dependencies.

### Windows
Installs Scoop if not present, then uses `scoop` for dependencies.

---

## Manual install

Download the binary for your platform from the [latest release](https://github.com/OsamuDazai666/nexus-tui/releases/latest):

| Platform       | File                          |
|----------------|-------------------------------|
| Linux x86_64   | `nexus-linux-x86_64`          |
| Linux ARM64    | `nexus-linux-aarch64`         |
| macOS x86_64   | `nexus-macos-x86_64`          |
| macOS ARM64    | `nexus-macos-aarch64`         |
| Windows x86_64 | `nexus-windows-x86_64.exe`    |

```bash
# Linux / macOS
chmod +x nexus-linux-x86_64
mv nexus-linux-x86_64 ~/.local/bin/nexus
```

---

## Dependencies

| Dependency | Required | Purpose |
|------------|----------|---------|
| mpv        | Yes      | Video playback |
| Kitty      | Recommended | Full-color cover art |

---

## Building from source

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Linux deps
sudo apt install build-essential pkg-config libssl-dev mpv

# Build
git clone https://github.com/OsamuDazai666/nexus-tui
cd nexus-tui
cargo build --release

# Run
./target/release/nexus
```