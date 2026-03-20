//! Config loader — reads ~/.config/nexus/config.toml
//! Falls back gracefully to env vars and defaults.

#![allow(dead_code)]
use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub player: PlayerConfig,

    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Deserialize)]
pub struct PlayerConfig {
    pub mpv_path: String,
    pub extra_args: Vec<String>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            mpv_path: "mpv".to_string(),
            extra_args: vec!["--no-terminal".to_string()],
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UiConfig {
    pub image_protocol: String,
    pub results_limit: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            image_protocol: "auto".to_string(),
            results_limit: 25,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        match Self::try_load() {
            Ok(cfg) => cfg,
            Err(_) => Self::default(),
        }
    }

    fn try_load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let cfg: Config = toml::from_str(&content)?;
        Ok(cfg)
    }

    pub fn write_sample() -> Result<()> {
        let path = config_path();
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &path,
            r#"# nexus-tui configuration

[player]
mpv_path   = "mpv"
extra_args = ["--no-terminal", "--really-quiet"]

[ui]
# Image rendering: "auto" | "kitty" | "sixel" | "halfblock"
image_protocol = "auto"
results_limit  = 25
"#,
        )?;
        Ok(())
    }
}

fn config_path() -> PathBuf {
    directories::ProjectDirs::from("dev", "nexus", "nexus-tui")
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from(".nexus/config.toml"))
}