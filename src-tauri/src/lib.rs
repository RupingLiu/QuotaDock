#![cfg_attr(test, allow(dead_code))]

mod app_server;
mod models;
mod startup;
mod status_parser;
#[cfg(feature = "desktop")]
mod tray;
mod updates;
mod usage_store;
mod version;
#[cfg(feature = "desktop")]
mod window_state;

mod commands;
#[cfg(feature = "desktop")]
mod details;

#[cfg(not(test))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "desktop")]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build());
    #[cfg(feature = "desktop")]
    let builder = builder.setup(|app| {
        commands::install_refresh_coordinator(app);
        tray::install(app)?;
        if let Ok(state) = commands::load_app_state(app.handle()) {
            tray::sync_from_app_state(app.handle(), &state);
        }
        window_state::restore_main_window(app.handle());
        commands::prewarm_codex_status_session();
        commands::start_auto_refresh(app.handle().clone());
        updates::start_auto_check(app.handle().clone());
        Ok(())
    });

    builder
        .on_window_event(|window, event| {
            #[cfg(feature = "desktop")]
            match window.label() {
                "main" => match event {
                    tauri::WindowEvent::Moved(position) => {
                        window_state::snap_main_window_position(window, *position);
                    }
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        window_state::save_current_main_window_position(window);
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    _ => {}
                },
                "details" => {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
                _ => {}
            }
            #[cfg(not(feature = "desktop"))]
            let _ = (window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::refresh_usage,
            commands::show_dashboard_context_menu,
            commands::update_settings,
            commands::acknowledge_recovery,
            commands::get_diagnostics,
            commands::set_startup_enabled,
            commands::show_details,
            commands::hide_details,
            commands::open_official_usage
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
pub fn run() {}
