# ◈ nexus-tui

A blazing-fast terminal UI for **Anime** — website-quality browsing and streaming, zero browser required.

![nexus-tui demo](assets/nexus-demo.gif)

---

## Install

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/OsamuDazai666/nexus-tui/main/install.sh | bash
```

The script will:
- Install Kitty terminal (optional, for best image quality)
- Install `mpv` and `yt-dlp`
- Install Rust if not present
- Build nexus-tui from source (~1 min)
- Add `nexus` to your PATH

### Windows

Open PowerShell and run:

```powershell
irm https://raw.githubusercontent.com/OsamuDazai666/nexus-tui/main/install.ps1 | iex
```

---

## Keybindings

| Key | Action |
|-----|--------|
| `F1` | Anime tab |
| `F2` | History tab |
| `/` | Focus search bar (from anywhere) |
| `Enter` | Execute search / select item |
| `j` / `k` or `↑↓` | Navigate results |
| `l` or `→` | Focus detail panel |
| `h` or `←` | Back to results |
| `Ctrl+↑↓←→` | Move between panes |
| `p` | Play in mpv |
| `Tab` | Toggle sub / dub (in episode prompt) |
| `Ctrl+Q` | Cycle stream quality |
| `d` | Delete from history |
| `q` | Quit |
| `Ctrl+C` | Force quit |

---

## Image Rendering

nexus-tui auto-detects the best image protocol for your terminal:

| Protocol | Terminal | Quality |
|----------|----------|---------|
| Kitty | Kitty, WezTerm | ★★★★★ |
| Half-blocks | All terminals | ★★★☆☆ |

---

## Content Source

| Source | Auth |
|--------|------|
| AllAnime | None — works out of the box |

---

## Project Structure

```
src/
├── main.rs          # Entry point, terminal lifecycle
├── app.rs           # State machine, async message bus
├── api/
│   ├── mod.rs       # ContentItem enum
│   └── allanime.rs  # AllAnime GraphQL client
├── ui/
│   ├── mod.rs       # Layout composition, palette
│   ├── search.rs    # Search bar + results list
│   ├── detail.rs    # Meta, synopsis, episode grid
│   ├── image.rs     # Cover art renderer
│   └── history.rs   # History view + progress bars
├── db/
│   └── history.rs   # sled-backed persistent history
├── config.rs        # Config loader
└── player.rs        # mpv launcher + stream resolution
```

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