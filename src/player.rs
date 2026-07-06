//! Stream resolution + mpv launcher with IPC observe_property tracking
//! and watch_later-based exact quit-position saving.

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use std::process::Command;
use tokio::sync::mpsc;

pub const ALLANIME_API: &str = "https://api.allanime.day/api";
pub const ALLANIME_BASE: &str = "allanime.day";
pub const ALLANIME_REFR: &str = "https://youtu-chan.com";
pub const AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:150.0) Gecko/20100101 Firefox/150.0";

// ── Public event type ─────────────────────────────────────────────────────────

/// Sent back to App during and after playback.
pub enum PlaybackEvent {
    /// Live position update from observe_property stream (~1s granularity).
    /// `checkpoint=true` means write to DB now (every 30s).
    Position {
        anime_id: String,
        episode: String,
        position: f64,
        duration: f64,
        checkpoint: bool,
    },
    /// mpv exited — position is the authoritative value read from watch_later file.
    Finished {
        anime_id: String,
        episode: String,
        position: f64,
        duration: f64,
    },
}

// ── ani-cli AES-256-CTR decryption for AllAnime ───────────────────────────────

use aes::cipher::{KeyIvInit, StreamCipher};
use ctr::Ctr32BE;
use sha2::{Digest, Sha256};

/// Derive the AES-256 key exactly as ani-cli does: SHA256("Xot36i3lK3:v1")
fn allanime_key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"Xot36i3lK3:v1");
    hasher.finalize().into()
}

/// Decrypt the base64-encoded `tobeparsed` data using AES-256-CTR.
/// Matches ani-cli exactly: skip byte 0, IV at bytes 1-12 (12 bytes), ciphertext at 13..len-16, counter starts at 2.
/// Robustly handles trailing garbage bytes by finding JSON boundary.
fn decipher_tobeparsed(encoded: &str) -> Option<serde_json::Value> {
    // Base64 decode
    let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded) {
        Ok(d) => d,
        Err(e) => {
            crate::debug_log!("Base64 decode failed: {e}");
            return None;
        }
    };

    // ani-cli format: byte 0 = unknown, bytes 1-12 = IV (12 bytes), bytes 13..len-16 = ciphertext, last 16 = auth tag
    if decoded.len() < 1 + 12 + 16 {
        crate::debug_log!("Decoded data too short: {} bytes", decoded.len());
        return None;
    }

    // Extract IV (bytes 1-12, skipping first byte)
    let iv = &decoded[1..13];
    // Extract ciphertext (bytes 13 to len-16, excluding 16-byte auth tag at end)
    let ciphertext = &decoded[13..decoded.len() - 16];

    // Derive AES key: SHA-256 of the secret string
    let key = allanime_key();

    // Construct 16-byte nonce for AES-256-CTR
    // Format: 12-byte IV + 4-byte counter starting at 00000002
    let mut nonce = [0u8; 16];
    nonce[0..12].copy_from_slice(iv);
    nonce[12..16].copy_from_slice(&[0x00, 0x00, 0x00, 0x02]);

    // Decrypt using AES-256-CTR
    let mut cipher = Ctr32BE::<aes::Aes256>::new(&key.into(), &nonce.into());
    let mut plaintext = ciphertext.to_vec();
    cipher.apply_keystream(&mut plaintext);

    // Extract valid JSON from potentially corrupted output
    extract_json_from_decrypted(&plaintext)
}

/// Extract valid JSON array from decrypted bytes that may contain trailing garbage.
/// The decrypted data contains valid JSON followed by random bytes (likely auth tags).
/// Strategy: Scan from the end to find the JSON array boundary, then try parsing.
fn extract_json_from_decrypted(plaintext: &[u8]) -> Option<serde_json::Value> {
    // First attempt: try to parse as valid UTF-8 string directly
    if let Ok(s) = std::str::from_utf8(plaintext) {
        let trimmed = s.trim();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            crate::debug_log!("Decrypted JSON parsed successfully (clean)");
            return Some(v);
        }
    }

    // Second attempt: find the JSON boundary by looking for valid UTF-8 prefix
    // The JSON we expect is an array: [{...}, {...}]
    // Scan backwards from the end to find a valid closing position

    // The valid JSON ends with something like: "streamerId":"allanime"}]}]}
    // We look for the pattern: ]}]} or similar valid JSON endings

    for i in (1..=plaintext.len()).rev() {
        let slice = &plaintext[..i];

        // Quick check: must end with a valid JSON closing character
        let last_byte = slice.last().copied()?;
        if !matches!(last_byte, b'}' | b']' | b'"' | b'0'..=b'9') {
            continue;
        }

        // Try to parse as UTF-8
        if let Ok(s) = std::str::from_utf8(slice) {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as JSON
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                crate::debug_log!(
                    "Found valid JSON ending at byte {}/{} (trimmed {} trailing bytes)",
                    i,
                    plaintext.len(),
                    plaintext.len() - i
                );
                return Some(v);
            }
        }
    }

    // Third attempt: aggressive JSON extraction by searching for common patterns
    // Look for the array structure: starts with [ and contains objects with sourceName
    if let Some(start) = plaintext.iter().position(|&b| b == b'[') {
        // Try progressively smaller slices from the end
        for end in (start + 10..=plaintext.len()).rev() {
            let candidate = &plaintext[start..end];
            if let Ok(s) = std::str::from_utf8(candidate) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(s.trim()) {
                    if v.is_array() {
                        crate::debug_log!("Extracted JSON array from bytes {}..{}", start, end);
                        return Some(v);
                    }
                }
            }
        }
    }

    // Final fallback: log what we found for debugging
    let preview_len = plaintext.len().min(200);
    let valid_prefix_len = plaintext
        .iter()
        .position(|&b| b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t')
        .unwrap_or(plaintext.len());

    crate::debug_log!(
        "JSON extraction failed. Total: {} bytes, valid UTF-8 prefix: {} bytes, preview: {:?}",
        plaintext.len(),
        valid_prefix_len,
        String::from_utf8_lossy(&plaintext[..preview_len])
    );

    None
}

fn hex_decipher(s: &str) -> String {
    let pairs: Vec<&str> = (0..s.len())
        .step_by(2)
        .map(|i| &s[i..=(i + 1).min(s.len() - 1)])
        .filter(|p| p.len() == 2)
        .collect();

    pairs
        .iter()
        .map(|hex| match *hex {
            "79" => "A",
            "7a" => "B",
            "7b" => "C",
            "7c" => "D",
            "7d" => "E",
            "7e" => "F",
            "7f" => "G",
            "70" => "H",
            "71" => "I",
            "72" => "J",
            "73" => "K",
            "74" => "L",
            "75" => "M",
            "76" => "N",
            "77" => "O",
            "68" => "P",
            "69" => "Q",
            "6a" => "R",
            "6b" => "S",
            "6c" => "T",
            "6d" => "U",
            "6e" => "V",
            "6f" => "W",
            "60" => "X",
            "61" => "Y",
            "62" => "Z",
            "59" => "a",
            "5a" => "b",
            "5b" => "c",
            "5c" => "d",
            "5d" => "e",
            "5e" => "f",
            "5f" => "g",
            "50" => "h",
            "51" => "i",
            "52" => "j",
            "53" => "k",
            "54" => "l",
            "55" => "m",
            "56" => "n",
            "57" => "o",
            "48" => "p",
            "49" => "q",
            "4a" => "r",
            "4b" => "s",
            "4c" => "t",
            "4d" => "u",
            "4e" => "v",
            "4f" => "w",
            "40" => "x",
            "41" => "y",
            "42" => "z",
            "08" => "0",
            "09" => "1",
            "0a" => "2",
            "0b" => "3",
            "0c" => "4",
            "0d" => "5",
            "0e" => "6",
            "0f" => "7",
            "00" => "8",
            "01" => "9",
            "15" => "-",
            "16" => ".",
            "67" => "_",
            "46" => "~",
            "02" => ":",
            "17" => "/",
            "07" => "?",
            "1b" => "#",
            "63" => "[",
            "65" => "]",
            "78" => "@",
            "19" => "!",
            "1c" => "$",
            "1e" => "&",
            "10" => "(",
            "11" => ")",
            "12" => "*",
            "13" => "+",
            "14" => ",",
            "03" => ";",
            "05" => "=",
            "1d" => "%",
            _ => "",
        })
        .collect::<String>()
        .replace("/clock", "/clock.json")
}

// ── Public API ────────────────────────────────────────────────────────────────

pub async fn stream_anime(
    show_id: &str,
    episode: u32,
    mode: &str,
    quality: &str,
) -> Result<String> {
    let (url, _) = get_episode_url(show_id, episode, mode, quality).await?;
    Ok(url)
}

pub async fn fetch_episode_list(show_id: &str, mode: &str) -> Result<Vec<String>> {
    // Wrap with timeout to prevent indefinite hangs on network issues
    match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        episodes_list(show_id, mode),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(anyhow!(
            "Episode list fetch timed out after 15 seconds. Network or API may be slow."
        )),
    }
}

/// Launch mpv with full tracking:
/// - `--save-position-on-quit` into an isolated tmp dir → exact quit position
/// - `observe_property` event stream → live position updates (~1s)
///
/// The caller (main loop) is responsible for terminal teardown before calling
/// this and restore afterwards. Returns `(quit_position_seconds, duration_seconds)`.
pub fn launch_mpv_tracked(
    url: &str,
    anime_id: &str,
    episode: &str,
    resume_from: f64,
    tx: Option<mpsc::UnboundedSender<PlaybackEvent>>,
    skip_times: Option<crate::api::allanime::SkipTimes>,
    skip_setting: &str,
) -> Result<(f64, f64)> {
    let url = url
        .replace("https://https://", "https://")
        .replace("http://http://", "http://");

    let needs_referer =
        url.contains("fast4speed") || url.contains("clock.json") || url.contains(".m3u8");

    // Platform-specific paths
    #[cfg(unix)]
    let watch_dir = format!("/tmp/nexus-watch-{}", std::process::id());
    #[cfg(windows)]
    let watch_dir = format!(
        "{}\\nexus-watch-{}",
        std::env::var("TEMP").unwrap_or_else(|_| "C:\\Temp".into()),
        std::process::id()
    );
    let _ = std::fs::create_dir_all(&watch_dir);

    // IPC socket/pipe path
    #[cfg(unix)]
    let socket = format!("/tmp/nexus-mpv-{}.sock", std::process::id());
    #[cfg(windows)]
    let socket = format!("\\\\.\\pipe\\nexus-mpv-{}", std::process::id());

    let mut cmd = Command::new("mpv");
    cmd.arg(&url);

    // Position tracking
    cmd.arg("--save-position-on-quit");
    cmd.arg(format!("--watch-later-dir={watch_dir}"));
    cmd.arg("--watch-later-options=start"); // only save start position, not other state

    // IPC for live observe_property stream
    cmd.arg(format!("--input-ipc-server={socket}"));
    cmd.arg("--idle=no");

    // HTTP headers
    cmd.arg(format!("--http-header-fields-append=User-Agent: {AGENT}"));
    if needs_referer {
        cmd.arg(format!(
            "--http-header-fields-append=Referer: {ALLANIME_REFR}"
        ));
    }

    // Resume from saved position
    if resume_from > 5.0 {
        cmd.arg(format!("--start={resume_from:.1}"));
    }

    // ── Skip intro/outro via skip.lua + --script-opts ─────────────────────────
    // This is the correct approach used by ani-cli/ani-skip:
    // pass timestamps as script-opts, let the Lua script do the seeking
    // internally within mpv (IPC seek from outside is unreliable on streams).
    if let Some(ref st) = skip_times {
        let skip_intro = matches!(skip_setting, "intro" | "both");
        let skip_outro = matches!(skip_setting, "outro" | "both");

        crate::api::allanime::skip_log(&format!("[nexus-skip] skip_setting={skip_setting} skip_intro={skip_intro} skip_outro={skip_outro}"));
        crate::api::allanime::skip_log(&format!(
            "[nexus-skip] intro={:?}",
            st.intro.as_ref().map(|i| (i.start, i.end))
        ));
        crate::api::allanime::skip_log(&format!(
            "[nexus-skip] outro={:?}",
            st.outro.as_ref().map(|o| (o.start, o.end))
        ));

        let lua_path = ensure_skip_lua_installed();
        crate::api::allanime::skip_log(&format!("[nexus-skip] lua_path={:?}", lua_path));

        let mut opts_parts: Vec<String> = Vec::new();
        if skip_intro {
            if let Some(ref i) = st.intro {
                opts_parts.push(format!("nexus_skip-op_start={:.3}", i.start));
                opts_parts.push(format!("nexus_skip-op_end={:.3}", i.end));
            }
        }
        if skip_outro {
            if let Some(ref o) = st.outro {
                opts_parts.push(format!("nexus_skip-ed_start={:.3}", o.start));
                opts_parts.push(format!("nexus_skip-ed_end={:.3}", o.end));
            }
        }
        if !opts_parts.is_empty() {
            let script_opts = opts_parts.join(",");
            crate::api::allanime::skip_log(&format!("[nexus-skip] --script-opts={script_opts}"));
            if let Some(ref path) = lua_path {
                crate::api::allanime::skip_log(&format!(
                    "[nexus-skip] --script={}",
                    path.display()
                ));
                cmd.arg(format!("--script={}", path.display()));
            }
            cmd.arg(format!("--script-opts={script_opts}"));
        } else {
            crate::api::allanime::skip_log(
                "[nexus-skip] no opts_parts built — check skip_intro/outro flags and interval data",
            );
        }
    } else {
        crate::api::allanime::skip_log(&format!(
            "[nexus-skip] skip_times=None — skip_setting={skip_setting}, no timestamps available"
        ));
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to launch mpv: {e}\nInstall: sudo apt install mpv"))?;

    // ── observe_property stream thread ────────────────────────────────────────
    let socket2 = socket.clone();
    let anime_id2 = anime_id.to_string();
    let episode2 = episode.to_string();
    let tx2 = tx.clone();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();

    let last_known = std::sync::Arc::new(std::sync::Mutex::new((0.0f64, 0.0f64)));
    let last_known2 = last_known.clone();

    let observer = std::thread::spawn(move || {
        observe_stream(&socket2, &anime_id2, &episode2, tx2, stop_rx, last_known2);
    });

    // ── Wait for mpv ──────────────────────────────────────────────────────────
    let _ = child.wait();

    // Final IPC read BEFORE stopping observer — captures position on natural end
    // (when mpv ends naturally it doesn't write a watch_later file)
    let ipc_final = ipc_get_position_once(&socket).ok();

    let _ = stop_tx.send(());
    let _ = observer.join();

    // ── Authoritative quit position ────────────────────────────────────────────
    // Priority: watch_later (q/close, exact) > final IPC read > observer last known
    let (wl_pos, _) = read_watch_later(&watch_dir).unwrap_or((0.0, 0.0));
    let (obs_pos, obs_dur) = *last_known.lock().unwrap();
    let (ipc_pos, ipc_dur) = ipc_final.unwrap_or((0.0, 0.0));

    let final_pos = if wl_pos > 0.0 {
        wl_pos
    } else if ipc_pos > 0.0 {
        ipc_pos
    } else {
        obs_pos
    };

    let final_dur = if ipc_dur > 0.0 {
        ipc_dur
    } else if obs_dur > 0.0 {
        obs_dur
    } else {
        0.0
    };

    // Clean up
    let _ = std::fs::remove_dir_all(&watch_dir);
    #[cfg(unix)]
    let _ = std::fs::remove_file(&socket);

    // Notify app with the authoritative final position
    if !anime_id.is_empty() {
        if let Some(ref t) = tx {
            let _ = t.send(PlaybackEvent::Finished {
                anime_id: anime_id.to_string(),
                episode: episode.to_string(),
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
    let pos = content
        .lines()
        .find(|l| l.starts_with("start="))
        .and_then(|l| l["start=".len()..].trim().parse::<f64>().ok())
        .unwrap_or(0.0);

    Ok((pos, 0.0)) // watch_later doesn't store duration; we already have it in DB
}

// ── observe_property stream ───────────────────────────────────────────────────

/// Connects to the IPC socket, subscribes to time-pos and duration changes,
/// and reads the event stream until the socket closes or stop is signalled.
/// Sends Position events to the app. Checkpoints to DB every 30 seconds.
/// Install skip.lua to the user's mpv scripts directory if not already present.
/// The script is embedded as a string constant — no external dependency needed.
fn ensure_skip_lua_installed() -> Option<std::path::PathBuf> {
    const SKIP_LUA: &str = r#"
-- nexus-skip.lua — nexus-tui aniskip integration
local opts = {
    op_start = -1,
    op_end   = -1,
    ed_start = -1,
    ed_end   = -1,
}
require("mp.options").read_options(opts, "nexus_skip")

local last_intro_seek = -10
local last_outro_seek = -10
local COOLDOWN = 3.0

local function do_seek(target, label)
    -- Pause briefly so the stream buffer settles before seeking
    mp.set_property("pause", "yes")
    mp.add_timeout(0.1, function()
        mp.commandv("seek", string.format("%.3f", target), "absolute")
        mp.set_property("pause", "no")
        mp.osd_message("\xe2\x8f\xad " .. label, 2)
    end)
end

mp.observe_property("time-pos", "number", function(_, pos)
    if pos == nil then return end
    local now = mp.get_time()

    if opts.op_start >= 0 and opts.op_end > 0
        and pos >= opts.op_start and pos <= opts.op_end
        and (now - last_intro_seek) > COOLDOWN then
            last_intro_seek = now
            do_seek(opts.op_end, "Skipped intro")
    end

    if opts.ed_start >= 0 and opts.ed_end > 0
        and pos >= opts.ed_start and pos <= opts.ed_end
        and (now - last_outro_seek) > COOLDOWN then
            last_outro_seek = now
            do_seek(opts.ed_end, "Skipped outro")
    end
end)
"#;

    let scripts_dir = mpv_scripts_dir();
    if let Some(dir) = scripts_dir {
        let path = dir.join("nexus-skip.lua");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, SKIP_LUA);
        return Some(path);
    }
    None
}

/// Get the mpv scripts directory for the current platform.
fn mpv_scripts_dir() -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    {
        let home = std::env::var("HOME").ok()?;
        Some(std::path::PathBuf::from(home).join(".config/mpv/scripts"))
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA").ok()?;
        Some(std::path::PathBuf::from(appdata).join("mpv\\scripts"))
    }
    #[cfg(not(any(unix, windows)))]
    None
}

/// One-shot IPC query — returns (time-pos, duration) from mpv socket/pipe.
fn ipc_get_position_once(socket: &str) -> Result<(f64, f64)> {
    use std::io::{BufReader, Write};

    let subscribe = concat!(
        "{\"command\":[\"get_property\",\"time-pos\"]}\n",
        "{\"command\":[\"get_property\",\"duration\"]}\n",
    );

    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let mut stream = UnixStream::connect(socket).map_err(|e| anyhow!("IPC connect: {e}"))?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
        stream.write_all(subscribe.as_bytes())?;
        let mut reader = BufReader::new(stream);
        return parse_two_ipc_responses(&mut reader);
    }

    #[cfg(windows)]
    {
        use std::fs::OpenOptions;
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(socket)
            .map_err(|e| anyhow!("IPC pipe open: {e}"))?;
        f.write_all(subscribe.as_bytes())?;
        let mut reader = BufReader::new(f);
        return parse_two_ipc_responses(&mut reader);
    }

    #[allow(unreachable_code)]
    Err(anyhow!("IPC not supported on this platform"))
}

fn parse_two_ipc_responses(reader: &mut impl std::io::BufRead) -> Result<(f64, f64)> {
    let mut pos = 0.0f64;
    let mut dur = 0.0f64;
    for _ in 0..2 {
        let mut line = String::new();
        if reader.read_line(&mut line).is_ok() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                let val = v["data"].as_f64().unwrap_or(0.0);
                if pos == 0.0 {
                    pos = val;
                } else {
                    dur = val;
                }
            }
        }
    }
    Ok((pos, dur))
}

fn observe_stream(
    socket: &str,
    anime_id: &str,
    episode: &str,
    tx: Option<mpsc::UnboundedSender<PlaybackEvent>>,
    stop: std::sync::mpsc::Receiver<()>,
    last_known: std::sync::Arc<std::sync::Mutex<(f64, f64)>>,
) {
    use std::io::{BufRead, BufReader, Write};

    // Give mpv time to create the socket/pipe
    std::thread::sleep(std::time::Duration::from_millis(500));

    // ── Platform-specific connection ──────────────────────────────────────────
    // Returns a Box<dyn Read+Write> so the rest of the function is platform-agnostic.

    #[cfg(unix)]
    let connected: Option<Box<dyn ipc_rw::IpcStream>> = {
        use std::os::unix::net::UnixStream;
        let mut result = None;
        for _ in 0..10 {
            if stop.try_recv().is_ok() {
                return;
            }
            match UnixStream::connect(socket) {
                Ok(s) => {
                    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    result = Some(Box::new(s) as Box<dyn ipc_rw::IpcStream>);
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(300)),
            }
        }
        result
    };

    #[cfg(windows)]
    let connected: Option<Box<dyn ipc_rw::IpcStream>> = {
        use std::fs::OpenOptions;
        let mut result = None;
        for _ in 0..10 {
            if stop.try_recv().is_ok() {
                return;
            }
            match OpenOptions::new().read(true).write(true).open(socket) {
                Ok(f) => {
                    result = Some(Box::new(f) as Box<dyn ipc_rw::IpcStream>);
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(300)),
            }
        }
        result
    };

    #[cfg(not(any(unix, windows)))]
    let connected: Option<Box<dyn ipc_rw::IpcStream>> = None;

    let Some(mut stream) = connected else { return };

    // Subscribe to time-pos (id=1) and duration (id=2)
    let subscribe = concat!(
        "{\"command\":[\"observe_property\",1,\"time-pos\"]}\n",
        "{\"command\":[\"observe_property\",2,\"duration\"]}\n",
    );
    if stream.write_all(subscribe.as_bytes()).is_err() {
        return;
    }

    let mut reader = BufReader::new(stream);

    let mut cur_pos: f64 = 0.0;
    let mut cur_dur: f64 = 0.0;
    let mut last_checkpoint = std::time::Instant::now();

    loop {
        if stop.try_recv().is_ok() {
            break;
        }

        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue
            }
            Err(_) => break,
        }

        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
            if v["event"] == "property-change" {
                match v["id"].as_u64() {
                    Some(1) => {
                        cur_pos = v["data"].as_f64().unwrap_or(cur_pos);
                    }
                    Some(2) => {
                        cur_dur = v["data"].as_f64().unwrap_or(cur_dur);
                    }
                    _ => {}
                }

                if cur_pos > 0.0 || cur_dur > 0.0 {
                    if let Ok(mut lk) = last_known.lock() {
                        *lk = (cur_pos, cur_dur);
                    }
                }

                // Skip is handled by nexus-skip.lua via --script-opts

                if let Some(ref t) = tx {
                    if cur_pos > 0.0 {
                        let is_checkpoint = last_checkpoint.elapsed().as_secs() >= 30;
                        let _ = t.send(PlaybackEvent::Position {
                            anime_id: anime_id.to_string(),
                            episode: episode.to_string(),
                            position: cur_pos,
                            duration: cur_dur,
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

// ── IPC stream trait ──────────────────────────────────────────────────────────
// Abstracts over UnixStream (Unix) and File/named-pipe (Windows)
mod ipc_rw {
    use std::io::{Read, Write};
    pub trait IpcStream: Read + Write + Send {}
    #[cfg(unix)]
    impl IpcStream for std::os::unix::net::UnixStream {}
    #[cfg(windows)]
    impl IpcStream for std::fs::File {}
}

// ── episodes_list ─────────────────────────────────────────────────────────────

async fn episodes_list(show_id: &str, mode: &str) -> Result<Vec<String>> {
    crate::debug_log!("episodes_list: show_id={show_id}, mode={mode}");
    let gql = r#"query ($showId: String!) { show( _id: $showId ) { _id availableEpisodesDetail }}"#;
    let vars = format!(r#"{{"showId":"{}"}}"#, show_id);

    // Use browser_auth which handles FlareSolverr -> visible browser fallback chain
    let text = crate::browser_auth::fetch_text_with_query(
        ALLANIME_API,
        &[
            ("variables".to_string(), vars),
            ("query".to_string(), gql.to_string()),
        ],
    )
    .await
    .map_err(|e| anyhow!("Episode-list request failed: {e}"))?;

    crate::debug_log!("episodes_list response: {} bytes", text.len());

    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        crate::debug_log!("episodes_list JSON parse error: {e}");
        anyhow!("Episode-list parse error: {e}. Upstream likely returned non-JSON.")
    })?;
    crate::debug_log!("episodes_list parsed JSON successfully");
    let mut eps: Vec<String> =
        if let Some(arr) = json["data"]["show"]["availableEpisodesDetail"][mode].as_array() {
            let count = arr.len();
            crate::debug_log!("episodes_list found {count} episodes for mode {mode}");
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            crate::debug_log!("episodes_list: no episodes found for mode {mode}");
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

async fn get_episode_url(
    id: &str,
    ep: u32,
    mode: &str,
    quality: &str,
) -> Result<(String, Option<String>)> {
    crate::debug_log!("get_episode_url: id={id}, ep={ep}, mode={mode}");

    // Use persisted query for episode sources (bypasses Cloudflare challenges)
    let text = crate::browser_auth::fetch_episode_sources(id, mode, &ep.to_string())
        .await
        .map_err(|e| anyhow!("Episode-source request failed: {e}"))?;

    crate::debug_log!("get_episode_url response: {} bytes", text.len());
    crate::debug_log!("Response preview: {}", &text[..500.min(text.len())]);

    let mut providers: Vec<(String, String)> = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        crate::debug_log!("JSON parsed successfully");
        crate::debug_log!(
            "Full JSON keys: {:?}",
            json.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
        if let Some(data) = json.get("data") {
            crate::debug_log!(
                "data keys: {:?}",
                data.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );
        }

        // Check for new API format with tobeparsed
        if let Some(tobeparsed) = json["data"]["tobeparsed"].as_str() {
            crate::debug_log!(
                "Found tobeparsed field ({} chars), deciphering...",
                tobeparsed.len()
            );
            if let Some(deciphered) = decipher_tobeparsed(tobeparsed) {
                crate::debug_log!("Deciphered data: {:?}", deciphered);
                // The deciphered data structure is: {"episode": {"episodeString": "1", "sourceUrls": [...]}}
                // Extract sourceUrls from the nested episode object
                let source_urls = deciphered["episode"]["sourceUrls"]
                    .as_array()
                    .or_else(|| deciphered.as_array()) // Fallback: handle if it's a raw array (old format)
                    .or_else(|| deciphered["sourceUrls"].as_array()); // Fallback: direct sourceUrls field

                if let Some(source_urls) = source_urls {
                    crate::debug_log!("sourceUrls array found with {} entries", source_urls.len());
                    for (i, entry) in source_urls.iter().enumerate() {
                        let Some(name) = entry["sourceName"].as_str() else {
                            crate::debug_log!("Entry {} missing sourceName", i);
                            continue;
                        };
                        let Some(raw_url) = entry["sourceUrl"].as_str() else {
                            crate::debug_log!("Entry {} missing sourceUrl", i);
                            continue;
                        };
                        crate::debug_log!(
                            "Entry {}: name={}, url starts with --: {}",
                            i,
                            name,
                            raw_url.starts_with("--")
                        );
                        if let Some(encoded) = raw_url.strip_prefix("--") {
                            providers.push((name.to_string(), encoded.to_string()));
                        } else if raw_url.starts_with("http") {
                            // Direct video URL (e.g., tools.fast4speed.rsvp) - handle in get_links
                            providers.push((name.to_string(), raw_url.to_string()));
                        }
                    }
                } else {
                    crate::debug_log!(
                        "No sourceUrls array found in deciphered data. Keys: {:?}",
                        deciphered.as_object().map(|o| o.keys().collect::<Vec<_>>())
                    );
                }
            } else {
                crate::debug_log!("decipher_tobeparsed returned None - decryption failed");
            }
        } else if let Some(arr) = json["data"]["episode"]["sourceUrls"].as_array() {
            // Old API format
            crate::debug_log!("sourceUrls array found with {} entries", arr.len());
            for (i, entry) in arr.iter().enumerate() {
                let Some(name) = entry["sourceName"].as_str() else {
                    crate::debug_log!("Entry {} missing sourceName", i);
                    continue;
                };
                let Some(raw_url) = entry["sourceUrl"].as_str() else {
                    crate::debug_log!("Entry {} missing sourceUrl", i);
                    continue;
                };
                crate::debug_log!(
                    "Entry {}: name={}, url starts with --: {}",
                    i,
                    name,
                    raw_url.starts_with("--")
                );
                if let Some(encoded) = raw_url.strip_prefix("--") {
                    providers.push((name.to_string(), encoded.to_string()));
                } else if raw_url.starts_with("http") {
                    providers.push((name.to_string(), raw_url.to_string()));
                }
            }
        } else {
            crate::debug_log!("Neither tobeparsed nor sourceUrls found in data");
            if let Some(errors) = json.get("errors") {
                crate::debug_log!("GraphQL errors: {:?}", errors);
            }
        }
    } else {
        crate::debug_log!("JSON parse failed");
    }

    if providers.is_empty() {
        let normalized = text
            .replace(['{', '}'], "\n")
            .replace("\\u002F", "/")
            .replace('\\', "");

        for line in normalized.lines() {
            if let (Some(url_part), Some(name_part)) = (
                extract_between(line, "\"sourceUrl\":\"--", "\""),
                extract_between(line, "\"sourceName\":\"", "\""),
            ) {
                providers.push((name_part.to_string(), url_part.to_string()));
            }
            // Also try to extract direct HTTP URLs
            if let (Some(url_part), Some(name_part)) = (
                extract_between(line, "\"sourceUrl\":\"https://", "\""),
                extract_between(line, "\"sourceName\":\"", "\""),
            ) {
                providers.push((name_part.to_string(), format!("https://{}", url_part)));
            }
        }
    }

    if providers.is_empty() {
        crate::debug_log!("get_episode_url: no providers found for ep {ep}");
        bail!("No providers found for episode {ep}. Check show ID and mode ({mode}).");
    }
    crate::debug_log!("get_episode_url: found {} providers", providers.len());

    let mut all_links: Vec<(String, String, Option<String>)> = Vec::new();
    let client = client();
    let mut set = tokio::task::JoinSet::new();

    for (name, encoded) in &providers {
        // Direct HTTP URLs (e.g., fast4speed) don't need hex_decipher
        let path = if encoded.starts_with("http") {
            encoded.to_string()
        } else {
            hex_decipher(encoded)
        };
        if path.is_empty() {
            continue;
        }
        let c = client.clone();
        let _ = name;
        set.spawn(async move { get_links(&c, &path).await });
    }

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(links)) => {
                crate::debug_log!("get_links task returned {} links", links.len());
                if !links.is_empty() {
                    all_links.extend(links);
                    break;
                }
            }
            Ok(Err(e)) => {
                crate::debug_log!("get_links task failed: {e}");
            }
            Err(_) => {}
        }
    }

    if all_links.is_empty() {
        bail!(
            "No playable links found for episode {ep}.\nProviders tried: {}\n\
             Install ani-cli for best compatibility:\nsudo apt install ani-cli",
            providers
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    all_links.sort_by(|a, b| {
        let an: u32 = a.0.replace('p', "").parse().unwrap_or(0);
        let bn: u32 = b.0.replace('p', "").parse().unwrap_or(0);
        bn.cmp(&an)
    });

    let selected = match quality {
        "best" => all_links.first(),
        "worst" => all_links.last(),
        q => all_links
            .iter()
            .find(|(res, _, _)| res.contains(q))
            .or_else(|| all_links.first()),
    };

    let (_, url, refr) = selected.ok_or_else(|| anyhow!("No link selected"))?;
    Ok((url.clone(), refr.clone()))
}

// ── get_links ─────────────────────────────────────────────────────────────────

async fn get_links(
    _client: &reqwest::Client,
    path: &str,
) -> Result<Vec<(String, String, Option<String>)>> {
    crate::debug_log!("get_links: path={path}");
    let url = if path.starts_with("http") {
        path.to_string()
    } else {
        format!("https://{ALLANIME_BASE}{path}")
    };
    crate::debug_log!("get_links: full_url={url}");

    // Fast path: direct video URLs (e.g., tools.fast4speed.rsvp MP4 files)
    // These return binary video data, not JSON provider pages.
    // Detect early to avoid downloading large files unnecessarily.
    if url.contains("tools.fast4speed.rsvp") {
        crate::debug_log!("get_links: fast4speed direct URL detected, returning directly");
        return Ok(vec![(
            "1080p".to_string(),
            url,
            Some(ALLANIME_REFR.to_string()),
        )]);
    }

    // Use browser_auth which handles FlareSolverr -> visible browser fallback chain
    let response = match crate::browser_auth::fetch_text_from_url(&url).await {
        Ok(r) => {
            crate::debug_log!("get_links: fetched {} bytes from {url}", r.len());
            r
        }
        Err(e) => {
            crate::debug_log!("get_links: failed to fetch {url}: {e}");
            return Err(anyhow!("Provider request failed for {url}: {e}"));
        }
    };
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

    let master_link = links
        .iter()
        .find(|(_, u, _)| u.contains("master.m3u8"))
        .cloned();
    if let Some((_, master_url, _)) = master_link {
        let c = client();
        if let Ok(m3u8_links) = parse_master_m3u8(&c, &master_url, m3u8_refr.as_deref()).await {
            if !m3u8_links.is_empty() {
                links = m3u8_links;
            }
        }
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
    if let Some(r) = refr {
        req = req.header("Referer", r);
    }
    let body = req.send().await?.text().await?;

    let mut links = Vec::new();
    let mut current_res = String::from("unknown");

    for line in body.lines() {
        if line.starts_with("#EXT-X-STREAM-INF") {
            current_res = line
                .split("RESOLUTION=")
                .nth(1)
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.split('x').next_back())
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

fn expand_wixmp(url: &str) -> Vec<(String, String, Option<String>)> {
    let stripped = url.replace("repackager.wixmp.com/", "");
    let base = stripped.split(".urlset").next().unwrap_or(&stripped);

    if let Some(res_start) = base.find("/,") {
        let resolutions_part = &base[res_start + 2..];
        if let Some(res_end) = resolutions_part.find('/') {
            let base_path = &base[..res_start];
            let suffix = &resolutions_part[res_end..];
            return resolutions_part[..res_end]
                .split(',')
                .filter(|r| !r.is_empty())
                .map(|r| {
                    let clean_base = base_path
                        .trim_start_matches("https://")
                        .trim_start_matches("http://");
                    (
                        r.to_string(),
                        format!("https://{clean_base}/{r}{suffix}"),
                        None,
                    )
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
        .emulation(wreq_util::Emulation::Chrome140)
        .user_agent(AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                "Referer",
                reqwest::header::HeaderValue::from_static(ALLANIME_REFR),
            );
            h
        })
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use sha2::{Digest, Sha256};

    #[test]
    fn test_allanime_key_derivation() {
        // Verify key matches ani-cli: SHA256("Xot36i3lK3:v1")
        let mut hasher = Sha256::new();
        hasher.update(b"Xot36i3lK3:v1");
        let expected = hasher.finalize();
        let actual = allanime_key();
        assert_eq!(actual.as_slice(), expected.as_slice());
    }

    #[test]
    fn test_hex_decipher_basic_mappings() {
        // Test known mappings from the hex_decipher table
        assert_eq!(hex_decipher("79"), "A");
        assert_eq!(hex_decipher("7a"), "B");
        assert_eq!(hex_decipher("7b"), "C");
        assert_eq!(hex_decipher("59"), "a");
        assert_eq!(hex_decipher("5a"), "b");
        assert_eq!(hex_decipher("08"), "0");
        assert_eq!(hex_decipher("09"), "1");
        assert_eq!(hex_decipher("15"), "-");
        assert_eq!(hex_decipher("16"), ".");
        assert_eq!(hex_decipher("67"), "_");
    }

    #[test]
    fn test_decipher_tobeparsed_structure() {
        // Test that the function exists and handles empty/invalid input
        assert!(decipher_tobeparsed("").is_none());
        assert!(decipher_tobeparsed("invalid_base64!!!").is_none());

        // Test with too-short valid base64
        let short = base64::engine::general_purpose::STANDARD.encode(b"short");
        assert!(decipher_tobeparsed(&short).is_none());
    }
}
