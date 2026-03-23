//! Config — persisted to ~/.config/nexus-tui/config.toml
//! Loaded on startup, saved when leaving the Settings tab.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub player: PlayerConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player: PlayerConfig::default(),
            ui:     UiConfig::default(),
            theme:  ThemeConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    pub mpv_path:           String,
    pub extra_args:         Vec<String>,
    pub stream_mode:        String,
    pub quality:            String,
    pub skip_segments:      String,
    pub resume_offset_secs: u32,   // 0–60, default 5
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            mpv_path:           "mpv".to_string(),
            extra_args:         vec!["--no-terminal".to_string()],
            stream_mode:        "sub".to_string(),
            quality:            "best".to_string(),
            skip_segments:      "none".to_string(),
            resume_offset_secs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub image_protocol:   String,  // "auto"|"kitty"|"halfblock"
    pub results_limit:    usize,
    pub episode_cols:     String,  // "auto"|"2"|"3"
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            image_protocol: "auto".to_string(),
            results_limit:  25,
            episode_cols:   "auto".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub accent:               String,
    pub bar_progress:         String,
    pub bar_complete:         String,
    #[serde(default)]
    pub accent_custom:        Vec<String>,
    #[serde(default)]
    pub bar_progress_custom:  Vec<String>,
    #[serde(default)]
    pub bar_complete_custom:  Vec<String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            accent:              "Yellow".to_string(),
            bar_progress:        "Yellow".to_string(),
            bar_complete:        "Teal".to_string(),
            accent_custom:       Vec::new(),
            bar_progress_custom: Vec::new(),
            bar_complete_custom: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        match Self::try_load() {
            Ok(cfg) => cfg,
            Err(_)  => Self::default(),
        }
    }

    fn try_load() -> Result<Self> {
        let path = config_path();
        if !path.exists() { return Ok(Self::default()); }
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn write_sample() -> Result<()> {
        let path = config_path();
        if path.exists() { return Ok(()); }
        Self::default().save()
    }

    /// Parse accent string → RGB tuple
    pub fn accent_rgb(accent: &str) -> (u8, u8, u8) {
        Self::color_rgb(accent)
    }

    pub fn color_rgb(name: &str) -> (u8, u8, u8) {
        match name {
            "Yellow" => (255, 255,   0),
            "Cyan"   => (  0, 220, 255),
            "Green"  => (  0, 255, 128),
            "Orange" => (255, 140,   0),
            "Pink"   => (255,  80, 180),
            "Purple" => (160,  80, 255),
            "Teal"   => (  0, 200, 150),
            "Red"    => (255,  60,  60),
            "White"  => (220, 220, 220),
            custom   => parse_custom_color(custom).unwrap_or((255, 255, 0)),
        }
    }
}

/// Parse "#rrggbb", "rrggbb", or "r,g,b" / "r,g,b,a"
pub fn parse_custom_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    // Hex: rrggbb or rgb
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        return Some((r, g, b));
    }
    if s.len() == 3 {
        let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
        let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
        let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
        return Some((r, g, b));
    }
    // CSV: r,g,b or r,g,b,a
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() >= 3 {
        let r = parts[0].trim().parse::<u8>().ok()?;
        let g = parts[1].trim().parse::<u8>().ok()?;
        let b = parts[2].trim().parse::<u8>().ok()?;
        return Some((r, g, b));
    }
    None
}

fn config_path() -> PathBuf {
    directories::ProjectDirs::from("dev", "nexus", "nexus-tui")
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from(".nexus/config.toml"))
}

/// All named color presets (used for cycling in settings).
pub const COLOR_PRESET_NAMES: &[&str] = &[
    "Yellow", "Cyan", "Green", "Teal", "Orange", "Pink", "Purple", "Red", "White", "Custom",
];