use crate::version::{APP_NAME, APP_VERSION};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO};

const AUTO_FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
static UPDATE_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);
static AUTO_PROMPTED_VERSION: Mutex<Option<String>> = Mutex::new(None);

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

pub fn start_auto_check(app: AppHandle) {
    let _ = thread::Builder::new()
        .name("quotadock-auto-update-check".to_string())
        .spawn(move || {
            thread::sleep(AUTO_FIRST_CHECK_DELAY);
            loop {
                if automatic_checks_enabled(&app) {
                    let check_app = app.clone();
                    tauri::async_runtime::block_on(async move {
                        if let Err(error) =
                            check_download_and_install(check_app, CheckOrigin::Automatic).await
                        {
                            eprintln!("automatic update check failed: {error}");
                        }
                    });
                }
                thread::sleep(AUTO_CHECK_INTERVAL);
            }
        });
}

pub fn check_now(app: AppHandle) {
    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, "更新检查中...");

    tauri::async_runtime::spawn(async move {
        let message = match check_download_and_install(app.clone(), CheckOrigin::Manual).await {
            Ok(message) => message,
            Err(error) => format!("更新检查失败：{error}"),
        };

        #[cfg(feature = "desktop")]
        crate::tray::set_menu_status_temporarily(&app, message);
    });
}

async fn check_download_and_install(app: AppHandle, origin: CheckOrigin) -> Result<String, String> {
    let _permit = begin_update_check(origin)?;
    let updater = app
        .updater()
        .map_err(|error| format!("初始化签名更新器失败：{error}"))?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|error| format!("获取签名更新清单失败：{error}"))?
    else {
        return Ok(format!("已是最新版本 v{APP_VERSION}"));
    };
    let version = update.version.to_string();
    if origin == CheckOrigin::Automatic && was_auto_prompted(&version) {
        return Ok(format!("已提醒过更新包 v{version}"));
    }

    let message = update_prompt_message(&version, update.body.as_deref());
    let answer = show_message("QuotaDock 有签名更新", &message, MB_YESNO | MB_ICONQUESTION);
    if origin == CheckOrigin::Automatic {
        remember_auto_prompt(&version);
    }
    if answer != IDYES {
        return Ok(format!("已跳过更新包 v{version}"));
    }

    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, format!("正在下载签名更新 v{version}..."));

    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| format!("签名验证或安装失败：{error}"))?;

    #[cfg(feature = "desktop")]
    crate::window_state::save_main_window_position_for_app(&app);
    app.restart();
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
        message.push_str(&format!("\n\n更新内容：\n{notes}"));
    }
    message.push_str("\n\n是否下载、验证并安装此更新？");
    message
}

fn show_message(title: &str, body: &str, flags: u32) -> i32 {
    let title = wide_null(title);
    let body = wide_null(body);
    unsafe { MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), flags) }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        begin_update_check, remember_auto_prompt, update_prompt_message, was_auto_prompted,
        CheckOrigin,
    };

    #[test]
    fn signed_update_prompt_explains_verification() {
        let message = update_prompt_message("0.5.0", Some("可靠性更新"));
        assert!(message.contains("内置公钥"));
        assert!(message.contains("可靠性更新"));
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
