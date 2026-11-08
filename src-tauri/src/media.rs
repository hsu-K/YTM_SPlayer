//! Reads playback info from Windows SMTC (GlobalSystemMediaTransportControls).
//!
//! A dedicated worker thread owns every WinRT object. SMTC events only send a
//! signal through a channel; the worker then re-reads the state and emits a
//! `media-state` event to the frontend whenever it changes. A 1 s timeout acts
//! as a polling fallback because browsers don't always fire events reliably.
//!
//! Source selection: `None` = auto (YouTube Music desktop app first), or a
//! specific SMTC AppUserModelId chosen by the user. The choice is persisted to
//! `<app_config_dir>/settings.json`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as MediaProperties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::core::Result as WinResult;

use crate::ytm_meta;

pub const EVENT_NAME: &str = "media-state";

/// Highest priority: YouTube Music installed as an Edge app (PWA). Its AUMID
/// looks like `music.youtube.com-5929F88E_vezhnr0wkvrcy!App`.
const YTM_PREFERRED: &[&str] = &["music.youtube.com"];

/// Other YouTube Music apps (th-ch / pear desktop, ytmdesktop, Chromium PWA id).
const YTM_APPS: &[&str] = &[
    "th-ch.youtube-music",
    "youtube-music",
    "youtube music",
    "youtubemusic",
    "ytmdesktop",
    "pear-desktop",
    "cinhimbnkkaeohfgghhklpknlkffjgod",
];

/// Browsers that might be playing music.youtube.com in a tab.
const BROWSERS: &[&str] = &[
    "chrome", "msedge", "brave", "opera", "vivaldi", "firefox", "arc",
];

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub source_app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// "Artist • Album • Year" from YouTube Music, empty if unavailable.
    pub subtitle: String,
    /// `data:<mime>;base64,...`
    pub thumbnail: Option<String>,
    /// playing | paused | stopped | changing | opened | closed
    pub status: &'static str,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Unix ms at which `position_ms` was valid (used to extrapolate).
    pub position_updated_at_ms: i64,
    pub playback_rate: f64,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_play_pause: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// SMTC AppUserModelId
    pub id: String,
    pub name: String,
    pub is_ytm: bool,
    pub playing: bool,
}

/// Everything the frontend needs, sent as one event.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaSnapshot {
    pub now_playing: Option<NowPlaying>,
    pub sources: Vec<SourceInfo>,
    /// `None` = auto
    pub selected_source: Option<String>,
    /// The manually selected source isn't running, so auto is used instead.
    pub using_fallback: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlAction {
    PlayPause,
    Next,
    Previous,
}

enum Signal {
    SessionsChanged,
    MediaPropertiesChanged,
    Other,
    Control(ControlAction),
    SetSource(Option<String>),
    /// Result of a background YouTube Music metadata lookup.
    Meta(TrackKey, Option<ytm_meta::Extra>),
}

/// (SMTC title, SMTC artist)
type TrackKey = (String, String);

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    source: Option<String>,
}

impl Settings {
    fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &PathBuf) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, json) {
                    eprintln!("[smtc] failed to save settings: {e}");
                }
            }
            Err(e) => eprintln!("[smtc] failed to serialize settings: {e}"),
        }
    }
}

/// Shared with Tauri commands.
pub struct MediaState {
    latest: Arc<Mutex<MediaSnapshot>>,
    tx: Sender<Signal>,
}

impl MediaState {
    pub fn snapshot(&self) -> MediaSnapshot {
        self.latest.lock().unwrap().clone()
    }

    pub fn control(&self, action: ControlAction) -> Result<(), String> {
        self.send(Signal::Control(action))
    }

    pub fn set_source(&self, source: Option<String>) -> Result<(), String> {
        self.send(Signal::SetSource(source))
    }

    fn send(&self, signal: Signal) -> Result<(), String> {
        self.tx
            .send(signal)
            .map_err(|_| "media worker is not running".to_string())
    }
}

pub fn start(app: AppHandle, settings_path: PathBuf) -> MediaState {
    let (tx, rx) = mpsc::channel();
    let settings = Settings::load(&settings_path);
    let latest = Arc::new(Mutex::new(MediaSnapshot {
        selected_source: settings.source.clone(),
        ..Default::default()
    }));
    let worker_latest = latest.clone();
    let worker_tx = tx.clone();
    std::thread::Builder::new()
        .name("smtc-worker".into())
        .spawn(move || worker(app, worker_tx, rx, worker_latest, settings, settings_path))
        .expect("failed to spawn smtc worker thread");
    MediaState { latest, tx }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

struct Subscription {
    session: Session,
    media_token: i64,
    playback_token: i64,
    timeline_token: i64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.session.RemoveMediaPropertiesChanged(self.media_token);
        let _ = self.session.RemovePlaybackInfoChanged(self.playback_token);
        let _ = self.session.RemoveTimelinePropertiesChanged(self.timeline_token);
    }
}

#[derive(Default)]
struct ThumbCache {
    key: Option<(String, String, String, String)>,
    data: Option<String>,
}

fn worker(
    app: AppHandle,
    tx: Sender<Signal>,
    rx: Receiver<Signal>,
    latest: Arc<Mutex<MediaSnapshot>>,
    mut settings: Settings,
    settings_path: PathBuf,
) {
    // Getting the manager can fail transiently (e.g. right after login); retry.
    let manager = loop {
        match Manager::RequestAsync().and_then(|op| op.join()) {
            Ok(m) => break m,
            Err(e) => {
                eprintln!("[smtc] failed to get session manager: {e}, retrying in 5s");
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    };

    let _manager_tokens = subscribe_manager(&manager, &tx);
    let mut subs = subscribe_sessions(&manager, &tx);
    let mut thumbs = ThumbCache::default();
    let mut selected: Option<Session> = None;
    let mut last: Option<MediaSnapshot> = None;
    let mut meta_cache: HashMap<TrackKey, Option<ytm_meta::Extra>> = HashMap::new();
    let mut meta_pending: HashSet<TrackKey> = HashSet::new();
    let mut hold = SourceHold::default();

    loop {
        let mut sessions_changed = false;
        let mut media_changed = false;

        let first_signal = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(s) => Some(s),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // Coalesce bursts of events into a single refresh.
        for signal in first_signal.into_iter().chain(rx.try_iter()) {
            match signal {
                Signal::SessionsChanged => sessions_changed = true,
                Signal::MediaPropertiesChanged => media_changed = true,
                Signal::Other => {}
                Signal::Control(action) => {
                    if let Some(s) = &selected {
                        if let Err(e) = run_control(s, action) {
                            eprintln!("[smtc] control {action:?} failed: {e}");
                        }
                    }
                }
                Signal::SetSource(source) => {
                    settings.source = source;
                    settings.save(&settings_path);
                    thumbs.key = None;
                    hold = SourceHold::default(); // user's choice applies immediately
                }
                Signal::Meta(key, extra) => {
                    meta_pending.remove(&key);
                    if meta_cache.len() > 500 {
                        meta_cache.clear();
                    }
                    meta_cache.insert(key, extra);
                }
            }
        }

        if sessions_changed {
            subs.clear(); // unsubscribe old handlers first
            subs = subscribe_sessions(&manager, &tx);
        }
        if media_changed {
            // Apps often deliver artwork after the title; force a re-read.
            thumbs.key = None;
        }

        let prev_np = last.as_ref().and_then(|l| l.now_playing.clone());
        let snapshot = match collect(&manager, settings.source.as_deref()) {
            Ok((sources, session, using_fallback)) => {
                // The shown app's session can vanish for a moment (e.g. YouTube
                // Music while changing tracks). Don't flash another source in
                // the meantime: keep the current display for a short grace period.
                let new_id = session
                    .as_ref()
                    .and_then(|s| s.SourceAppUserModelId().ok())
                    .map(|h| h.to_string());
                let present: Vec<&str> = sources.iter().map(|s| s.id.as_str()).collect();
                if hold.should_hold(&present, new_id.as_deref(), Instant::now()) {
                    if let Some(prev) = &last {
                        let held = MediaSnapshot { sources, ..prev.clone() };
                        if last.as_ref() != Some(&held) {
                            *latest.lock().unwrap() = held.clone();
                            let _ = app.emit(EVENT_NAME, &held);
                            last = Some(held);
                        }
                    }
                    continue;
                }

                let now_playing = match &session {
                    Some(s) => match read_now_playing(s, &mut thumbs) {
                        Ok(mut np) => {
                            let is_ytm = s
                                .SourceAppUserModelId()
                                .map(|id| app_kind(&id.to_string()) >= AppKind::YouTubeMusic)
                                .unwrap_or(false);
                            if is_ytm && !np.title.is_empty() {
                                let key = (np.title.clone(), np.artist.clone());
                                match meta_cache.get(&key) {
                                    Some(Some(extra)) => {
                                        np.title = extra.title.clone();
                                        np.subtitle = extra.subtitle.clone();
                                    }
                                    Some(None) => {}
                                    None => {
                                        if meta_pending.insert(key.clone()) {
                                            let tx = tx.clone();
                                            std::thread::spawn(move || {
                                                let extra = ytm_meta::lookup(&key.0, &key.1);
                                                let _ = tx.send(Signal::Meta(key, extra));
                                            });
                                        }
                                    }
                                }
                            }
                            Some(np)
                        }
                        Err(e) => {
                            eprintln!("[smtc] failed to read session: {e}");
                            prev_np
                        }
                    },
                    None => None,
                };
                selected = session;
                MediaSnapshot {
                    now_playing,
                    sources,
                    selected_source: settings.source.clone(),
                    using_fallback,
                }
            }
            Err(e) => {
                eprintln!("[smtc] failed to list sessions: {e}");
                continue;
            }
        };

        if last.as_ref() != Some(&snapshot) {
            *latest.lock().unwrap() = snapshot.clone();
            if let Err(e) = app.emit(EVENT_NAME, &snapshot) {
                eprintln!("[smtc] emit failed: {e}");
            }
            last = Some(snapshot);
        }
    }
}

/// How long to keep showing a source whose session temporarily disappeared.
const SOURCE_HOLD: Duration = Duration::from_secs(3);

/// Remembers which app is shown, and since when it has been missing.
#[derive(Default)]
struct SourceHold {
    shown: Option<String>,
    lost_at: Option<Instant>,
}

impl SourceHold {
    /// `present`: AppUserModelIds currently in SMTC; `chosen`: what selection
    /// would show now. Returns true while the previously shown app is missing
    /// (and wasn't replaced on purpose) for less than SOURCE_HOLD — the caller
    /// should then keep the previous display.
    fn should_hold(&mut self, present: &[&str], chosen: Option<&str>, now: Instant) -> bool {
        if let Some(prev) = self.shown.as_deref() {
            if chosen != Some(prev) && !present.contains(&prev) {
                let since = *self.lost_at.get_or_insert(now);
                if now.duration_since(since) < SOURCE_HOLD {
                    return true;
                }
            }
        }
        self.lost_at = None;
        self.shown = chosen.map(str::to_string);
        false
    }
}

fn subscribe_manager(manager: &Manager, tx: &Sender<Signal>) -> Vec<i64> {
    let mut tokens = Vec::new();
    let t = tx.clone();
    if let Ok(tok) = manager.SessionsChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Signal::SessionsChanged);
        Ok(())
    })) {
        tokens.push(tok);
    }
    let t = tx.clone();
    if let Ok(tok) = manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Signal::SessionsChanged);
        Ok(())
    })) {
        tokens.push(tok);
    }
    tokens
}

fn subscribe_sessions(manager: &Manager, tx: &Sender<Signal>) -> Vec<Subscription> {
    let Ok(sessions) = manager.GetSessions() else {
        return Vec::new();
    };
    sessions
        .into_iter()
        .filter_map(|session| subscribe_session(session, tx).ok())
        .collect()
}

fn subscribe_session(session: Session, tx: &Sender<Signal>) -> WinResult<Subscription> {
    let t = tx.clone();
    let media_token = session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Signal::MediaPropertiesChanged);
        Ok(())
    }))?;
    let t = tx.clone();
    let playback_token = session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Signal::Other);
        Ok(())
    }))?;
    let t = tx.clone();
    let timeline_token = session.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Signal::Other);
        Ok(())
    }))?;
    Ok(Subscription {
        session,
        media_token,
        playback_token,
        timeline_token,
    })
}

// ---------------------------------------------------------------------------
// Source listing & selection
// ---------------------------------------------------------------------------

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum AppKind {
    Other = 0,
    Browser = 1,
    YouTubeMusic = 2,
    YouTubeMusicPreferred = 3,
}

fn app_kind(aumid: &str) -> AppKind {
    let id = aumid.to_ascii_lowercase();
    if YTM_PREFERRED.iter().any(|k| id.contains(k)) {
        AppKind::YouTubeMusicPreferred
    } else if YTM_APPS.iter().any(|k| id.contains(k)) {
        AppKind::YouTubeMusic
    } else if BROWSERS.iter().any(|k| id.contains(k)) {
        AppKind::Browser
    } else {
        AppKind::Other
    }
}

/// Human-friendly name for an AppUserModelId.
fn display_name(aumid: &str) -> String {
    let id = aumid.to_ascii_lowercase();
    let known = [
        ("music.youtube.com", "YouTube Music (Edge)"),
        ("cinhimbnkkaeohfgghhklpknlkffjgod", "YouTube Music (PWA)"),
        ("ytmdesktop", "YouTube Music Desktop"),
        ("youtube", "YouTube Music"),
        ("pear-desktop", "YouTube Music"),
        ("msedge", "Microsoft Edge"),
        ("chrome", "Google Chrome"),
        ("brave", "Brave"),
        ("firefox", "Firefox"),
        ("opera", "Opera"),
        ("vivaldi", "Vivaldi"),
        ("spotify", "Spotify"),
    ];
    if let Some((_, name)) = known.iter().find(|(k, _)| id.contains(k)) {
        return name.to_string();
    }
    // e.g. "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic" or "C:\...\foo.exe"
    let tail = aumid
        .rsplit(['!', '\\', '/'])
        .next()
        .unwrap_or(aumid)
        .trim_end_matches(".exe");
    let tail = tail.rsplit('.').next().filter(|s| !s.is_empty()).unwrap_or(tail);
    if tail.is_empty() { aumid.to_string() } else { tail.to_string() }
}

/// Lists all sessions and picks the one to display.
/// Returns (sources, chosen session, using_fallback).
fn collect(
    manager: &Manager,
    preferred: Option<&str>,
) -> WinResult<(Vec<SourceInfo>, Option<Session>, bool)> {
    let current_id = manager
        .GetCurrentSession()
        .and_then(|s| s.SourceAppUserModelId())
        .map(|h| h.to_string())
        .unwrap_or_default();

    let mut sources = Vec::new();
    let mut manual: Option<Session> = None;
    let mut auto: Option<(i32, Session)> = None;

    for session in manager.GetSessions()? {
        let id = session.SourceAppUserModelId()?.to_string();
        // Several sessions can share one AUMID; keep the first.
        if sources.iter().any(|s: &SourceInfo| s.id == id) {
            continue;
        }
        let kind = app_kind(&id);
        let playing = session
            .GetPlaybackInfo()
            .and_then(|i| i.PlaybackStatus())
            .map(|s| s == Status::Playing)
            .unwrap_or(false);

        sources.push(SourceInfo {
            id: id.clone(),
            name: display_name(&id),
            is_ytm: kind >= AppKind::YouTubeMusic,
            playing,
        });

        if preferred == Some(id.as_str()) {
            manual = Some(session.clone());
        }
        // Auto: YouTube Music app > browser > (never other apps);
        // within the same kind prefer playing, then Windows' current session.
        if kind != AppKind::Other {
            let score = (kind as i32) * 100 + (playing as i32) * 10 + (id == current_id) as i32;
            if auto.as_ref().is_none_or(|(b, _)| score > *b) {
                auto = Some((score, session));
            }
        }
    }

    // Highest-priority app first, then the rest alphabetically.
    sources.sort_by(|a, b| {
        app_kind(&b.id)
            .cmp(&app_kind(&a.id))
            .then_with(|| a.name.cmp(&b.name))
    });

    let auto = auto.map(|(_, s)| s);
    Ok(match preferred {
        None => (sources, auto, false),
        Some(_) => match manual {
            Some(s) => (sources, Some(s), false),
            None => (sources, auto, true),
        },
    })
}

// ---------------------------------------------------------------------------
// Reading a session
// ---------------------------------------------------------------------------

fn read_now_playing(session: &Session, thumbs: &mut ThumbCache) -> WinResult<NowPlaying> {
    let source_app = session.SourceAppUserModelId()?.to_string();
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let title = props.Title()?.to_string();
    let artist = props.Artist()?.to_string();
    let album = props.AlbumTitle()?.to_string();

    let key = (source_app.clone(), title.clone(), artist.clone(), album.clone());
    if thumbs.key.as_ref() != Some(&key) || thumbs.data.is_none() {
        thumbs.data = read_thumbnail(&props).ok().flatten();
        thumbs.key = Some(key);
    }

    let info = session.GetPlaybackInfo()?;
    let status = match info.PlaybackStatus()? {
        Status::Playing => "playing",
        Status::Paused => "paused",
        Status::Stopped => "stopped",
        Status::Changing => "changing",
        Status::Opened => "opened",
        _ => "closed",
    };
    let playback_rate = info
        .PlaybackRate()
        .and_then(|r| r.Value())
        .unwrap_or(1.0);
    let controls = info.Controls()?;

    let timeline = session.GetTimelineProperties()?;
    let start = timeline.StartTime()?.Duration;
    let position_ms = (timeline.Position()?.Duration - start) / 10_000;
    let duration_ms = (timeline.EndTime()?.Duration - start) / 10_000;
    let updated = filetime_to_unix_ms(timeline.LastUpdatedTime()?.UniversalTime);
    let position_updated_at_ms = if updated > 0 { updated } else { now_unix_ms() };

    Ok(NowPlaying {
        source_app: display_name(&source_app),
        title,
        artist,
        album,
        subtitle: String::new(),
        thumbnail: thumbs.data.clone(),
        status,
        position_ms: position_ms.max(0),
        duration_ms: duration_ms.max(0),
        position_updated_at_ms,
        playback_rate,
        can_prev: controls.IsPreviousEnabled().unwrap_or(false),
        can_next: controls.IsNextEnabled().unwrap_or(false),
        can_play_pause: controls.IsPlayPauseToggleEnabled().unwrap_or(false),
    })
}

fn read_thumbnail(props: &MediaProperties) -> WinResult<Option<String>> {
    let Ok(reference) = props.Thumbnail() else {
        return Ok(None); // no artwork
    };
    let stream = reference.OpenReadAsync()?.join()?;
    let size = stream.Size()? as u32;
    if size == 0 {
        return Ok(None);
    }
    let mime = stream.ContentType()?.to_string();
    let mime = if mime.is_empty() { "image/png".to_string() } else { mime };

    let reader = DataReader::CreateDataReader(&stream)?;
    reader.LoadAsync(size)?.join()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf)?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok(Some(format!("data:{mime};base64,{b64}")))
}

fn run_control(session: &Session, action: ControlAction) -> WinResult<bool> {
    match action {
        ControlAction::PlayPause => session.TryTogglePlayPauseAsync()?.join(),
        ControlAction::Next => session.TrySkipNextAsync()?.join(),
        ControlAction::Previous => session.TrySkipPreviousAsync()?.join(),
    }
}

/// WinRT DateTime = 100ns ticks since 1601-01-01.
fn filetime_to_unix_ms(ticks: i64) -> i64 {
    const EPOCH_DIFF: i64 = 116_444_736_000_000_000;
    if ticks <= EPOCH_DIFF {
        return 0;
    }
    (ticks - EPOCH_DIFF) / 10_000
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diagnostic: `cargo test -- --nocapture` prints what SMTC currently sees.
    #[test]
    fn dump_sessions() {
        let manager = Manager::RequestAsync().and_then(|op| op.join()).unwrap();
        let (sources, session, _) = collect(&manager, None).unwrap();
        for s in &sources {
            println!("source: {s:?}");
        }
        let mut thumbs = ThumbCache::default();
        match session {
            Some(s) => {
                let mut np = read_now_playing(&s, &mut thumbs).unwrap();
                np.thumbnail = np
                    .thumbnail
                    .map(|t| format!("{}... ({} chars)", &t[..30.min(t.len())], t.len()));
                println!("auto picked: {np:#?}");
            }
            None => println!("no matching session"),
        }
    }

    /// Diagnostic (read-only): watches SMTC sessions every 50 ms until the
    /// YouTube Music track changes, printing every change of the session list.
    /// `cargo test watch_track_change -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn watch_track_change() {
        let manager = Manager::RequestAsync().and_then(|op| op.join()).unwrap();
        let snap = || -> String {
            let mut parts = Vec::new();
            for s in manager.GetSessions().unwrap() {
                let id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
                let status = s
                    .GetPlaybackInfo()
                    .and_then(|i| i.PlaybackStatus())
                    .map(|st| st.0)
                    .unwrap_or(-1);
                let title = s
                    .TryGetMediaPropertiesAsync()
                    .and_then(|o| o.join())
                    .and_then(|p| p.Title())
                    .map(|t| t.to_string())
                    .unwrap_or_default();
                parts.push(format!("{}[st={status}]'{title}'", display_name(&id)));
            }
            parts.join(" | ")
        };
        let start = Instant::now();
        let mut last = snap();
        println!("{:>6}ms {last}", 0);
        let first_ytm = last.split(" | ").find(|p| p.starts_with("YouTube Music")).map(str::to_string);
        let mut changed_track = false;
        while start.elapsed() < Duration::from_secs(300) {
            std::thread::sleep(Duration::from_millis(50));
            let now = snap();
            if now != last {
                println!("{:>6}ms {now}", start.elapsed().as_millis());
                let ytm = now.split(" | ").find(|p| p.starts_with("YouTube Music")).map(str::to_string);
                if ytm.is_some() && ytm != first_ytm && !changed_track {
                    changed_track = true;
                }
                last = now;
            }
            // keep watching 3 s after the track changed, then stop
            if changed_track && start.elapsed() > Duration::from_secs(1) {
                std::thread::sleep(Duration::from_secs(3));
                println!("{:>6}ms {}", start.elapsed().as_millis(), snap());
                break;
            }
        }
    }

    #[test]
    fn source_hold_bridges_short_gaps() {
        let ytm = "music.youtube.com-5929F88E_vezhnr0wkvrcy!App";
        let t0 = Instant::now();
        let mut h = SourceHold::default();
        assert!(!h.should_hold(&[ytm, "Chrome"], Some(ytm), t0)); // showing YTM

        // YTM vanishes while changing tracks; auto would pick Chrome -> hold
        assert!(h.should_hold(&["Chrome"], Some("Chrome"), t0 + Duration::from_millis(100)));
        assert!(h.should_hold(&["Chrome"], Some("Chrome"), t0 + Duration::from_millis(900)));
        // YTM is back -> show it, no switch happened
        assert!(!h.should_hold(&[ytm, "Chrome"], Some(ytm), t0 + Duration::from_secs(1)));

        // gone for longer than the grace period -> switch to Chrome
        assert!(h.should_hold(&["Chrome"], Some("Chrome"), t0 + Duration::from_secs(2)));
        assert!(!h.should_hold(&["Chrome"], Some("Chrome"), t0 + Duration::from_secs(6)));
        assert_eq!(h.shown.as_deref(), Some("Chrome"));
    }

    #[test]
    fn source_hold_does_not_block_deliberate_switches() {
        let t0 = Instant::now();
        let mut h = SourceHold::default();
        assert!(!h.should_hold(&["A", "B"], Some("A"), t0));
        // A still exists but selection now prefers B (e.g. user picked B) -> no hold
        assert!(!h.should_hold(&["A", "B"], Some("B"), t0));
        assert_eq!(h.shown.as_deref(), Some("B"));
    }

    #[test]
    fn ytm_has_priority_and_names() {
        let edge_ytm = "music.youtube.com-5929F88E_vezhnr0wkvrcy!App";
        assert!(app_kind(edge_ytm) > app_kind("com.github.th-ch.youtube-music"));
        assert!(app_kind(edge_ytm) > app_kind("MSEdge"));
        assert_eq!(display_name(edge_ytm), "YouTube Music (Edge)");
        assert!(app_kind("com.github.th-ch.youtube-music") > app_kind("Chrome"));
        assert!(app_kind("Chrome") > app_kind("Spotify.exe"));
        assert_eq!(display_name("com.github.th-ch.youtube-music"), "YouTube Music");
        assert_eq!(display_name("Chrome"), "Google Chrome");
        assert_eq!(
            display_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "ZuneMusic"
        );
        assert_eq!(display_name(r"C:\Apps\foobar2000.exe"), "foobar2000");
    }
}
