# ◈ ani-nexus-tui

![License](https://img.shields.io/badge/license-CC%20BY--NC--SA%204.0-blue.svg)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg)
![Language](https://img.shields.io/badge/language-Rust-orange.svg)

A blazing-fast, strictly terminal-based UI for **Anime**. Experience website-quality browsing and streaming mechanics directly in your terminal, with absolutely zero browser overhead.

![ani-nexus-tui demo](assets/nexus-demo.gif)

---

## ✨ Features

- **No API Keys Required:** Powered by AllAnime out of the box. Zero authentication, zero setup.
- **High-Fidelity Rendering:** Auto-detects Kitty image protocol for rendering full-color, high-resolution anime cover art directly in the terminal (graceful fallback to half-blocks for standard terminals).
- **Persistent Local History:** Granular SQLite-backed history tracking. Automatically remembers exactly where you paused playback across episodes.
- **AniSkip Integration:** Optionally skip parsed intro and outro sequences cleanly.
- **Highly Customizable:** Edit your UI palette, layout limits, playback qualities, and key behavior directly from the interactive `F3` settings tab.
- **Cross-Platform:** Distributed with 1-click installer scripts for Linux, macOS, and Windows.

---

## 🚀 Getting Started

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/OsamuDazai666/ani-nexus-tui/main/install.sh | bash
```

The script automates:
- Downloading the latest prebuilt `ani-nexus` release for your platform
- Installing it through the generated release installer
- Avoiding a local Rust build during install

### Windows

Open PowerShell and execute:

```powershell
irm https://raw.githubusercontent.com/OsamuDazai666/ani-nexus-tui/main/install.ps1 | iex
```

---

## 🗑️ Uninstalling

If you ever need to remove `ani-nexus-tui`, you can run the provided uninstallation scripts which safely clean up the binary, application data, and config.

### Linux / macOS
```bash
curl -fsSL https://raw.githubusercontent.com/OsamuDazai666/ani-nexus-tui/main/uninstall.sh | bash
```

### Windows
Open PowerShell and execute:
```powershell
irm https://raw.githubusercontent.com/OsamuDazai666/ani-nexus-tui/main/uninstall.ps1 | iex
```

---

## ⌨️ Advanced Keybindings

Mastering the keyboard shortcuts is the key to navigating `ani-nexus-tui` smoothly.

### Global & Navigation
| Keybind | Action |
|---------|--------|
| `F1` | Jump to **Anime** tab |
| `F2` | Jump to **History** tab |
| `F3` | Jump to **Settings** tab |
| `/` | Focus search bar (from anywhere) |
| `Ctrl+C` / `q` | Force quit application |
| `Esc` | Unfocus / Go back |

### Browsing & Selecting
| Keybind | Action |
|---------|--------|
| `j` / `k` (or `↑`/`↓`) | Move cursor up or down |
| `l` / `h` (or `→`/`←`) | Move focus between panes (e.g., Sidebar to Main Content) |
| `g` / `G` | Jump to top / bottom of results |
| `Enter` | Confirm selection / Open details |
| `Ctrl+N` | Load next page of search results |
| `Ctrl+↓`/`↑` | Fast-scroll the synopsis and text details |

### Playback
| Keybind | Action |
|---------|--------|
| `p` | Quick-play the currently selected item / episode |
| `Tab` | Toggle between **Sub** and **Dub** streams |
| `Ctrl+Q` | Cycle stream video quality (`best` → `1080` → `720` → `480`) |

### History Tab
| Keybind | Action |
|---------|--------|
| `Any Letter` | Immediately begins fuzzy-filtering your history list |
| `Backspace` | Erase recent character in filter |
| `Delete` (`d`) | Remove selected anime from watch history |

### Settings Tab
| Keybind | Action |
|---------|--------|
| `l` / `→` | Cycle configuration value forward |
| `h` / `←` | Cycle configuration value backward |
| `Enter` | Manually input exact value (Text fields & Colors) |
| `Ctrl+←` | Break out of configuration panel |

---

## 💡 How to Get the Most Out of The Application

`ani-nexus-tui` uses powerful defaults, but to get a truly cinematic and modern experience, we recommend tweaking the following:

1. **Use a Kitty-Compatible Terminal**
   By default, standard terminals render images using ASCII half-blocks. If you use [Kitty](https://sw.kovidgoyal.net/kitty/), [WezTerm](https://wezfurlong.org/wezterm/), or [Ghostty](https://ghostty.org/), `ani-nexus-tui` will detect this and stream raw pixel data, rendering beautiful high-definition cover art.
   
2. **Configure Automatic Intro/Outro Skips**
   Don't want to skip manually? Go into your Settings (`F3`), navigate to **Playback**, and change `Skip segments` to `both`. The built-in AniSkip integration will scrub past intros and credits without dropping a frame.

3. **Dial In a Resume Offset**
   When clicking an episode you partially watched, `ani-nexus-tui` automatically resumes the video from where you left off. Adding a **5-second resume offset** in Settings (`F3` → Playback) is highly recommended. It rewinds playbacks slightly so you have time to adjust to the context of the scene before action resumes.

4. **Instant Fuzzy Finding**
   Your watch history scales efficiently using our native SQLite backend. No matter how large your history gets, simply **start typing** in the History tab (`F2`) to instantly fuzzy-filter for any anime.

---

## 🏗 Project Architecture

An industrial-grade Rust terminal UI utilizing an async actor model:

```
src/
├── main.rs          # Entry point, terminal lifecycle management
├── app.rs           # Core state machine, async message passing
├── api/             # API layer
│   └── allanime.rs  # AllAnime GraphQL client & domain logic
├── ui/              # Presentation layer
│   ├── mod.rs       # Layout compositor & runtime palette
│   ├── search.rs    # Search bar & infinite results logic
│   ├── detail.rs    # Meta, text reflow, episode grid
│   ├── image.rs     # Async cover-art fetching & terminal image protocol
│   └── history.rs   # Persistent watch tracking view
├── db/              # Persistence layer
│   └── history.rs   # SQLite thread-safe transactional store
├── config.rs        # TOML configuration loader & saver
└── player.rs        # Interactive MPV launcher & IPC stream resolution
```

---

---

## 🛠 Building From Source

1. **Install Rust**  
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **System Dependencies (Linux)**  
   ```bash
   sudo apt install build-essential pkg-config libssl-dev mpv
   ```
3. **Compile**  
   ```bash
   git clone https://github.com/OsamuDazai666/nexus-tui
   cd nexus-tui
   cargo build --release
   ```
4. **Execute**  
   ```bash
   ./target/release/ani-nexus
   ```

---

*Available under the [CC BY-NC-SA 4.0 License](LICENSE).*
