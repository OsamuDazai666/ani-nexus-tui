use crate::app::Tab;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

const TMDB_BASE: &str = "https://api.themoviedb.org/3";
const TMDB_IMAGE: &str = "https://image.tmdb.org/t/p/w1280";

/// Read TMDB_API_KEY from environment or config
fn api_key() -> Result<String> {
    std::env::var("TMDB_API_KEY").map_err(|_| {
        anyhow!(
            "TMDB_API_KEY not set.\n\
             Get a free key at https://www.themoviedb.org/settings/api\n\
             Then: export TMDB_API_KEY=\"your_key\""
        )
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbItem {
    pub id: String,
    pub title: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub overview: Option<String>,
    pub vote_average: Option<f32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub seasons: Option<u32>,
    pub runtime: Option<u32>,
    pub tagline: Option<String>,
}

// ── Raw API shapes ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PagedResponse {
    results: Vec<RawTmdb>,
}

#[derive(Deserialize)]
struct RawGenreList {
    genres: Vec<RawGenre>,
}

#[derive(Deserialize)]
struct RawGenre {
    id: u64,
    name: String,
}

#[derive(Deserialize)]
struct RawTmdb {
    id: u64,
    title: Option<String>,           // movies
    name: Option<String>,            // tv
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    overview: Option<String>,
    vote_average: Option<f32>,
    release_date: Option<String>,    // "YYYY-MM-DD"
    first_air_date: Option<String>,  // tv
    genre_ids: Option<Vec<u64>>,
}

// ── Conversion ──────────────────────────────────────────────────────────────

fn raw_to_item(r: RawTmdb, genre_map: &[(u64, String)]) -> TmdbItem {
    let title = r.title.or(r.name);
    let poster_url = r.poster_path.as_ref().map(|p| format!("{TMDB_IMAGE}{p}"));
    let backdrop_url = r.backdrop_path.as_ref().map(|p| format!("{TMDB_IMAGE}{p}"));
    let year = r
        .release_date
        .as_deref()
        .or(r.first_air_date.as_deref())
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse().ok());
    let genres = r
        .genre_ids
        .unwrap_or_default()
        .iter()
        .filter_map(|gid| {
            genre_map
                .iter()
                .find(|(id, _)| id == gid)
                .map(|(_, n)| n.clone())
        })
        .collect();

    TmdbItem {
        id: r.id.to_string(),
        title,
        poster_url,
        backdrop_url,
        overview: r.overview,
        vote_average: r.vote_average,
        year,
        genres,
        status: None,
        seasons: None,
        runtime: None,
        tagline: None,
    }
}

// ── Genre map (async, cached per-process via OnceCell) ──────────────────────

use tokio::sync::OnceCell as AsyncOnceCell;

static MOVIE_GENRE_MAP: AsyncOnceCell<Vec<(u64, String)>> = AsyncOnceCell::const_new();
static TV_GENRE_MAP: AsyncOnceCell<Vec<(u64, String)>> = AsyncOnceCell::const_new();

async fn genre_map(key: &str, kind: &str) -> Vec<(u64, String)> {
    async fn fetch(key: &str, kind: &str) -> Vec<(u64, String)> {
        let url = format!("{TMDB_BASE}/genre/{kind}/list?api_key={key}&language=en-US");
        let Ok(resp) = reqwest::get(&url).await else { return vec![] };
        let Ok(gl) = resp.json::<RawGenreList>().await else { return vec![] };
        gl.genres.into_iter().map(|g| (g.id, g.name)).collect()
    }

    match kind {
        "tv" => {
            TV_GENRE_MAP
                .get_or_init(|| fetch(key, kind))
                .await
                .clone()
        }
        _ => {
            MOVIE_GENRE_MAP
                .get_or_init(|| fetch(key, kind))
                .await
                .clone()
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

pub async fn search_movies(query: &str) -> Result<Vec<TmdbItem>> {
    let key = api_key()?;
    let encoded = urlencoding::encode(query);
    let url = format!("{TMDB_BASE}/search/movie?api_key={key}&query={encoded}&page=1&language=en-US");
    let genres = genre_map(&key, "movie").await;
    let resp: PagedResponse = reqwest::get(&url).await?.json().await?;
    Ok(resp.results.into_iter().map(|r| raw_to_item(r, &genres)).collect())
}

pub async fn search_tv(query: &str) -> Result<Vec<TmdbItem>> {
    let key = api_key()?;
    let encoded = urlencoding::encode(query);
    let url = format!("{TMDB_BASE}/search/tv?api_key={key}&query={encoded}&page=1&language=en-US");
    let genres = genre_map(&key, "tv").await;
    let resp: PagedResponse = reqwest::get(&url).await?.json().await?;
    Ok(resp.results.into_iter().map(|r| raw_to_item(r, &genres)).collect())
}

pub async fn fetch_recommendations(id: &str, tab: &Tab) -> Result<Vec<TmdbItem>> {
    let key = api_key()?;
    let kind = match tab {
        Tab::TV => "tv",
        _ => "movie",
    };
    let url =
        format!("{TMDB_BASE}/{kind}/{id}/recommendations?api_key={key}&page=1&language=en-US");
    let genres = genre_map(&key, kind).await;
    let resp: PagedResponse = reqwest::get(&url).await?.json().await?;
    Ok(resp.results.into_iter().map(|r| raw_to_item(r, &genres)).collect())
}
