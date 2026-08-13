use tauri::{AppHandle, Manager};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::models::ProviderId;

pub const OFFICIAL_USAGE_URL: &str = "https://chatgpt.com/codex/settings/usage";
pub const LATEST_RELEASE_URL: &str = "https://github.com/RupingLiu/QuotaDock/releases/latest";
pub const DEEPSEEK_BALANCE_URL: &str = "https://platform.deepseek.com/usage";
pub const KIMI_ACCOUNT_URL: &str = "https://platform.moonshot.cn/console/account";

pub fn show(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("details")
        .ok_or_else(|| "详情窗口不存在。".to_string())?;
    window.center().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    let _ = window.unminimize();
    window.set_focus().map_err(|error| error.to_string())
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    app.get_webview_window("details")
        .ok_or_else(|| "详情窗口不存在。".to_string())?
        .hide()
        .map_err(|error| error.to_string())
}

pub fn open_official_usage() -> Result<(), String> {
    open_external_url(OFFICIAL_USAGE_URL, "打开官方用量页面")
}

#[tauri::command]
pub fn open_provider_portal(provider: ProviderId) -> Result<(), String> {
    let (url, operation_name) = match provider {
        ProviderId::Codex => (OFFICIAL_USAGE_URL, "打开 Codex 官方用量页面"),
        ProviderId::DeepSeek => (DEEPSEEK_BALANCE_URL, "打开 DeepSeek 官方余额页面"),
        ProviderId::Kimi => (KIMI_ACCOUNT_URL, "打开 Kimi 官方账户页面"),
    };
    open_external_url(url, operation_name)
}

#[tauri::command]
pub fn open_latest_release() -> Result<(), String> {
    open_external_url(LATEST_RELEASE_URL, "打开最新版下载页")
}

fn open_external_url(url: &str, operation_name: &str) -> Result<(), String> {
    let operation = wide_null("open");
    let target = wide_null(url);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(format!("{operation_name}失败：ShellExecuteW 返回 {result}"))
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_portals_are_fixed_https_urls() {
        for url in [OFFICIAL_USAGE_URL, DEEPSEEK_BALANCE_URL, KIMI_ACCOUNT_URL] {
            assert!(url.starts_with("https://"));
            assert!(!url.contains(['\r', '\n']));
        }
    }
}
