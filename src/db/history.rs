use crate::api::ContentItem;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id:           String,
    pub title:        String,
    pub media_type:   String,
    pub cover_url:    Option<String>,
    pub stream_url:   Option<String>,        // last resolved stream
    pub last_watched: DateTime<Utc>,
    pub progress:     Option<u32>,           // episode / chapter
    pub total:        Option<u32>,
    pub play_count:   u32,
}

impl HistoryEntry {
    pub fn from_content(item: &ContentItem) -> Self {
        let total = match item {
            ContentItem::Anime(a) => {
                let eps = a.total_episodes();
                if eps > 0 { Some(eps) } else { None }
            }
        };
        Self {
            id:           item.id().to_string(),
            title:        item.title().to_string(),
            media_type:   format!("{:?}", item.media_type()),
            cover_url:    item.cover_url().map(String::from),
            stream_url:   None,
            last_watched: Utc::now(),
            progress:     None,
            total,
            play_count:   1,
        }
    }

    pub fn progress_pct(&self) -> Option<f64> {
        match (self.progress, self.total) {
            (Some(p), Some(t)) if t > 0 => Some((p as f64 / t as f64).clamp(0.0, 1.0)),
            _ => None,
        }
    }

    pub fn progress_bar(&self, width: usize) -> String {
        let pct = self.progress_pct().unwrap_or(0.0);
        let filled = (pct * width as f64) as usize;
        let empty  = width.saturating_sub(filled);
        format!("{}{}",
            "█".repeat(filled),
            "░".repeat(empty),
        )
    }
}

// ── Store ─────────────────────────────────────────────────────────────────────

pub struct HistoryStore { db: sled::Db }

impl HistoryStore {
    pub fn open() -> Result<Self> {
        let dir = directories::ProjectDirs::from("dev", "nexus", "nexus-tui")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from(".nexus-data"));
        std::fs::create_dir_all(&dir)?;
        let db = sled::open(dir.join("history.db"))?;
        Ok(Self { db })
    }

    pub fn save(&self, entry: &HistoryEntry) -> Result<()> {
        // Merge play_count if entry already exists
        let mut e = entry.clone();
        if let Ok(Some(existing)) = self.db.get(e.id.as_bytes()) {
            if let Ok(old) = serde_json::from_slice::<HistoryEntry>(&existing) {
                e.play_count = old.play_count + 1;
                // Preserve progress if not set in new entry
                if e.progress.is_none() { e.progress = old.progress; }
            }
        }
        let bytes = serde_json::to_vec(&e)?;
        self.db.insert(e.id.as_bytes(), bytes)?;
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        self.db.remove(id.as_bytes())?;
        Ok(())
    }

    pub fn update_progress(&self, id: &str, progress: u32) -> Result<()> {
        if let Some(bytes) = self.db.get(id.as_bytes())? {
            if let Ok(mut e) = serde_json::from_slice::<HistoryEntry>(&bytes) {
                e.progress     = Some(progress);
                e.last_watched = Utc::now();
                self.db.insert(id.as_bytes(), serde_json::to_vec(&e)?)?;
            }
        }
        Ok(())
    }

    pub fn load_all(&self) -> Result<Vec<HistoryEntry>> {
        let mut entries: Vec<HistoryEntry> = self.db.iter()
            .filter_map(|r| r.ok())
            .filter_map(|(_, v)| serde_json::from_slice(&v).ok())
            .collect();
        entries.sort_by(|a, b| b.last_watched.cmp(&a.last_watched));
        Ok(entries)
    }
}