//! Bar layout: the user-adjustable collapsed width (persisted), and the
//! "adjusting" mode entered from the right-click menu.
//!
//! The window is always exactly as wide as the *expanded* bar
//! (collapsed width + controls area), and HEIGHT tall.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, Runtime, State, WebviewWindow};

/// Bounds of the collapsed bar width (logical px).
pub const MIN_WIDTH: f64 = 140.0;
pub const MAX_WIDTH: f64 = 360.0;
pub const DEFAULT_WIDTH: f64 = 220.0;
/// Extra width revealed when expanded; must match `--controls-width` in App.css.
pub const CONTROLS_WIDTH: f64 = 112.0;
/// Window height; must match `height` in tauri.conf.json.
pub const HEIGHT: f64 = 52.0;

pub const ADJUST_EVENT: &str = "adjust-width";

pub struct Layout {
    collapsed: AtomicU64, // f64 bits
    adjusting: AtomicBool,
    path: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Saved {
    bar_width: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInfo {
    width: f64,
    min: f64,
    max: f64,
    adjusting: bool,
}

impl Layout {
    pub fn load(path: PathBuf) -> Arc<Self> {
        let saved: Saved = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let width = clamp(saved.bar_width.unwrap_or(DEFAULT_WIDTH));
        Arc::new(Self {
            collapsed: AtomicU64::new(width.to_bits()),
            adjusting: AtomicBool::new(false),
            path,
        })
    }

    pub fn collapsed_width(&self) -> f64 {
        f64::from_bits(self.collapsed.load(Ordering::Relaxed))
    }

    pub fn expanded_width(&self) -> f64 {
        self.collapsed_width() + CONTROLS_WIDTH
    }

    pub fn is_adjusting(&self) -> bool {
        self.adjusting.load(Ordering::Relaxed)
    }

    /// Resizes the window to fit the expanded bar.
    pub fn apply_window_size<R: Runtime>(&self, window: &WebviewWindow<R>) {
        if let Err(e) = window.set_size(LogicalSize::new(self.expanded_width(), HEIGHT)) {
            eprintln!("[layout] resize failed: {e}");
        }
    }

    fn info(&self) -> LayoutInfo {
        LayoutInfo {
            width: self.collapsed_width(),
            min: MIN_WIDTH,
            max: MAX_WIDTH,
            adjusting: self.is_adjusting(),
        }
    }

    fn save(&self) {
        let saved = Saved { bar_width: Some(self.collapsed_width()) };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&saved) {
            if let Err(e) = std::fs::write(&self.path, json) {
                eprintln!("[layout] failed to save: {e}");
            }
        }
    }
}

fn clamp(width: f64) -> f64 {
    if width.is_finite() { width.clamp(MIN_WIDTH, MAX_WIDTH).round() } else { DEFAULT_WIDTH }
}

/// Enters width-adjust mode (from the context menu).
pub fn start_adjusting<R: Runtime>(app: &AppHandle<R>) {
    let layout = app.state::<Arc<Layout>>();
    layout.adjusting.store(true, Ordering::Relaxed);
    let _ = app.emit(ADJUST_EVENT, true);
}

#[tauri::command]
pub fn get_layout(layout: State<'_, Arc<Layout>>) -> LayoutInfo {
    layout.info()
}

/// Sets the collapsed width (clamped to the bounds) and resizes the window.
/// `persist` saves it to disk (done once when the user finishes adjusting).
#[tauri::command]
pub fn set_bar_width(
    window: WebviewWindow,
    layout: State<'_, Arc<Layout>>,
    width: f64,
    persist: bool,
) -> f64 {
    let width = clamp(width);
    layout.collapsed.store(width.to_bits(), Ordering::Relaxed);
    layout.apply_window_size(&window);
    if persist {
        layout.save();
    }
    width
}

#[tauri::command]
pub fn set_adjusting(app: AppHandle, layout: State<'_, Arc<Layout>>, adjusting: bool) {
    layout.adjusting.store(adjusting, Ordering::Relaxed);
    let _ = app.emit(ADJUST_EVENT, adjusting);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_is_clamped() {
        assert_eq!(clamp(100.0), MIN_WIDTH);
        assert_eq!(clamp(9999.0), MAX_WIDTH);
        assert_eq!(clamp(250.4), 250.0);
        assert_eq!(clamp(f64::NAN), DEFAULT_WIDTH);
    }
}
