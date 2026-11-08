//! Launch at login, starting minimized to the tray.
//!
//! Uses tauri-plugin-autostart (on Windows: HKCU\...\CurrentVersion\Run). The
//! registered command line includes `--minimized`, so a login launch stays in
//! the tray; a normal launch shows the window.
//!
//! Only release builds register themselves: a debug build needs the Vite dev
//! server and would just show a blank window at login.

use std::path::Path;

use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

pub const MINIMIZED_ARG: &str = "--minimized";

/// Was this process started by the autostart entry?
pub fn launched_minimized() -> bool {
    std::env::args().any(|a| a == MINIMIZED_ARG)
}

/// Autostart is only offered in release builds.
pub fn is_supported() -> bool {
    !cfg!(debug_assertions)
}

pub fn is_enabled<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, on: bool) {
    if !is_supported() {
        return;
    }
    let result = if on { app.autolaunch().enable() } else { app.autolaunch().disable() };
    if let Err(e) = result {
        eprintln!("[autostart] failed to {}: {e}", if on { "enable" } else { "disable" });
    }
}

/// Release builds: turn autostart on the first time the app runs (the user can
/// switch it off from the tray menu afterwards, and that choice sticks).
/// If it's already on, re-register so the entry points at this exe even if
/// the app was moved or reinstalled elsewhere.
pub fn init<R: Runtime>(app: &AppHandle<R>, config_dir: &Path) {
    if !is_supported() {
        return;
    }
    let marker = config_dir.join("autostart-initialized");
    if !marker.exists() {
        set_enabled(app, true);
        let _ = std::fs::create_dir_all(config_dir);
        let _ = std::fs::write(&marker, "");
    } else if is_enabled(app) {
        set_enabled(app, true); // refresh the registered path
    }
}
