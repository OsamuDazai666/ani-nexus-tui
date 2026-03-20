//! AllAnime search API — single source for anime search + streaming.
//! Replaces AniList so search results, episode counts, and streaming all use the same catalog.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

const ALLANIME_API: &str = "https://api.allanime.day/api";
const ALLANIME_REFR: &str = "https://allmanga.to";
const AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/121.0";

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllAnimeItem {
    pub id: String,            // AllAnime _id — used directly for episodes + streaming
    pub name: String,          // romaji/primary name
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
    pub show_type: Option<String>,  // "TV", "Movie", "OVA", etc.
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
        let eps = r.available_episodes.unwrap_or(RawEpisodes { sub: None, dub: None });
        AllAnimeItem {
            id: r.id,
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
        query.replace('"', "\\\""), mode
    );

    let client = reqwest::Client::builder()
        .user_agent(AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert("Referer", reqwest::header::HeaderValue::from_static(ALLANIME_REFR));
            h
        })
        .build()
        .unwrap_or_default();

    let text = client.get(ALLANIME_API)
        .query(&[("variables", &vars), ("query", &gql.to_string())])
        .send().await?
        .text().await?;

    let resp: GqlResponse = serde_json::from_str(&text)
        .map_err(|e| anyhow!("AllAnime parse error: {e}"))?;

    let mut items: Vec<AllAnimeItem> = resp.data
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
fn rank_allanime(items: &mut Vec<AllAnimeItem>, query: &str) {
    let q = query.to_lowercase();
    items.sort_by(|a, b| {
        score_allanime(b, &q)
            .partial_cmp(&score_allanime(a, &q))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn score_allanime(item: &AllAnimeItem, q: &str) -> f32 {
    let name = item.name.to_lowercase();
    let eng  = item.english_name.as_deref().unwrap_or("").to_lowercase();

    let title_bonus = |t: &str| -> f32 {
        if t == q              { 300.0 }
        else if t.starts_with(q) { 150.0 }
        else if t.contains(q)    {  50.0 }
        else                     {   0.0 }
    };
    let tb = title_bonus(&name).max(title_bonus(&eng));

    let score = item.score.unwrap_or(0.0);
    let eps   = item.total_episodes() as f32;

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