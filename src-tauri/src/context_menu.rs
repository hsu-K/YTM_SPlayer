//! Native right-click menu for the main window, built and handled in Rust.
//!
//! The menu used to be created from JS (`Menu.new` + `menu.popup()`). That
//! popup ran a modal menu loop on the main thread *inside* WebView2's IPC
//! callback, a re-entrancy WebView2 doesn't tolerate well and which could hang
//! the app. Now the frontend only calls the async `show_context_menu` command
//! (runs off the main thread); `popup_menu` dispatches to the event loop, and
//! clicks arrive through `handle_menu_event` — no JS callbacks involved.

use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

use crate::tray;

const ID_PREFIX: &str = "ctx:";
const ID_SOURCE_AUTO: &str = "ctx:src-auto";
const ID_SOURCE: &str = "ctx:src:"; // followed by the AppUserModelId
const ID_ADJUST_WIDTH: &str = "ctx:adjust-width";
const ID_HIDE: &str = "ctx:hide";
const ID_QUIT: &str = "ctx:quit";

#[tauri::command]
pub async fn show_context_menu(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    let menu = build_menu(&app).map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let sources = source_submenu(app)?;
    let width = MenuItem::with_id(app, ID_ADJUST_WIDTH, "調整寬度…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let hide = MenuItem::with_id(app, ID_HIDE, "最小化到托盤", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "結束", true, None::<&str>)?;
    Menu::with_items(app, &[&sources, &width, &separator, &hide, &quit])
}

#[cfg(windows)]
fn source_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    use crate::media::MediaState;

    let snapshot = app.state::<MediaState>().snapshot();
    let selected = snapshot.selected_source.as_deref();
    let sub = Submenu::new(app, "來源", true)?;

    sub.append(&CheckMenuItem::with_id(
        app,
        ID_SOURCE_AUTO,
        "自動（Edge 版 YouTube Music 優先）",
        true,
        selected.is_none(),
        None::<&str>,
    )?)?;
    for s in &snapshot.sources {
        let label = format!(
            "{}{}{}",
            if s.is_ytm { "★ " } else { "" },
            s.name,
            if s.playing { " ▶" } else { "" }
        );
        sub.append(&CheckMenuItem::with_id(
            app,
            format!("{ID_SOURCE}{}", s.id),
            label,
            true,
            selected == Some(s.id.as_str()),
            None::<&str>,
        )?)?;
    }
    if let Some(sel) = selected {
        if !snapshot.sources.iter().any(|s| s.id == sel) {
            sub.append(&CheckMenuItem::with_id(
                app,
                format!("{ID_SOURCE}{sel}"),
                format!("{sel}（未執行）"),
                true,
                true,
                None::<&str>,
            )?)?;
        }
    }
    if snapshot.using_fallback {
        sub.append(&MenuItem::new(app, "選擇的來源未執行，暫時使用自動", false, None::<&str>)?)?;
    }
    Ok(sub)
}

#[cfg(not(windows))]
fn source_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    Submenu::new(app, "來源", false)
}

/// Registered with `Builder::on_menu_event`; ignores ids that aren't ours
/// (e.g. the tray menu, which has its own handler).
pub fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let id = event.id().as_ref();
    if !id.starts_with(ID_PREFIX) {
        return;
    }
    match id {
        ID_ADJUST_WIDTH => crate::layout::start_adjusting(app),
        ID_HIDE => tray::hide_window(app),
        ID_QUIT => app.exit(0),
        ID_SOURCE_AUTO => set_source(app, None),
        _ => {
            if let Some(source) = id.strip_prefix(ID_SOURCE) {
                set_source(app, Some(source.to_string()));
            }
        }
    }
}

#[cfg(windows)]
fn set_source<R: Runtime>(app: &AppHandle<R>, source: Option<String>) {
    if let Err(e) = app.state::<crate::media::MediaState>().set_source(source) {
        eprintln!("[menu] set_source failed: {e}");
    }
}

#[cfg(not(windows))]
fn set_source<R: Runtime>(_app: &AppHandle<R>, _source: Option<String>) {}
