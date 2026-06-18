use crate::codex_probe::probe_codex;
use crate::models::{AppState, ManualUpdateInput, Settings, UsageSnapshot};
use crate::status_parser::{parse_status_text as parse_status_text_impl, ParseClock, ParseResult};
use crate::usage_store::{StoreError, UsageStore};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn get_app_state(app: AppHandle) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    store
        .load()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)
}

#[tauri::command]
pub fn parse_status_text(raw_text: String) -> ParseResult {
    parse_status_text_impl(&raw_text, ParseClock::now())
}

#[tauri::command]
pub fn refresh_codex_probe() -> crate::models::CodexHealth {
    probe_codex(Duration::from_secs(5))
}

#[tauri::command]
pub fn save_snapshot(app: AppHandle, snapshot: UsageSnapshot) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    store
        .save_snapshot(snapshot)
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)
}

#[tauri::command]
pub fn update_manual_fields(app: AppHandle, input: ManualUpdateInput) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    store
        .update_manual_fields(input)
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)
}

#[tauri::command]
pub fn update_settings(app: AppHandle, settings: Settings) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    store
        .update_settings(settings)
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)
}

#[tauri::command]
pub fn backup_and_reset_store(app: AppHandle) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    store
        .backup_and_reset()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)
}

pub fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("quotadock-state.json"))
        .map_err(|error| error.to_string())
}

fn store_for_app(app: &AppHandle) -> Result<UsageStore, String> {
    Ok(UsageStore::new(state_path(app)?))
}

fn to_command_error(error: StoreError) -> String {
    error.to_string()
}
