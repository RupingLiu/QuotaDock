use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, Position, WebviewWindow, Window,
    Wry,
};

const STATE_FILE_NAME: &str = "quotadock-window-state.json";
const STATE_VERSION: u8 = 1;
const MAIN_WINDOW_LABEL: &str = "main";
const EDGE_MARGIN: i32 = 12;
const MIN_VISIBLE_WIDTH: i64 = 64;
const MIN_VISIBLE_HEIGHT: i64 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct SavedWindowState {
    version: u8,
    main: SavedPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct SavedPosition {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub fn restore_main_window(app: &AppHandle<Wry>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    if let Err(error) = restore_window_position(&window) {
        eprintln!("restore main window position failed: {error}");
    }
    if let Err(error) = window.show() {
        eprintln!("show main window after positioning failed: {error}");
    }
}

pub fn save_main_window_position(window: &Window<Wry>, position: PhysicalPosition<i32>) {
    if let Err(error) = save_position(window.app_handle(), position) {
        eprintln!("save main window position failed: {error}");
    }
}

pub fn save_current_main_window_position(window: &Window<Wry>) {
    match window.outer_position() {
        Ok(position) => save_main_window_position(window, position),
        Err(error) => eprintln!("read main window position failed: {error}"),
    }
}

fn restore_window_position(window: &WebviewWindow<Wry>) -> Result<(), String> {
    let size = window.outer_size().map_err(|error| error.to_string())?;

    let saved_position = load_position(window.app_handle())?.filter(|position| {
        window
            .available_monitors()
            .map(|monitors| position_is_visible_on_any_monitor(*position, size, &monitors))
            .unwrap_or(false)
    });

    let position = saved_position
        .or_else(|| fallback_bottom_right_position(window, size))
        .ok_or_else(|| "no available monitor for window positioning".to_string())?;

    window
        .set_position(Position::Physical(position))
        .map_err(|error| error.to_string())
}

fn fallback_bottom_right_position(
    window: &WebviewWindow<Wry>,
    size: PhysicalSize<u32>,
) -> Option<PhysicalPosition<i32>> {
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| {
            window
                .available_monitors()
                .ok()
                .and_then(|monitors| monitors.into_iter().next())
        })?;

    Some(bottom_right_position(
        work_area_from_monitor(&monitor),
        size,
    ))
}

fn load_position(app: &AppHandle<Wry>) -> Result<Option<PhysicalPosition<i32>>, String> {
    let path = state_path(app)?;
    if !path.exists() {
        return Ok(None);
    }

    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return Ok(None),
    };
    let state = match serde_json::from_str::<SavedWindowState>(&raw) {
        Ok(state) => state,
        Err(_) => return Ok(None),
    };
    if state.version != STATE_VERSION {
        return Ok(None);
    }

    Ok(Some(PhysicalPosition::new(state.main.x, state.main.y)))
}

fn save_position(app: &AppHandle<Wry>, position: PhysicalPosition<i32>) -> Result<(), String> {
    let path = state_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let state = SavedWindowState {
        version: STATE_VERSION,
        main: SavedPosition {
            x: position.x,
            y: position.y,
        },
    };
    let json = serde_json::to_string_pretty(&state).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| error.to_string())
}

fn state_path(app: &AppHandle<Wry>) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join(STATE_FILE_NAME))
        .map_err(|error| error.to_string())
}

fn position_is_visible_on_any_monitor(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    monitors: &[Monitor],
) -> bool {
    monitors.iter().any(|monitor| {
        position_is_visible_in_work_area(position, size, work_area_from_monitor(monitor))
    })
}

fn position_is_visible_in_work_area(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    work_area: WorkArea,
) -> bool {
    let left = i64::from(position.x);
    let top = i64::from(position.y);
    let right = left + i64::from(size.width);
    let bottom = top + i64::from(size.height);

    let area_left = i64::from(work_area.x);
    let area_top = i64::from(work_area.y);
    let area_right = area_left + i64::from(work_area.width);
    let area_bottom = area_top + i64::from(work_area.height);

    let visible_width = right.min(area_right) - left.max(area_left);
    let visible_height = bottom.min(area_bottom) - top.max(area_top);

    visible_width >= MIN_VISIBLE_WIDTH && visible_height >= MIN_VISIBLE_HEIGHT
}

fn bottom_right_position(work_area: WorkArea, size: PhysicalSize<u32>) -> PhysicalPosition<i32> {
    let width = i32::try_from(size.width).unwrap_or(i32::MAX);
    let height = i32::try_from(size.height).unwrap_or(i32::MAX);
    let area_width = i32::try_from(work_area.width).unwrap_or(i32::MAX);
    let area_height = i32::try_from(work_area.height).unwrap_or(i32::MAX);

    let x = work_area.x + area_width - width - EDGE_MARGIN;
    let y = work_area.y + area_height - height - EDGE_MARGIN;

    PhysicalPosition::new(x.max(work_area.x), y.max(work_area.y))
}

fn work_area_from_monitor(monitor: &Monitor) -> WorkArea {
    let work_area = monitor.work_area();
    WorkArea {
        x: work_area.position.x,
        y: work_area.position.y,
        width: work_area.size.width,
        height: work_area.size.height,
    }
}

#[cfg(test)]
mod tests {
    use super::{bottom_right_position, position_is_visible_in_work_area, WorkArea, EDGE_MARGIN};
    use tauri::{PhysicalPosition, PhysicalSize};

    #[test]
    fn bottom_right_position_uses_work_area_and_margin() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(306, 92);

        let position = bottom_right_position(work_area, size);

        assert_eq!(position.x, 1920 - 306 - EDGE_MARGIN);
        assert_eq!(position.y, 1040 - 92 - EDGE_MARGIN);
    }

    #[test]
    fn bottom_right_position_supports_negative_monitor_coordinates() {
        let work_area = WorkArea {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(306, 92);

        let position = bottom_right_position(work_area, size);

        assert_eq!(position.x, -306 - EDGE_MARGIN);
        assert_eq!(position.y, 1040 - 92 - EDGE_MARGIN);
    }

    #[test]
    fn visible_position_is_accepted_when_enough_window_remains_on_screen() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(306, 92);
        let position = PhysicalPosition::new(1880, 100);

        assert!(position_is_visible_in_work_area(position, size, work_area));
    }

    #[test]
    fn offscreen_position_is_rejected() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(306, 92);
        let position = PhysicalPosition::new(2200, 100);

        assert!(!position_is_visible_in_work_area(position, size, work_area));
    }
}
