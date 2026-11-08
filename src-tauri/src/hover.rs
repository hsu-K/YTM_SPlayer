//! Hover / swipe tracking for the collapsible bar.
//!
//! The window is transparent and as wide as the *expanded* bar. Only the bar
//! itself is visible: collapsed (cover + text) by default, sliding out to the
//! right to reveal the controls after a rightward swipe over it.
//!
//! A small thread polls the cursor (Win32 only; touches tauri only when a
//! state changes):
//! - cursor over the visible bar  -> window accepts mouse input (drag, right-click)
//! - anywhere else                -> window is click-through, so the transparent
//!                                   strip never blocks clicks to apps below
//! - rightward swipe over the bar -> expand (emit `bar-hover: true`)
//! - cursor leaves the bar        -> collapse (emit `bar-hover: false`)

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{Emitter, WebviewWindow};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect, IsWindowVisible};

use crate::layout::Layout;

pub const EVENT_NAME: &str = "bar-hover";

const POLL: Duration = Duration::from_millis(16);

/// Rightward travel (logical px) needed to expand.
const SWIPE_DISTANCE: f64 = 30.0;
/// The swipe must be mostly horizontal: dx >= SWIPE_RATIO * |dy|.
const SWIPE_RATIO: f64 = 1.5;
/// Pausing longer than this restarts the swipe.
const SWIPE_IDLE_RESET: Duration = Duration::from_millis(400);

/// Bar widths are read from `layout` every tick, so width changes apply live.
pub fn start(window: WebviewWindow, layout: Arc<Layout>) {
    let Ok(hwnd) = window.hwnd() else {
        eprintln!("[hover] no window handle; hover tracking disabled");
        return;
    };
    // Raw handle value: Send-able, and independent of tauri's `windows` version.
    let raw = hwnd.0 as isize;

    let _ = window.set_ignore_cursor_events(true);
    std::thread::Builder::new()
        .name("bar-hover".into())
        .spawn(move || {
            let hwnd = HWND(raw as *mut _);
            let mut over = false; // cursor over the visible bar
            let mut expanded = false;
            let mut swipe = Swipe::default();

            loop {
                std::thread::sleep(POLL);
                let Some(c) = (unsafe { sample(hwnd) }) else {
                    // hidden (e.g. in tray) or API failure: reset everything
                    if over {
                        over = false;
                        let _ = window.set_ignore_cursor_events(true);
                    }
                    if expanded {
                        expanded = false;
                        let _ = window.emit(EVENT_NAME, false);
                    }
                    swipe = Swipe::default();
                    continue;
                };

                // Width-adjust mode: the slider must keep working even when the
                // cursor wanders off the bar mid-drag, so always accept input.
                if layout.is_adjusting() {
                    if !over {
                        over = true;
                        let _ = window.set_ignore_cursor_events(false);
                    }
                    swipe = Swipe::default();
                    continue;
                }

                let collapsed = layout.collapsed_width();
                let expanded_width = layout.expanded_width();

                // Leaving the collapsed bar through its right edge while moving
                // right also expands, even if the swipe was still too short.
                if !expanded && over && exits_right_edge(swipe.last, &c, collapsed, expanded_width) {
                    expanded = true;
                    swipe = Swipe::default();
                    let _ = window.emit(EVENT_NAME, true);
                }

                let visible_width = if expanded { expanded_width } else { collapsed };
                let now_over = c.x >= 0.0 && c.x < visible_width && c.y >= 0.0 && c.y < c.height;

                if now_over != over {
                    over = now_over;
                    let _ = window.set_ignore_cursor_events(!over);
                }

                if !over {
                    swipe = Swipe::default();
                    if expanded {
                        expanded = false;
                        let _ = window.emit(EVENT_NAME, false);
                    }
                    continue;
                }

                if !expanded && swipe.feed(c.x, c.y) {
                    expanded = true;
                    swipe = Swipe::default();
                    let _ = window.emit(EVENT_NAME, true);
                }
            }
        })
        .expect("failed to spawn hover thread");
}

/// Cursor position relative to the window, in logical px. Relative coords mean
/// dragging the window itself (cursor moves with it) never counts as a swipe.
struct Cursor {
    x: f64,
    y: f64,
    height: f64,
}

unsafe fn sample(hwnd: HWND) -> Option<Cursor> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        let mut rect = RECT::default();
        let mut pt = POINT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        GetCursorPos(&mut pt).ok()?;
        let scale = GetDpiForWindow(hwnd) as f64 / 96.0;
        Some(Cursor {
            x: (pt.x - rect.left) as f64 / scale,
            y: (pt.y - rect.top) as f64 / scale,
            height: (rect.bottom - rect.top) as f64 / scale,
        })
    }
}

/// Was the cursor inside the collapsed bar last sample, and has it now crossed
/// the bar's right edge (still within the bar's height) moving right?
/// The area right of the collapsed bar is still inside the (transparent)
/// window, so the cursor is measurable there.
fn exits_right_edge(last: Option<(f64, f64)>, c: &Cursor, collapsed: f64, expanded: f64) -> bool {
    let Some((lx, _)) = last else {
        return false;
    };
    lx < collapsed
        && c.x >= collapsed
        && c.x < expanded
        && c.y >= 0.0
        && c.y < c.height
}

/// Accumulates a rightward, mostly horizontal stroke.
#[derive(Default)]
struct Swipe {
    last: Option<(f64, f64)>,
    last_move: Option<Instant>,
    dx: f64,
    dy: f64,
}

impl Swipe {
    /// Feeds a cursor sample; returns true once a rightward swipe is detected.
    fn feed(&mut self, x: f64, y: f64) -> bool {
        let Some((lx, ly)) = self.last.replace((x, y)) else {
            return false; // first sample after entering: just remember it
        };
        let (dx, dy) = (x - lx, y - ly);
        if dx == 0.0 && dy == 0.0 {
            if self.last_move.is_some_and(|t| t.elapsed() > SWIPE_IDLE_RESET) {
                self.dx = 0.0;
                self.dy = 0.0;
            }
            return false;
        }
        self.last_move = Some(Instant::now());
        if dx < 0.0 {
            // moving left cancels the stroke
            self.dx = 0.0;
            self.dy = 0.0;
            return false;
        }
        self.dx += dx;
        self.dy += dy.abs();
        self.dx >= SWIPE_DISTANCE && self.dx >= SWIPE_RATIO * self.dy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(points: &[(f64, f64)]) -> bool {
        let mut s = Swipe::default();
        points.iter().any(|&(x, y)| s.feed(x, y))
    }

    #[test]
    fn exiting_through_right_edge_expands() {
        let at = |x: f64, y: f64| Cursor { x, y, height: 52.0 };
        let exits = |last, c: &Cursor| exits_right_edge(last, c, 220.0, 332.0);
        assert!(exits(Some((210.0, 30.0)), &at(225.0, 31.0)));
        // still inside, or never was inside, or left above/below the bar
        assert!(!exits(Some((200.0, 30.0)), &at(215.0, 30.0)));
        assert!(!exits(None, &at(225.0, 30.0)));
        assert!(!exits(Some((210.0, 30.0)), &at(225.0, 60.0)));
        // works with a user-adjusted width too
        assert!(exits_right_edge(Some((295.0, 20.0)), &at(305.0, 20.0), 300.0, 412.0));
    }

    #[test]
    fn rightward_swipe_expands() {
        assert!(run(&[(20.0, 30.0), (30.0, 31.0), (40.0, 30.0), (55.0, 32.0)]));
    }

    #[test]
    fn small_or_vertical_or_leftward_moves_do_not_expand() {
        assert!(!run(&[(20.0, 30.0), (35.0, 30.0)])); // too short
        assert!(!run(&[(20.0, 10.0), (30.0, 30.0), (40.0, 50.0), (50.0, 60.0)])); // too vertical
        assert!(!run(&[(60.0, 30.0), (40.0, 30.0), (20.0, 30.0)])); // leftward
        // right 20, back left (reset), right 20 -> still not enough
        assert!(!run(&[(20.0, 30.0), (40.0, 30.0), (35.0, 30.0), (55.0, 30.0)]));
    }
}
