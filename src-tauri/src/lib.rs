#![cfg_attr(test, allow(dead_code))]

mod models;
mod status_parser;
#[cfg(feature = "desktop")]
mod floating_bar;
#[cfg(feature = "desktop")]
mod tray;
mod usage_store;

#[cfg(not(test))]
mod commands;

#[cfg(not(test))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "desktop")]
    let builder = builder.setup(|app| {
        tray::install(app)?;
        if let Ok(state) = commands::load_app_state(app.handle()) {
            tray::sync_from_app_state(app.handle(), &state);
        }
        floating_bar::position_main_window(app.handle());
        commands::prewarm_codex_status_session();
        Ok(())
    });

    builder
        .on_window_event(|window, event| {
            #[cfg(feature = "desktop")]
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            #[cfg(not(feature = "desktop"))]
            let _ = (window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::refresh_usage
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
pub fn run() {}
