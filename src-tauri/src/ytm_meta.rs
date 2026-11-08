//! Extra metadata for YouTube Music, which SMTC doesn't provide.
//!
//! Edge's YouTube Music app only reports e.g. title "Ai" / artist "TOMOO" to
//! SMTC. The full title ("アイ - Ai") is in the app's window title, and the
//! byline YouTube Music shows ("TOMOO • <album> • 2026年") is fetched once per
//! track from YouTube Music's web API (the same one music.youtube.com uses).

use std::time::Duration;

use serde_json::{Value, json};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextW, IsWindowVisible,
};
use windows::core::BOOL;

const YTM_SUFFIX: &str = " | YouTube Music";
const API: &str = "https://music.youtube.com/youtubei/v1";
/// Language of the byline (e.g. "2026年"). Matches the user's YTM UI.
const HL: &str = "zh-TW";
const GL: &str = "TW";

#[derive(Debug, Clone, PartialEq)]
pub struct Extra {
    /// Full title as YouTube Music shows it, e.g. "アイ - Ai".
    pub title: String,
    /// "Artist • Album • Year", empty if unknown.
    pub subtitle: String,
}

/// Looks up the full title and byline for a track. Blocking (network); call
/// from a background thread. Returns `None` when nothing reliable was found.
pub fn lookup(smtc_title: &str, artist: &str) -> Option<Extra> {
    let window_title = find_window_title(smtc_title);
    let query = format!("{} {}", window_title.as_deref().unwrap_or(smtc_title), artist);

    let from_api = search_song(&query, smtc_title, artist).and_then(|(video_id, fallback)| {
        let (title, byline) = watch_info(&video_id).unwrap_or_default();
        Some(Extra {
            title: if title.is_empty() { fallback.0 } else { title },
            subtitle: if byline.is_empty() { fallback.1 } else { byline },
        })
    });

    match (from_api, window_title) {
        (Some(mut extra), Some(wt)) => {
            // The window title is what the user sees right now; prefer it.
            extra.title = wt;
            Some(extra)
        }
        (Some(extra), None) => Some(extra),
        (None, Some(wt)) => Some(Extra { title: wt, subtitle: String::new() }),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// Window title
// ---------------------------------------------------------------------------

/// Finds a visible YouTube Music window ("YouTube Music - アイ - Ai | YouTube Music")
/// whose song title contains the SMTC title, and returns "アイ - Ai".
fn find_window_title(smtc_title: &str) -> Option<String> {
    let needle = smtc_title.to_lowercase();
    if needle.is_empty() {
        return None;
    }
    enum_window_titles()
        .into_iter()
        .filter_map(|t| parse_ytm_window_title(&t))
        .find(|t| t.to_lowercase().contains(&needle))
}

fn parse_ytm_window_title(title: &str) -> Option<String> {
    let core = title.strip_suffix(YTM_SUFFIX)?;
    let core = core.strip_prefix("YouTube Music - ").unwrap_or(core).trim();
    (!core.is_empty() && core != "YouTube Music").then(|| core.to_string())
}

fn enum_window_titles() -> Vec<String> {
    enum_chromium_windows().into_iter().map(|(_, t)| t).collect()
}

/// Visible top-level Chromium windows (Chrome, Edge, their app windows) with titles.
fn enum_chromium_windows() -> Vec<(HWND, String)> {
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Vec passed below, alive for the call.
        let windows = unsafe { &mut *(lparam.0 as *mut Vec<(HWND, String)>) };
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let mut class = [0u16; 64];
                let n = GetClassNameW(hwnd, &mut class) as usize;
                if String::from_utf16_lossy(&class[..n]).starts_with("Chrome_WidgetWin") {
                    let mut buf = [0u16; 512];
                    let n = GetWindowTextW(hwnd, &mut buf) as usize;
                    if n > 0 {
                        windows.push((hwnd, String::from_utf16_lossy(&buf[..n])));
                    }
                }
            }
        }
        BOOL(1)
    }

    let mut windows: Vec<(HWND, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut windows as *mut _ as isize));
    }
    windows
}

// ---------------------------------------------------------------------------
// YouTube Music web API
// ---------------------------------------------------------------------------

fn call(endpoint: &str, mut body: Value) -> Option<Value> {
    body["context"] = json!({
        "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": "1.20251101.01.00",
            "hl": HL,
            "gl": GL,
        }
    });
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into();
    let mut resp = agent
        .post(format!("{API}/{endpoint}?prettyPrint=false"))
        .header("Origin", "https://music.youtube.com")
        .send_json(&body)
        .map_err(|e| eprintln!("[ytm] {endpoint} request failed: {e}"))
        .ok()?;
    resp.body_mut().read_json::<Value>().ok()
}

fn runs(v: &Value) -> String {
    v["runs"]
        .as_array()
        .map(|a| a.iter().filter_map(|r| r["text"].as_str()).collect())
        .unwrap_or_default()
}

/// Searches songs and returns (videoId, (title, byline-without-duration)) of
/// the first result whose title contains `title` and byline contains `artist`.
fn search_song(query: &str, title: &str, artist: &str) -> Option<(String, (String, String))> {
    const SONGS_FILTER: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D";
    let resp = call("search", json!({ "query": query, "params": SONGS_FILTER }))?;
    let title_l = title.to_lowercase();
    let artist_l = artist.to_lowercase();

    let sections = resp["contents"]["tabbedSearchResultsRenderer"]["tabs"][0]["tabRenderer"]
        ["content"]["sectionListRenderer"]["contents"]
        .as_array()?;
    sections
        .iter()
        .filter_map(|s| s["musicShelfRenderer"]["contents"].as_array())
        .flatten()
        .map(|item| &item["musicResponsiveListItemRenderer"])
        .find_map(|item| {
            let cols = &item["flexColumns"];
            let t = runs(&cols[0]["musicResponsiveListItemFlexColumnRenderer"]["text"]);
            let byline = runs(&cols[1]["musicResponsiveListItemFlexColumnRenderer"]["text"]);
            if !t.to_lowercase().contains(&title_l) || !byline.to_lowercase().contains(&artist_l) {
                return None;
            }
            let video_id = item["playlistItemData"]["videoId"].as_str()?.to_string();
            // "Artist • Album • 5:38" -> drop the duration
            let byline = match byline.rsplit_once(" • ") {
                Some((rest, last)) if last.contains(':') => rest.to_string(),
                _ => byline,
            };
            Some((video_id, (t, byline)))
        })
}

/// Returns (title, "Artist • Album • Year") as shown in YouTube Music's player.
fn watch_info(video_id: &str) -> Option<(String, String)> {
    let resp = call("next", json!({ "videoId": video_id, "isAudioOnly": true }))?;
    let item = &resp["contents"]["singleColumnMusicWatchNextResultsRenderer"]["tabbedRenderer"]
        ["watchNextTabbedResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]
        ["musicQueueRenderer"]["content"]["playlistPanelRenderer"]["contents"][0]
        ["playlistPanelVideoRenderer"];
    Some((runs(&item["title"]), runs(&item["longBylineText"])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_window_title() {
        assert_eq!(
            parse_ytm_window_title("YouTube Music - アイ - Ai | YouTube Music").as_deref(),
            Some("アイ - Ai")
        );
        assert_eq!(parse_ytm_window_title("YouTube Music"), None);
        assert_eq!(parse_ytm_window_title("Something - Google Chrome"), None);
    }

    /// Network test: `cargo test lookup_online -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn lookup_online() {
        let extra = lookup("Snow Country", "Yorushika");
        println!("{extra:?}");
        assert!(extra.is_some());
    }
}
