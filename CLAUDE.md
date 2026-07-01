# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & run

```bash
cargo build              # debug build
cargo build --release    # release (LTO, single codegen unit, stripped)
cargo run                # runs the `ani-nexus` binary
cargo test               # only one test currently: flaresolverr::extract_domain
cargo fmt && cargo clippy
```

The binary is named `ani-nexus` (`[[bin]]` in `Cargo.toml`), entry point `src/main.rs`. The crate is published as `ani-nexus-tui`.

External runtime dependencies (not built — must exist on PATH or via override):
- **mpv** — required for playback (`PlayerConfig::mpv_path`, default `mpv`).
- **FlareSolverr** at `http://localhost:8191` — optional but strongly preferred for AllAnime requests (Cloudflare). Override with `NEXUS_FLARESOLVERR_URL`, disable with `NEXUS_DISABLE_FLARESOLVERR=1`.
- **Chromium-family browser** — fallback when FlareSolverr is absent. Auto-detected (Brave/Chrome/Chromium); override with `NEXUS_CHROME_BIN`. Firefox is not supported (chromiumoxide).

User-facing data paths (via `directories::ProjectDirs("dev","nexus","nexus-tui")`):
- Config: `~/.config/nexus-tui/config.toml` (auto-created on first run by `Config::write_sample`)
- SQLite history: `data_local_dir/history.db`
- Session cookies: `data_local_dir/sessions/{domain}.json`
- Browser profile: `data_local_dir/browser-profile/`
- Debug log: `data_local_dir/debug.log` (FlareSolverr/browser diagnostics)
- Skip log: `data_local_dir/skip.log` (AniSkip/Jikan diagnostics)

## Architecture

A ratatui TUI driven by an **async actor model**. The main loop, `App` state, and mpv all live on the main thread because terminal teardown/restore must happen there.

### Main loop (`src/main.rs::run`)
- `Picker::from_query_stdio()` is called **before** `enable_raw_mode()` — protocol probing writes escape sequences and reads stdin, which only works outside raw mode. After detection, the protocol is overridden by `TERM_PROGRAM` heuristics (WezTerm → Kitty, vscode → Sixel, iTerm/Tabby/Hyper/Mintty/rio → Iterm2).
- Each iteration: redraw → check `app.pending_mpv` → poll for input (100ms) → `app.tick()` to drain async messages.
- **mpv runs synchronously on the main thread.** When `pending_mpv` is set (by `handle_msg(LaunchMpv)`), the loop disables raw mode + leaves the alternate screen, calls `player::launch_mpv_tracked`, then re-enables. This is the only way to safely hand the terminal to mpv and reclaim it. Do not move mpv launching into a tokio task.
- After mpv exits: if `pos/dur >= 0.80` it's stored as fully watched (`position = duration`), else the actual quit position is persisted via `db.update_position`.

### Async messages (`src/app.rs`)
`AppMsg` is the actor mailbox (`tokio::sync::mpsc::UnboundedChannel`). Background tokio tasks send back `SearchResults`, `DetailLoaded`, `ImageFetched/Decoded`, `EpisodeList`, `MalIdResolved`, `Playback(...)`, etc. The app uses a **`search_gen` generation counter** to drop stale results when the user types a new query — handlers must check it before mutating state. Caches: `image_cache` (LRU, 30), `detail_cache` (LRU, 50), `rgba_cache` (decoded `DynamicImage` keyed by item id, skips JPEG/PNG decode on revisit).

### API layer (`src/api/`)
- `ContentItem` is a single-variant enum (`Anime(AllAnimeItem)`) — kept as an enum so a future media type can be added without rewriting consumers. Don't collapse it.
- `allanime::search_allanime` issues GraphQL via `browser_auth::fetch_text_with_query`. Results are re-ranked locally by `title_bonus + score * log2(eps + 2)` (episode count is a popularity proxy).
- AniSkip integration: `resolve_mal_id` queries Jikan (no auth) for a MAL id from a title, then `fetch_skip_times` hits `api.aniskip.com/v2/skip-times/{mal_id}/{episode}`. Both degrade gracefully — failures return `None` and skipping just doesn't happen.

### Cloudflare bypass (`src/browser_auth.rs`, `src/flaresolverr.rs`)
Two-tier chain in `browser_auth::fetch_text_from_url`:
1. **FlareSolverr** (preferred). Loads persisted session cookies for the domain, retries with a fresh session if the response still looks like a challenge, saves new cookies on success. If FlareSolverr is reachable but cannot solve, the function returns a hard error rather than falling through — falling through produces a worse UX (visible browser window).
2. **Visible chromium** (fallback only when FlareSolverr is absent). Uses a persistent `user_data_dir` profile, hides webdriver flags, and on a detected challenge polls the page for up to `NEXUS_BROWSER_AUTH_WAIT_SECS` (default 180s) waiting for the user to clear it.

`looks_like_bot_challenge` checks for Cloudflare-specific markers (`cf-chl`, `/cdn-cgi/challenge-platform`, "just a moment", etc.) — do not relax this to plain `<html`, since legitimate AllAnime responses are sometimes wrapped in HTML and decoded by `extract_json_from_html` (pulls JSON out of `<pre>` tags or between `<body>` markers).

HTTP client uses `wreq` aliased as `reqwest` in `Cargo.toml` (`package = "wreq"`) for browser TLS fingerprint emulation (`wreq_util::Emulation::Chrome140`). Importing `reqwest::*` actually pulls from `wreq` — don't add the real `reqwest` crate.

### Streaming + AES decryption (`src/player.rs`)
AllAnime returns a `tobeparsed` blob that must be decrypted to get source URLs:
- AES-256-CTR with `key = SHA256("SimtVuagFbGR2K7P")`.
- Nonce = first 12 bytes of the base64-decoded payload + 4-byte counter starting at `0x00000002` (`Ctr32BE::<Aes256>`).
- Decrypted output has trailing garbage; `extract_json_from_decrypted` walks back from the end looking for a position that parses as JSON. If you change decryption, keep this fallback — clean parsing is the happy path but real responses regularly have ~16 bytes of trailing noise.
- A separate `hex_decipher` function decodes a custom hex→char mapping used for non-AES paths (legacy ani-cli compatible).

### Playback tracking
`launch_mpv_tracked` runs mpv with `--input-ipc-server`, subscribes to `time-pos`/`duration` via `observe_property`, and emits `PlaybackEvent::Position { checkpoint }` (`checkpoint=true` every 30s → DB write). On exit it reads the authoritative quit position from the watch_later file. The bundled `src/player_skip.lua` handles intro/outro skip according to `PlayerConfig::skip_segments` (`none`/`intro`/`outro`/`both`).

### UI (`src/ui/`)
- `ui::draw` is the master compositor. It pulls theme colors from `app.config.theme` and pushes them into `thread_local!` `Cell`s (`ACCENT_*`, `BAR_*`) **every frame** before drawing. Widget-level helpers (`accent()`, `bar_progress()`, etc.) read these cells. Don't try to thread theme colors through every widget — read them via the helpers.
- Tabs: `Anime` / `History` / `Settings`. Focus enum tracks which pane has keyboard focus within a tab.
- Cover art is fetched + decoded on a tokio task and posted back as `ImageDecoded`; `ratatui_image` then renders via the picker's protocol.

### Persistence (`src/db/history.rs`)
SQLite via `rusqlite` (`bundled` feature — no system libsqlite required). Two tables: `anime` (one row per show) and `episodes` (per-episode position/duration). Wrapped in `Mutex<Connection>`; `HistoryStore` exposes thread-safe operations (`update_position`, `load_all`, episode windows). The Anime tab also reads per-episode records into `App.anime_episode_records` to render in-progress bars on the episode grid.

### Config (`src/config.rs`)
TOML; loaded once at startup, saved on leaving the Settings tab. `Config::color_rgb` resolves named presets (`Yellow`, `Cyan`, …, `Custom`) and falls back through `parse_custom_color` for `#rrggbb` / `r,g,b` / `r,g,b,a`. `COLOR_PRESET_NAMES` is the cycling order in the settings UI.

## Environment variables

| Var | Default | Purpose |
|---|---|---|
| `NEXUS_IMAGE_PROTOCOL` | `auto` | Force `kitty`/`halfblock`/`auto` image rendering. |
| `NEXUS_FLARESOLVERR_URL` | `http://localhost:8191` | FlareSolverr endpoint. |
| `NEXUS_FLARESOLVERR_TIMEOUT` | `60000` | Max wait (ms) for a FlareSolverr challenge solve. |
| `NEXUS_DISABLE_FLARESOLVERR` | unset | Set to `1` to skip FlareSolverr and go straight to the visible browser. |
| `NEXUS_CHROME_BIN` | autodetect | Path to chromium-family browser for visible fallback. |
| `NEXUS_BROWSER_AUTH_WAIT_SECS` | `180` | Max seconds to wait for the user to clear a manual challenge. |
| `NEXUS_BROWSER_AUTH_POLL_MS` | `2000` | Poll interval while waiting for a manual challenge. |

## Things that look wrong but aren't

- `reqwest = { package = "wreq", … }` in `Cargo.toml` is intentional — wreq has the same API surface as reqwest plus TLS fingerprint emulation.
- The Picker is constructed before raw mode in `main.rs` — protocol detection requires reading stdin responses to escape sequences, which raw mode would intercept.
- `AppMsg::LaunchMpv` doesn't actually launch mpv. It sets `pending_mpv` so the **main loop** can launch it on the next iteration with the terminal restored. Spawning mpv from a tokio task will leave the terminal in a broken state.
- `looks_like_bot_challenge` checks for Cloudflare-specific markers, not generic HTML — `extract_json_from_html` is the legitimate path for HTML-wrapped JSON responses.
