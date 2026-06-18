use crate::models::{AppState, QuotaReading, QuotaSnapshot};
use crate::tray_icon::render_quota_icon_rgba;
use tauri::image::Image;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};

const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";
const TRAY_SIZE: u32 = 64;

pub struct TrayState {
    icon: TrayIcon<Wry>,
}

pub fn install(app: &App) -> tauri::Result<()> {
    let handle = app.handle();
    let menu = MenuBuilder::new(handle)
        .text(MENU_SHOW, "显示 QuotaDock")
        .separator()
        .text(MENU_QUIT, "退出")
        .build()?;

    let icon = quota_icon_image(None, TRAY_SIZE).unwrap_or_else(|_| {
        app.default_window_icon().cloned().unwrap_or_else(|| {
            Image::new_owned(
                vec![0; (TRAY_SIZE * TRAY_SIZE * 4) as usize],
                TRAY_SIZE,
                TRAY_SIZE,
            )
        })
    });

    let tray = TrayIconBuilder::with_id("quotadock")
        .menu(&menu)
        .tooltip("QuotaDock：尚未获取额度")
        .show_menu_on_left_click(false)
        .icon(icon)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_main_window(app),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayState { icon: tray });
    Ok(())
}

pub fn sync_from_app_state(app: &AppHandle, state: &AppState) {
    let snapshot = state.latest_snapshot.as_ref();
    if let Some(tray) = app.try_state::<TrayState>() {
        if let Ok(icon) = quota_icon_image(snapshot, TRAY_SIZE) {
            let _ = tray.icon.set_icon(Some(icon));
        }
        let _ = tray.icon.set_tooltip(Some(tray_tooltip(snapshot)));
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn quota_icon_image(snapshot: Option<&QuotaSnapshot>, size: u32) -> Result<Image<'static>, String> {
    Ok(Image::new_owned(
        render_quota_icon_rgba(snapshot, size)?,
        size,
        size,
    ))
}

fn tray_tooltip(snapshot: Option<&QuotaSnapshot>) -> String {
    let Some(snapshot) = snapshot else {
        return "QuotaDock：尚未获取额度".to_string();
    };

    format!(
        "5小时：{}，更新 {}；1周：{}，更新 {}",
        tooltip_percent(&snapshot.five_hour),
        tooltip_reset(&snapshot.five_hour),
        tooltip_percent(&snapshot.weekly),
        tooltip_reset(&snapshot.weekly)
    )
}

fn tooltip_percent(reading: &QuotaReading) -> String {
    reading
        .remaining_percent
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "--".to_string())
}

fn tooltip_reset(reading: &QuotaReading) -> String {
    if let Some(reset_at) = &reading.reset_at {
        return reset_at.clone();
    }
    reading
        .reset_countdown_seconds
        .map(format_duration_zh)
        .unwrap_or_else(|| "--".to_string())
}

fn format_duration_zh(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    let hours = minutes / 60;
    let remaining_minutes = minutes % 60;
    if hours > 0 && remaining_minutes > 0 {
        format!("{hours}小时{remaining_minutes}分钟后")
    } else if hours > 0 {
        format!("{hours}小时后")
    } else {
        format!("{remaining_minutes}分钟后")
    }
}
