use crate::floating_bar;
use crate::models::AppState;
use tauri::image::Image;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};

const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";

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

pub fn sync_from_app_state(app: &AppHandle, _state: &AppState) {
    if let Some(tray) = app.try_state::<TrayState>() {
        let _ = tray.icon.set_tooltip(Some("QuotaDock"));
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
