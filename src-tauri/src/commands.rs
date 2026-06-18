use crate::models::{AppState, QuotaSnapshot, RefreshUsageResult, SnapshotSource};
use crate::status_parser::{
    parse_status_text as parse_status_text_impl, parse_status_text_with_source, ParseClock,
    ParseResult,
};
use crate::usage_store::{StoreError, UsageStore};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn get_app_state(app: AppHandle) -> Result<AppState, String> {
    let state = load_app_state(&app)?;
    sync_tray(&app, &state);
    Ok(state)
}

#[tauri::command]
pub fn parse_status_text(raw_text: String) -> ParseResult {
    parse_status_text_impl(&raw_text, ParseClock::now())
}

#[tauri::command]
pub fn save_snapshot(app: AppHandle, snapshot: QuotaSnapshot) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    let app_state = store
        .save_snapshot(snapshot)
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)?;
    sync_tray(&app, &app_state);
    Ok(app_state)
}

#[tauri::command]
pub fn clear_snapshot(app: AppHandle) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    let app_state = store
        .clear_snapshot()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)?;
    sync_tray(&app, &app_state);
    Ok(app_state)
}

#[tauri::command]
pub fn refresh_usage(app: AppHandle) -> Result<RefreshUsageResult, String> {
    let store = store_for_app(&app)?;
    match fetch_usage_from_codex_cli() {
        Ok(snapshot) => {
            let app_state = store
                .save_snapshot(snapshot)
                .map(|outcome| outcome.into_app_state())
                .map_err(to_command_error)?;
            sync_tray(&app, &app_state);
            Ok(RefreshUsageResult {
                app_state,
                updated: true,
                message: "已通过 Codex CLI 更新额度。".to_string(),
            })
        }
        Err(message) => {
            let app_state = load_app_state(&app)?;
            sync_tray(&app, &app_state);
            Ok(RefreshUsageResult {
                app_state,
                updated: false,
                message,
            })
        }
    }
}

pub fn load_app_state(app: &AppHandle) -> Result<AppState, String> {
    let store = store_for_app(app)?;
    store
        .load()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)
}

pub fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("quotadock-state.json"))
        .map_err(|error| error.to_string())
}

fn fetch_usage_from_codex_cli() -> Result<QuotaSnapshot, String> {
    let help = run_codex(&["--help"], Duration::from_secs(3))?;
    if !help.success {
        return Err("Codex CLI 无法执行，请确认 codex 已安装并可登录。".to_string());
    }

    let args: Vec<&str> = if command_list_contains(&help.stdout, "usage") {
        vec!["usage", "--json"]
    } else if command_list_contains(&help.stdout, "status") {
        vec!["status", "--json"]
    } else {
        return Err("当前 Codex CLI 未提供额度查询，请粘贴 /status。".to_string());
    };

    let output = run_codex(&args, Duration::from_secs(8))?;
    if !output.success {
        return Err("Codex CLI 额度查询失败，请粘贴 /status。".to_string());
    }

    let mut result =
        parse_status_text_with_source(&output.stdout, ParseClock::now(), SnapshotSource::CodexCli);
    result.snapshot.status_message = "已通过 Codex CLI 更新额度。".to_string();
    if result.snapshot.has_any_usage() {
        Ok(result.snapshot)
    } else {
        Err("Codex CLI 没有返回可识别的额度，请粘贴 /status。".to_string())
    }
}

fn command_list_contains(help: &str, command: &str) -> bool {
    help.lines().any(|line| {
        let trimmed = line.trim_start().to_ascii_lowercase();
        let Some(rest) = trimmed.strip_prefix(command) else {
            return false;
        };
        rest.starts_with(char::is_whitespace)
    })
}

#[derive(Debug)]
struct CodexOutput {
    success: bool,
    stdout: String,
}

fn run_codex(args: &[&str], timeout: Duration) -> Result<CodexOutput, String> {
    let mut command = codex_command(args)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let mut child = command
        .spawn()
        .map_err(|_| "未找到 Codex CLI，请确认 codex 命令可用。".to_string())?;
    let started = Instant::now();

    loop {
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Codex CLI 查询超时，请粘贴 /status。".to_string());
        }

        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("读取 Codex CLI 输出失败：{error}"))?;
                return Ok(CodexOutput {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => return Err(format!("Codex CLI 查询失败：{error}")),
        }
    }
}

fn codex_command(args: &[&str]) -> Result<Command, String> {
    let Some(target) = find_codex_binary() else {
        return Err("未找到 Codex CLI，请确认 codex 命令可用。".to_string());
    };

    let mut command = if is_cmd_shim(&target) {
        let mut command = Command::new("cmd");
        command.arg("/D").arg("/C").arg(&target);
        command
    } else {
        Command::new(&target)
    };
    command.args(args);
    Ok(command)
}

fn find_codex_binary() -> Option<PathBuf> {
    codex_candidate_paths()
        .into_iter()
        .find(|path| path.is_file())
}

fn codex_candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            push_codex_names(&mut candidates, &dir);
        }
    }

    for variable in ["APPDATA", "USERPROFILE", "LOCALAPPDATA"] {
        if let Some(value) = std::env::var_os(variable) {
            let base = PathBuf::from(value);
            match variable {
                "APPDATA" => push_codex_names(&mut candidates, &base.join("npm")),
                "USERPROFILE" => {
                    push_codex_names(
                        &mut candidates,
                        &base.join("AppData").join("Roaming").join("npm"),
                    );
                    push_codex_names(
                        &mut candidates,
                        &base
                            .join("AppData")
                            .join("Local")
                            .join("Microsoft")
                            .join("WindowsApps"),
                    );
                }
                "LOCALAPPDATA" => {
                    push_codex_names(&mut candidates, &base.join("Microsoft").join("WindowsApps"))
                }
                _ => {}
            }
        }
    }

    candidates
}

fn push_codex_names(candidates: &mut Vec<PathBuf>, dir: &Path) {
    #[cfg(windows)]
    for name in ["codex.exe", "codex.cmd", "codex.bat", "codex"] {
        candidates.push(dir.join(name));
    }

    #[cfg(not(windows))]
    candidates.push(dir.join("codex"));
}

fn is_cmd_shim(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
}

fn store_for_app(app: &AppHandle) -> Result<UsageStore, String> {
    Ok(UsageStore::new(state_path(app)?))
}

fn sync_tray(_app: &AppHandle, _state: &AppState) {
    #[cfg(feature = "desktop")]
    crate::tray::sync_from_app_state(_app, _state);
}

fn to_command_error(error: StoreError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use crate::commands::command_list_contains;

    #[test]
    fn detects_usage_command_as_top_level_command_only() {
        let help = "Commands:\n  usage   Show quota\n  login   Manage login";

        assert!(command_list_contains(help, "usage"));
        assert!(!command_list_contains(help, "status"));
    }

    #[test]
    fn does_not_treat_login_status_as_status_command() {
        let help = "Commands:\n  login   Manage login status\n  doctor  Diagnose";

        assert!(!command_list_contains(help, "status"));
    }

    #[test]
    fn detects_windows_cmd_shims() {
        assert!(super::is_cmd_shim(std::path::Path::new("codex.cmd")));
        assert!(super::is_cmd_shim(std::path::Path::new("codex.bat")));
        assert!(!super::is_cmd_shim(std::path::Path::new("codex.exe")));
    }
}
