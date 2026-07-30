use tauri::{AppHandle, Manager};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub const OFFICIAL_USAGE_URL: &str = "https://chatgpt.com/codex/settings/usage";

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
    let operation = wide_null("open");
    let target = wide_null(OFFICIAL_USAGE_URL);
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
        Err(format!("打开官方用量页面失败：ShellExecuteW 返回 {result}"))
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
