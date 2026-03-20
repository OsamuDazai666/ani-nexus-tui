pub mod allanime;
pub mod mangadex;
pub mod tmdb;

use serde::{Deserialize, Serialize};

/// Unified enum wrapping all content sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentItem {
    Anime(allanime::AllAnimeItem),
    Movie(tmdb::TmdbItem),
    TV(tmdb::TmdbItem),
    Manga(mangadex::MangaItem),
}

/// What kind of media this is (for display/player logic)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaType {
    Anime,
    Movie,
    TV,
    Manga,
}

/// Common trait-like methods via impl
impl ContentItem {
    pub fn id(&self) -> &str {
        match self {
            ContentItem::Anime(a) => &a.id,
            ContentItem::Movie(m) | ContentItem::TV(m) => &m.id,
            ContentItem::Manga(m) => &m.id,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            ContentItem::Anime(a) => a.display_title(),
            ContentItem::Movie(m) | ContentItem::TV(m) => m.title.as_deref().unwrap_or("Unknown"),
            ContentItem::Manga(m) => m.title.as_deref().unwrap_or("Unknown"),
        }
    }

    pub fn cover_url(&self) -> Option<&str> {
        match self {
            ContentItem::Anime(a) => a.thumbnail.as_deref(),
            ContentItem::Movie(m) | ContentItem::TV(m) => m.poster_url.as_deref(),
            ContentItem::Manga(m) => m.cover_url.as_deref(),
        }
    }

    pub fn synopsis(&self) -> &str {
        match self {
            ContentItem::Anime(a) => a.description.as_deref().unwrap_or("No synopsis available."),
            ContentItem::Movie(m) | ContentItem::TV(m) => m.overview.as_deref().unwrap_or("No overview available."),
            ContentItem::Manga(m) => m.synopsis.as_deref().unwrap_or("No synopsis available."),
        }
    }

    pub fn score(&self) -> Option<f32> {
        match self {
            ContentItem::Anime(a) => a.score,
            ContentItem::Movie(m) | ContentItem::TV(m) => m.vote_average,
            ContentItem::Manga(_m) => None,
        }
    }

    pub fn year(&self) -> Option<i32> {
        match self {
            ContentItem::Anime(a) => a.year,
            ContentItem::Movie(m) | ContentItem::TV(m) => m.year,
            ContentItem::Manga(m) => m.year,
        }
    }

    pub fn genres(&self) -> Vec<&str> {
        match self {
            ContentItem::Anime(a) => a.genres.iter().map(String::as_str).collect(),
            ContentItem::Movie(m) | ContentItem::TV(m) => m.genres.iter().map(String::as_str).collect(),
            ContentItem::Manga(m) => m.genres.iter().map(String::as_str).collect(),
        }
    }

    pub fn episodes_or_chapters(&self) -> Option<String> {
        match self {
            ContentItem::Anime(a) => {
                let sub = a.episodes_sub;
                let dub = a.episodes_dub;
                match (sub, dub) {
                    (0, 0) => None,
                    (s, 0) => Some(format!("{s} eps (SUB)")),
                    (0, d) => Some(format!("{d} eps (DUB)")),
                    (s, d) => Some(format!("{s} eps (SUB) · {d} eps (DUB)")),
                }
            }
            ContentItem::Movie(_) => None,
            ContentItem::TV(m) => m.seasons.map(|s| format!("{s} seasons")),
            ContentItem::Manga(m) => m.chapters.map(|c| format!("{c} chapters")),
        }
    }

    pub fn status(&self) -> Option<&str> {
        match self {
            ContentItem::Anime(a) => a.status.as_deref(),
            ContentItem::Movie(m) | ContentItem::TV(m) => m.status.as_deref(),
            ContentItem::Manga(m) => m.status.as_deref(),
        }
    }

    pub fn media_type(&self) -> MediaType {
        match self {
            ContentItem::Anime(_) => MediaType::Anime,
            ContentItem::Movie(_) => MediaType::Movie,
            ContentItem::TV(_) => MediaType::TV,
            ContentItem::Manga(_) => MediaType::Manga,
        }
    }

    pub fn source_badge(&self) -> &str {
        match self {
            ContentItem::Anime(_) => "AllAnime",
            ContentItem::Movie(_) => "TMDB",
            ContentItem::TV(_) => "TMDB",
            ContentItem::Manga(_) => "MangaDex",
        }
    }
}