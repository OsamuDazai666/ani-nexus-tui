use crate::{
    api::{allanime, mangadex, tmdb, ContentItem},
    db::history::{HistoryEntry, HistoryStore},
    player,
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};

// ── Tabs ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, strum::Display, strum::EnumIter)]
pub enum Tab { Anime, Movies, TV, Manga, History }

// ── Focus ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus { Search, Results, Detail, History, EpisodePrompt }

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
    ImageFetched { url: String, bytes: Vec<u8> },
    ImageLoaded(Vec<u8>),
    StreamUrl(String),
    EpisodeList { id: String, eps: Vec<String> },
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
    pub last_key:      Option<Instant>,   // debounce timer
    pub search_gen:    u64,               // generation counter — drop stale results
    pub current_page:  u32,              // for pagination

    // Results
    pub results:     Vec<ContentItem>,
    pub results_idx: usize,
    pub has_more:    bool,

    // Detail
    pub selected:     Option<ContentItem>,
    pub detail_scroll: u16,
    pub cover_image:  Option<Vec<u8>>,

    // History
    pub history:     Vec<HistoryEntry>,
    pub history_idx: usize,

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

    // ── In-memory caches ──────────────────────────────────────────────────────
    // Cover images keyed by URL — avoids re-fetching the same image when
    // scrolling back through results. Capped at IMAGE_CACHE_MAX entries (LRU).
    pub image_cache:  std::collections::HashMap<String, Vec<u8>>,
    image_cache_order: std::collections::VecDeque<String>,

    // Detail cache keyed by item ID — stores episode list + recs so revisiting
    // an already-seen result is instant. Capped at DETAIL_CACHE_MAX entries.
    pub detail_cache: std::collections::HashMap<String, CachedDetail>,
    detail_cache_order: std::collections::VecDeque<String>,
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
            last_key:      None,
            search_gen:    0,
            current_page:  1,
            results:       Vec::new(),
            results_idx:   0,
            has_more:      false,
            selected:      None,
            detail_scroll: 0,
            cover_image:   None,
            history,
            history_idx:   0,
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
            status:   "Type to search  •  F1-F5 switch tabs".to_string(),
            image_protocol: protocol,
            image_picker,
            cover_protocol: None,
            needs_redraw:  false,
            image_cache:       std::collections::HashMap::new(),
            image_cache_order: std::collections::VecDeque::new(),
            detail_cache:       std::collections::HashMap::new(),
            detail_cache_order: std::collections::VecDeque::new(),
        })
    }

    // ── Tick (called every ~100ms) ────────────────────────────────────────────

    pub async fn tick(&mut self) -> Result<()> {
        if self.is_searching { self.spinner.tick(); }
        self.toasts.retain(|t| t.alive());

        // Debounce: fire search 350ms after last keystroke
        if let Some(t) = self.last_key {
            if t.elapsed() >= Duration::from_millis(600)
                && !self.search_input.trim().is_empty()
                && self.focus == Focus::Search
            {
                self.last_key = None;
                self.do_search(1).await;
            }
        }

        // Drain messages
        let msgs: Vec<AppMsg> = {
            let mut rx = self.msg_rx.lock().await;
            let mut v = Vec::new();
            while let Ok(m) = rx.try_recv() { v.push(m); }
            v
        };
        for msg in msgs { self.handle_msg(msg); }

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
                    if self.last_key.is_none() { self.focus = Focus::Results; }
                    let first = self.results[0].clone();
                    self.load_detail(first);
                }
            }
            AppMsg::MoreResults(items) => {
                self.is_searching = false;
                self.has_more = items.len() >= 25;
                let added = items.len();
                self.results.extend(items);
                self.status = format!("{} results  (+{added} more)", self.results.len());
            }
            AppMsg::DetailLoaded(item) => { self.selected = Some(item); self.detail_scroll = 0; }

            // Image fetched from network — store in cache, then render if still selected
            AppMsg::ImageFetched { url, bytes } => {
                self.cache_image(url, bytes.clone());
                self.render_cover(bytes);
            }
            // Image served directly from cache (legacy path, kept for compat)
            AppMsg::ImageLoaded(bytes) => {
                self.render_cover(bytes);
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
                if let Err(e) = player::launch_mpv_url(&url) {
                    self.toast_error(format!("mpv: {e}"));
                }
                self.needs_redraw = true;
            }
            AppMsg::Error(e) => {
                self.is_searching = false;
                self.toast_error(e);
            }
        }
    }

    // ── Cache helpers ─────────────────────────────────────────────────────────

    fn render_cover(&mut self, bytes: Vec<u8>) {
        self.cover_protocol = None;
        if let Ok(home) = std::env::var("HOME") {
            let debug_dir = std::path::PathBuf::from(&home).join("Desktop").join("nexus-debug");
            let _ = std::fs::create_dir_all(&debug_dir);
            let name = self.selected.as_ref()
                .map(|s| s.title().replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_"))
                .unwrap_or_else(|| "unknown".to_string());
            let ext = if bytes.starts_with(b"\x89PNG") { "png" } else { "jpg" };
            let _ = std::fs::write(debug_dir.join(format!("{name}.{ext}")), &bytes);
        }
        if let Ok(img) = image::load_from_memory(&bytes) {
            let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
            self.cover_protocol = Some(self.image_picker.new_resize_protocol(dyn_img));
        }
        self.cover_image = Some(bytes);
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

        // Global: '/' jumps to search bar from anywhere except Search itself
        if key.code == KeyCode::Char('/') && self.focus != Focus::Search {
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
                    };
                    return Ok(false);
                }
                KeyCode::Down => {
                    self.focus = match &self.focus {
                        Focus::Search   => { if !self.results.is_empty() { Focus::Results } else { Focus::Search } }
                        Focus::Results  => { if self.selected.is_some() { Focus::EpisodePrompt } else { Focus::Results } }
                        Focus::Detail   => { if self.selected.is_some() { Focus::EpisodePrompt } else { Focus::Detail } }
                        Focus::EpisodePrompt => Focus::EpisodePrompt,
                        Focus::History       => Focus::History,
                    };
                    return Ok(false);
                }
                KeyCode::Right => {
                    self.focus = match &self.focus {
                        Focus::Results         => { if self.selected.is_some() { Focus::Detail } else { Focus::Results } }
                        Focus::Detail          => Focus::Detail,
                        Focus::EpisodePrompt   => Focus::EpisodePrompt,
                        Focus::Search          => Focus::Search,
                        Focus::History         => Focus::History,
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
                    };
                    return Ok(false);
                }
                _ => {}
            }
        }

        // Tab switching — F1..F5
        match key.code {
            KeyCode::F(1) => { self.switch_tab(Tab::Anime);   return Ok(false); }
            KeyCode::F(2) => { self.switch_tab(Tab::Movies);  return Ok(false); }
            KeyCode::F(3) => { self.switch_tab(Tab::TV);      return Ok(false); }
            KeyCode::F(4) => { self.switch_tab(Tab::Manga);   return Ok(false); }
            KeyCode::F(5) => { self.switch_tab(Tab::History); return Ok(false); }
            _ => {}
        }

        match self.focus.clone() {
            Focus::Search          => self.key_search(key).await?,
            Focus::Results         => self.key_results(key).await?,
            Focus::Detail          => self.key_detail(key).await?,
            Focus::History         => self.key_history(key).await?,
            Focus::EpisodePrompt   => self.key_episode_prompt(key).await?,
        }
        Ok(false)
    }

    async fn key_search(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char(c) => {
                self.search_input.insert(self.search_cursor, c);
                self.search_cursor += 1;
                self.last_key = Some(Instant::now());
            }
            KeyCode::Backspace => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                    self.search_input.remove(self.search_cursor);
                    self.last_key = Some(Instant::now());
                }
            }
            KeyCode::Delete => {
                if self.search_cursor < self.search_input.len() {
                    self.search_input.remove(self.search_cursor);
                    self.last_key = Some(Instant::now());
                }
            }
            KeyCode::Left  => { if self.search_cursor > 0 { self.search_cursor -= 1; } }
            KeyCode::Right => { if self.search_cursor < self.search_input.len() { self.search_cursor += 1; } }
            KeyCode::Home  => { self.search_cursor = 0; }
            KeyCode::End   => { self.search_cursor = self.search_input.len(); }
            KeyCode::Enter => {
                self.last_key = None;
                if !self.search_input.trim().is_empty() {
                    self.do_search(1).await;
                }
            }
            KeyCode::Esc | KeyCode::Tab | KeyCode::Down => {
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
            KeyCode::Up   | KeyCode::Char('k') => { if self.history_idx > 0 { self.history_idx -= 1; } }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.history_idx + 1 < self.history.len() { self.history_idx += 1; }
            }
            KeyCode::Delete | KeyCode::Char('d') => {
                if let Some(e) = self.history.get(self.history_idx).cloned() {
                    let _ = self.db.remove(&e.id);
                    self.toast_info(format!("Removed \"{}\"", e.title));
                    self.history.remove(self.history_idx);
                    if self.history_idx > 0 { self.history_idx -= 1; }
                }
            }
            KeyCode::Char('p') => {
                if let Some(e) = self.history.get(self.history_idx).cloned() {
                    if let Some(url) = &e.stream_url {
                        let url = url.clone();
                        if let Err(err) = player::launch_mpv_url(&url) {
                            self.toast_error(err.to_string());
                        } else {
                            self.toast_success("Launching mpv…");
                        }
                    } else {
                        self.toast_info("No stream URL saved for this entry");
                    }
                }
            }
            _ => {}
        }
        Ok(())
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
        } else if self.has_more && !self.is_searching {
            // Auto-load more when reaching the end
            self.load_next_page().await;
        }
    }

    // ── Tab switch ────────────────────────────────────────────────────────────

    pub fn switch_tab(&mut self, tab: Tab) {
        self.active_tab  = tab.clone();
        self.results.clear();
        self.selected    = None;
        self.cover_image = None;
        self.cover_protocol = None;
        self.episode_list.clear();
        self.search_input.clear();
        self.search_cursor = 0;
        self.last_key    = None;
        self.current_page = 1;
        self.has_more    = false;
        self.focus = if tab == Tab::History { Focus::History } else { Focus::Search };
        self.status = format!("{tab}  •  type to search");
        // Note: image_cache and detail_cache intentionally kept across tab switches
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
            self.cover_image = None;
            self.status = format!("Searching \"{query}\"…");
        } else {
            self.status = format!("Loading page {page}…");
        }

        let tx  = self.msg_tx.clone();
        let tab = self.active_tab.clone();
        let mode = self.stream_mode.clone();

        tokio::spawn(async move {
            let res: anyhow::Result<Vec<ContentItem>> = match tab {
                Tab::Anime  => allanime::search_allanime(&query, &mode).await
                                   .map(|v| v.into_iter().map(ContentItem::Anime).collect()),
                Tab::Movies => tmdb::search_movies(&query).await
                                   .map(|v| v.into_iter().map(ContentItem::Movie).collect()),
                Tab::TV     => tmdb::search_tv(&query).await
                                   .map(|v| v.into_iter().map(ContentItem::TV).collect()),
                Tab::Manga  => mangadex::search_manga(&query).await
                                   .map(|v| v.into_iter().map(ContentItem::Manga).collect()),
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
        self.cover_image    = None;

        if let Some(url) = item.cover_url().map(String::from) {
            if let Some(cached) = self.image_cache.get(&url).cloned() {
                // Cache hit — render immediately, no network call
                self.render_cover(cached);
            } else {
                // Cache miss — fetch and store
                let tx = self.msg_tx.clone();
                let url_clone = url.clone();
                tokio::spawn(async move {
                    let client = reqwest::Client::builder()
                        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
                        .timeout(Duration::from_secs(15))
                        .connect_timeout(Duration::from_secs(8))
                        .build().unwrap_or_default();
                    for attempt in 0..2u8 {
                        match client.get(&url_clone).header("Accept", "image/*").send().await {
                            Ok(r) if r.status().is_success() => {
                                match r.bytes().await {
                                    Ok(b) => {
                                        let _ = tx.send(AppMsg::ImageFetched {
                                            url: url_clone,
                                            bytes: b.to_vec(),
                                        });
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
    }

    // ── Playback ──────────────────────────────────────────────────────────────

    pub async fn resolve_and_play(&mut self) {
        let Some(item) = self.selected.clone() else {
            self.toast_info("Select something first");
            return;
        };
        // For anime, show episode prompt
        if matches!(item, ContentItem::Anime(_)) {
            self.episode_input = String::from("1");
            self.episode_cursor = 1;
            self.focus = Focus::EpisodePrompt;
            return;
        }
        self.do_play(item).await;
    }

    pub async fn do_play(&mut self, item: ContentItem) {
        self.toast_info("Resolving stream…");
        self.is_searching = true;
        self.status = "Resolving stream…".to_string();
        
        let tx = self.msg_tx.clone();
        let db = self.db.clone();
        tokio::spawn(async move {
            match player::resolve_stream(&item).await {
                Ok(url) => {
                    let mut entry = HistoryEntry::from_content(&item);
                    entry.stream_url = Some(url.clone());
                    let _ = db.save(&entry);
                    let _ = tx.send(AppMsg::StreamUrl(url));
                }
                Err(e) => { let _ = tx.send(AppMsg::Error(format!("Stream: {e}"))); }
            }
        });
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
                        _ => String::new(),
                    };
                    self.toast_info(format!("Fetching ep {ep} [{mode}]…"));
                    tokio::spawn(async move {
                        match player::stream_anime(&id, ep, &mode, &quality).await {
                            Ok(url) => {
                                let mut entry = HistoryEntry::from_content(&item);
                                entry.stream_url = Some(url.clone());
                                entry.progress   = Some(ep);
                                let _ = db.save(&entry);
                                let _ = tx.send(AppMsg::StreamUrl(url));
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
        // Don't re-query stdio — we're already in raw mode.
        // Just re-create the cover protocol so it re-renders at the new size.
        if let Some(bytes) = self.cover_image.clone() {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let dyn_img = image::DynamicImage::ImageRgba8(img.into_rgba8());
                self.cover_protocol = Some(self.image_picker.new_resize_protocol(dyn_img));
            }
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