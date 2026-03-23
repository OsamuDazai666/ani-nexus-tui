//! SQLite-backed history store.
//!
//! Schema
//! ──────
//! anime        – one row per watched anime
//! episodes     – one row per (anime, episode number)

use crate::api::ContentItem;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: String,
    pub title: String,
    pub media_type: String,
    pub cover_url: Option<String>,
    pub last_watched: DateTime<Utc>,
    pub play_count: u32,
    pub user_rating: Option<f32>, // 0.0–10.0
    pub notes: Option<String>,
    pub total_watch_seconds: i64,
    pub progress: Option<u32>, // last watched episode number
    pub total: Option<u32>,    // total episodes

    // Episode list cache
    pub episodes_cache: Option<Vec<String>>,
    pub episodes_cache_updated_at: Option<DateTime<Utc>>,
}

impl HistoryEntry {
    pub fn from_content(item: &ContentItem) -> Self {
        let total = match item {
            ContentItem::Anime(a) => {
                let eps = a.total_episodes();
                if eps > 0 {
                    Some(eps)
                } else {
                    None
                }
            }
        };
        Self {
            id: item.id().to_string(),
            title: item.title().to_string(),
            media_type: format!("{:?}", item.media_type()),
            cover_url: item.cover_url().map(String::from),
            last_watched: Utc::now(),
            play_count: 1,
            user_rating: None,
            notes: None,
            total_watch_seconds: 0,
            progress: None,
            total,
            episodes_cache: None,
            episodes_cache_updated_at: None,
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
        let empty = width.saturating_sub(filled);
        format!("{}{}", "█".repeat(filled), "░".repeat(empty))
    }

    /// True if the episode cache is missing or older than 24 hours.
    pub fn episodes_cache_stale(&self) -> bool {
        match self.episodes_cache_updated_at {
            None => true,
            Some(t) => (Utc::now() - t).num_hours() >= 24,
        }
    }
}

// ── Episode record ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EpisodeRecord {
    pub anime_id: String,
    pub episode_number: String, // episode numbers can be "1", "1.5", "SP1" etc.
    pub stream_url: Option<String>,
    pub watched: bool,
    pub watch_timestamp: Option<DateTime<Utc>>,
    pub position_seconds: f64, // resume point
    pub duration_seconds: f64, // total duration (0 = unknown)
    pub fully_watched: bool,   // position > 95% of duration
}

// ── Store ─────────────────────────────────────────────────────────────────────

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    pub fn open() -> Result<Self> {
        let path = db_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let conn =
            Connection::open(&path).with_context(|| format!("opening SQLite DB at {path:?}"))?;

        // WAL mode — safe for concurrent reads from the async runtime
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Self::migrate(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn migrate(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS anime (
                id                        TEXT PRIMARY KEY,
                title                     TEXT NOT NULL,
                media_type                TEXT NOT NULL DEFAULT 'Anime',
                cover_url                 TEXT,
                last_watched              INTEGER NOT NULL,   -- unix timestamp
                play_count                INTEGER NOT NULL DEFAULT 1,
                user_rating               REAL,
                notes                     TEXT,
                total_watch_seconds       INTEGER NOT NULL DEFAULT 0,
                progress                  INTEGER,           -- last watched ep number
                total                     INTEGER,           -- total episodes
                episodes_cache            TEXT,              -- JSON array of ep strings
                episodes_cache_updated_at INTEGER            -- unix timestamp
            );

            CREATE TABLE IF NOT EXISTS episodes (
                anime_id         TEXT NOT NULL REFERENCES anime(id) ON DELETE CASCADE,
                episode_number   TEXT NOT NULL,
                stream_url       TEXT,
                watched          INTEGER NOT NULL DEFAULT 0,
                watch_timestamp  INTEGER,                    -- unix timestamp
                position_seconds REAL    NOT NULL DEFAULT 0,
                duration_seconds REAL    NOT NULL DEFAULT 0,
                fully_watched    INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (anime_id, episode_number)
            );

            CREATE INDEX IF NOT EXISTS idx_anime_last_watched ON anime(last_watched DESC);
            CREATE INDEX IF NOT EXISTS idx_episodes_anime     ON episodes(anime_id);
        ",
        )?;
        Ok(())
    }

    // ── Anime CRUD ────────────────────────────────────────────────────────────

    pub fn save(&self, entry: &HistoryEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO anime
               (id, title, media_type, cover_url, last_watched, play_count,
                user_rating, notes, total_watch_seconds, progress, total,
                episodes_cache, episodes_cache_updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET
               title                     = excluded.title,
               cover_url                 = excluded.cover_url,
               last_watched              = excluded.last_watched,
               play_count                = anime.play_count + 1,
               total                     = coalesce(excluded.total, anime.total),
               progress                  = coalesce(excluded.progress, anime.progress)",
            params![
                entry.id,
                entry.title,
                entry.media_type,
                entry.cover_url,
                entry.last_watched.timestamp(),
                entry.play_count,
                entry.user_rating,
                entry.notes,
                entry.total_watch_seconds,
                entry.progress.map(|p| p as i64),
                entry.total.map(|t| t as i64),
                entry
                    .episodes_cache
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_default()),
                entry.episodes_cache_updated_at.map(|t| t.timestamp()),
            ],
        )?;
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM anime WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_progress(&self, id: &str, episode: u32) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE anime SET progress = ?1, last_watched = ?2 WHERE id = ?3",
            params![episode as i64, Utc::now().timestamp(), id],
        )?;
        Ok(())
    }

    /// Store the fetched episode list and mark cache timestamp.
    pub fn save_episodes_cache(&self, id: &str, episodes: &[String]) -> Result<()> {
        let json = serde_json::to_string(episodes)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE anime SET episodes_cache = ?1, episodes_cache_updated_at = ?2 WHERE id = ?3",
            params![json, Utc::now().timestamp(), id],
        )?;
        Ok(())
    }

    pub fn load_all(&self) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, media_type, cover_url, last_watched, play_count,
                    user_rating, notes, total_watch_seconds, progress, total,
                    episodes_cache, episodes_cache_updated_at
             FROM anime ORDER BY last_watched DESC",
        )?;

        let entries = stmt
            .query_map([], row_to_entry)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn get(&self, id: &str) -> Result<Option<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, media_type, cover_url, last_watched, play_count,
                    user_rating, notes, total_watch_seconds, progress, total,
                    episodes_cache, episodes_cache_updated_at
             FROM anime WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_entry)?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    // ── Episode CRUD ──────────────────────────────────────────────────────────

    pub fn save_episode(&self, rec: &EpisodeRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO episodes
               (anime_id, episode_number, stream_url, watched, watch_timestamp,
                position_seconds, duration_seconds, fully_watched)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(anime_id, episode_number) DO UPDATE SET
               stream_url       = coalesce(excluded.stream_url, episodes.stream_url),
               watched          = excluded.watched,
               watch_timestamp  = excluded.watch_timestamp,
               position_seconds = excluded.position_seconds,
               duration_seconds = case when excluded.duration_seconds > 0
                                       then excluded.duration_seconds
                                       else episodes.duration_seconds end,
               fully_watched    = excluded.fully_watched",
            params![
                rec.anime_id,
                rec.episode_number,
                rec.stream_url,
                rec.watched as i64,
                rec.watch_timestamp.map(|t| t.timestamp()),
                rec.position_seconds,
                rec.duration_seconds,
                rec.fully_watched as i64,
            ],
        )?;
        Ok(())
    }

    pub fn update_position(
        &self,
        anime_id: &str,
        episode_number: &str,
        position: f64,
        duration: f64,
    ) -> Result<()> {
        let fully = duration > 0.0 && position / duration >= 0.95;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO episodes (anime_id, episode_number, position_seconds, duration_seconds, fully_watched, watched)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(anime_id, episode_number) DO UPDATE SET
               position_seconds = ?3,
               duration_seconds = case when ?4 > 0 then ?4 else episodes.duration_seconds end,
               fully_watched    = ?5,
               watched          = case when ?5 then 1 else episodes.watched end,
               watch_timestamp  = case when ?5 then ?6 else episodes.watch_timestamp end",
            params![
                anime_id,
                episode_number,
                position,
                duration,
                fully as i64,
                Utc::now().timestamp(),
            ],
        )?;
        Ok(())
    }

    pub fn load_episodes(&self, anime_id: &str) -> Result<Vec<EpisodeRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT anime_id, episode_number, stream_url, watched, watch_timestamp,
                    position_seconds, duration_seconds, fully_watched
             FROM episodes WHERE anime_id = ?1
             ORDER BY CAST(episode_number AS REAL)",
        )?;
        let recs = stmt
            .query_map(params![anime_id], row_to_episode)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(recs)
    }

    /// Load only the episode records for the given episode number strings.
    /// Used for windowed/paginated rendering — avoids loading 1000+ records.
    pub fn load_episodes_in(
        &self,
        anime_id: &str,
        episode_numbers: &[&str],
    ) -> Result<Vec<EpisodeRecord>> {
        if episode_numbers.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn.lock().unwrap();

        // rusqlite anonymous ? params — one per episode plus one for anime_id
        let placeholders = episode_numbers
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT anime_id, episode_number, stream_url, watched, watch_timestamp,
                    position_seconds, duration_seconds, fully_watched
             FROM episodes WHERE anime_id = ? AND episode_number IN ({placeholders})"
        );

        let mut stmt = conn.prepare(&sql)?;

        // Build a single Vec<Box<dyn ToSql>>: anime_id first, then each episode string
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(anime_id.to_string())];
        for ep in episode_numbers {
            params.push(Box::new(ep.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let recs = stmt
            .query_map(param_refs.as_slice(), row_to_episode)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(recs)
    }

    pub fn get_episode(&self, anime_id: &str, ep: &str) -> Result<Option<EpisodeRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT anime_id, episode_number, stream_url, watched, watch_timestamp,
                    position_seconds, duration_seconds, fully_watched
             FROM episodes WHERE anime_id = ?1 AND episode_number = ?2",
        )?;
        let mut rows = stmt.query_map(params![anime_id, ep], row_to_episode)?;
        Ok(rows.next().and_then(|r| r.ok()))
    }
}

// ── Row mappers ───────────────────────────────────────────────────────────────

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let ts: i64 = row.get(4)?;
    let cache_ts: Option<i64> = row.get(12)?;
    let cache_json: Option<String> = row.get(11)?;

    Ok(HistoryEntry {
        id: row.get(0)?,
        title: row.get(1)?,
        media_type: row.get(2)?,
        cover_url: row.get(3)?,
        last_watched: DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now),
        play_count: row.get::<_, i64>(5)? as u32,
        user_rating: row.get(6)?,
        notes: row.get(7)?,
        total_watch_seconds: row.get(8)?,
        progress: row.get::<_, Option<i64>>(9)?.map(|v| v as u32),
        total: row.get::<_, Option<i64>>(10)?.map(|v| v as u32),
        episodes_cache: cache_json.and_then(|j| serde_json::from_str(&j).ok()),
        episodes_cache_updated_at: cache_ts.and_then(|t| DateTime::from_timestamp(t, 0)),
    })
}

fn row_to_episode(row: &rusqlite::Row<'_>) -> rusqlite::Result<EpisodeRecord> {
    let ts: Option<i64> = row.get(4)?;
    Ok(EpisodeRecord {
        anime_id: row.get(0)?,
        episode_number: row.get(1)?,
        stream_url: row.get(2)?,
        watched: row.get::<_, i64>(3)? != 0,
        watch_timestamp: ts.and_then(|t| DateTime::from_timestamp(t, 0)),
        position_seconds: row.get(5)?,
        duration_seconds: row.get(6)?,
        fully_watched: row.get::<_, i64>(7)? != 0,
    })
}

// ── Path helper ───────────────────────────────────────────────────────────────

fn db_path() -> PathBuf {
    directories::ProjectDirs::from("dev", "nexus", "nexus-tui")
        .map(|d| d.data_dir().join("nexus.db"))
        .unwrap_or_else(|| PathBuf::from(".nexus-data/nexus.db"))
}
