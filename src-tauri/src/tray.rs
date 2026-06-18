use crate::codex_probe::probe_codex;
use crate::official_links::USAGE_DASHBOARD_URL;
use std::time::Duration;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

const MENU_SHOW: &str = "show";
const MENU_USAGE: &str = "usage";
const MENU_REFRESH: &str = "refresh-probe";
const MENU_QUIT: &str = "quit";

pub struct TrayState {
    #[allow(dead_code)]
    icon: TrayIcon<Wry>,
}

pub fn install(app: &App) -> tauri::Result<()> {
    let handle = app.handle();
    let menu = MenuBuilder::new(handle)
        .text(MENU_SHOW, "Show QuotaDock")
        .text(MENU_USAGE, "Open Usage Dashboard")
        .text(MENU_REFRESH, "Refresh Codex Probe")
        .separator()
        .text(MENU_QUIT, "Quit")
        .build()?;

    let mut builder = TrayIconBuilder::with_id("quotadock")
        .menu(&menu)
        .tooltip("QuotaDock")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_main_window(app),
            MENU_USAGE => open_usage_dashboard(app),
            MENU_REFRESH => refresh_probe_from_tray(app),
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
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    let icon = builder.build(app)?;
    app.manage(TrayState { icon });
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn open_usage_dashboard(app: &AppHandle) {
    let _ = app.opener().open_url(USAGE_DASHBOARD_URL, None::<&str>);
}

fn refresh_probe_from_tray(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let health = probe_codex(Duration::from_secs(5));
        let body = format!(
            "Status: {:?}, auth: {}",
            health.status,
            match health.authenticated {
                Some(true) => "signed in",
                Some(false) => "signed out",
                None => "unknown",
            }
        );
        let _ = app
            .notification()
            .builder()
            .title("QuotaDock probe refreshed")
            .body(body)
            .show();
    });
}
