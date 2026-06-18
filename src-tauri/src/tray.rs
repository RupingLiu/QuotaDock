use crate::models::AppState;
use crate::{floating_bar, startup, updates, version};
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};

const MENU_SHOW: &str = "show";
const MENU_STARTUP: &str = "startup";
const MENU_CHECK_UPDATES: &str = "check_updates";
const MENU_VERSION: &str = "version";
const MENU_QUIT: &str = "quit";

pub struct TrayState {
    icon: TrayIcon<Wry>,
}

pub fn install(app: &App) -> tauri::Result<()> {
    let handle = app.handle();
    let menu = build_menu(handle)?;

    let icon = app
        .default_window_icon()
        .cloned()
        .map(Image::to_owned)
        .unwrap_or_else(transparent_fallback_icon);

    let tray = TrayIconBuilder::with_id("quotadock")
        .menu(&menu)
        .tooltip("QuotaDock")
        .show_menu_on_left_click(false)
        .icon(icon)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_main_window(app),
            MENU_STARTUP => {
                if let Err(error) = startup::toggle() {
                    eprintln!("toggle startup failed: {error}");
                }
                refresh_menu(app);
            }
            MENU_CHECK_UPDATES => updates::check_now(app.clone()),
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

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let startup_label = if startup::is_enabled().unwrap_or(false) {
        "开机自启动：已开启"
    } else {
        "开机自启动：已关闭"
    };
    let version_item =
        MenuItemBuilder::with_id(MENU_VERSION, format!("版本 v{}", version::APP_VERSION))
            .enabled(false)
            .build(app)?;

    MenuBuilder::new(app)
        .text(MENU_SHOW, "显示 QuotaDock")
        .text(MENU_STARTUP, startup_label)
        .text(MENU_CHECK_UPDATES, "检查更新")
        .item(&version_item)
        .separator()
        .text(MENU_QUIT, "退出")
        .build()
}

fn refresh_menu(app: &AppHandle) {
    let Some(tray) = app.try_state::<TrayState>() else {
        return;
    };
    if let Ok(menu) = build_menu(app) {
        let _ = tray.icon.set_menu(Some(menu));
    }
}

pub fn sync_from_app_state(app: &AppHandle, _state: &AppState) {
    if let Some(tray) = app.try_state::<TrayState>() {
        let _ = tray
            .icon
            .set_tooltip(Some(format!("QuotaDock v{}", version::APP_VERSION)));
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        floating_bar::position_main_window(app);
        let _ = window.set_focus();
    }
}

fn transparent_fallback_icon() -> Image<'static> {
    const SIZE: u32 = 32;
    Image::new_owned(vec![0; (SIZE * SIZE * 4) as usize], SIZE, SIZE)
}
