//! Stream resolution + mpv launcher with IPC observe_property tracking
//! and watch_later-based exact quit-position saving.

use anyhow::{anyhow, bail, Result};
use std::process::Command;
use tokio::sync::mpsc;

const ALLANIME_API:  &str = "https://api.allanime.day/api";
const ALLANIME_BASE: &str = "allanime.day";
const ALLANIME_REFR: &str = "https://allmanga.to";
const AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/121.0";

// ── Public event type ─────────────────────────────────────────────────────────

/// Sent back to App during and after playback.
pub enum PlaybackEvent {
    /// Live position update from observe_property stream (~1s granularity).
    /// `checkpoint=true` means write to DB now (every 30s).
    Position { anime_id: String, episode: String, position: f64, duration: f64, checkpoint: bool },
    /// mpv exited — position is the authoritative value read from watch_later file.
    Finished { anime_id: String, episode: String, position: f64, duration: f64 },
}

// ── ani-cli hex cipher ────────────────────────────────────────────────────────

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

pub async fn stream_anime(show_id: &str, episode: u32, mode: &str, quality: &str) -> Result<String> {
    let (url, _) = get_episode_url(show_id, episode, mode, quality).await?;
    Ok(url)
}

pub async fn fetch_episode_list(show_id: &str, mode: &str) -> Result<Vec<String>> {
    episodes_list(show_id, mode).await
}

/// Simple fire-and-forget launch (no tracking). Used as a thin wrapper.
pub fn launch_mpv_url(url: &str) -> Result<()> {
    launch_mpv_tracked(url, "", "", 0.0, None).map(|_| ())
}

/// Launch mpv with full tracking:
/// - `--save-position-on-quit` into an isolated tmp dir → exact quit position
/// - `observe_property` event stream → live position updates (~1s)
///
/// The caller (main loop) is responsible for terminal teardown before calling
/// this and restore afterwards. Returns `(quit_position_seconds, duration_seconds)`.
pub fn launch_mpv_tracked(
    url:         &str,
    anime_id:    &str,
    episode:     &str,
    resume_from: f64,
    tx:          Option<mpsc::UnboundedSender<PlaybackEvent>>,
) -> Result<(f64, f64)> {
    let url = url.replace("https://https://", "https://")
                 .replace("http://http://", "http://");

    let needs_referer = url.contains("fast4speed") || url.contains("clock.json")
        || url.contains(".m3u8");

    // Isolated watch_later dir — one file per session, easy to read back
    let watch_dir = format!("/tmp/nexus-watch-{}", std::process::id());
    let _ = std::fs::create_dir_all(&watch_dir);

    // IPC socket
    let socket = format!("/tmp/nexus-mpv-{}.sock", std::process::id());

    let mut cmd = Command::new("mpv");
    cmd.arg(&url);

    // Position tracking
    cmd.arg("--save-position-on-quit");
    cmd.arg(format!("--watch-later-dir={watch_dir}"));
    cmd.arg("--watch-later-options=start");   // only save start position, not other state

    // IPC for live observe_property stream
    cmd.arg(format!("--input-ipc-server={socket}"));
    cmd.arg("--idle=no");

    // HTTP headers
    cmd.arg(format!("--http-header-fields-append=User-Agent: {AGENT}"));
    if needs_referer {
        cmd.arg(format!("--http-header-fields-append=Referer: {ALLANIME_REFR}"));
    }

    // Resume from saved position
    if resume_from > 5.0 {
        cmd.arg(format!("--start={resume_from:.1}"));
    }

    let mut child = cmd.spawn().map_err(|e| anyhow!(
        "Failed to launch mpv: {e}\nInstall: sudo apt install mpv"
    ))?;

    // ── observe_property stream thread ────────────────────────────────────────
    let socket2   = socket.clone();
    let anime_id2 = anime_id.to_string();
    let episode2  = episode.to_string();
    let tx2       = tx.clone();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();

    // Shared last-known position/duration from the observer stream
    let last_known = std::sync::Arc::new(std::sync::Mutex::new((0.0f64, 0.0f64)));
    let last_known2 = last_known.clone();

    let observer = std::thread::spawn(move || {
        observe_stream(&socket2, &anime_id2, &episode2, tx2, stop_rx, last_known2);
    });

    // ── Wait for mpv ──────────────────────────────────────────────────────────
    let _ = child.wait();
    let _ = stop_tx.send(());
    let _ = observer.join();

    // ── Authoritative quit position: watch_later file (exact) ─────────────────
    // Duration: from the observer stream (watch_later doesn't store it)
    let (quit_pos, _) = read_watch_later(&watch_dir).unwrap_or((0.0, 0.0));
    let (obs_pos, obs_dur) = *last_known.lock().unwrap();

    // Use watch_later position as authoritative (exact to ms),
    // observer duration as the best duration source
    let final_pos = if quit_pos > 0.0 { quit_pos } else { obs_pos };
    let final_dur = obs_dur;

    // Clean up
    let _ = std::fs::remove_dir_all(&watch_dir);
    let _ = std::fs::remove_file(&socket);

    // Notify app with the authoritative final position
    if !anime_id.is_empty() {
        if let Some(ref t) = tx {
            let _ = t.send(PlaybackEvent::Finished {
                anime_id: anime_id.to_string(),
                episode:  episode.to_string(),
                position: final_pos,
                duration: final_dur,
            });
        }
    }

    Ok((final_pos, final_dur))
}

// ── watch_later reader ────────────────────────────────────────────────────────

/// Read `start=<seconds>` from the single file mpv writes in watch_later_dir.
/// mpv names the file after an MD5 hash of the URL so we just grab whatever
/// file is in the dir — there will be exactly one (or zero if the episode
/// was less than a few seconds in).
fn read_watch_later(dir: &str) -> Result<(f64, f64)> {
    let entry = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_file())
        .ok_or_else(|| anyhow!("no watch_later file"))?;

    let content = std::fs::read_to_string(entry.path())?;

    // Format: one key=value per line, e.g.:
    //   start=842.123000
    //   volume=100
    let pos = content.lines()
        .find(|l| l.starts_with("start="))
        .and_then(|l| l["start=".len()..].trim().parse::<f64>().ok())
        .unwrap_or(0.0);

    Ok((pos, 0.0))   // watch_later doesn't store duration; we already have it in DB
}

// ── observe_property stream ───────────────────────────────────────────────────

/// Connects to the IPC socket, subscribes to time-pos and duration changes,
/// and reads the event stream until the socket closes or stop is signalled.
/// Sends Position events to the app. Checkpoints to DB every 30 seconds.
fn observe_stream(
    socket:     &str,
    anime_id:   &str,
    episode:    &str,
    tx:         Option<mpsc::UnboundedSender<PlaybackEvent>>,
    stop:       std::sync::mpsc::Receiver<()>,
    last_known: std::sync::Arc<std::sync::Mutex<(f64, f64)>>,
) {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    // Give mpv time to create the socket
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Retry connect up to 10 times (mpv may take a moment to start)
    let mut stream = None;
    for _ in 0..10 {
        if stop.try_recv().is_ok() { return; }
        match UnixStream::connect(socket) {
            Ok(s) => { stream = Some(s); break; }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(300)),
        }
    }
    let Some(mut stream) = stream else { return };

    // 2s read timeout so we can check stop signal periodically
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));

    // Subscribe to time-pos (id=1) and duration (id=2)
    let subscribe = concat!(
        "{\"command\":[\"observe_property\",1,\"time-pos\"]}\n",
        "{\"command\":[\"observe_property\",2,\"duration\"]}\n",
    );
    if stream.write_all(subscribe.as_bytes()).is_err() { return; }

    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(reader_stream);

    let mut cur_pos: f64 = 0.0;
    let mut cur_dur: f64 = 0.0;
    let mut last_checkpoint = std::time::Instant::now();

    loop {
        // Check stop signal
        if stop.try_recv().is_ok() { break; }

        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,  // EOF — mpv closed the socket
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                   || e.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout — just loop and check stop signal
                continue;
            }
            Err(_) => break,  // Real error — mpv likely exited
        }

        // Parse the event line
        // {"event":"property-change","id":1,"data":842.5}
        // {"event":"property-change","id":2,"data":1440.0}
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
            if v["event"] == "property-change" {
                match v["id"].as_u64() {
                    Some(1) => { cur_pos = v["data"].as_f64().unwrap_or(cur_pos); }
                    Some(2) => { cur_dur = v["data"].as_f64().unwrap_or(cur_dur); }
                    _ => {}
                }

                // Always update last_known so launch_mpv_tracked has fresh data on exit
                if cur_pos > 0.0 || cur_dur > 0.0 {
                    if let Ok(mut lk) = last_known.lock() {
                        *lk = (cur_pos, cur_dur);
                    }
                }

                // Send live position update to UI
                if let Some(ref t) = tx {
                    if cur_pos > 0.0 {
                        let is_checkpoint = last_checkpoint.elapsed().as_secs() >= 30;
                        let _ = t.send(PlaybackEvent::Position {
                            anime_id:   anime_id.to_string(),
                            episode:    episode.to_string(),
                            position:   cur_pos,
                            duration:   cur_dur,
                            checkpoint: is_checkpoint,
                        });
                        if is_checkpoint {
                            last_checkpoint = std::time::Instant::now();
                        }
                    }
                }
            }
        }
    }
}

// ── episodes_list ─────────────────────────────────────────────────────────────

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

    eps.sort_by(|a, b| {
        let an: f64 = a.parse().unwrap_or(0.0);
        let bn: f64 = b.parse().unwrap_or(0.0);
        an.partial_cmp(&bn).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(eps)
}

// ── get_episode_url ───────────────────────────────────────────────────────────

async fn get_episode_url(id: &str, ep: u32, mode: &str, quality: &str) -> Result<(String, Option<String>)> {
    let gql = r#"query ($showId: String!, $translationType: VaildTranslationTypeEnumType!, $episodeString: String!) { episode( showId: $showId translationType: $translationType episodeString: $episodeString ) { episodeString sourceUrls }}"#;
    let vars = format!(
        r#"{{"showId":"{}","translationType":"{}","episodeString":"{}"}}"#,
        id, mode, ep
    );

    let text = client().get(ALLANIME_API.to_string())
        .query(&[("variables", &vars), ("query", &gql.to_string())])
        .send().await?.text().await?;

    let normalized = text
        .replace('{', "\n").replace('}', "\n")
        .replace("\\u002F", "/").replace('\\', "");

    let mut providers: Vec<(String, String)> = Vec::new();
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

    let mut all_links: Vec<(String, String, Option<String>)> = Vec::new();
    let client = client();
    let mut set = tokio::task::JoinSet::new();

    for (_name, encoded) in &providers {
        let path = hex_decipher(encoded);
        if path.is_empty() { continue; }
        let c = client.clone();
        set.spawn(async move { get_links(&c, &path).await });
    }

    while let Some(res) = set.join_next().await {
        if let Ok(Ok(links)) = res {
            if !links.is_empty() { all_links.extend(links); break; }
        }
    }

    if all_links.is_empty() {
        bail!(
            "No playable links found for episode {ep}.\nProviders tried: {}\n\
             Install ani-cli for best compatibility:\nsudo apt install ani-cli",
            providers.iter().map(|(n,_)| n.as_str()).collect::<Vec<_>>().join(", ")
        );
    }

    all_links.sort_by(|a, b| {
        let an: u32 = a.0.replace('p', "").parse().unwrap_or(0);
        let bn: u32 = b.0.replace('p', "").parse().unwrap_or(0);
        bn.cmp(&an)
    });

    let selected = match quality {
        "best"  => all_links.first(),
        "worst" => all_links.last(),
        q => all_links.iter().find(|(res, _, _)| res.contains(q))
                 .or_else(|| all_links.first()),
    };

    let (_, url, refr) = selected.ok_or_else(|| anyhow!("No link selected"))?;
    Ok((url.clone(), refr.clone()))
}

// ── get_links ─────────────────────────────────────────────────────────────────

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
    let separated = response.replace("},{", "\n");

    let mut links: Vec<(String, String, Option<String>)> = Vec::new();
    let mut m3u8_refr: Option<String> = None;

    for line in separated.lines() {
        if let Some(refr) = extract_between(line, "\"Referer\":\"", "\"") {
            m3u8_refr = Some(refr.to_string());
        }
    }

    for chunk in separated.split('\n') {
        if let (Some(link), Some(res)) = (
            extract_between(chunk, "\"link\":\"", "\""),
            extract_between(chunk, "\"resolutionStr\":\"", "\""),
        ) {
            let link = link.replace("\\u002F", "/").replace("\\/", "/");
            if link.starts_with("http") {
                if link.contains("repackager.wixmp.com") {
                    links.extend(expand_wixmp(&link));
                } else {
                    links.push((res.to_string(), link, None));
                }
            }
        }

        if chunk.contains("\"hls\"") && chunk.contains("\"hardsub_lang\":\"en-US\"") {
            if let Some(hls) = extract_between(chunk, "\"url\":\"", "\"") {
                let hls = hls.replace("\\u002F", "/").replace("\\/", "/");
                if hls.starts_with("http") {
                    links.push(("1080p".to_string(), hls, m3u8_refr.clone()));
                }
            }
        }
    }

    let master_link = links.iter().find(|(_, u, _)| u.contains("master.m3u8")).cloned();
    if let Some((_, master_url, _)) = master_link {
        if let Ok(m3u8_links) = parse_master_m3u8(client, &master_url, m3u8_refr.as_deref()).await {
            if !m3u8_links.is_empty() { links = m3u8_links; }
        }
    }

    if url.contains("tools.fast4speed.rsvp") && links.is_empty() {
        links.push(("Yt".to_string(), url.clone(), Some(ALLANIME_REFR.to_string())));
    }

    Ok(links)
}

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
        if line.starts_with("#EXT-X-STREAM-INF") {
            current_res = line.split("RESOLUTION=")
                .nth(1)
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.split('x').last())
                .map(|h| format!("{h}p"))
                .unwrap_or_else(|| "unknown".to_string());
        } else if !line.starts_with('#') && !line.is_empty() {
            let full_url = if line.starts_with("http") { line.to_string() }
                           else { format!("{base}{line}") };
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
                    let clean_base = base_path
                        .trim_start_matches("https://")
                        .trim_start_matches("http://");
                    (r.to_string(), format!("https://{clean_base}/{r}{suffix}"), None)
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