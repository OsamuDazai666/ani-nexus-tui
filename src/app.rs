use crate::{
    api::{allanime, ContentItem},
    db::history::{EpisodeRecord, HistoryEntry, HistoryStore},
    player,
    player::PlaybackEvent,
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};

// ── Tabs ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, strum::Display, strum::EnumIter)]
pub enum Tab { Anime, History }

// ── Focus ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus { Search, Results, Detail, History, HistoryDetail, HistoryEpisodes, EpisodePrompt }

// ── Spinner ───────────────────────────────────────────────────────────────────

const SPINNER: &[&str] = &["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"];

pub struct Spinner { pub frame: usize, last: Instant }

impl Spinner {
    pub fn new() -> Self { Self { frame: 0, last: Instant::now() } }
    pub fn tick(&mut self) {
        if self.last.elapsed() >= Duration::from_millis(80) {
            self.frame = (self.frame + 1) % SPINNER.len();
            self.last = Instant::now();
        }
    }
    pub fn symbol(&self) -> &'static str { SPINNER[self.frame] }
}

// ── Toasts ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastKind { Info, Success, Error }

pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    born: Instant,
}

impl Toast {
    pub fn info(msg: impl Into<String>)    -> Self { Self { message: msg.into(), kind: ToastKind::Info,    born: Instant::now() } }
    pub fn success(msg: impl Into<String>) -> Self { Self { message: msg.into(), kind: ToastKind::Success, born: Instant::now() } }
    pub fn error(msg: impl Into<String>)   -> Self { Self { message: msg.into(), kind: ToastKind::Error,   born: Instant::now() } }
    pub fn alive(&self) -> bool { self.born.elapsed() < Duration::from_secs(5) }
    pub fn age_pct(&self) -> f64 { (self.born.elapsed().as_secs_f64() / 5.0).clamp(0.0, 1.0) }
}

// ── Image protocol ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageProtocol { Kitty, HalfBlock }

// ── Async messages ────────────────────────────────────────────────────────────

pub enum AppMsg {
    SearchResults { items: Vec<ContentItem>, gen: u64 },
    MoreResults(Vec<ContentItem>),
    DetailLoaded(ContentItem),
    ImageFetched { url: String, item_id: String, bytes: Vec<u8> },
    ImageDecoded { id: String, image: image::DynamicImage },
    StreamUrl(String),
    EpisodeList { id: String, eps: Vec<String> },
    HistoryEpisodeList { anime_id: String, eps: Vec<String> },
    AnimeEpisodeRecords { anime_id: String, records: Vec<EpisodeRecord> },
    /// Windowed episode records loaded from DB
    EpisodeWindowLoaded { anime_id: String, start: usize, end: usize, records: Vec<EpisodeRecord> },
    /// Request main loop to launch mpv synchronously (terminal-safe)
    LaunchMpv { url: String, anime_id: String, episode: String, resume_from: f64 },
    Playback(PlaybackEvent),
    Error(String),
}

// ── App state ─────────────────────────────────────────────────────────────────

pub struct App {
    // Navigation
    pub active_tab:   Tab,
    pub focus:        Focus,

    // Search
    pub search_input:  String,
    pub search_cursor: usize,
    pub is_searching:  bool,
    pub search_gen:    u64,   // generation counter — drop stale results
    pub current_page:  u32,  // for pagination

    // Results
    pub results:     Vec<ContentItem>,
    pub results_idx: usize,
    pub has_more:    bool,

    // Detail
    pub selected:     Option<ContentItem>,
    pub detail_scroll: u16,
    /// Per-episode DB records for the currently selected anime (Anime tab progress display)
    pub anime_episode_records: std::collections::HashMap<String, EpisodeRecord>,

    // History
    pub history:            Vec<HistoryEntry>,
    pub history_idx:        usize,
    pub history_filter:     String,
    pub history_filtered:   Vec<usize>,
    pub history_cover:      Option<ratatui_image::protocol::StatefulProtocol>,
    pub history_cover_id:   Option<String>,
    // Episode list for the selected history entry
    pub history_episode_list:      Vec<String>,
    pub history_episode_idx:       usize,
    pub history_episodes_loading:  bool,
    // Virtual scroll window — only records for [window_start..window_end] are in memory
    pub history_ep_window_start:   usize,
    pub history_ep_window_end:     usize,
    pub history_ep_cols:           usize,
    pub history_ep_window_records: std::collections::HashMap<String, EpisodeRecord>,

    // Episode prompt + playback options
    pub episode_input:  String,
    pub episode_cursor: usize,
    pub stream_mode:    String,   // "sub" | "dub"
    pub stream_quality: String,   // "best" | "1080" | "720" | "480"
    pub episode_list:       Vec<String>,
    pub episode_list_idx:   usize,
    pub episode_cols:       usize,   // synced from UI each frame for 2-D grid nav

    // Async
    pub msg_tx: mpsc::UnboundedSender<AppMsg>,
    msg_rx:     Arc<Mutex<mpsc::UnboundedReceiver<AppMsg>>>,
    pub db:     Arc<HistoryStore>,

    // UX feedback
    pub spinner:  Spinner,
    pub toasts:   Vec<Toast>,
    pub status:   String,

    pub image_protocol: ImageProtocol,
    pub image_picker: ratatui_image::picker::Picker,
    pub cover_protocol: Option<ratatui_image::protocol::StatefulProtocol>,
    pub needs_redraw: bool,
    /// Set by handle_msg(LaunchMpv) — consumed by main loop to run mpv on main thread
    pub pending_mpv: Option<(String, String, String, f64)>, // (url, anime_id, episode, resume)

    // ── In-memory caches ──────────────────────────────────────────────────────
    pub image_cache:  std::collections::HashMap<String, Vec<u8>>,
    image_cache_order: std::collections::VecDeque<String>,
    pub detail_cache: std::collections::HashMap<String, CachedDetail>,
    detail_cache_order: std::collections::VecDeque<String>,
    /// Decoded RGBA pixels keyed by item ID — skips JPEG/PNG decode on revisit.
    /// new_resize_protocol() from raw RGBA is fast; only the decode is expensive.
    pub rgba_cache: std::collections::HashMap<String, image::DynamicImage>,
}

const IMAGE_CACHE_MAX:  usize = 30;
const DETAIL_CACHE_MAX: usize = 50;

/// Everything we pre-fetch when an item is selected, stored so revisiting is free.
#[derive(Clone)]
pub struct CachedDetail {
    pub episode_list: Option<Vec<String>>,
}

impl App {
    pub async fn new(image_picker: ratatui_image::picker::Picker) -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let db      = Arc::new(HistoryStore::open()?);
        let history = db.load_all()?;
        let protocol = detect_image_protocol();

        Ok(Self {
            active_tab:    Tab::Anime,
            focus:         Focus::Search,
            search_input:  String::new(),
            search_cursor: 0,
            is_searching:  false,
            search_gen:    0,
            current_page:  1,
            results:       Vec::new(),
            results_idx:   0,
            has_more:      false,
            selected:      None,
            detail_scroll: 0,
            anime_episode_records: std::collections::HashMap::new(),
            history,
            history_idx:   0,
            history_filter:   String::new(),
            history_filtered: Vec::new(),
            history_cover:    None,
            history_cover_id: None,
            history_episode_list:     Vec::new(),
            history_episode_idx:       0,
            history_episodes_loading:  false,
            history_ep_window_start:   0,
            history_ep_window_end:     0,
            history_ep_cols:           2,
            history_ep_window_records: std::collections::HashMap::new(),
            episode_input:    String::new(),
            episode_cursor:   0,
            stream_mode:      "sub".to_string(),
            stream_quality:   "best".to_string(),
            episode_list:     Vec::new(),
            episode_list_idx: 0,
            episode_cols:     8,
            msg_tx: tx,
            msg_rx: Arc::new(Mutex::new(rx)),
            db,
            spinner:  Spinner::new(),
            toasts:   Vec::new(),
            status:   "Type to search  •  press Enter".to_string(),
            image_protocol: protocol,
            image_picker,
            cover_protocol: None,
            needs_redraw:  false,
            pending_mpv:   None,
            image_cache:       std::collections::HashMap::new(),
            image_cache_order: std::collections::VecDeque::new(),
            detail_cache:       std::collections::HashMap::new(),
            detail_cache_order: std::collections::VecDeque::new(),
            rgba_cache:         std::collections::HashMap::new(),
        })
    }

    // ── Tick (called every ~100ms) ────────────────────────────────────────────

    pub async fn tick(&mut self) -> Result<()> {
        if self.is_searching { self.spinner.tick(); }
        self.toasts.retain(|t| t.alive());

        // Drain messages
        let msgs: Vec<AppMsg> = {
            let mut rx = self.msg_rx.lock().await;
            let mut v = Vec::new();
            while let Ok(m) = rx.try_recv() { v.push(m); }
            v
        };
        for msg in msgs { self.handle_msg(msg); }

        // Lazily load cover and episode list for selected history entry
        if self.active_tab == Tab::History {
            self.load_history_cover();
            self.load_history_episodes();
            self.load_episode_window();
        }

        Ok(())
    }

    fn handle_msg(&mut self, msg: AppMsg) {
        match msg {
            AppMsg::SearchResults { items, gen } => {
                if gen != self.search_gen { return; }
                self.is_searching = false;
                self.has_more = items.len() >= 25;
                self.results = items;
                self.results_idx = 0;
                if self.results.is_empty() {
                    self.toast_info("No results found");
                    self.status = "No results".into();
                } else {
                    self.status = format!("{} results", self.results.len());
                    self.focus = Focus::Results;
                    let first = self.results[0].clone();
                    self.load_detail(first);

                    // Prefetch episode lists for ALL results — they're small and cheap
                    self.prefetch_episode_lists(0, self.results.len());

                    // Prefetch cover images for first 5 results
                    self.prefetch_images(0, 5);
                }
            }
            AppMsg::MoreResults(items) => {
                self.is_searching = false;
                self.has_more = items.len() >= 25;
                let added = items.len();
                let prev_len = self.results.len();
                self.results.extend(items);
                self.status = format!("{} results  (+{added} more)", self.results.len());

                // Prefetch episode lists and images for the newly arrived page
                self.prefetch_episode_lists(prev_len, self.results.len());
                self.prefetch_images(prev_len, (prev_len + 5).min(self.results.len()));
            }
            AppMsg::DetailLoaded(item) => { self.selected = Some(item); self.detail_scroll = 0; }

            // Bytes fetched — store in byte cache, spawn JPEG/PNG decode off main thread
            AppMsg::ImageFetched { url, item_id, bytes } => {
                self.cache_image(url, bytes.clone());
                let tx = self.msg_tx.clone();
                tokio::spawn(async move {
                    if let Ok(img) = image::load_from_memory(&bytes) {
                        let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
                        let _ = tx.send(AppMsg::ImageDecoded { id: item_id, image: dyn_img });
                    }
                });
            }

            // Decoded RGBA ready — store in rgba_cache, build protocol if still selected
            AppMsg::ImageDecoded { id, image } => {
                // Store decoded pixels — future visits skip the decode entirely
                self.rgba_cache.insert(id.clone(), image);
                // Build and assign the protocol if this item is still on screen
                if self.selected.as_ref().map(|s| s.id() == id).unwrap_or(false) {
                    self.build_cover_protocol(&id);
                }
                // Also build history cover if it matches the currently viewed history entry
                if self.history_cover_id.as_deref() == Some(id.as_str()) {
                    self.build_history_cover_protocol(&id);
                }
            }

            // Episode list fetched — store in detail cache and apply if still selected
            AppMsg::EpisodeList { id, eps } => {
                let entry = self.detail_cache.entry(id.clone()).or_insert(CachedDetail {
                    episode_list: None,
                });
                entry.episode_list = Some(eps.clone());
                self.bump_detail_cache(&id);
                if self.selected.as_ref().map(|s| s.id() == id).unwrap_or(false) {
                    self.episode_list = eps;
                    self.episode_list_idx = 0;
                }
            }

            AppMsg::StreamUrl(url) => {
                self.is_searching = false;
                self.status = "Playing".to_string();
                self.toast_success("Launching mpv…");
                // Defer to main loop for terminal-safe launch
                self.pending_mpv = Some((url, String::new(), String::new(), 0.0));
                self.needs_redraw = true;
            }

            AppMsg::LaunchMpv { url, anime_id, episode, resume_from } => {
                self.toast_success("Launching mpv…");
                self.pending_mpv = Some((url, anime_id, episode, resume_from));
                self.needs_redraw = true;
            }
            AppMsg::EpisodeWindowLoaded { anime_id, start, end, records } => {
                if self.history_selected().map(|e| e.id == anime_id).unwrap_or(false) {
                    self.history_ep_window_start = start;
                    self.history_ep_window_end   = end;
                    self.history_ep_window_records.clear();
                    for rec in records {
                        self.history_ep_window_records.insert(rec.episode_number.clone(), rec);
                    }
                }
            }

            AppMsg::AnimeEpisodeRecords { anime_id, records } => {
                // Only apply if this anime is still selected
                if self.selected.as_ref().map(|s| s.id() == anime_id).unwrap_or(false) {
                    self.anime_episode_records.clear();
                    for rec in records {
                        self.anime_episode_records.insert(rec.episode_number.clone(), rec);
                    }
                }
            }

            AppMsg::HistoryEpisodeList { anime_id, eps } => {
                let _ = self.db.save_episodes_cache(&anime_id, &eps);
                if let Ok(Some(updated)) = self.db.get(&anime_id) {
                    if let Some(pos) = self.history.iter().position(|e| e.id == anime_id) {
                        self.history[pos] = updated;
                    }
                }
                if self.history_selected().map(|e| e.id == anime_id).unwrap_or(false) {
                    self.history_episode_list = eps;
                    self.history_episodes_loading = false;
                    self.history_ep_window_start = 0;
                    self.history_ep_window_end   = 0;
                    self.history_ep_window_records.clear();
                    self.history_episode_idx = self.last_watched_episode_idx();

                    // Synchronously load the initial window so first render has data
                    let ep_count = self.history_episode_list.len();
                    let idx      = self.history_episode_idx;
                    let start    = idx.saturating_sub(40);
                    let end      = (idx + 40).min(ep_count);
                    let window_eps: Vec<&str> = self.history_episode_list
                        .get(start..end)
                        .unwrap_or(&[])
                        .iter()
                        .map(|s| s.as_str())
                        .collect();
                    if let Ok(recs) = self.db.load_episodes_in(&anime_id, &window_eps) {
                        self.history_ep_window_start = start;
                        self.history_ep_window_end   = end;
                        for rec in recs {
                            self.history_ep_window_records.insert(rec.episode_number.clone(), rec);
                        }
                    }
                }
            }

            AppMsg::Playback(event) => {
                match event {
                    PlaybackEvent::Position { anime_id, episode, position, duration, checkpoint } => {
                        let fully = duration > 0.0 && position / duration >= 0.95;
                        // Update history window records (History tab live display)
                        if let Some(rec) = self.history_ep_window_records.get_mut(&episode) {
                            if rec.anime_id == anime_id {
                                rec.position_seconds = position;
                                if duration > 0.0 { rec.duration_seconds = duration; }
                                rec.fully_watched = fully;
                            }
                        }
                        // Update anime episode records (Anime tab live display)
                        let rec = self.anime_episode_records.entry(episode.clone())
                            .or_insert_with(|| crate::db::history::EpisodeRecord {
                                anime_id:         anime_id.clone(),
                                episode_number:   episode.clone(),
                                stream_url:       None,
                                watched:          true,
                                watch_timestamp:  None,
                                position_seconds: 0.0,
                                duration_seconds: 0.0,
                                fully_watched:    false,
                            });
                        rec.position_seconds = position;
                        if duration > 0.0 { rec.duration_seconds = duration; }
                        rec.fully_watched = fully;

                        if checkpoint {
                            let _ = self.db.update_position(&anime_id, &episode, position, duration);
                        }
                    }
                    PlaybackEvent::Finished { anime_id, episode, position, duration } => {
                        let _ = self.db.update_position(&anime_id, &episode, position, duration);
                        // Update anime_episode_records with final position
                        let fully = duration > 0.0 && position / duration >= 0.95;
                        let rec = self.anime_episode_records.entry(episode.clone())
                            .or_insert_with(|| crate::db::history::EpisodeRecord {
                                anime_id:         anime_id.clone(),
                                episode_number:   episode.clone(),
                                stream_url:       None,
                                watched:          true,
                                watch_timestamp:  None,
                                position_seconds: 0.0,
                                duration_seconds: 0.0,
                                fully_watched:    false,
                            });
                        rec.position_seconds = position;
                        if duration > 0.0 { rec.duration_seconds = duration; }
                        rec.fully_watched = fully;

                        if let Ok(all) = self.db.load_all() {
                            self.history = all;
                        }
                        // Force window reload for the finished anime
                        if self.history_selected().map(|e| e.id == anime_id).unwrap_or(false) {
                            self.history_ep_window_start = 0;
                            self.history_ep_window_end   = 0;
                            self.history_ep_window_records.clear();
                        }
                        self.toast_success("Playback saved");
                    }
                }
            }

            AppMsg::Error(e) => {
                self.is_searching = false;
                self.toast_error(e);
            }
        }
    }

    // ── Cache helpers ─────────────────────────────────────────────────────────

    /// Build cover_protocol from rgba_cache if available — fast path, no decode.
    fn build_cover_protocol(&mut self, id: &str) {
        if let Some(img) = self.rgba_cache.remove(id) {
            let protocol = self.image_picker.new_resize_protocol(img.clone());
            // Put the image back so future revisits can rebuild
            self.rgba_cache.insert(id.to_string(), img);
            self.cover_protocol = Some(protocol);
            self.save_debug_image(id);
        }
    }

    /// Build history_cover from rgba_cache for the given history entry id.
    fn build_history_cover_protocol(&mut self, id: &str) {
        if let Some(img) = self.rgba_cache.get(id) {
            let protocol = self.image_picker.new_resize_protocol(img.clone());
            self.history_cover = Some(protocol);
            self.history_cover_id = Some(id.to_string());
        }
    }

    /// Kick off a cover fetch for the currently selected history entry.
    pub fn load_history_cover(&mut self) {
        let Some(entry) = self.history_selected().cloned() else { return };
        let Some(url) = entry.cover_url.clone() else { return };
        let id = entry.id.clone();

        // Already loaded for this entry
        if self.history_cover_id.as_deref() == Some(id.as_str()) { return; }

        self.history_cover = None;
        self.history_cover_id = Some(id.clone());

        // Fast path — already decoded
        if self.rgba_cache.contains_key(&id) {
            self.build_history_cover_protocol(&id);
            return;
        }

        let tx = self.msg_tx.clone();
        if let Some(cached_bytes) = self.image_cache.get(&url).cloned() {
            tokio::spawn(async move {
                if let Ok(img) = image::load_from_memory(&cached_bytes) {
                    let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
                    let _ = tx.send(AppMsg::ImageDecoded { id, image: dyn_img });
                }
            });
        } else {
            tokio::spawn(async move {
                let client = reqwest::Client::builder()
                    .user_agent("Mozilla/5.0")
                    .timeout(std::time::Duration::from_secs(15))
                    .build().unwrap_or_default();
                if let Ok(r) = client.get(&url).send().await {
                    if let Ok(b) = r.bytes().await {
                        let bytes = b.to_vec();
                        let _ = tx.send(AppMsg::ImageFetched { url, item_id: id.clone(), bytes: bytes.clone() });
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
                            let _ = tx.send(AppMsg::ImageDecoded { id, image: dyn_img });
                        }
                    }
                }
            });
        }
    }

    /// Load/refresh the episode list for the currently selected history entry.
    /// Uses the cached list from SQLite if fresh; otherwise fetches from API.
    pub fn load_history_episodes(&mut self) {
        let Some(entry) = self.history_selected().cloned() else {
            // Selection changed — clear stale data
            if !self.history_episode_list.is_empty() {
                self.history_episode_list.clear();
                self.history_ep_window_records.clear();
                self.history_episode_idx = 0;
                self.history_ep_window_start = 0;
                self.history_ep_window_end   = 0;
            }
            return;
        };

        // Detect which anime is currently loaded in the window
        let loaded_id = self.history_ep_window_records.values().next()
            .map(|r| r.anime_id.clone());

        // If we already have a fresh list for this entry, nothing to do
        let already_loaded = !self.history_episode_list.is_empty()
            && loaded_id.as_deref() == Some(entry.id.as_str());

        if already_loaded && !entry.episodes_cache_stale() {
            return;
        }

        // Switching to a different anime — reset immediately
        let switching = loaded_id.as_deref() != Some(entry.id.as_str());
        if switching {
            self.history_episode_list.clear();
            self.history_ep_window_records.clear();
            self.history_episode_idx = 0;
            self.history_ep_window_start = 0;
            self.history_ep_window_end   = 0;

            // Use cached episode list if fresh
            if let Some(ref cached) = entry.episodes_cache {
                if !entry.episodes_cache_stale() {
                    self.history_episode_list = cached.clone();
                    self.history_episode_idx  = self.last_watched_episode_idx();
                    // Synchronously load the initial window
                    let ep_count = self.history_episode_list.len();
                    let idx      = self.history_episode_idx;
                    let start    = idx.saturating_sub(40);
                    let end      = (idx + 40).min(ep_count);
                    let anime_id = entry.id.clone();
                    let window_eps: Vec<&str> = self.history_episode_list
                        .get(start..end)
                        .unwrap_or(&[])
                        .iter()
                        .map(|s| s.as_str())
                        .collect();
                    if let Ok(recs) = self.db.load_episodes_in(&anime_id, &window_eps) {
                        self.history_ep_window_start = start;
                        self.history_ep_window_end   = end;
                        for rec in recs {
                            self.history_ep_window_records.insert(rec.episode_number.clone(), rec);
                        }
                    }
                    return;
                }
            }
        }

        // Need fresh data from API
        if self.history_episodes_loading { return; }
        self.history_episodes_loading = true;

        let tx       = self.msg_tx.clone();
        let anime_id = entry.id.clone();
        let mode     = self.stream_mode.clone();

        tokio::spawn(async move {
            match player::fetch_episode_list(&anime_id, &mode).await {
                Ok(eps) => {
                    let _ = tx.send(AppMsg::HistoryEpisodeList { anime_id, eps });
                }
                Err(e) => {
                    let _ = tx.send(AppMsg::Error(format!("Episode list: {e}")));
                }
            }
        });
    }

    /// Index into history_episode_list for the last watched episode (from progress field).
    fn last_watched_episode_idx(&self) -> usize {
        let Some(entry) = self.history_selected() else { return 0 };
        let Some(progress) = entry.progress else { return 0 };
        let target = progress.to_string();
        let raw = self.history_episode_list
            .iter()
            .position(|ep| ep == &target)
            .unwrap_or(0);
        // In a 2/3-col grid, go back one full row so last watched isn't at the very top
        raw.saturating_sub(self.history_ep_cols)
    }

    /// Load episode records for the window around history_episode_idx.
    /// Window = [idx - 40, idx + 40] clamped to list length.
    /// Only triggers a DB fetch if the current window doesn't cover idx + 10-row buffer.
    pub fn load_episode_window(&mut self) {
        let ep_count = self.history_episode_list.len();
        if ep_count == 0 { return; }

        let Some(entry) = self.history_selected().cloned() else { return };

        let idx = self.history_episode_idx;
        let desired_start = idx.saturating_sub(40);
        let desired_end   = (idx + 40).min(ep_count);

        // Check if the current window already covers the desired range + 10 buffer
        let cur_start = self.history_ep_window_start;
        let cur_end   = self.history_ep_window_end;

        let needs_slide = self.history_ep_window_records.is_empty()
            || desired_start < cur_start.saturating_sub(10)
            || desired_end   > cur_end + 10;

        if !needs_slide { return; }

        // Collect the episode strings for the window
        let window_eps: Vec<String> = self.history_episode_list
            .get(desired_start..desired_end)
            .unwrap_or(&[])
            .to_vec();

        if window_eps.is_empty() { return; }

        let tx       = self.msg_tx.clone();
        let db       = self.db.clone();
        let anime_id = entry.id.clone();

        tokio::spawn(async move {
            let ep_refs: Vec<&str> = window_eps.iter().map(|s| s.as_str()).collect();
            match db.load_episodes_in(&anime_id, &ep_refs) {
                Ok(records) => {
                    let _ = tx.send(AppMsg::EpisodeWindowLoaded {
                        anime_id,
                        start: desired_start,
                        end:   desired_end,
                        records,
                    });
                }
                Err(e) => { let _ = tx.send(AppMsg::Error(format!("ep window: {e}"))); }
            }
        });
    }

    fn save_debug_image(&self, id: &str) {
        if let Some(img) = self.rgba_cache.get(id) {
            if let Ok(home) = std::env::var("HOME") {
                let debug_dir = std::path::PathBuf::from(&home).join("Desktop").join("nexus-debug");
                let _ = std::fs::create_dir_all(&debug_dir);
                let name = self.selected.as_ref()
                    .map(|s| s.title().replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_"))
                    .unwrap_or_else(|| "unknown".to_string());
                let _ = img.save(debug_dir.join(format!("{name}.png")));
            }
        }
    }

    fn cache_image(&mut self, url: String, bytes: Vec<u8>) {
        if self.image_cache.contains_key(&url) {
            // Refresh LRU position
            self.image_cache_order.retain(|u| u != &url);
            self.image_cache_order.push_back(url.clone());
            return;
        }
        if self.image_cache.len() >= IMAGE_CACHE_MAX {
            if let Some(evict) = self.image_cache_order.pop_front() {
                self.image_cache.remove(&evict);
            }
        }
        self.image_cache.insert(url.clone(), bytes);
        self.image_cache_order.push_back(url);
    }

    fn bump_detail_cache(&mut self, id: &str) {
        self.detail_cache_order.retain(|i| i != id);
        self.detail_cache_order.push_back(id.to_string());
        if self.detail_cache.len() > DETAIL_CACHE_MAX {
            if let Some(evict) = self.detail_cache_order.pop_front() {
                self.detail_cache.remove(&evict);
            }
        }
    }

    /// Prefetch cover images for results[start..end] that aren't already cached.
    fn prefetch_images(&self, start: usize, end: usize) {
        for item in self.results.get(start..end).unwrap_or(&[]) {
            let Some(url) = item.cover_url().map(String::from) else { continue };
            let item_id = item.id().to_string();

            // Skip if already decoded
            if self.rgba_cache.contains_key(&item_id) { continue; }

            let tx = self.msg_tx.clone();

            if let Some(cached_bytes) = self.image_cache.get(&url).cloned() {
                // Byte cache hit — just decode
                tokio::spawn(async move {
                    if let Ok(img) = image::load_from_memory(&cached_bytes) {
                        let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
                        let _ = tx.send(AppMsg::ImageDecoded { id: item_id, image: dyn_img });
                    }
                });
            } else {
                // Full miss — fetch + decode
                tokio::spawn(async move {
                    let client = reqwest::Client::builder()
                        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
                        .timeout(Duration::from_secs(15))
                        .connect_timeout(Duration::from_secs(8))
                        .build().unwrap_or_default();
                    for attempt in 0..2u8 {
                        match client.get(&url).header("Accept", "image/*").send().await {
                            Ok(r) if r.status().is_success() => {
                                match r.bytes().await {
                                    Ok(b) => {
                                        let bytes = b.to_vec();
                                        let _ = tx.send(AppMsg::ImageFetched { url: url.clone(), item_id: item_id.clone(), bytes: bytes.clone() });
                                        if let Ok(img) = image::load_from_memory(&bytes) {
                                            let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
                                            let _ = tx.send(AppMsg::ImageDecoded { id: item_id, image: dyn_img });
                                        }
                                        return;
                                    }
                                    Err(_) if attempt == 0 => continue,
                                    Err(_) => return,
                                }
                            }
                            Ok(_) if attempt == 0 => continue,
                            _ => return,
                        }
                    }
                });
            }
        }
    }

    /// Prefetch episode lists for results[start..end] that aren't already cached.
    fn prefetch_episode_lists(&self, start: usize, end: usize) {
        let mode = self.stream_mode.clone();
        for item in self.results.get(start..end).unwrap_or(&[]) {
            let ContentItem::Anime(a) = item;
            let id = a.id.clone();
            if self.detail_cache.get(&id).and_then(|c| c.episode_list.as_ref()).is_some() {
                continue; // already cached
            }
            let tx  = self.msg_tx.clone();
            let mode = mode.clone();
            tokio::spawn(async move {
                if let Ok(eps) = player::fetch_episode_list(&id, &mode).await {
                    let _ = tx.send(AppMsg::EpisodeList { id, eps });
                }
            });
        }
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    /// Returns true → quit
    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Global: Ctrl+C always quits
        if ctrl && key.code == KeyCode::Char('c') {
            return Ok(true);
        }

        // Global: q quits — but NOT while in Search or EpisodePrompt (q has local meaning there)
        if key.code == KeyCode::Char('q')
            && self.focus != Focus::Search
            && self.focus != Focus::EpisodePrompt
        {
            return Ok(true);
        }

        // Global: '/' jumps to search bar from anywhere except Search itself or History panes
        if key.code == KeyCode::Char('/') && self.focus != Focus::Search
            && self.focus != Focus::History && self.focus != Focus::HistoryDetail
        {
            self.focus = Focus::Search;
            self.search_cursor = self.search_input.len();
            return Ok(false);
        }

        // Global: Ctrl+Arrow — 2D pane navigation, works from every focus state
        //
        // Layout map:
        //   [Search                    ]   ↑ row 0
        //   [Results | Detail/Meta/Synopsis]   row 1
        //   [Results | EpisodePrompt   ]   ↓ row 2
        //
        //   Ctrl+↑ / Ctrl+↓  move vertically between rows
        //   Ctrl+→ / Ctrl+←  move horizontally between columns
        if ctrl {
            match key.code {
                KeyCode::Up => {
                    self.focus = match &self.focus {
                        Focus::EpisodePrompt   => Focus::Detail,
                        Focus::Results         => Focus::Search,
                        Focus::Detail          => Focus::Search,
                        Focus::Search          => Focus::Search,
                        Focus::History         => Focus::History,
                        Focus::HistoryDetail   => Focus::HistoryDetail,
                        Focus::HistoryEpisodes => Focus::HistoryEpisodes,
                    };
                    return Ok(false);
                }
                KeyCode::Down => {
                    self.focus = match &self.focus {
                        Focus::Search   => { if !self.results.is_empty() { Focus::Results } else { Focus::Search } }
                        Focus::Results  => { if self.selected.is_some() { Focus::EpisodePrompt } else { Focus::Results } }
                        Focus::Detail   => { if self.selected.is_some() { Focus::EpisodePrompt } else { Focus::Detail } }
                        Focus::EpisodePrompt   => Focus::EpisodePrompt,
                        Focus::History         => Focus::History,
                        Focus::HistoryDetail   => Focus::HistoryEpisodes,
                        Focus::HistoryEpisodes => Focus::HistoryEpisodes,
                    };
                    return Ok(false);
                }
                KeyCode::Right => {
                    self.focus = match &self.focus {
                        Focus::Results         => { if self.selected.is_some() { Focus::Detail } else { Focus::Results } }
                        Focus::Detail          => Focus::Detail,
                        Focus::EpisodePrompt   => Focus::EpisodePrompt,
                        Focus::Search          => Focus::Search,
                        Focus::History         => Focus::HistoryDetail,
                        Focus::HistoryDetail   => Focus::HistoryEpisodes,
                        Focus::HistoryEpisodes => Focus::HistoryEpisodes,
                    };
                    return Ok(false);
                }
                KeyCode::Left => {
                    self.focus = match &self.focus {
                        Focus::Detail          => Focus::Results,
                        Focus::EpisodePrompt   => Focus::Results,
                        Focus::Results         => Focus::Results,
                        Focus::Search          => Focus::Search,
                        Focus::History         => Focus::History,
                        Focus::HistoryDetail   => Focus::History,
                        Focus::HistoryEpisodes => Focus::HistoryDetail,
                    };
                    return Ok(false);
                }
                _ => {}
            }
        }

        // Tab switching — F1..F5
        match key.code {
            KeyCode::F(1) => { self.switch_tab(Tab::Anime);   return Ok(false); }
            KeyCode::F(2) => { self.switch_tab(Tab::History); return Ok(false); }
            _ => {}
        }

        match self.focus.clone() {
            Focus::Search          => self.key_search(key).await?,
            Focus::Results         => self.key_results(key).await?,
            Focus::Detail          => self.key_detail(key).await?,
            Focus::History         => self.key_history(key).await?,
            Focus::HistoryDetail   => self.key_history_detail(key).await?,
            Focus::HistoryEpisodes => self.key_history_episodes(key).await?,
            Focus::EpisodePrompt   => self.key_episode_prompt(key).await?,
        }
        Ok(false)
    }

    async fn key_search(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char(c) => {
                self.search_input.insert(self.search_cursor, c);
                self.search_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                    self.search_input.remove(self.search_cursor);
                }
            }
            KeyCode::Delete => {
                if self.search_cursor < self.search_input.len() {
                    self.search_input.remove(self.search_cursor);
                }
            }
            KeyCode::Left  => { if self.search_cursor > 0 { self.search_cursor -= 1; } }
            KeyCode::Right => { if self.search_cursor < self.search_input.len() { self.search_cursor += 1; } }
            KeyCode::Home  => { self.search_cursor = 0; }
            KeyCode::End   => { self.search_cursor = self.search_input.len(); }
            KeyCode::Enter => {
                let query = self.search_input.trim().to_string();
                if query.is_empty() {
                    self.results.clear();
                    self.selected = None;
                    self.cover_protocol = None;
                    self.episode_list.clear();
                    self.status = "Type to search  •  press Enter".to_string();
                } else {
                    self.do_search(1).await;
                }
            }
            // ↓ / Tab / Esc — move to results if they exist, otherwise stay
            KeyCode::Down | KeyCode::Tab | KeyCode::Esc => {
                if !self.results.is_empty() { self.focus = Focus::Results; }
            }
            _ => {}
        }
        Ok(())
    }

    async fn key_results(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up   | KeyCode::Char('k') => self.results_up(),
            KeyCode::Down | KeyCode::Char('j') => self.results_down().await,

            // gg → go to top
            KeyCode::Char('g') => {
                self.results_idx = 0;
                if !self.results.is_empty() {
                    let item = self.results[0].clone();
                    self.load_detail(item);
                }
            }
            // G → go to bottom
            KeyCode::Char('G') => {
                if !self.results.is_empty() {
                    self.results_idx = self.results.len() - 1;
                    let item = self.results[self.results_idx].clone();
                    self.load_detail(item);
                }
            }

            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.focus = Focus::Detail;
            }
            KeyCode::Tab => { self.focus = Focus::Search; }
            KeyCode::Char('p') => { self.resolve_and_play().await; }

            // Ctrl+N → load next page
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.has_more && !self.is_searching {
                    self.load_next_page().await;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn key_detail(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up   | KeyCode::Char('k') => { self.detail_scroll = self.detail_scroll.saturating_sub(1); }
            KeyCode::Down | KeyCode::Char('j') => { self.detail_scroll += 1; }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.detail_scroll = self.detail_scroll.saturating_sub(10);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.detail_scroll += 10;
            }
            KeyCode::PageUp   => { self.detail_scroll = self.detail_scroll.saturating_sub(10); }
            KeyCode::PageDown => { self.detail_scroll += 10; }
            KeyCode::Char('p') => { self.resolve_and_play().await; }
            KeyCode::Left | KeyCode::Esc => { self.focus = Focus::Results; }
            _ => {}
        }
        Ok(())
    }

    async fn key_history(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if self.history_filter.is_empty() => {
                if self.history_idx > 0 {
                    self.history_idx -= 1;
                    self.history_cover = None;
                    self.history_cover_id = None;
                }
            }
            KeyCode::Up => {
                if self.history_idx > 0 {
                    self.history_idx -= 1;
                    self.history_cover = None;
                    self.history_cover_id = None;
                }
            }
            KeyCode::Down | KeyCode::Char('j') if self.history_filter.is_empty() => {
                let len = if self.history_filter.is_empty() {
                    self.history.len()
                } else {
                    self.history_filtered.len()
                };
                if self.history_idx + 1 < len {
                    self.history_idx += 1;
                    self.history_cover = None;
                    self.history_cover_id = None;
                }
            }
            KeyCode::Down => {
                let len = if self.history_filter.is_empty() {
                    self.history.len()
                } else {
                    self.history_filtered.len()
                };
                if self.history_idx + 1 < len {
                    self.history_idx += 1;
                    self.history_cover = None;
                    self.history_cover_id = None;
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') if self.history_filter.is_empty() => {
                self.focus = Focus::HistoryDetail;
            }
            KeyCode::Right if !self.history_filter.is_empty() => {
                self.focus = Focus::HistoryDetail;
            }
            KeyCode::Delete => {
                let actual_idx = if self.history_filter.is_empty() {
                    Some(self.history_idx)
                } else {
                    self.history_filtered.get(self.history_idx).copied()
                };
                if let Some(idx) = actual_idx {
                    if let Some(e) = self.history.get(idx).cloned() {
                        let _ = self.db.remove(&e.id);
                        self.toast_info(format!("Removed \"{}\"", e.title));
                        self.history.remove(idx);
                        self.history_cover = None;
                        self.history_cover_id = None;
                        self.history_episode_list.clear();
                        self.history_ep_window_records.clear();
                        self.history_episode_idx = 0;
                        self.history_ep_window_start = 0;
                        self.history_ep_window_end   = 0;
                        self.history_episodes_loading = false;
                        self.rebuild_history_filter();
                        let new_len = if self.history_filter.is_empty() {
                            self.history.len()
                        } else {
                            self.history_filtered.len()
                        };
                        if self.history_idx > 0 && self.history_idx >= new_len {
                            self.history_idx = new_len.saturating_sub(1);
                        }
                    }
                }
            }
            KeyCode::Right => { self.focus = Focus::HistoryDetail; }
            KeyCode::Esc => {
                if !self.history_filter.is_empty() {
                    self.history_filter.clear();
                    self.history_filtered.clear();
                    self.history_idx = 0;
                    self.history_cover = None;
                    self.history_cover_id = None;
                }
            }
            KeyCode::Backspace => {
                if !self.history_filter.is_empty() {
                    self.history_filter.pop();
                    self.history_idx = 0;
                    self.history_cover = None;
                    self.history_cover_id = None;
                    self.rebuild_history_filter();
                }
            }
            KeyCode::Char(c) => {
                self.history_filter.push(c);
                self.history_idx = 0;
                self.history_cover = None;
                self.history_cover_id = None;
                self.rebuild_history_filter();
            }
            _ => {}
        }
        Ok(())
    }

    async fn key_history_detail(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Left | KeyCode::Esc => { self.focus = Focus::History; }
            KeyCode::Right | KeyCode::Char('l') => {
                if !self.history_episode_list.is_empty() {
                    self.focus = Focus::HistoryEpisodes;
                }
            }
            KeyCode::Delete => {
                let actual_idx = if self.history_filter.is_empty() {
                    Some(self.history_idx)
                } else {
                    self.history_filtered.get(self.history_idx).copied()
                };
                if let Some(idx) = actual_idx {
                    if let Some(e) = self.history.get(idx).cloned() {
                        let _ = self.db.remove(&e.id);
                        self.toast_info(format!("Removed \"{}\"", e.title));
                        self.history.remove(idx);
                        self.history_cover = None;
                        self.history_cover_id = None;
                        self.history_episode_list.clear();
                        self.history_ep_window_records.clear();
                        self.rebuild_history_filter();
                        self.focus = Focus::History;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn key_history_episodes(&mut self, key: KeyEvent) -> Result<()> {
        let ep_count = self.history_episode_list.len();
        let cols     = self.history_ep_cols.max(1);
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                // Move left within row, or exit to detail pane if already at col 0
                if self.history_episode_idx % cols == 0 {
                    self.focus = Focus::HistoryDetail;
                } else {
                    self.history_episode_idx -= 1;
                }
            }
            KeyCode::Esc => { self.focus = Focus::HistoryDetail; }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.history_episode_idx + 1 < ep_count {
                    self.history_episode_idx += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.history_episode_idx = self.history_episode_idx.saturating_sub(cols);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let next = self.history_episode_idx + cols;
                if next < ep_count {
                    self.history_episode_idx = next;
                }
            }
            KeyCode::Enter | KeyCode::Char('p') => {
                // Jump to this episode in the Anime tab — switch tab, search, pre-select episode
                let Some(entry) = self.history_selected().cloned() else { return Ok(()); };
                let Some(ep_str) = self.history_episode_list.get(self.history_episode_idx).cloned()
                    else { return Ok(()); };

                // Get resume position from DB
                let ep_record = self.db.get_episode(&entry.id, &ep_str).ok().flatten();
                let resume_from = ep_record.as_ref()
                    .filter(|r| !r.fully_watched && r.position_seconds > 5.0)
                    .map(|r| r.position_seconds)
                    .unwrap_or(0.0);

                // Get stream URL if already saved in DB
                let saved_url = ep_record.and_then(|r| r.stream_url);

                let anime_id  = entry.id.clone();
                let ep_clone  = ep_str.clone();
                let tx        = self.msg_tx.clone();
                let mode      = self.stream_mode.clone();
                let quality   = self.stream_quality.clone();
                let db        = self.db.clone();

                self.toast_info(format!("Loading Ep {ep_str}…"));

                if let Some(url) = saved_url {
                    // Already have URL — send directly to main loop
                    let _ = self.msg_tx.send(AppMsg::LaunchMpv {
                        url,
                        anime_id: anime_id.clone(),
                        episode:  ep_clone.clone(),
                        resume_from,
                    });
                } else {
                    // Resolve URL first, then send LaunchMpv
                    let ep_num: u32 = ep_str.parse().unwrap_or(1);
                    let tx_app = tx.clone();
                    tokio::spawn(async move {
                        match player::stream_anime(&anime_id, ep_num, &mode, &quality).await {
                            Ok(url) => {
                                let rec = crate::db::history::EpisodeRecord {
                                    anime_id:         anime_id.clone(),
                                    episode_number:   ep_clone.clone(),
                                    stream_url:       Some(url.clone()),
                                    watched:          true,
                                    watch_timestamp:  Some(chrono::Utc::now()),
                                    position_seconds: 0.0,
                                    duration_seconds: 0.0,
                                    fully_watched:    false,
                                };
                                let _ = db.save_episode(&rec);
                                let _ = tx_app.send(AppMsg::LaunchMpv {
                                    url,
                                    anime_id,
                                    episode: ep_clone,
                                    resume_from,
                                });
                            }
                            Err(e) => { let _ = tx_app.send(AppMsg::Error(e.to_string())); }
                        }
                    });
                }

                // Update progress in DB and in memory
                let ep_num: u32 = ep_str.parse().unwrap_or(0);
                if ep_num > 0 {
                    let _ = self.db.update_progress(&entry.id, ep_num);
                    if let Some(pos) = self.history.iter().position(|e| e.id == entry.id) {
                        self.history[pos].progress = Some(ep_num);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Rebuild history_filtered from history_filter using fuzzy match.
    pub fn rebuild_history_filter(&mut self) {
        if self.history_filter.is_empty() {
            self.history_filtered.clear();
            return;
        }
        let needle: Vec<char> = self.history_filter.to_lowercase().chars().collect();
        self.history_filtered = self.history.iter().enumerate()
            .filter(|(_, e)| fuzzy_match(&e.title.to_lowercase(), &needle))
            .map(|(i, _)| i)
            .collect();
    }

    /// Get the currently selected HistoryEntry, respecting the filter.
    pub fn history_selected(&self) -> Option<&HistoryEntry> {
        if self.history_filter.is_empty() {
            self.history.get(self.history_idx)
        } else {
            self.history_filtered.get(self.history_idx)
                .and_then(|&i| self.history.get(i))
        }
    }

    // ── Navigation helpers ────────────────────────────────────────────────────

    fn results_up(&mut self) {
        if self.results_idx > 0 {
            self.results_idx -= 1;
            let item = self.results[self.results_idx].clone();
            self.load_detail(item);
        }
    }

    async fn results_down(&mut self) {
        if self.results_idx + 1 < self.results.len() {
            self.results_idx += 1;
            let item = self.results[self.results_idx].clone();
            self.load_detail(item);

            // Prefetch images 2-3 positions ahead of current
            let ahead_start = self.results_idx + 1;
            let ahead_end   = (self.results_idx + 3).min(self.results.len());
            if ahead_start < ahead_end {
                self.prefetch_images(ahead_start, ahead_end);
            }

            // Speculatively fetch page 2 when user reaches result #15
            if self.results_idx == 14 && self.has_more && !self.is_searching {
                self.load_next_page().await;
            }
        } else if self.has_more && !self.is_searching {
            self.load_next_page().await;
        }
    }

    // ── Tab switch ────────────────────────────────────────────────────────────

    pub fn switch_tab(&mut self, tab: Tab) {
        self.active_tab  = tab.clone();
        self.results.clear();
        self.selected    = None;
        self.cover_protocol = None;
        self.episode_list.clear();
        self.anime_episode_records.clear();
        self.search_input.clear();
        self.search_cursor = 0;
        self.current_page = 1;
        self.has_more    = false;
        // Clear history filter when switching tabs
        self.history_filter.clear();
        self.history_filtered.clear();
        self.history_cover = None;
        self.history_cover_id = None;
        self.history_episode_list.clear();
        self.history_ep_window_records.clear();
        self.history_episode_idx = 0;
                        self.history_ep_window_start = 0;
                        self.history_ep_window_end   = 0;
        self.history_episodes_loading = false;
        self.history_ep_window_start = 0;
        self.history_ep_window_end   = 0;
        self.focus = if tab == Tab::History { Focus::History } else { Focus::Search };
        self.status = format!("{tab}  •  type to search");
    }

    // ── Search ────────────────────────────────────────────────────────────────

    async fn do_search(&mut self, page: u32) {
        let query = self.search_input.trim().to_string();
        if query.is_empty() { return; }

        self.is_searching  = true;
        self.current_page  = page;
        self.search_gen   += 1;
        let gen            = self.search_gen;

        if page == 1 {
            self.results.clear();
            self.selected    = None;
            self.status = format!("Searching \"{query}\"…");
        } else {
            self.status = format!("Loading page {page}…");
        }

        let tx  = self.msg_tx.clone();
        let tab = self.active_tab.clone();
        let mode = self.stream_mode.clone();

        tokio::spawn(async move {
            let res: anyhow::Result<Vec<ContentItem>> = match tab {
                Tab::Anime   => allanime::search_allanime(&query, &mode).await
                                    .map(|v| v.into_iter().map(ContentItem::Anime).collect()),
                Tab::History => Ok(vec![]),
            };
            match res {
                Ok(items) => {
                    if page == 1 {
                        let _ = tx.send(AppMsg::SearchResults { items, gen });
                    } else {
                        let _ = tx.send(AppMsg::MoreResults(items));
                    }
                }
                Err(e) => { let _ = tx.send(AppMsg::Error(e.to_string())); }
            }
        });
    }

    async fn load_next_page(&mut self) {
        self.do_search(self.current_page + 1).await;
    }

    // ── Detail ────────────────────────────────────────────────────────────────

    fn load_detail(&mut self, item: ContentItem) {
        let item_id = item.id().to_string();

        // ── Cover image ───────────────────────────────────────────────────────
        self.cover_protocol = None;

        // Fast path: decoded pixels already available — build protocol immediately, no decode, no blank frame
        if self.rgba_cache.contains_key(&item_id) {
            self.build_cover_protocol(&item_id);
        } else if let Some(url) = item.cover_url().map(String::from) {
            if let Some(cached_bytes) = self.image_cache.get(&url).cloned() {
                // Byte cache hit — bytes available, but need to decode (off main thread)
                let tx = self.msg_tx.clone();
                let id = item_id.clone();
                tokio::spawn(async move {
                    if let Ok(img) = image::load_from_memory(&cached_bytes) {
                        let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
                        let _ = tx.send(AppMsg::ImageDecoded { id, image: dyn_img });
                    }
                });
            } else {
                // Full cache miss — fetch, then decode
                let tx = self.msg_tx.clone();
                let id = item_id.clone();
                tokio::spawn(async move {
                    let client = reqwest::Client::builder()
                        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
                        .timeout(Duration::from_secs(15))
                        .connect_timeout(Duration::from_secs(8))
                        .build().unwrap_or_default();
                    for attempt in 0..2u8 {
                        match client.get(&url).header("Accept", "image/*").send().await {
                            Ok(r) if r.status().is_success() => {
                                match r.bytes().await {
                                    Ok(b) => {
                                        let bytes = b.to_vec();
                                        let _ = tx.send(AppMsg::ImageFetched {
                                            url: url.clone(),
                                            item_id: id.clone(),
                                            bytes: bytes.clone(),
                                        });
                                        // Also decode immediately for this item
                                        if let Ok(img) = image::load_from_memory(&bytes) {
                                            let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
                                            let _ = tx.send(AppMsg::ImageDecoded { id, image: dyn_img });
                                        }
                                        return;
                                    }
                                    Err(_) if attempt == 0 => continue,
                                    Err(e) => { let _ = tx.send(AppMsg::Error(format!("Image bytes: {e}"))); return; }
                                }
                            }
                            Ok(_) if attempt == 0 => continue,
                            Ok(r) => { let _ = tx.send(AppMsg::Error(format!("Image HTTP {}", r.status()))); return; }
                            Err(_) if attempt == 0 => {
                                tokio::time::sleep(Duration::from_millis(500)).await;
                                continue;
                            }
                            Err(e) => { let _ = tx.send(AppMsg::Error(format!("Image fetch: {e}"))); return; }
                        }
                    }
                });
            }
        }

        // ── Episode list — serve from cache or fetch ──────────────────────────
        let cached = self.detail_cache.get(&item_id).cloned();

        if let Some(ref c) = cached {
            if let Some(ref eps) = c.episode_list {
                self.episode_list = eps.clone();
                self.episode_list_idx = 0;
            }
        } else {
            self.episode_list.clear();

            // Episode list (Anime only)
            if matches!(item, ContentItem::Anime(_)) {
                let tx4  = self.msg_tx.clone();
                let id4  = item_id.clone();
                let mode = self.stream_mode.clone();
                tokio::spawn(async move {
                    if let Ok(eps) = player::fetch_episode_list(&id4, &mode).await {
                        let _ = tx4.send(AppMsg::EpisodeList { id: id4, eps });
                    }
                });
            }
        }

        let _ = self.msg_tx.send(AppMsg::DetailLoaded(item));

        // ── Load per-episode progress from DB for the progress fill display ───
        self.anime_episode_records.clear();
        let db  = self.db.clone();
        let tx  = self.msg_tx.clone();
        let aid = item_id.clone();
        tokio::spawn(async move {
            if let Ok(recs) = db.load_episodes(&aid) {
                if !recs.is_empty() {
                    let _ = tx.send(AppMsg::AnimeEpisodeRecords { anime_id: aid, records: recs });
                }
            }
        });
    }

    // ── Playback ──────────────────────────────────────────────────────────────

    pub async fn resolve_and_play(&mut self) {
        if self.selected.is_none() {
            self.toast_info("Select something first");
            return;
        }
        self.episode_input = String::from("1");
        self.episode_cursor = 1;
        self.focus = Focus::EpisodePrompt;
    }

    async fn key_episode_prompt(&mut self, key: KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let cols  = self.episode_cols.max(1);

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.episode_list_idx = self.episode_list_idx.saturating_sub(cols);
                if let Some(ep) = self.episode_list.get(self.episode_list_idx) {
                    self.episode_input = ep.clone();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let next = (self.episode_list_idx + cols).min(self.episode_list.len().saturating_sub(1));
                self.episode_list_idx = next;
                if let Some(ep) = self.episode_list.get(self.episode_list_idx) {
                    self.episode_input = ep.clone();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.episode_list_idx > 0 {
                    self.episode_list_idx -= 1;
                    if let Some(ep) = self.episode_list.get(self.episode_list_idx) {
                        self.episode_input = ep.clone();
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.episode_list_idx + 1 < self.episode_list.len() {
                    self.episode_list_idx += 1;
                    if let Some(ep) = self.episode_list.get(self.episode_list_idx) {
                        self.episode_input = ep.clone();
                    }
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.episode_input.insert(self.episode_cursor, c);
                self.episode_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.episode_cursor > 0 {
                    self.episode_cursor -= 1;
                    self.episode_input.remove(self.episode_cursor);
                }
            }
            // Toggle sub/dub with Tab
            KeyCode::Tab => {
                self.stream_mode = if self.stream_mode == "sub" {
                    "dub".to_string()
                } else {
                    "sub".to_string()
                };
            }
            // Cycle quality with Ctrl+Q (was bare 'q' — conflicted with global quit)
            KeyCode::Char('q') if ctrl => {
                self.stream_quality = match self.stream_quality.as_str() {
                    "best" => "1080".to_string(),
                    "1080" => "720".to_string(),
                    "720"  => "480".to_string(),
                    _      => "best".to_string(),
                };
            }
            KeyCode::Enter => {
                let ep_str = self.episode_input.trim().to_string();
                let ep: u32 = ep_str.parse().unwrap_or(1);
                self.focus = Focus::Detail;
                if let Some(item) = self.selected.clone() {
                    let tx   = self.msg_tx.clone();
                    let db   = self.db.clone();
                    let mode    = self.stream_mode.clone();
                    let quality = self.stream_quality.clone();
                    let id = match &item {
                        ContentItem::Anime(a) => a.id.clone(),
                    };

                    // Look up resume position from cached episode records
                    let resume_from = self.anime_episode_records
                        .get(&ep_str)
                        .filter(|r| !r.fully_watched && r.position_seconds > 5.0)
                        .map(|r| r.position_seconds)
                        .unwrap_or(0.0);

                    if resume_from > 5.0 {
                        let mins = (resume_from / 60.0) as u64;
                        let secs = (resume_from as u64) % 60;
                        self.toast_info(format!("Resuming Ep {ep} from {mins}:{secs:02}…"));
                    } else {
                        self.toast_info(format!("Fetching ep {ep} [{mode}]…"));
                    }

                    tokio::spawn(async move {
                        match player::stream_anime(&id, ep, &mode, &quality).await {
                            Ok(url) => {
                                // Save anime to history
                                let entry = HistoryEntry::from_content(&item);
                                let _ = db.save(&entry);
                                // Save episode record
                                let rec = crate::db::history::EpisodeRecord {
                                    anime_id:         entry.id.clone(),
                                    episode_number:   ep.to_string(),
                                    stream_url:       Some(url.clone()),
                                    watched:          true,
                                    watch_timestamp:  Some(chrono::Utc::now()),
                                    position_seconds: 0.0,
                                    duration_seconds: 0.0,
                                    fully_watched:    false,
                                };
                                let _ = db.save_episode(&rec);
                                // Use LaunchMpv so tracking works (anime_id + episode filled in)
                                let ep_num = ep;
                                let _ = db.update_progress(&entry.id, ep_num);
                                let _ = tx.send(AppMsg::LaunchMpv {
                                    url,
                                    anime_id: entry.id,
                                    episode:  ep.to_string(),
                                    resume_from,
                                });
                            }
                            Err(e) => { let _ = tx.send(AppMsg::Error(e.to_string())); }
                        }
                    });
                }
            }
            KeyCode::Esc => { self.focus = Focus::Detail; }
            _ => {}
        }
        Ok(())
    }

    // ── Toast helpers ─────────────────────────────────────────────────────────

    pub fn toast_info(&mut self, msg: impl Into<String>) {
        self.push_toast(Toast::info(msg));
    }
    pub fn toast_success(&mut self, msg: impl Into<String>) {
        self.push_toast(Toast::success(msg));
    }
    pub fn toast_error(&mut self, msg: impl Into<String>) {
        self.push_toast(Toast::error(msg));
    }
    fn push_toast(&mut self, t: Toast) {
        self.toasts.push(t);
        if self.toasts.len() > 4 { self.toasts.remove(0); }
    }

    pub fn on_resize(&mut self) {
        // Rebuild the cover protocol at the new terminal size.
        // rgba_cache already has decoded pixels — just recreate the protocol, no re-decode.
        if let Some(id) = self.selected.as_ref().map(|s| s.id().to_string()) {
            self.cover_protocol = None;
            self.build_cover_protocol(&id);
        }
    }
}

// ── Protocol detection ────────────────────────────────────────────────────────

fn detect_image_protocol() -> ImageProtocol {
    let term    = std::env::var("TERM").unwrap_or_default();
    let prog    = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let kitty   = std::env::var("KITTY_WINDOW_ID").is_ok();

    if kitty || term.contains("kitty") || prog.contains("WezTerm") || prog.contains("iTerm") {
        ImageProtocol::Kitty
    } else {
        ImageProtocol::HalfBlock
    }
}

// ── Fuzzy match ───────────────────────────────────────────────────────────────

/// Returns true if every char in `needle` appears in `haystack` in order.
fn fuzzy_match(haystack: &str, needle: &[char]) -> bool {
    if needle.is_empty() { return true; }
    let mut it = haystack.chars();
    let mut ni = 0;
    for c in it.by_ref() {
        if c == needle[ni] {
            ni += 1;
            if ni == needle.len() { return true; }
        }
    }
    false
}