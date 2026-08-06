use crate::version::{APP_NAME, APP_VERSION};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{App, AppHandle, Emitter, Manager, Url};
use tauri_plugin_updater::{Updater, UpdaterExt};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO};

const AUTO_FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const UPDATE_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_READ_TIMEOUT: Duration = Duration::from_secs(60);
const UPDATE_STATUS_EVENT: &str = "quotadock:update-status";
const RELEASE_NOTES_PROMPT_LIMIT: usize = 600;
static UPDATE_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);
static AUTO_PROMPTED_VERSION: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub phase: UpdatePhase,
    pub message: String,
    pub technical_detail: Option<String>,
    pub available_version: Option<String>,
    pub progress_percent: Option<u8>,
    pub checked_at: Option<String>,
}

impl UpdateStatus {
    fn idle() -> Self {
        Self {
            current_version: APP_VERSION.to_string(),
            phase: UpdatePhase::Idle,
            message: "尚未检查软件更新。".to_string(),
            technical_detail: None,
            available_version: None,
            progress_percent: None,
            checked_at: None,
        }
    }

    fn checking() -> Self {
        Self {
            phase: UpdatePhase::Checking,
            message: "正在连接更新服务…".to_string(),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn up_to_date() -> Self {
        Self {
            phase: UpdatePhase::UpToDate,
            message: format!("已是最新版本 v{APP_VERSION}。"),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn available(version: &str, message: impl Into<String>) -> Self {
        Self {
            phase: UpdatePhase::Available,
            message: message.into(),
            available_version: Some(version.to_string()),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn downloading(version: &str, progress_percent: Option<u8>) -> Self {
        let message = progress_percent.map_or_else(
            || format!("正在下载签名更新 v{version}…"),
            |progress| format!("正在下载签名更新 v{version}（{progress}%）…"),
        );
        Self {
            phase: UpdatePhase::Downloading,
            message,
            available_version: Some(version.to_string()),
            progress_percent,
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn verifying(version: &str) -> Self {
        Self {
            phase: UpdatePhase::Downloading,
            message: format!("正在验证 v{version} 的发布签名并安装…"),
            available_version: Some(version.to_string()),
            progress_percent: Some(100),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn error(message: impl Into<String>, technical_detail: impl Into<String>) -> Self {
        Self {
            phase: UpdatePhase::Error,
            message: message.into(),
            technical_detail: Some(technical_detail.into()),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }
}

pub struct UpdateStatusState(Mutex<UpdateStatus>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckOrigin {
    Manual,
    Automatic,
}

#[derive(Debug)]
struct UpdateCheckPermit;

impl Drop for UpdateCheckPermit {
    fn drop(&mut self) {
        UPDATE_CHECK_RUNNING.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
struct UpdateFailure {
    message: String,
    technical_detail: String,
}

impl UpdateFailure {
    fn new(message: impl Into<String>, technical_detail: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            technical_detail: technical_detail.into(),
        }
    }

    fn into_status(self) -> UpdateStatus {
        UpdateStatus::error(self.message, self.technical_detail)
    }
}

pub fn install(app: &App) {
    app.manage(UpdateStatusState(Mutex::new(UpdateStatus::idle())));
}

pub fn start_auto_check(app: AppHandle) {
    let _ = thread::Builder::new()
        .name("quotadock-auto-update-check".to_string())
        .spawn(move || {
            thread::sleep(AUTO_FIRST_CHECK_DELAY);
            loop {
                if automatic_checks_enabled(&app) {
                    let check_app = app.clone();
                    tauri::async_runtime::block_on(async move {
                        let status = execute_check(check_app, CheckOrigin::Automatic).await;
                        if status.phase == UpdatePhase::Error {
                            eprintln!("automatic update check failed: {}", status.message);
                        }
                    });
                }
                thread::sleep(AUTO_CHECK_INTERVAL);
            }
        });
}

pub fn check_now(app: AppHandle) {
    #[cfg(feature = "desktop")]
    {
        if let Err(error) = crate::details::show(&app) {
            crate::tray::set_menu_status_temporarily(&app, error);
        }
        crate::tray::set_menu_status(&app, "正在检查更新…");
    }

    tauri::async_runtime::spawn(async move {
        let status = execute_check(app.clone(), CheckOrigin::Manual).await;

        #[cfg(feature = "desktop")]
        crate::tray::set_menu_status_temporarily(&app, menu_status_for(&status));
    });
}

#[tauri::command]
pub fn get_update_status(app: AppHandle) -> UpdateStatus {
    current_update_status(&app)
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> UpdateStatus {
    execute_check(app, CheckOrigin::Manual).await
}

pub fn is_check_running() -> bool {
    UPDATE_CHECK_RUNNING.load(Ordering::Acquire)
}

async fn execute_check(app: AppHandle, origin: CheckOrigin) -> UpdateStatus {
    let status = match check_download_and_install(app.clone(), origin).await {
        Ok(status) => status,
        Err(failure) => {
            let status = failure.into_status();
            set_update_status(&app, status.clone());
            status
        }
    };
    #[cfg(feature = "desktop")]
    crate::tray::refresh_menu(&app);
    status
}

async fn check_download_and_install(
    app: AppHandle,
    origin: CheckOrigin,
) -> Result<UpdateStatus, UpdateFailure> {
    let _permit = match begin_update_check(origin) {
        Ok(permit) => permit,
        Err(_) => {
            let current = current_update_status(&app);
            return Ok(current);
        }
    };
    set_update_status(&app, UpdateStatus::checking());
    #[cfg(feature = "desktop")]
    crate::tray::refresh_menu(&app);

    let updater = build_updater(&app).map_err(|error| {
        UpdateFailure::new(
            "更新服务初始化失败，请稍后重试。",
            format!("初始化签名更新器失败：{error}"),
        )
    })?;
    let Some(update) = updater.check().await.map_err(check_request_failure)? else {
        let status = UpdateStatus::up_to_date();
        set_update_status(&app, status.clone());
        return Ok(status);
    };

    let version = update.version.to_string();
    if origin == CheckOrigin::Automatic && was_auto_prompted(&version) {
        let status =
            UpdateStatus::available(&version, format!("可用更新 v{version}，本次不再重复提醒。"));
        set_update_status(&app, status.clone());
        return Ok(status);
    }

    let available = UpdateStatus::available(&version, format!("发现新版本 v{version}。"));
    set_update_status(&app, available.clone());
    let message = update_prompt_message(&version, update.body.as_deref());
    let answer = show_message(
        &app,
        "QuotaDock 有签名更新",
        &message,
        MB_YESNO | MB_ICONQUESTION,
    );
    if origin == CheckOrigin::Automatic {
        remember_auto_prompt(&version);
    }
    if answer != IDYES {
        let status =
            UpdateStatus::available(&version, format!("新版本 v{version} 可用，已暂缓安装。"));
        set_update_status(&app, status.clone());
        return Ok(status);
    }

    let downloading = UpdateStatus::downloading(&version, None);
    set_update_status(&app, downloading);
    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, format!("正在下载更新 v{version}…"));

    let progress_app = app.clone();
    let progress_version = version.clone();
    let finish_app = app.clone();
    let finish_version = version.clone();
    let mut downloaded_bytes = 0_u64;
    let mut last_reported_percent = 0_u8;
    update
        .download_and_install(
            move |chunk_length, total_length| {
                downloaded_bytes = downloaded_bytes.saturating_add(chunk_length as u64);
                let Some(total_length) = total_length.filter(|total| *total > 0) else {
                    return;
                };
                let percent =
                    ((downloaded_bytes.saturating_mul(100) / total_length).min(100)) as u8;
                if percent == 100 || percent >= last_reported_percent.saturating_add(5) {
                    last_reported_percent = percent;
                    set_update_status(
                        &progress_app,
                        UpdateStatus::downloading(&progress_version, Some(percent)),
                    );
                }
            },
            move || {
                set_update_status(&finish_app, UpdateStatus::verifying(&finish_version));
            },
        )
        .await
        .map_err(|error| {
            UpdateFailure::new(
                "更新下载或签名验证失败，未执行安装。",
                format!("签名验证或安装失败：{error}"),
            )
        })?;

    #[cfg(feature = "desktop")]
    crate::window_state::save_main_window_position_for_app(&app);
    app.restart();
}

fn build_updater(app: &AppHandle) -> Result<Updater, tauri_plugin_updater::Error> {
    let mut builder = app.updater_builder().configure_client(|client| {
        client
            .connect_timeout(UPDATE_CONNECT_TIMEOUT)
            .read_timeout(UPDATE_READ_TIMEOUT)
    });

    if !environment_proxy_configured() {
        if let Some(proxy) = windows_internet_proxy() {
            builder = builder.proxy(proxy);
        }
    }

    #[cfg(feature = "desktop")]
    {
        let exit_app = app.clone();
        builder = builder.on_before_exit(move || {
            crate::window_state::save_main_window_position_for_app(&exit_app);
        });
    }

    builder.build()
}

fn environment_proxy_configured() -> bool {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|name| {
            std::env::var_os(name).is_some_and(|value| !value.to_string_lossy().trim().is_empty())
        })
}

#[cfg(windows)]
fn windows_internet_proxy() -> Option<Url> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let internet_settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enabled = internet_settings
        .get_value::<u32, _>("ProxyEnable")
        .unwrap_or(0);
    if enabled == 0 {
        return None;
    }
    let proxy_server = internet_settings
        .get_value::<String, _>("ProxyServer")
        .ok()?;
    parse_windows_proxy_server(&proxy_server)
}

#[cfg(not(windows))]
fn windows_internet_proxy() -> Option<Url> {
    None
}

fn parse_windows_proxy_server(raw: &str) -> Option<Url> {
    let mut generic = None;
    let mut http = None;
    let mut https = None;

    for entry in raw
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        if let Some((protocol, address)) = entry.split_once('=') {
            let address = address.trim();
            if address.is_empty() {
                continue;
            }
            match protocol.trim().to_ascii_lowercase().as_str() {
                "https" => https = Some(address),
                "http" => http = Some(address),
                _ => {}
            }
        } else if generic.is_none() {
            generic = Some(entry);
        }
    }

    let selected = https.or(http).or(generic)?;
    let normalized = if selected.contains("://") {
        selected.to_string()
    } else {
        format!("http://{selected}")
    };
    let parsed = Url::parse(&normalized).ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(parsed)
}

fn check_request_failure(error: tauri_plugin_updater::Error) -> UpdateFailure {
    let detail = format!("获取签名更新清单失败：{error}");
    let lower = detail.to_ascii_lowercase();
    let message = if [
        "error sending request",
        "timed out",
        "timeout",
        "connect",
        "dns",
        "network",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        "暂时无法连接更新服务，请检查网络或代理后重试。"
    } else {
        "无法读取更新信息，请稍后重试。"
    };
    UpdateFailure::new(message, detail)
}

fn automatic_checks_enabled(app: &AppHandle) -> bool {
    crate::commands::load_app_state(app)
        .map(|state| state.settings.automatic_update_checks)
        .unwrap_or(true)
}

fn begin_update_check(origin: CheckOrigin) -> Result<UpdateCheckPermit, String> {
    UPDATE_CHECK_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| UpdateCheckPermit)
        .map_err(|_| match origin {
            CheckOrigin::Manual => "已有更新检查正在进行。".to_string(),
            CheckOrigin::Automatic => "已有更新检查正在进行，跳过本次自动检查。".to_string(),
        })
}

fn was_auto_prompted(version: &str) -> bool {
    AUTO_PROMPTED_VERSION
        .lock()
        .map(|prompted| prompted.as_deref() == Some(version))
        .unwrap_or(false)
}

fn remember_auto_prompt(version: &str) {
    if let Ok(mut prompted) = AUTO_PROMPTED_VERSION.lock() {
        *prompted = Some(version.to_string());
    }
}

fn update_prompt_message(version: &str, notes: Option<&str>) -> String {
    let mut message =
        format!("发现 {APP_NAME} v{version}。\n\n安装包将由应用内置公钥验证发布者签名。");
    if let Some(notes) = notes.filter(|notes| !notes.trim().is_empty()) {
        message.push_str("\n\n更新内容：\n");
        message.push_str(&truncate_text(notes.trim(), RELEASE_NOTES_PROMPT_LIMIT));
    }
    message.push_str("\n\n是否下载、验证并安装此更新？");
    message
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value
        .chars()
        .take(max_chars.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

fn current_update_status(app: &AppHandle) -> UpdateStatus {
    app.try_state::<UpdateStatusState>()
        .and_then(|state| state.0.lock().ok().map(|status| status.clone()))
        .unwrap_or_else(UpdateStatus::idle)
}

fn set_update_status(app: &AppHandle, status: UpdateStatus) {
    if let Some(state) = app.try_state::<UpdateStatusState>() {
        if let Ok(mut current) = state.0.lock() {
            *current = status.clone();
        }
    }
    let _ = app.emit_to("details", UPDATE_STATUS_EVENT, status);
}

fn menu_status_for(status: &UpdateStatus) -> String {
    match status.phase {
        UpdatePhase::Idle => "更新：尚未检查".to_string(),
        UpdatePhase::Checking => "正在检查更新…".to_string(),
        UpdatePhase::UpToDate => format!("已是最新版 v{}", status.current_version),
        UpdatePhase::Available => status
            .available_version
            .as_deref()
            .map(|version| format!("发现新版 v{version}"))
            .unwrap_or_else(|| "发现可用更新".to_string()),
        UpdatePhase::Downloading => "正在下载并验证更新…".to_string(),
        UpdatePhase::Error => "更新检查失败，请查看详情".to_string(),
    }
}

fn now_marker() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{seconds}")
}

fn show_message(app: &AppHandle, title: &str, body: &str, flags: u32) -> i32 {
    let title = wide_null(title);
    let body = wide_null(body);
    let owner = app
        .get_webview_window("details")
        .and_then(|window| window.hwnd().ok())
        .map(|handle| handle.0)
        .unwrap_or(std::ptr::null_mut());
    unsafe { MessageBoxW(owner, body.as_ptr(), title.as_ptr(), flags) }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        begin_update_check, check_request_failure, menu_status_for, parse_windows_proxy_server,
        remember_auto_prompt, truncate_text, update_prompt_message, was_auto_prompted, CheckOrigin,
        UpdatePhase, UpdateStatus, RELEASE_NOTES_PROMPT_LIMIT,
    };

    #[test]
    fn signed_update_prompt_explains_verification_and_limits_notes() {
        let long_notes = "可靠性更新".repeat(200);
        let message = update_prompt_message("0.5.2", Some(&long_notes));
        assert!(message.contains("内置公钥"));
        assert!(message.contains("可靠性更新"));
        assert!(message.chars().count() < RELEASE_NOTES_PROMPT_LIMIT + 100);
        assert!(message.contains('…'));
    }

    #[test]
    fn parses_common_windows_proxy_formats() {
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:7897")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:7897/"
        );
        assert_eq!(
            parse_windows_proxy_server("http=proxy.local:8080;https=secure.local:8443")
                .unwrap()
                .as_str(),
            "http://secure.local:8443/"
        );
        assert_eq!(
            parse_windows_proxy_server("https://proxy.local:9443")
                .unwrap()
                .as_str(),
            "https://proxy.local:9443/"
        );
        assert!(parse_windows_proxy_server("socks=127.0.0.1:1080").is_none());
        assert!(parse_windows_proxy_server("not a proxy").is_none());
    }

    #[test]
    fn network_failure_has_recovery_copy_without_exposing_url_in_menu() {
        let failure = check_request_failure(tauri_plugin_updater::Error::Network(
            "error sending request for url (https://example.invalid/latest.json)".to_string(),
        ));
        let status = failure.into_status();
        assert_eq!(status.phase, UpdatePhase::Error);
        assert!(status.message.contains("代理"));
        assert!(status
            .technical_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("latest.json")));
        assert!(!menu_status_for(&status).contains("http"));
    }

    #[test]
    fn menu_update_statuses_stay_compact() {
        let status = UpdateStatus::error(
            "暂时无法连接更新服务，请检查网络或代理后重试。",
            "https://example.invalid/a/very/long/path/latest.json",
        );
        let menu = menu_status_for(&status);
        assert!(menu.chars().count() <= 16);
        assert!(!menu.contains("http"));
    }

    #[test]
    fn truncates_unicode_by_characters() {
        assert_eq!(truncate_text("更新检查失败", 5), "更新检查…");
    }

    #[test]
    fn remembers_automatic_prompted_version() {
        remember_auto_prompt("9.9.9");
        assert!(was_auto_prompted("9.9.9"));
        assert!(!was_auto_prompted("9.9.8"));
    }

    #[test]
    fn prevents_concurrent_update_checks() {
        let permit = begin_update_check(CheckOrigin::Manual).unwrap();
        let duplicate = begin_update_check(CheckOrigin::Automatic).unwrap_err();
        assert!(duplicate.contains("跳过"));
        drop(permit);
        assert!(begin_update_check(CheckOrigin::Manual).is_ok());
    }
}
