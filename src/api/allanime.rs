//! AllAnime search API — single source for anime search + streaming.
//! Replaces AniList so search results, episode counts, and streaming all use the same catalog.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

const ALLANIME_API: &str = "https://api.allanime.day/api";
const AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/121.0";

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllAnimeItem {
    pub id: String,          // AllAnime _id — used directly for episodes + streaming
    pub mal_id: Option<u32>, // MyAnimeList ID — used for AniSkip
    pub name: String,        // romaji/primary name
    pub english_name: Option<String>,
    pub thumbnail: Option<String>,
    pub banner: Option<String>,
    pub episodes_sub: u32,
    pub episodes_dub: u32,
    pub year: Option<i32>,
    pub status: Option<String>,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub score: Option<f32>,
    pub studios: Vec<String>,
    pub show_type: Option<String>, // "TV", "Movie", "OVA", etc.
}

impl AllAnimeItem {
    /// Best display title: English if available, otherwise romaji name
    pub fn display_title(&self) -> &str {
        self.english_name.as_deref().unwrap_or(&self.name)
    }

    /// Total available episodes (sub or dub, whichever is higher)
    pub fn total_episodes(&self) -> u32 {
        self.episodes_sub.max(self.episodes_dub)
    }
}

// ── Raw GQL response types ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GqlResponse {
    data: Option<ShowsData>,
}

#[derive(Deserialize)]
struct ShowsData {
    shows: ShowsEdges,
}

#[derive(Deserialize)]
struct ShowsEdges {
    edges: Vec<RawShow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawShow {
    #[serde(rename = "_id")]
    id: String,
    name: Option<String>,
    english_name: Option<String>,
    thumbnail: Option<String>,
    banner: Option<String>,
    available_episodes: Option<RawEpisodes>,
    aired_start: Option<RawDate>,
    status: Option<String>,
    description: Option<String>,
    genres: Option<Vec<String>>,
    score: Option<f64>,
    studios: Option<Vec<String>>,
    #[serde(rename = "type")]
    show_type: Option<String>,
}

#[derive(Deserialize)]
struct RawEpisodes {
    sub: Option<u32>,
    dub: Option<u32>,
}

#[derive(Deserialize)]
struct RawDate {
    year: Option<i32>,
}

// ── Conversion ────────────────────────────────────────────────────────────────

impl From<RawShow> for AllAnimeItem {
    fn from(r: RawShow) -> Self {
        let eps = r.available_episodes.unwrap_or(RawEpisodes {
            sub: None,
            dub: None,
        });
        AllAnimeItem {
            id: r.id,
            mal_id: None, // resolved later via AniList
            name: r.name.unwrap_or_default(),
            english_name: r.english_name,
            thumbnail: r.thumbnail,
            banner: r.banner,
            episodes_sub: eps.sub.unwrap_or(0),
            episodes_dub: eps.dub.unwrap_or(0),
            year: r.aired_start.and_then(|d| d.year),
            status: r.status,
            description: r.description.map(|d| strip_html(&d)),
            genres: r.genres.unwrap_or_default(),
            score: r.score.map(|s| s as f32),
            studios: r.studios.unwrap_or_default(),
            show_type: r.show_type,
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub async fn search_allanime(query: &str, mode: &str) -> Result<Vec<AllAnimeItem>> {
    let gql = r#"query($search:SearchInput $limit:Int $page:Int $translationType:VaildTranslationTypeEnumType $countryOrigin:VaildCountryOriginEnumType){shows(search:$search limit:$limit page:$page translationType:$translationType countryOrigin:$countryOrigin){edges{_id name englishName thumbnail banner availableEpisodes airedStart status type description genres score studios}}}"#;

    let vars = format!(
        r#"{{"search":{{"allowAdult":false,"allowUnknown":false,"query":"{}"}},"limit":25,"page":1,"translationType":"{}","countryOrigin":"ALL"}}"#,
        query.replace('"', "\\\""),
        mode
    );

    // Use browser_auth which handles FlareSolverr -> visible browser fallback chain
    let text = crate::browser_auth::fetch_text_with_query(
        ALLANIME_API,
        &[
            ("variables".to_string(), vars),
            ("query".to_string(), gql.to_string()),
        ],
    )
    .await
    .map_err(|e| anyhow!("AllAnime request failed: {e}"))?;

    let resp: GqlResponse =
        serde_json::from_str(&text).map_err(|e| anyhow!("AllAnime parse error: {e}"))?;

    let mut items: Vec<AllAnimeItem> = resp
        .data
        .map(|d| d.shows.edges.into_iter().map(AllAnimeItem::from).collect())
        .unwrap_or_default();

    rank_allanime(&mut items, query);

    Ok(items)
}

// ── Ranking ───────────────────────────────────────────────────────────────────

/// Re-rank AllAnime results using:
///   title_match_bonus  (exact > prefix > contains > none)
/// + score × log2(total_episodes + 2)
///
/// Episode count is a popularity proxy — long-running, well-rated series rank
/// above obscure shorts with the same title fragment.
fn rank_allanime(items: &mut [AllAnimeItem], query: &str) {
    let q = query.to_lowercase();
    items.sort_by(|a, b| {
        score_allanime(b, &q)
            .partial_cmp(&score_allanime(a, &q))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn score_allanime(item: &AllAnimeItem, q: &str) -> f32 {
    let name = item.name.to_lowercase();
    let eng = item.english_name.as_deref().unwrap_or("").to_lowercase();

    let title_bonus = |t: &str| -> f32 {
        if t == q {
            300.0
        } else if t.starts_with(q) {
            150.0
        } else if t.contains(q) {
            50.0
        } else {
            0.0
        }
    };
    let tb = title_bonus(&name).max(title_bonus(&eng));

    let score = item.score.unwrap_or(0.0);
    let eps = item.total_episodes() as f32;

    tb + score * (eps + 2.0).log2()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&apos;", "'")
        .replace("&#x2014;", "—")
        .replace("<br>", "\n")
}

// ── MAL ID resolver via AniList ───────────────────────────────────────────────

/// Resolve a MyAnimeList ID for an anime title via AniList's GraphQL API.
/// Returns None if not found — skip feature degrades gracefully.
pub async fn resolve_mal_id(title: &str) -> Option<u32> {
    // Use Jikan (unofficial MAL API) — no auth needed, reliable
    let client = reqwest::Client::builder()
        .emulation(wreq_util::Emulation::Chrome140)
        .user_agent(AGENT)
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    let url = format!(
        "https://api.jikan.moe/v4/anime?q={}&limit=1&sfw=false",
        urlencoding::encode(title)
    );
    skip_log(&format!("Jikan search: {url}"));

    let resp = client.get(&url).send().await;
    match resp {
        Err(e) => {
            skip_log(&format!("Jikan error: {e}"));
            None
        }
        Ok(r) => {
            skip_log(&format!("Jikan status: {}", r.status()));
            let json: serde_json::Value = r.json().await.ok()?;
            let id = json["data"][0]["mal_id"].as_u64().map(|id| id as u32);
            skip_log(&format!("Jikan MAL ID: {id:?}"));
            id
        }
    }
}

// ── AniSkip fetch ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SkipInterval {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SkipTimes {
    pub intro: Option<SkipInterval>,
    pub outro: Option<SkipInterval>,
}

/// Fetch skip timestamps from AniSkip for a given MAL ID + episode.
pub async fn fetch_skip_times(mal_id: u32, episode: u32) -> Option<SkipTimes> {
    // episodeLength=0 tells AniSkip to return results for any episode length
    let url = format!(
        "https://api.aniskip.com/v2/skip-times/{mal_id}/{episode}?types[]=op&types[]=ed&types[]=mixed-op&types[]=mixed-ed&episodeLength=0"
    );
    skip_log(&format!("[nexus-skip] AniSkip URL: {url}"));

    let client = reqwest::Client::builder()
        .emulation(wreq_util::Emulation::Chrome140)
        .user_agent(AGENT)
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    let resp = client.get(&url).send().await;
    match &resp {
        Err(e) => {
            skip_log(&format!("[nexus-skip] AniSkip request error: {e}"));
            return None;
        }
        Ok(r) => skip_log(&format!("[nexus-skip] AniSkip status: {}", r.status())),
    }
    let resp = resp.ok()?;
    if !resp.status().is_success() {
        skip_log("[nexus-skip] AniSkip non-success status");
        return None;
    }

    let json: serde_json::Value = resp.json().await.ok()?;
    skip_log(&format!(
        "[nexus-skip] AniSkip found={} raw={}",
        json["found"].as_bool().unwrap_or(false),
        &json.to_string()[..300.min(json.to_string().len())]
    ));
    if !json["found"].as_bool().unwrap_or(false) {
        return None;
    }

    let results = json["results"].as_array()?;

    let mut times = SkipTimes::default();
    for result in results {
        let skip_type = result["skipType"].as_str().unwrap_or("");
        // Use if-let to skip malformed entries without aborting the whole fetch
        if let (Some(start), Some(end)) = (
            result["interval"]["startTime"].as_f64(),
            result["interval"]["endTime"].as_f64(),
        ) {
            let interval = SkipInterval { start, end };
            match skip_type {
                "op" | "mixed-op" => {
                    if times.intro.is_none() {
                        times.intro = Some(interval);
                    }
                }
                "ed" | "mixed-ed" => {
                    if times.outro.is_none() {
                        times.outro = Some(interval);
                    }
                }
                _ => {}
            }
        }
    }

    if times.intro.is_none() && times.outro.is_none() {
        return None;
    }

    Some(times)
}

// ── Skip debug log ────────────────────────────────────────────────────────────

/// Write skip debug messages to ~/.local/share/nexus-tui/skip.log
/// Never writes to stderr — won't leak into the TUI.
pub fn skip_log(msg: &str) {
    use std::io::Write;
    let path = dirs_log();
    if let Some(p) = path {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{ts}] {msg}");
        }
    }
}

fn dirs_log() -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    {
        let home = std::env::var("HOME").ok()?;
        let dir = std::path::PathBuf::from(home).join(".local/share/nexus-tui");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join("skip.log"))
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA").ok()?;
        let dir = std::path::PathBuf::from(appdata).join("nexus-tui");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join("skip.log"))
    }
    #[cfg(not(any(unix, windows)))]
    None
}
