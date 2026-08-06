use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, Position, WebviewWindow, Window,
    Wry,
};

const STATE_FILE_NAME: &str = "quotadock-window-state.json";
const STATE_VERSION: u8 = 2;
const LEGACY_STATE_VERSION: u8 = 1;
const LEGACY_MAIN_WINDOW_WIDTH: u64 = 360;
const MAIN_WINDOW_WIDTH: u64 = 260;
const MAIN_WINDOW_LABEL: &str = "main";
const EDGE_MARGIN: i32 = 12;
const SNAP_THRESHOLD: i32 = 18;
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
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MonitorGeometry {
    work_area: WorkArea,
    scale_factor: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LogicalWindowSize {
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SnapCandidate {
    position: PhysicalPosition<i32>,
    distance: i64,
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

pub fn snap_main_window_position(window: &Window<Wry>, position: PhysicalPosition<i32>) {
    let _ = snap_window_position(window, position);
}

pub fn save_current_main_window_position(window: &Window<Wry>) {
    match (window.outer_position(), window.outer_size()) {
        (Ok(position), Ok(size)) => {
            let position = snap_window_position(window, position).unwrap_or(position);
            if let Err(error) = save_position(window.app_handle(), position, size) {
                eprintln!("save main window position failed: {error}");
            }
        }
        (Err(error), _) => eprintln!("read main window position failed: {error}"),
        (_, Err(error)) => eprintln!("read main window size failed: {error}"),
    }
}

pub fn save_current_main_webview_window_position(window: &WebviewWindow<Wry>) {
    let native_window = window.as_ref().window();
    save_current_main_window_position(&native_window);
}

pub fn save_main_window_position_for_app(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        save_current_main_webview_window_position(&window);
    }
}

fn restore_window_position(window: &WebviewWindow<Wry>) -> Result<(), String> {
    let size = window
        .outer_size()
        .map_err(|error| format!("read initial window size: {error}"))?;
    let scale_factor = window
        .scale_factor()
        .map_err(|error| format!("read initial monitor scale: {error}"))?;
    let logical_size = logical_size_from_physical(size, scale_factor)
        .ok_or_else(|| "invalid main window size or scale factor".to_string())?;
    let monitors = window
        .available_monitors()
        .map_err(|error| format!("enumerate monitors: {error}"))?;
    let monitor_geometries = monitors.iter().map(monitor_geometry).collect::<Vec<_>>();
    let resolved = load_saved_state(window.app_handle())?
        .and_then(|state| resolve_saved_position(state, logical_size, &monitor_geometries));

    let position = resolved
        .map(|value| value.position)
        .or_else(|| fallback_bottom_right_position(window, logical_size))
        .ok_or_else(|| "no available monitor for window positioning".to_string())?;

    window
        .set_position(Position::Physical(position))
        .map_err(|error| format!("move restored window: {error}"))?;

    if let Some(resolved) = resolved.filter(|value| value.should_persist) {
        if let Err(error) = save_position(
            window.app_handle(),
            resolved.position,
            resolved.physical_size,
        ) {
            eprintln!("persist migrated main window position failed: {error}");
        }
    }

    Ok(())
}

fn fallback_bottom_right_position(
    window: &WebviewWindow<Wry>,
    logical_size: LogicalWindowSize,
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

    let geometry = monitor_geometry(&monitor);
    Some(bottom_right_position(
        geometry.work_area,
        physical_size_for_scale(logical_size, geometry.scale_factor)?,
    ))
}

fn load_saved_state(app: &AppHandle<Wry>) -> Result<Option<SavedWindowState>, String> {
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
    if !matches!(state.version, LEGACY_STATE_VERSION | STATE_VERSION) {
        return Ok(None);
    }

    Ok(Some(state))
}

fn save_position(
    app: &AppHandle<Wry>,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> Result<(), String> {
    let path = state_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let state = SavedWindowState {
        version: STATE_VERSION,
        main: SavedPosition {
            x: position.x,
            y: position.y,
            width: Some(size.width),
            height: Some(size.height),
        },
    };
    let json = serde_json::to_string_pretty(&state).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedPosition {
    position: PhysicalPosition<i32>,
    physical_size: PhysicalSize<u32>,
    should_persist: bool,
}

fn resolve_saved_position(
    state: SavedWindowState,
    current_logical_size: LogicalWindowSize,
    monitors: &[MonitorGeometry],
) -> Option<ResolvedPosition> {
    let original = PhysicalPosition::new(state.main.x, state.main.y);
    monitors.iter().find_map(|monitor| {
        let current_size = physical_size_for_scale(current_logical_size, monitor.scale_factor)?;
        let saved_size = saved_size_for_state(state, current_size)?;
        if !position_is_visible_in_work_area(original, saved_size, monitor.work_area) {
            return None;
        }
        let position =
            resize_preserving_edge_anchor(original, saved_size, current_size, monitor.work_area);
        position_is_visible_in_work_area(position, current_size, monitor.work_area).then_some(
            ResolvedPosition {
                position,
                physical_size: current_size,
                should_persist: state.version != STATE_VERSION || saved_size != current_size,
            },
        )
    })
}

fn logical_size_from_physical(
    physical_size: PhysicalSize<u32>,
    scale_factor: f64,
) -> Option<LogicalWindowSize> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return None;
    }
    Some(LogicalWindowSize {
        width: f64::from(physical_size.width) / scale_factor,
        height: f64::from(physical_size.height) / scale_factor,
    })
}

fn physical_size_for_scale(
    logical_size: LogicalWindowSize,
    scale_factor: f64,
) -> Option<PhysicalSize<u32>> {
    fn dimension(value: f64) -> Option<u32> {
        let value = value.round();
        (value.is_finite() && value >= 1.0 && value <= f64::from(u32::MAX)).then_some(value as u32)
    }

    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return None;
    }
    Some(PhysicalSize::new(
        dimension(logical_size.width * scale_factor)?,
        dimension(logical_size.height * scale_factor)?,
    ))
}

fn saved_size_for_state(
    state: SavedWindowState,
    current_size: PhysicalSize<u32>,
) -> Option<PhysicalSize<u32>> {
    match state.version {
        STATE_VERSION => Some(PhysicalSize::new(
            state.main.width.filter(|width| *width > 0)?,
            state.main.height.filter(|height| *height > 0)?,
        )),
        LEGACY_STATE_VERSION => {
            let scaled_width = (u64::from(current_size.width)
                .saturating_mul(LEGACY_MAIN_WINDOW_WIDTH)
                .saturating_add(MAIN_WINDOW_WIDTH / 2)
                / MAIN_WINDOW_WIDTH)
                .min(u64::from(u32::MAX)) as u32;
            Some(PhysicalSize::new(scaled_width, current_size.height))
        }
        _ => None,
    }
}

fn resize_preserving_edge_anchor(
    position: PhysicalPosition<i32>,
    saved_size: PhysicalSize<u32>,
    current_size: PhysicalSize<u32>,
    work_area: WorkArea,
) -> PhysicalPosition<i32> {
    let x = resize_axis_preserving_edge_anchor(
        i64::from(position.x),
        i64::from(saved_size.width),
        i64::from(current_size.width),
        i64::from(work_area.x),
        i64::from(work_area.x) + i64::from(work_area.width),
    );
    let y = resize_axis_preserving_edge_anchor(
        i64::from(position.y),
        i64::from(saved_size.height),
        i64::from(current_size.height),
        i64::from(work_area.y),
        i64::from(work_area.y) + i64::from(work_area.height),
    );
    PhysicalPosition::new(i32_from_i64(x), i32_from_i64(y))
}

fn resize_axis_preserving_edge_anchor(
    start: i64,
    saved_span: i64,
    current_span: i64,
    area_start: i64,
    area_end: i64,
) -> i64 {
    let start_gap = start - area_start;
    let end_gap = area_end - (start + saved_span);
    let threshold = i64::from(SNAP_THRESHOLD);

    if end_gap.abs() <= threshold && end_gap.abs() < start_gap.abs() {
        area_end - current_span - end_gap
    } else {
        start
    }
}

fn state_path(app: &AppHandle<Wry>) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join(STATE_FILE_NAME))
        .map_err(|error| error.to_string())
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

    let required_width = MIN_VISIBLE_WIDTH.min(i64::from(size.width));
    let required_height = MIN_VISIBLE_HEIGHT.min(i64::from(size.height));

    visible_width >= required_width && visible_height >= required_height
}

fn snap_window_position(
    window: &Window<Wry>,
    position: PhysicalPosition<i32>,
) -> Option<PhysicalPosition<i32>> {
    let size = window.outer_size().ok()?;
    let monitors = window.available_monitors().ok()?;
    snap_to_nearest_work_area_edge(position, size, &monitors)
        .map(|candidate| candidate.position)
        .inspect(|snapped| {
            if *snapped != position {
                let _ = window.set_position(Position::Physical(*snapped));
            }
        })
}

fn snap_to_nearest_work_area_edge(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    monitors: &[Monitor],
) -> Option<SnapCandidate> {
    monitors
        .iter()
        .filter_map(|monitor| {
            snap_to_work_area_edge(position, size, work_area_from_monitor(monitor))
        })
        .min_by_key(|candidate| candidate.distance)
}

fn snap_to_work_area_edge(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    work_area: WorkArea,
) -> Option<SnapCandidate> {
    let left = i64::from(position.x);
    let top = i64::from(position.y);
    let width = i64::from(size.width);
    let height = i64::from(size.height);
    let area_left = i64::from(work_area.x);
    let area_top = i64::from(work_area.y);
    let area_right = area_left + i64::from(work_area.width);
    let area_bottom = area_top + i64::from(work_area.height);

    let mut x = left;
    let mut y = top;
    let mut distance = 0_i64;
    let mut snapped = false;

    if let Some((snapped_x, edge_distance)) =
        nearest_axis_snap(left, left + width, area_left, area_right, width)
    {
        x = snapped_x;
        distance += edge_distance;
        snapped = true;
    }
    if let Some((snapped_y, edge_distance)) =
        nearest_axis_snap(top, top + height, area_top, area_bottom, height)
    {
        y = snapped_y;
        distance += edge_distance;
        snapped = true;
    }

    snapped.then_some(SnapCandidate {
        position: PhysicalPosition::new(i32_from_i64(x), i32_from_i64(y)),
        distance,
    })
}

fn nearest_axis_snap(
    start: i64,
    end: i64,
    area_start: i64,
    area_end: i64,
    span: i64,
) -> Option<(i64, i64)> {
    let start_distance = (start - area_start).abs();
    let end_distance = (end - area_end).abs();
    let threshold = i64::from(SNAP_THRESHOLD);

    match (start_distance <= threshold, end_distance <= threshold) {
        (true, true) if start_distance <= end_distance => Some((area_start, start_distance)),
        (true, true) => Some((area_end - span, end_distance)),
        (true, false) => Some((area_start, start_distance)),
        (false, true) => Some((area_end - span, end_distance)),
        (false, false) => None,
    }
}

fn i32_from_i64(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
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

fn monitor_geometry(monitor: &Monitor) -> MonitorGeometry {
    MonitorGeometry {
        work_area: work_area_from_monitor(monitor),
        scale_factor: monitor.scale_factor(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bottom_right_position, logical_size_from_physical, physical_size_for_scale,
        position_is_visible_in_work_area, resolve_saved_position, snap_to_work_area_edge,
        LogicalWindowSize, MonitorGeometry, SavedPosition, SavedWindowState, WorkArea, EDGE_MARGIN,
        LEGACY_STATE_VERSION, STATE_VERSION,
    };
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
        let position = PhysicalPosition::new(1856, 100);

        assert!(position_is_visible_in_work_area(position, size, work_area));
    }

    #[test]
    fn compact_status_bar_position_is_restored_when_fully_visible_vertically() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(260, 36);
        let position = PhysicalPosition::new(1648, 992);

        assert!(position_is_visible_in_work_area(position, size, work_area));
    }

    #[test]
    fn compact_status_bar_still_requires_a_useful_visible_width() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(260, 36);
        let position = PhysicalPosition::new(1880, 992);

        assert!(!position_is_visible_in_work_area(position, size, work_area));
    }

    #[test]
    fn legacy_right_edge_anchor_moves_with_compact_width() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let state = saved_state(LEGACY_STATE_VERSION, 1560, 900, None);

        let resolved = resolve(state, work_area, 1.0).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(1660, 900));
        assert!(resolved.should_persist);
    }

    #[test]
    fn legacy_right_edge_margin_is_preserved() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let state = saved_state(LEGACY_STATE_VERSION, 1548, 900, None);

        let resolved = resolve(state, work_area, 1.0).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(1648, 900));
    }

    #[test]
    fn legacy_free_position_is_not_shifted() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let state = saved_state(LEGACY_STATE_VERSION, 720, 420, None);

        let resolved = resolve(state, work_area, 1.0).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(720, 420));
    }

    #[test]
    fn legacy_left_edge_anchor_is_not_shifted() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let state = saved_state(LEGACY_STATE_VERSION, 0, 420, None);

        let resolved = resolve(state, work_area, 1.0).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(0, 420));
    }

    #[test]
    fn legacy_right_anchor_supports_negative_monitor_coordinates() {
        let work_area = WorkArea {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let state = saved_state(LEGACY_STATE_VERSION, -360, 900, None);

        let resolved = resolve(state, work_area, 1.0).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(-260, 900));
    }

    #[test]
    fn legacy_right_anchor_scales_at_one_hundred_fifty_percent() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 2560,
            height: 1400,
        };
        let state = saved_state(LEGACY_STATE_VERSION, 2020, 1200, None);

        let resolved = resolve(state, work_area, 1.5).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(2170, 1200));
        assert_eq!(resolved.physical_size, PhysicalSize::new(390, 54));
    }

    #[test]
    fn legacy_right_anchor_migrates_from_one_hundred_to_one_hundred_fifty_percent() {
        let work_area = WorkArea {
            x: 1920,
            y: 0,
            width: 2560,
            height: 1400,
        };
        let state = saved_state(LEGACY_STATE_VERSION, 3940, 1200, None);

        let resolved = resolve(state, work_area, 1.5).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(4090, 1200));
        assert_eq!(resolved.physical_size, PhysicalSize::new(390, 54));
        assert!(resolved.should_persist);
    }

    #[test]
    fn version_two_restore_is_idempotent_after_migration() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let state = saved_state(STATE_VERSION, 1660, 900, Some(PhysicalSize::new(260, 36)));

        let resolved = resolve(state, work_area, 1.0).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(1660, 900));
        assert!(!resolved.should_persist);
    }

    #[test]
    fn version_two_restore_is_idempotent_on_one_hundred_fifty_percent_monitor() {
        let work_area = WorkArea {
            x: 1920,
            y: 0,
            width: 2560,
            height: 1400,
        };
        let state = saved_state(STATE_VERSION, 4090, 1200, Some(PhysicalSize::new(390, 54)));

        let resolved = resolve(state, work_area, 1.5).unwrap();

        assert_eq!(resolved.position, PhysicalPosition::new(4090, 1200));
        assert_eq!(resolved.physical_size, PhysicalSize::new(390, 54));
        assert!(!resolved.should_persist);
    }

    #[test]
    fn logical_window_size_converts_between_monitor_scale_factors() {
        let logical = logical_size_from_physical(PhysicalSize::new(260, 36), 1.0).unwrap();

        assert_eq!(logical, compact_logical_size());
        assert_eq!(
            physical_size_for_scale(logical, 1.5),
            Some(PhysicalSize::new(390, 54))
        );
    }

    #[test]
    fn offscreen_saved_state_is_rejected_for_fallback() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let state = saved_state(LEGACY_STATE_VERSION, 2400, 900, None);

        assert!(resolve(state, work_area, 1.0).is_none());
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

    #[test]
    fn position_near_left_edge_snaps_to_work_area_left() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(276, 76);
        let position = PhysicalPosition::new(14, 180);

        let snapped = snap_to_work_area_edge(position, size, work_area).unwrap();

        assert_eq!(snapped.position, PhysicalPosition::new(0, 180));
    }

    #[test]
    fn position_near_right_and_bottom_edges_snaps_to_corner() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(276, 76);
        let position = PhysicalPosition::new(1920 - 276 - 8, 1040 - 76 - 12);

        let snapped = snap_to_work_area_edge(position, size, work_area).unwrap();

        assert_eq!(
            snapped.position,
            PhysicalPosition::new(1920 - 276, 1040 - 76)
        );
    }

    #[test]
    fn position_away_from_edges_does_not_snap() {
        let work_area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let size = PhysicalSize::new(276, 76);
        let position = PhysicalPosition::new(120, 180);

        assert!(snap_to_work_area_edge(position, size, work_area).is_none());
    }

    fn saved_state(
        version: u8,
        x: i32,
        y: i32,
        size: Option<PhysicalSize<u32>>,
    ) -> SavedWindowState {
        SavedWindowState {
            version,
            main: SavedPosition {
                x,
                y,
                width: size.map(|value| value.width),
                height: size.map(|value| value.height),
            },
        }
    }

    fn compact_logical_size() -> LogicalWindowSize {
        LogicalWindowSize {
            width: 260.0,
            height: 36.0,
        }
    }

    fn resolve(
        state: SavedWindowState,
        work_area: WorkArea,
        scale_factor: f64,
    ) -> Option<super::ResolvedPosition> {
        resolve_saved_position(
            state,
            compact_logical_size(),
            &[MonitorGeometry {
                work_area,
                scale_factor,
            }],
        )
    }
}
