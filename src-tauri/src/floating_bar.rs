use tauri::{AppHandle, Manager, PhysicalPosition, Position, WebviewWindow};

const TASKBAR_GAP_PX: i32 = 12;
const SCREEN_EDGE_GAP_PX: i32 = 18;

pub fn position_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = position_above_taskbar(&window);
    }
}

fn position_above_taskbar(window: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else {
        return Ok(());
    };

    let work_area = monitor.work_area();
    let size = window.outer_size()?;
    let width = size.width as i32;
    let height = size.height as i32;
    let work_width = work_area.size.width as i32;
    let work_height = work_area.size.height as i32;

    let x_offset = (work_width - width - SCREEN_EDGE_GAP_PX).max(0);
    let y_offset = (work_height - height - TASKBAR_GAP_PX).max(0);
    let x = work_area.position.x + x_offset;
    let y = work_area.position.y + y_offset;

    window.set_position(Position::Physical(PhysicalPosition::new(x, y)))
}

