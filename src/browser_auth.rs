use anyhow::{anyhow, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures_util::StreamExt;
use std::env;
use std::path::PathBuf;
use tokio::time::{sleep, Duration};

const DEFAULT_WARMUP_URL: &str = "https://api.allanime.day/";

pub async fn fetch_text_with_query(url: &str, query: &[(String, String)]) -> Result<String> {
    let full_url = build_url(url, query);
    fetch_text_from_url(&full_url).await
}

pub async fn fetch_text_from_url(url: &str) -> Result<String> {
    ensure_gui_session()?;

    let profile_dir = browser_profile_dir();
    std::fs::create_dir_all(&profile_dir)?;

    let mut builder = BrowserConfig::builder()
        .with_head()
        .disable_default_args()
        .window_size(1360, 900)
        .user_data_dir(profile_dir)
        .args([
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--disable-features=TranslateUI",
            "--no-first-run",
            "--password-store=basic",
            "--lang=en-US",
        ]);

    if let Ok(bin) = env::var("NEXUS_CHROME_BIN") {
        if !bin.trim().is_empty() {
            builder = builder.chrome_executable(bin);
        }
    } else if let Some(bin) = autodetect_chromium_binary() {
        builder = builder.chrome_executable(bin);
    } else {
        return Err(anyhow!(
            "No Chromium-family browser found. Install Chrome/Chromium/Brave or set NEXUS_CHROME_BIN=/path/to/browser. Firefox is not supported by chromiumoxide."
        ));
    }

    let config = builder
        .build()
        .map_err(|e| anyhow!("Failed to build browser config: {e}"))?;
    let (mut browser, mut handler) = Browser::launch(config).await?;
    let handler_task = tokio::spawn(async move {
        while let Some(msg) = handler.next().await {
            if msg.is_err() {
                break;
            }
        }
    });

    let res = fetch_once_or_manual_retry(&browser, url).await;
    let _ = browser.close().await;
    let _ = handler_task.await;
    res
}

async fn fetch_once_or_manual_retry(browser: &Browser, url: &str) -> Result<String> {
    let page = browser.new_page("about:blank").await?;
    page.goto(DEFAULT_WARMUP_URL).await?;
    let _ = page.wait_for_navigation().await?;

    let mut body = fetch_page_text(&page, url).await?;
    if looks_like_bot_challenge(&body) {
        let wait_secs = std::env::var("NEXUS_BROWSER_AUTH_WAIT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(180);
        let poll_ms = std::env::var("NEXUS_BROWSER_AUTH_POLL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(2000);

        eprintln!(
            "nexus: browser auth required on {url}. Complete challenge in opened browser window (up to {wait_secs}s)"
        );

        let deadline = std::time::Instant::now() + Duration::from_secs(wait_secs);
        while std::time::Instant::now() < deadline {
            sleep(Duration::from_millis(poll_ms)).await;
            match current_page_text(&page).await {
                Ok(text) if !looks_like_bot_challenge(&text) => {
                    body = fetch_page_text(&page, url).await?;
                    if !looks_like_bot_challenge(&body) {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    if looks_like_bot_challenge(&body) {
        let snippet = body.chars().take(180).collect::<String>();
        return Err(anyhow!(
            "Browser session still challenged after manual wait. Last response snippet: {snippet}"
        ));
    }

    Ok(body)
}

async fn fetch_page_text(page: &chromiumoxide::Page, url: &str) -> Result<String> {
    page.goto(url).await?;
    let _ = page.wait_for_navigation().await?;
    current_page_text(page).await
}

async fn current_page_text(page: &chromiumoxide::Page) -> Result<String> {
    Ok(page
        .evaluate("document.body ? document.body.innerText : ''")
        .await?
        .into_value()
        .unwrap_or_default())
}

fn browser_profile_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "nexus", "nexus-tui")
        .map(|d| d.data_local_dir().join("browser-profile"))
        .unwrap_or_else(|| PathBuf::from(".nexus/browser-profile"))
}

fn build_url(base: &str, query: &[(String, String)]) -> String {
    if query.is_empty() {
        return base.to_string();
    }

    let qs = query
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{qs}")
}

fn looks_like_bot_challenge(body: &str) -> bool {
    let low = body.to_ascii_lowercase();
    low.contains("<html")
        || low.contains("cloudflare")
        || low.contains("cf-chl")
        || low.contains("captcha")
        || low.contains("attention required")
        || low.contains("/cdn-cgi/challenge-platform")
}

fn autodetect_chromium_binary() -> Option<PathBuf> {
    let absolute_candidates = [
        "/opt/brave.com/brave/brave-browser",
        "/usr/bin/brave-browser",
        "/usr/bin/brave",
        "/snap/bin/brave",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];
    for p in &absolute_candidates {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }

    let path = env::var_os("PATH")?;
    let candidates = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "brave-browser",
        "brave",
    ];

    for dir in env::split_paths(&path) {
        for name in &candidates {
            let full = dir.join(name);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

fn ensure_gui_session() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let has_display = env::var_os("DISPLAY").is_some();
        let has_wayland = env::var_os("WAYLAND_DISPLAY").is_some();
        if !has_display && !has_wayland {
            return Err(anyhow!(
                "No GUI session detected (DISPLAY/WAYLAND missing). Browser auth cannot open a window in this terminal session."
            ));
        }
    }
    Ok(())
}
