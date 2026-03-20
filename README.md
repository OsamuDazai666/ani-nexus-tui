# ◈ nexus-tui

A blazing-fast terminal UI for **Anime**, **Movies**, **TV** and **Manga** 
website-quality browsing, zero browser required.

<!-- ```
┌──────────────────────────────────────────────────────────────────┐
│  ◈ NEXUS  Anime[F1]│Movies[F2]│TV[F3]│Manga[F4]│History[F5]    │
├─────────────────┬──────────────────────────────────────────────  │
│ 🔍 cowboy bep.. │ [▓▓▓▓▓▓▓▓▓▓▓▓]  Cowboy Bebop                 │
│─────────────────│  ★ 8.9/10  ★★★★☆                             │
│ ▶ Cowboy Bebop  │  1998  ·  26 eps                              │
│   Trigun        │  ◉ Finished                                   │
│   Outlaw Star   │  [Action] [Sci-Fi] [Space] [Drama]            │
│   ...           │──────────────────────────────────────────────  │
│                 │  In the year 2071, humanity has colonized...   │
│                 │  ...                                           │
│─────────────────│──────────────────────────────────────────────  │
│ History         │  Related (8)                                   │
│  Ep 12/26 ████  │  ▶ Trigun  ★8.4  1998                        │
└─────────────────┴──────────────────────────────────────────────  │
│  [/] search  [j/k] navigate  [↵] select  [p] play  [q] quit     │
└──────────────────────────────────────────────────────────────────┘ 
``` -->


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

The script will:
- Install Scoop if not present
- Install `mpv` and `yt-dlp`
- Install Rust (GNU toolchain — no Visual Studio required)
- Build nexus-tui from source (~1 min)
- Add `nexus` to your PATH

### TMDB API Key (Movies & TV only)

Anime and Manga work out of the box. For Movies & TV you need a free TMDB key:

1. Sign up at [themoviedb.org](https://www.themoviedb.org/settings/api)
2. Copy your API key
3. The installer will prompt you for it, or set it manually:

```bash
# Linux / macOS — add to ~/.bashrc or ~/.zshrc
export TMDB_API_KEY="your_key_here"
```

```powershell
# Windows
[System.Environment]::SetEnvironmentVariable("TMDB_API_KEY", "your_key_here", "User")
```

---

## Keybindings

| Key | Action |
|-----|--------|
| `F1–F5` | Switch tab (Anime / Movies / TV / Manga / History) |
| `/` or `Tab` | Focus search bar |
| `Enter` | Execute search / select item |
| `j` / `k` or `↑↓` | Navigate list |
| `l` or `→` | Move focus to detail panel |
| `h` or `←` | Move focus back to results |
| `p` | Play in mpv |
| `d` | Delete from history (History tab) |
| `q` | Quit |
| `Ctrl+C` | Force quit |

---

## Image Rendering

nexus-tui auto-detects the best image protocol for your terminal:

| Protocol | Terminal | Quality |
|----------|----------|---------|
| Kitty | Kitty, WezTerm | ★★★★★ |
| Sixel | xterm, foot, mlterm | ★★★★☆ |
| Half-blocks | All terminals | ★★★☆☆ |

---

## Content Sources

| Source | Content | Auth |
|--------|---------|------|
| AniList (GraphQL) | Anime | None |
| TMDB | Movies & TV | API key |
| MangaDex | Manga | None |

---

## Project Structure

```
src/
├── main.rs          # Entry point, terminal lifecycle
├── app.rs           # State machine, async message bus
├── api/
│   ├── mod.rs       # Unified ContentItem enum
│   ├── anilist.rs   # AniList GraphQL client
│   ├── tmdb.rs      # TMDB REST client
│   └── mangadex.rs  # MangaDex REST client
├── ui/
│   ├── mod.rs       # Layout composition, palette
│   ├── search.rs    # Search bar + results list
│   ├── detail.rs    # Meta, synopsis, recommendations
│   ├── image.rs     # Cover art renderer (halfblock/kitty/sixel)
│   └── history.rs   # History view + progress bars
├── db/
│   └── history.rs   # sled-backed persistent history
└── player.rs        # mpv launcher
```