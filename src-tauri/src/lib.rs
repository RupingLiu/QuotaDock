use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppState {
    status: AppStateStatus,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AppStateStatus {
    Unavailable,
}

#[tauri::command]
fn get_app_state() -> AppState {
    AppState {
        status: AppStateStatus::Unavailable,
        message: "Usage state unavailable until the MVP data modules are implemented.".to_string(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_app_state])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_app_state_returns_unavailable_placeholder() {
        let state = serde_json::to_value(get_app_state()).expect("state should serialize");

        assert_eq!(state["status"], "unavailable");
        assert_eq!(
            state["message"],
            "Usage state unavailable until the MVP data modules are implemented."
        );
    }
}
