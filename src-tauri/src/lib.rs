#![cfg_attr(test, allow(dead_code))]

mod models;
mod status_parser;
#[cfg(feature = "desktop")]
mod tray;
mod tray_icon;
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
        Ok(())
    });

    builder
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::refresh_usage,
            commands::parse_status_text,
            commands::save_snapshot,
            commands::clear_snapshot
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
pub fn run() {}
