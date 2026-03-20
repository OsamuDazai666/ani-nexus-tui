//! Stream resolution + mpv launcher.
//! Exact port of ani-cli v4.10 source (pystardust/ani-cli).

use anyhow::{anyhow, bail, Result};
use std::process::{Command, Stdio};

const ALLANIME_API:  &str = "https://api.allanime.day/api";
const ALLANIME_BASE: &str = "allanime.day";
const ALLANIME_REFR: &str = "https://allmanga.to";
const AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/121.0";

// ── ani-cli hex cipher (provider_init) ───────────────────────────────────────
// ani-cli encodes provider paths with a custom hex→char substitution table
fn hex_decipher(s: &str) -> String {
    let pairs: Vec<&str> = (0..s.len()).step_by(2)
        .map(|i| &s[i..=(i+1).min(s.len()-1)])
        .filter(|p| p.len() == 2)
        .collect();

    pairs.iter().map(|hex| match *hex {
        "79"=>"A","7a"=>"B","7b"=>"C","7c"=>"D","7d"=>"E","7e"=>"F","7f"=>"G",
        "70"=>"H","71"=>"I","72"=>"J","73"=>"K","74"=>"L","75"=>"M","76"=>"N","77"=>"O",
        "68"=>"P","69"=>"Q","6a"=>"R","6b"=>"S","6c"=>"T","6d"=>"U","6e"=>"V","6f"=>"W",
        "60"=>"X","61"=>"Y","62"=>"Z",
        "59"=>"a","5a"=>"b","5b"=>"c","5c"=>"d","5d"=>"e","5e"=>"f","5f"=>"g",
        "50"=>"h","51"=>"i","52"=>"j","53"=>"k","54"=>"l","55"=>"m","56"=>"n","57"=>"o",
        "48"=>"p","49"=>"q","4a"=>"r","4b"=>"s","4c"=>"t","4d"=>"u","4e"=>"v","4f"=>"w",
        "40"=>"x","41"=>"y","42"=>"z",
        "08"=>"0","09"=>"1","0a"=>"2","0b"=>"3","0c"=>"4","0d"=>"5","0e"=>"6","0f"=>"7",
        "00"=>"8","01"=>"9",
        "15"=>"-","16"=>".","67"=>"_","46"=>"~","02"=>":","17"=>"/","07"=>"?",
        "1b"=>"#","63"=>"[","65"=>"]","78"=>"@","19"=>"!","1c"=>"$","1e"=>"&",
        "10"=>"(","11"=>")","12"=>"*","13"=>"+","14"=>",","03"=>";","05"=>"=","1d"=>"%",
        _ => "",
    }).collect::<String>()
    .replace("/clock", "/clock.json")
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Main entry for anime. Returns the resolved direct stream URL.
pub async fn stream_anime(show_id: &str, episode: u32, mode: &str, quality: &str) -> Result<String> {
    let (episode_url, _refr_flag) = get_episode_url(show_id, episode, mode, quality).await?;
    Ok(episode_url)
}

pub async fn fetch_episode_list(show_id: &str, mode: &str) -> Result<Vec<String>> {
    episodes_list(show_id, mode).await
}

pub fn launch_mpv_url(url: &str) -> Result<()> {
    // Sanitize double-protocol URLs (e.g. https://https://...)
    let url = url.replace("https://https://", "https://")
                 .replace("http://http://", "http://");
    let url = url.as_str();

    let needs_referer = url.contains("fast4speed") || url.contains("clock.json")
        || url.contains(".m3u8");

    // Decouple from ratatui
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;

    let mut cmd = Command::new("mpv");
    cmd.arg(url);

    // mpv expects each header as a separate --http-header-fields-append arg
    cmd.arg(format!("--http-header-fields-append=User-Agent: {}", AGENT));
    if needs_referer {
        cmd.arg(format!("--http-header-fields-append=Referer: {}", ALLANIME_REFR));
    }

    println!("Launching MPV with URL: {}", url);

    // Run the player in the terminal
    let status = cmd.status();

    // Reattach to ratatui
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;

    status.map_err(|e| anyhow!(
        "Failed to launch mpv: {e}\nInstall: sudo apt install mpv"
    ))?;
    Ok(())
}

// ── ani-cli delegation ────────────────────────────────────────────────────────

fn ani_cli_available() -> bool {
    Command::new("ani-cli").arg("--help")
        .stdout(Stdio::null()).stderr(Stdio::null())
        .status().map(|_| true).unwrap_or(false)
}

async fn stream_via_ani_cli(title: &str, episode: u32, mode: &str, quality: &str) -> Result<String> {
    // ani-cli debug player: prints "All links:\n<links>\nSelected link:\n<url>"
    let mut args = vec![
        "-e".to_string(), episode.to_string(),
        "-q".to_string(), quality.to_string(),
        title.to_string(),
    ];
    if mode == "dub" { args.push("--dub".to_string()); }

    let out = tokio::process::Command::new("ani-cli")
        .args(&args)
        .env("ANI_CLI_PLAYER", "debug")
        .output().await
        .map_err(|e| anyhow!("ani-cli exec: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let all = format!("{stdout}{stderr}");

    // ani-cli debug output format:
    // "All links:\n...\nSelected link:\n<url>"
    if let Some(pos) = all.find("Selected link:") {
        if let Some(url) = all[pos..].lines().nth(1) {
            let url = url.trim().to_string();
            if url.starts_with("http") {
                return Ok(url);
            }
        }
    }

    // Fallback: any http line
    if let Some(url) = all.lines().find(|l| l.trim().starts_with("http")) {
        return Ok(url.trim().to_string());
    }

    bail!("ani-cli returned no URL.\nOutput:\n{}", &all[..all.len().min(500)])
}

// ── episodes_list ─────────────────────────────────────────────────────────────
// ani-cli: curl ... | sed -nE "s|.*$mode\":\[([0-9.\",]*)\].*|\1|p" | sed 's|,|\n|g; s|"||g' | sort -n -k 1

async fn episodes_list(show_id: &str, mode: &str) -> Result<Vec<String>> {
    let gql = r#"query ($showId: String!) { show( _id: $showId ) { _id availableEpisodesDetail }}"#;
    let vars = format!(r#"{{"showId":"{}"}}"#, show_id);

    let text = client().get(ALLANIME_API.to_string())
        .query(&[("variables", &vars), ("query", &gql.to_string())])
        .send().await?.text().await?;

    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let mut eps: Vec<String> = if let Some(arr) = json["data"]["show"]["availableEpisodesDetail"][mode].as_array() {
        arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
    } else {
        vec![]
    };

    // sort -n -k 1 (numeric sort)
    eps.sort_by(|a, b| {
        let an: f64 = a.parse().unwrap_or(0.0);
        let bn: f64 = b.parse().unwrap_or(0.0);
        an.partial_cmp(&bn).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(eps)
}

// ── get_episode_url ───────────────────────────────────────────────────────────
// ani-cli: curl ... | tr '{}' '\n' | sed ... | sed -nE 's|.*sourceUrl":"--([^"]*)".*sourceName":"([^"]*)".*|\2 :\1|p'
// Then runs generate_link for providers 1..4 in parallel, cats results, select_quality

async fn get_episode_url(id: &str, ep: u32, mode: &str, quality: &str) -> Result<(String, Option<String>)> {
    let gql = r#"query ($showId: String!, $translationType: VaildTranslationTypeEnumType!, $episodeString: String!) { episode( showId: $showId translationType: $translationType episodeString: $episodeString ) { episodeString sourceUrls }}"#;
    let vars = format!(
        r#"{{"showId":"{}","translationType":"{}","episodeString":"{}"}}"#,
        id, mode, ep
    );

    let text = client().get(ALLANIME_API.to_string())
        .query(&[("variables", &vars), ("query", &gql.to_string())])
        .send().await?.text().await?;

    // tr '{}' '\n' | sed 's|\\u002F|/|g;s|\\||g'
    let normalized = text
        .replace('{', "\n").replace('}', "\n")
        .replace("\\u002F", "/").replace('\\', "");

    // sed -nE 's|.*sourceUrl":"--([^"]*)".*sourceName":"([^"]*)".*|\2 :\1|p'
    // Gives us: "ProviderName :encodedPath"
    let mut providers: Vec<(String, String)> = Vec::new(); // (name, encoded_path)
    for line in normalized.lines() {
        if let (Some(url_part), Some(name_part)) = (
            extract_between(line, "\"sourceUrl\":\"--", "\""),
            extract_between(line, "\"sourceName\":\"", "\""),
        ) {
            providers.push((name_part.to_string(), url_part.to_string()));
        }
    }

    if providers.is_empty() {
        bail!("No providers found for episode {ep}. Check show ID and mode ({mode}).");
    }

    // generate_link for each provider — decode hex path and get_links
    let mut all_links: Vec<(String, String, Option<String>)> = Vec::new(); // (res, url, referer)
    let client = client();
    let mut set = tokio::task::JoinSet::new();

    for (_name, encoded) in &providers {
        let path = hex_decipher(encoded);
        if path.is_empty() { continue; }

        let c = client.clone();
        set.spawn(async move {
            get_links(&c, &path).await
        });
    }

    while let Some(res) = set.join_next().await {
        if let Ok(Ok(links)) = res {
            if !links.is_empty() {
                all_links.extend(links);
                break; // Found our links! Drop the set to abort lagging tasks.
            }
        }
    }

    if all_links.is_empty() {
        bail!(
            "No playable links found for episode {ep}.\n\
             Providers tried: {}\n\
             Install ani-cli for best compatibility:\n\
             sudo apt install ani-cli",
            providers.iter().map(|(n,_)| n.as_str()).collect::<Vec<_>>().join(", ")
        );
    }

    // sort -g -r (numeric descending by resolution)
    all_links.sort_by(|a, b| {
        let an: u32 = a.0.replace('p', "").parse().unwrap_or(0);
        let bn: u32 = b.0.replace('p', "").parse().unwrap_or(0);
        bn.cmp(&an)
    });

    // select_quality
    let selected = match quality {
        "best"  => all_links.first(),
        "worst" => all_links.last(),
        q => all_links.iter().find(|(res, _, _)| res.contains(q))
                 .or_else(|| all_links.first()),
    };

    let (_, url, refr) = selected.ok_or_else(|| anyhow!("No link selected"))?;
    Ok((url.clone(), refr.clone()))
}

// ── get_links — exact port of ani-cli's get_links() ──────────────────────────
// Returns Vec<(resolution, url, Option<referer>)>

async fn get_links(
    client: &reqwest::Client,
    path: &str,
) -> Result<Vec<(String, String, Option<String>)>> {
    let url = if path.starts_with("http") {
        path.to_string()
    } else {
        format!("https://{ALLANIME_BASE}{path}")
    };

    let response = client.get(&url).send().await?.text().await?;

    // sed 's|},{|\n|g'
    let separated = response.replace("},{", "\n");

    let mut links: Vec<(String, String, Option<String>)> = Vec::new();
    let mut m3u8_refr: Option<String> = None;

    // Extract m3u8 referer
    for line in separated.lines() {
        if let Some(refr) = extract_between(line, "\"Referer\":\"", "\"") {
            m3u8_refr = Some(refr.to_string());
        }
    }

    for chunk in separated.split('\n') {
        // Pattern 1: {"link":"URL","resolutionStr":"1080p"}
        // sed -nE 's|.*link":"([^"]*)".*"resolutionStr":"([^"]*)".*|\2 >\1|p'
        if let (Some(link), Some(res)) = (
            extract_between(chunk, "\"link\":\"", "\""),
            extract_between(chunk, "\"resolutionStr\":\"", "\""),
        ) {
            let link = link.replace("\\u002F", "/").replace("\\/", "/");
            if link.starts_with("http") {
                // wixmp repackager
                if link.contains("repackager.wixmp.com") {
                    links.extend(expand_wixmp(&link));
                } else {
                    links.push((res.to_string(), link, None));
                }
            }
        }

        // Pattern 2: hls url with en-US hardsub
        // sed -nE 's|.*hls","url":"([^"]*)".*"hardsub_lang":"en-US".*|\1|p'
        if chunk.contains("\"hls\"") && chunk.contains("\"hardsub_lang\":\"en-US\"") {
            if let Some(hls) = extract_between(chunk, "\"url\":\"", "\"") {
                let hls = hls.replace("\\u002F", "/").replace("\\/", "/");
                if hls.starts_with("http") {
                    links.push(("1080p".to_string(), hls, m3u8_refr.clone()));
                }
            }
        }
    }

    // Handle master.m3u8 — fetch and parse quality list
    let master_link = links.iter().find(|(_, u, _)| u.contains("master.m3u8")).cloned();
    if let Some((_, master_url, _)) = master_link {
        if let Ok(m3u8_links) = parse_master_m3u8(client, &master_url, m3u8_refr.as_deref()).await {
            if !m3u8_links.is_empty() {
                links = m3u8_links;
            }
        }
    }

    // fast4speed: add Yt entry with referer ONLY if no other links were found
    // (the fast4speed URL itself IS the stream, but may fail if blocked)
    if url.contains("tools.fast4speed.rsvp") && links.is_empty() {
        links.push(("Yt".to_string(), url.clone(), Some(ALLANIME_REFR.to_string())));
    }

    Ok(links)
}

// Parse master.m3u8 into resolution→url pairs
// ani-cli: grep STREAM, sed resolution and url lines, sort -nr
async fn parse_master_m3u8(
    client: &reqwest::Client,
    url: &str,
    refr: Option<&str>,
) -> Result<Vec<(String, String, Option<String>)>> {
    let base = url.rsplitn(2, '/').last().unwrap_or("").to_string() + "/";
    let mut req = client.get(url);
    if let Some(r) = refr { req = req.header("Referer", r); }
    let body = req.send().await?.text().await?;

    let mut links = Vec::new();
    let mut current_res = String::from("unknown");

    for line in body.lines() {
        // #EXT-X-STREAM-INF:...,RESOLUTION=1920x1080,...
        if line.starts_with("#EXT-X-STREAM-INF") {
            current_res = line.split("RESOLUTION=")
                .nth(1)
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.split('x').last())
                .map(|h| format!("{h}p"))
                .unwrap_or_else(|| "unknown".to_string());
        } else if !line.starts_with('#') && !line.is_empty() {
            let full_url = if line.starts_with("http") {
                line.to_string()
            } else {
                format!("{base}{line}")
            };
            links.push((current_res.clone(), full_url, refr.map(String::from)));
        }
    }

    links.sort_by(|a, b| {
        let an: u32 = a.0.replace('p', "").parse().unwrap_or(0);
        let bn: u32 = b.0.replace('p', "").parse().unwrap_or(0);
        bn.cmp(&an)
    });
    Ok(links)
}

// wixmp repackager expansion
// ani-cli: sed '/,([^/]*),/mp4.*|\1|p' | sed 's|,|\n|g' → per-resolution urls
fn expand_wixmp(url: &str) -> Vec<(String, String, Option<String>)> {
    let stripped = url.replace("repackager.wixmp.com/", "");
    let base = stripped.split(".urlset").next().unwrap_or(&stripped);

    if let Some(res_start) = base.find("/,") {
        let resolutions_part = &base[res_start + 2..];
        if let Some(res_end) = resolutions_part.find('/') {
            let base_path = &base[..res_start];
            let suffix    = &resolutions_part[res_end..];
            return resolutions_part[..res_end]
                .split(',')
                .filter(|r| !r.is_empty())
                .map(|r| {
                    // Strip the protocol manually if it exists from the base_path
                    let clean_base = base_path
                        .trim_start_matches("https://")
                        .trim_start_matches("http://");
                    let u = format!("https://{clean_base}/{r}{suffix}");
                    (r.to_string(), u, None)
                })
                .collect();
        }
    }
    vec![]
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert("Referer", reqwest::header::HeaderValue::from_static(ALLANIME_REFR));
            h
        })
        .build()
        .unwrap_or_default()
}