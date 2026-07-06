//! Simplified Cloudflare-aware HTTP transport for AllAnime API.
//!
//! AllAnime's api.allanime.day now blocks GET requests with Cloudflare challenges.
//! Solution: Use POST with JSON body + correct headers (matching ani-cli).
//! This avoids the need for FlareSolverr or headless browser entirely.

use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT};
use std::time::Duration;

use crate::player::{AGENT, ALLANIME_API, ALLANIME_REFR};

/// Persisted query hash for episode sources (from ani-cli).
/// This bypasses additional Cloudflare rules that trigger on full query.
const EPISODE_PERSISTED_HASH: &str =
    "d405d0edd690624b66baba3068e0edc3ac90f1597d898a1ec8db4e5c43c00fec";

/// Build a reqwest client with Chrome TLS fingerprint and correct headers.
fn build_client() -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(REFERER, HeaderValue::from_static(ALLANIME_REFR));
    headers.insert(ORIGIN, HeaderValue::from_static(ALLANIME_REFR));
    headers.insert(USER_AGENT, HeaderValue::from_static(AGENT));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    Ok(reqwest::Client::builder()
        .emulation(wreq_util::Emulation::Chrome140)
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .build()?)
}

/// Extract JSON from HTML wrapper that some intermediaries add to API responses.
fn extract_json_from_html(body: &str) -> String {
    if !body.trim_start().starts_with('<') {
        return body.to_string();
    }

    if let Some(start) = body.find("<pre>") {
        let content_start = start + 5;
        if let Some(end) = body[content_start..].find("</pre>") {
            let json_content = &body[content_start..content_start + end];
            crate::debug_log!(
                "Extracted JSON from <pre> tags: {} bytes",
                json_content.len()
            );
            return json_content.to_string();
        }
    }

    if let Some(start) = body.find("<body>") {
        let after_body = &body[start + 6..];
        let trimmed = after_body.trim();
        if let Some(content) = trimmed.strip_suffix("</body></html>") {
            crate::debug_log!("Extracted JSON from <body>: {} bytes", content.len());
            return content.trim().to_string();
        }
    }

    body.to_string()
}

/// Generic POST request to AllAnime GraphQL API.
/// Used for search and episode list (full query, no persisted query).
pub async fn fetch_text_with_query(url: &str, query: &[(String, String)]) -> Result<String> {
    let client = build_client()?;

    // Build variables and query from the tuple list
    let vars: serde_json::Value = query
        .iter()
        .find(|(k, _)| k == "variables")
        .map(|(_, v)| serde_json::from_str(v))
        .transpose()?
        .unwrap_or(serde_json::json!({}));

    let gql = query
        .iter()
        .find(|(k, _)| k == "query")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    let body = serde_json::json!({
        "variables": vars,
        "query": gql
    });

    let resp = client.post(url).json(&body).send().await?;

    let text = resp.text().await?;
    Ok(extract_json_from_html(&text))
}

/// POST request with persisted query for episode sources.
/// This is the critical path that was failing with "NEED_CAPTCHA" errors.
/// Using the persisted query hash (from ani-cli) bypasses additional CF rules.
pub async fn fetch_episode_sources(show_id: &str, mode: &str, episode: &str) -> Result<String> {
    let client = build_client()?;

    let variables = serde_json::json!({
        "showId": show_id,
        "translationType": mode,
        "episodeString": episode
    });

    let extensions = serde_json::json!({
        "persistedQuery": {
            "version": 1,
            "sha256Hash": EPISODE_PERSISTED_HASH
        }
    });

    let body = serde_json::json!({
        "variables": variables,
        "extensions": extensions
    });

    let resp = client.post(ALLANIME_API).json(&body).send().await?;

    let text = resp.text().await?;
    Ok(extract_json_from_html(&text))
}

/// Simple GET request for provider/embed pages (not the AllAnime API)
/// Used for fetching video source pages from providers
pub async fn fetch_text_from_url(url: &str) -> Result<String> {
    let client = build_client()?;

    let resp = client
        .get(url)
        .header("Referer", ALLANIME_REFR)
        .send()
        .await
        .map_err(|e| anyhow!("Provider request failed: {e}"))?;

    let text = resp
        .text()
        .await
        .map_err(|e| anyhow!("Failed to read response: {e}"))?;
    Ok(text)
}

/// Check if response looks like a Cloudflare challenge
#[allow(dead_code)]
pub fn looks_like_bot_challenge(body: &str) -> bool {
    let low = body.to_ascii_lowercase();
    low.contains("cf-chl")
        || low.contains("/cdn-cgi/challenge-platform")
        || low.contains("just a moment")
        || low.contains("verifying you are human")
        || low.contains("attention required")
        || low.contains("need_captcha")
}

/// Compatibility stub - always returns true since we no longer use browser sessions
pub fn has_session(_domain: &str) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_html_pre_tag() {
        let html = r#"<html><body><pre>{"data":{"test":123}}</pre></body></html>"#;
        let result = extract_json_from_html(html);
        assert_eq!(result, r#"{"data":{"test":123}}"#);
    }

    #[test]
    fn test_extract_json_from_html_body_tag() {
        let html = r#"<html><body>{"data":{"test":456}}</body></html>"#;
        let result = extract_json_from_html(html);
        assert_eq!(result, r#"{"data":{"test":456}}"#);
    }

    #[test]
    fn test_extract_json_from_html_no_wrapper() {
        let json = r#"{"data":{"test":789}}"#;
        let result = extract_json_from_html(json);
        assert_eq!(result, json);
    }
}
