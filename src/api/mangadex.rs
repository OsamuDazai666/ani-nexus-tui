use anyhow::Result;
use serde::{Deserialize, Serialize};

const MANGADEX_BASE: &str = "https://api.mangadex.org";
const MANGADEX_COVERS: &str = "https://uploads.mangadex.org/covers";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MangaItem {
    pub id: String,
    pub title: Option<String>,
    pub cover_url: Option<String>,
    pub synopsis: Option<String>,
    pub year: Option<i32>,
    pub chapters: Option<u32>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub author: Option<String>,
}

// ── Raw shapes ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MangaListResponse {
    data: Vec<RawManga>,
}

#[derive(Deserialize)]
struct RawManga {
    id: String,
    attributes: RawMangaAttr,
    relationships: Vec<RawRelationship>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMangaAttr {
    title: serde_json::Value,
    description: serde_json::Value,
    year: Option<i32>,
    last_chapter: Option<String>,
    status: Option<String>,
    tags: Vec<RawTag>,
}

#[derive(Deserialize)]
struct RawTag {
    attributes: RawTagAttr,
}

#[derive(Deserialize)]
struct RawTagAttr {
    name: serde_json::Value,
}

#[derive(Deserialize)]
struct RawRelationship {
    #[serde(rename = "type")]
    rel_type: String,
    attributes: Option<serde_json::Value>,
    id: String,
}

// ── Conversion ─────────────────────────────────────────────────────────────

fn raw_to_item(r: RawManga) -> MangaItem {
    // Title — prefer English, fallback to first available
    let title = r.attributes.title.as_object().and_then(|m| {
        m.get("en")
            .or_else(|| m.values().next())
            .and_then(|v| v.as_str())
            .map(String::from)
    });

    // Synopsis
    let synopsis = r.attributes.description.as_object().and_then(|m| {
        m.get("en")
            .or_else(|| m.values().next())
            .and_then(|v| v.as_str())
            .map(String::from)
    });

    // Genres from tags
    let genres = r
        .attributes
        .tags
        .iter()
        .filter_map(|t| {
            t.attributes.name.as_object().and_then(|m| {
                m.get("en")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
        })
        .collect();

    // Cover art from relationships
    let cover_rel = r
        .relationships
        .iter()
        .find(|rel| rel.rel_type == "cover_art");
    let cover_url = cover_rel.and_then(|rel| {
        rel.attributes
            .as_ref()?
            .get("fileName")?
            .as_str()
            .map(|f| format!("{MANGADEX_COVERS}/{}/{f}", r.id))
    });

    // Author
    let author = r
        .relationships
        .iter()
        .find(|rel| rel.rel_type == "author")
        .and_then(|rel| {
            rel.attributes
                .as_ref()?
                .get("name")?
                .as_str()
                .map(String::from)
        });

    let chapters = r
        .attributes
        .last_chapter
        .as_deref()
        .and_then(|c| c.parse::<u32>().ok());

    MangaItem {
        id: r.id,
        title,
        cover_url,
        synopsis,
        year: r.attributes.year,
        chapters,
        genres,
        status: r.attributes.status,
        author,
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

pub async fn search_manga(query: &str) -> Result<Vec<MangaItem>> {
    let url = format!(
        "{MANGADEX_BASE}/manga?title={query}&limit=25&includes[]=cover_art&includes[]=author&order[relevance]=desc",
        query = urlencoding::encode(query)
    );
    let resp: MangaListResponse = reqwest::get(&url).await?.json().await?;
    Ok(resp.data.into_iter().map(raw_to_item).collect())
}
