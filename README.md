# YTM_SPlayer

A small always-on-top "now playing" bar for **YouTube Music on Windows**, built with Tauri 2 (Rust) + React.

It reads what's playing from Windows' media controls (SMTC), so it works with the YouTube Music app installed from Edge/Chrome, the YouTube Music desktop app, or a browser tab — no login or API key needed.

## Features

- Cover art, full title (including Japanese titles) and artist; long titles scroll
- Hover the song info to see "Artist • Album • Year"
- **Swipe right** on the bar (or move off its right edge) to reveal previous / play-pause / next
- Frameless, transparent, always on top; drag anywhere to move
- Right-click menu: switch media source (auto prefers YouTube Music; choice is remembered), adjust bar width with a slider, minimize to tray, quit
- System tray: left-click to show/hide, right-click for *Launch at login* and *Quit*
- Launch at login starts minimized to the tray (release builds)

## How it works

| Part | Where |
|---|---|
| Reading playback info & controls via `GlobalSystemMediaTransportControlsSessionManager` | `src-tauri/src/media.rs` |
| Full title from the YouTube Music window title; album/year from YouTube Music's web API | `src-tauri/src/ytm_meta.rs` |
| Hover / swipe detection and click-through for the transparent area | `src-tauri/src/hover.rs` |
| Bar width (persisted) and width-adjust mode | `src-tauri/src/layout.rs` |
| Right-click menu, tray, launch at login | `context_menu.rs`, `tray.rs`, `autostart.rs` |
| UI | `src/` (React + TypeScript) |

Edge's YouTube Music app only reports a short title and artist to SMTC, so the album and year are looked up once per track from `music.youtube.com`. This sends the song title and artist to YouTube. If the lookup fails, the basic info is shown.

## Requirements

- Windows 10/11
- [Rust](https://www.rust-lang.org/tools/install), [Node.js](https://nodejs.org/) 20+, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/)

## Develop

```sh
npm install
npm run tauri dev
```

Tests (`dump_sessions` prints what SMTC currently sees):

```sh
cd src-tauri
cargo test -- --nocapture
```

## Build

```sh
npm run tauri build
```

Installers are written to `src-tauri/target/release/bundle/`. The first time a release build runs it registers itself to launch at login (can be turned off from the tray menu).

## Icon

`app-icon.svg` is the icon source. After editing it, regenerate all sizes with `npx tauri icon app-icon.svg`.
