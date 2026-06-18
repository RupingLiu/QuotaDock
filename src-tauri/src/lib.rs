#![cfg_attr(test, allow(dead_code))]

mod codex_probe;
mod models;
mod redaction;
mod status_parser;
mod usage_store;

#[cfg(not(test))]
mod commands;

#[cfg(not(test))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::parse_status_text,
            commands::refresh_codex_probe,
            commands::save_snapshot,
            commands::update_manual_fields,
            commands::update_settings,
            commands::backup_and_reset_store
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
pub fn run() {}
