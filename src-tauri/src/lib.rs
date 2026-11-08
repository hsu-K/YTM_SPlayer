#[cfg(windows)]
mod media;
#[cfg(windows)]
mod ytm_meta;
mod tray;
mod context_menu;
mod layout;
mod autostart;
#[cfg(windows)]
mod hover;

#[cfg(windows)]
mod commands {
    use crate::media::{ControlAction, MediaSnapshot, MediaState};

    #[tauri::command]
    pub fn get_media_state(state: tauri::State<'_, MediaState>) -> MediaSnapshot {
        state.snapshot()
    }

    #[tauri::command]
    pub fn media_control(
        state: tauri::State<'_, MediaState>,
        action: ControlAction,
    ) -> Result<(), String> {
        state.control(action)
    }

    /// `source = null` switches back to auto.
    #[tauri::command]
    pub fn set_source(
        state: tauri::State<'_, MediaState>,
        source: Option<String>,
    ) -> Result<(), String> {
        state.set_source(source)
    }
}

#[cfg(not(windows))]
mod commands {
    #[tauri::command]
    pub fn get_media_state() -> Option<()> {
        None
    }

    #[tauri::command]
    pub fn media_control(_action: String) -> Result<(), String> {
        Err("SMTC is only available on Windows".into())
    }

    #[tauri::command]
    pub fn set_source(_source: Option<String>) -> Result<(), String> {
        Err("SMTC is only available on Windows".into())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // must be the first plugin: a second launch just shows the running app
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![autostart::MINIMIZED_ARG]),
        ))
        .plugin(tauri_plugin_opener::init())
        .on_menu_event(context_menu::handle_menu_event)
        .setup(|app| {
            use tauri::Manager;
            let config_dir = app.path().app_config_dir()?;

            autostart::init(app.handle(), &config_dir); // before the tray reads its state
            tray::setup(app.handle())?;

            let layout = layout::Layout::load(config_dir.join("layout.json"));
            if let Some(window) = app.get_webview_window("main") {
                layout.apply_window_size(&window); // restore saved width
                // The window starts hidden (tauri.conf.json "visible": false);
                // a login launch stays in the tray, a normal launch shows it.
                if !autostart::launched_minimized() {
                    let _ = window.show();
                }
                #[cfg(windows)]
                hover::start(window, layout.clone());
            }
            app.manage(layout);

            #[cfg(windows)]
            {
                let state = media::start(app.handle().clone(), config_dir.join("settings.json"));
                app.manage(state);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_media_state,
            commands::media_control,
            commands::set_source,
            context_menu::show_context_menu,
            layout::get_layout,
            layout::set_bar_width,
            layout::set_adjusting
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
