pub mod allanime;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentItem {
    Anime(allanime::AllAnimeItem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaType {
    Anime,
}

impl ContentItem {
    pub fn id(&self) -> &str {
        match self {
            ContentItem::Anime(a) => &a.id,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            ContentItem::Anime(a) => a.display_title(),
        }
    }

    pub fn cover_url(&self) -> Option<&str> {
        match self {
            ContentItem::Anime(a) => a.thumbnail.as_deref(),
        }
    }

    pub fn synopsis(&self) -> &str {
        match self {
            ContentItem::Anime(a) => a.description.as_deref().unwrap_or("No synopsis available."),
        }
    }

    pub fn score(&self) -> Option<f32> {
        match self {
            ContentItem::Anime(a) => a.score,
        }
    }

    pub fn year(&self) -> Option<i32> {
        match self {
            ContentItem::Anime(a) => a.year,
        }
    }

    pub fn genres(&self) -> Vec<&str> {
        match self {
            ContentItem::Anime(a) => a.genres.iter().map(String::as_str).collect(),
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
        }
    }

    pub fn status(&self) -> Option<&str> {
        match self {
            ContentItem::Anime(a) => a.status.as_deref(),
        }
    }

    pub fn media_type(&self) -> MediaType {
        match self {
            ContentItem::Anime(_) => MediaType::Anime,
        }
    }

    pub fn source_badge(&self) -> &str {
        match self {
            ContentItem::Anime(_) => "AllAnime",
        }
    }
}
