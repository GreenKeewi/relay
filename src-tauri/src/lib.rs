use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, Position, Size,
    WebviewWindow,
};
use tauri_plugin_opener::OpenerExt;
use url::Url;

mod claude;

const WIDGET_WIDTH: f64 = 380.0;
const WIDGET_HEIGHT: f64 = 700.0;
const WIDGET_MIN_WIDTH: f64 = 320.0;
const WIDGET_MIN_HEIGHT: f64 = 520.0;
const WIDGET_MAX_WIDTH: f64 = 720.0;
const WIDGET_MAX_HEIGHT: f64 = 1_000.0;
const DOCK_WALL_THICKNESS: f64 = 52.0;
const DOCK_WALL_LENGTH: f64 = 58.0;
const DOCK_DRAG_SIZE: f64 = 58.0;
const DOCK_MENU_WIDTH: f64 = 190.0;
const DOCK_MENU_HEIGHT: f64 = 166.0;
const WINDOW_GEOMETRY_VERSION: u8 = 1;
const MAX_WINDOW_GEOMETRY_BYTES: usize = 16 * 1024;
const WINDOW_GEOMETRY_FILE: &str = "window-geometry.json";
const EDGE_GAP: f64 = 0.0;
const FRAME_TIME: Duration = Duration::from_millis(16);
const PHASE_TIME: Duration = Duration::from_millis(110);
const GEOMETRY_SETTLE_TIME: Duration = Duration::from_millis(250);

const MODE_WIDGET: u8 = 0;
const MODE_TRANSITIONING: u8 = 1;
const MODE_DOCK: u8 = 2;

#[derive(Clone, Copy, Debug)]
struct WindowRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Deserialize, Serialize)]
struct StoredWindowGeometry {
    version: u8,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn drag_square_rect(current: WindowRect, scale: f64) -> WindowRect {
    let size = DOCK_DRAG_SIZE * scale;
    WindowRect {
        x: current.x + (current.width - size) / 2.0,
        y: current.y + (current.height - size) / 2.0,
        width: size,
        height: size,
    }
}

fn decode_window_geometry(bytes: &[u8]) -> Option<WindowRect> {
    if bytes.len() > MAX_WINDOW_GEOMETRY_BYTES {
        return None;
    }
    let stored = serde_json::from_slice::<StoredWindowGeometry>(bytes).ok()?;
    let values = [stored.x, stored.y, stored.width, stored.height];
    if stored.version != WINDOW_GEOMETRY_VERSION
        || values.iter().any(|value| !value.is_finite())
        || stored.x.abs() > 100_000.0
        || stored.y.abs() > 100_000.0
        || !(WIDGET_MIN_WIDTH..=4_096.0).contains(&stored.width)
        || !(400.0..=4_096.0).contains(&stored.height)
    {
        return None;
    }
    Some(WindowRect {
        x: stored.x,
        y: stored.y,
        width: stored.width,
        height: stored.height,
    })
}

fn fit_widget_rect_to_bounds(
    rect: WindowRect,
    monitor_left: f64,
    monitor_top: f64,
    monitor_width: f64,
    monitor_height: f64,
    scale: f64,
) -> WindowRect {
    let width = rect
        .width
        .clamp(WIDGET_MIN_WIDTH * scale, WIDGET_MAX_WIDTH * scale)
        .min(monitor_width);
    let height = rect
        .height
        .clamp(WIDGET_MIN_HEIGHT * scale, WIDGET_MAX_HEIGHT * scale)
        .min(monitor_height);
    WindowRect {
        x: rect.x.clamp(
            monitor_left,
            (monitor_left + monitor_width - width).max(monitor_left),
        ),
        y: rect.y.clamp(
            monitor_top,
            (monitor_top + monitor_height - height).max(monitor_top),
        ),
        width,
        height,
    }
}

fn should_persist_geometry(scheduled_revision: u64, current_revision: u64, mode: u8) -> bool {
    scheduled_revision == current_revision && mode == MODE_WIDGET
}

#[derive(Default)]
struct TransitionState {
    active: Arc<AtomicBool>,
    mode: AtomicU8,
    expanded_rect: Mutex<Option<WindowRect>>,
    dock_rect: Mutex<Option<WindowRect>>,
    geometry_revision: AtomicU64,
    hidden_revision: AtomicU64,
}

struct TransitionGuard(Arc<AtomicBool>);

impl Drop for TransitionGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowModeSnapshot {
    mode: &'static str,
    widget_visible: bool,
    dock_visible: bool,
    monitor: Option<MonitorPlacement>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorPlacement {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale_factor: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DockPlacement {
    edge: &'static str,
    x: i32,
    y: i32,
}

fn begin_transition(state: &TransitionState) -> Result<TransitionGuard, String> {
    state
        .active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "a window transition is already in progress".to_string())?;
    Ok(TransitionGuard(state.active.clone()))
}

fn ease_out_cubic(t: f64) -> f64 {
    let clamped = t.clamp(0.0, 1.0);
    1.0 - (1.0 - clamped).powi(3)
}

fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}

fn interpolate(start: WindowRect, end: WindowRect, amount: f64) -> WindowRect {
    WindowRect {
        x: lerp(start.x, end.x, amount),
        y: lerp(start.y, end.y, amount),
        width: lerp(start.width, end.width, amount),
        height: lerp(start.height, end.height, amount),
    }
}

fn set_rect(window: &WebviewWindow, rect: WindowRect) -> Result<(), String> {
    window
        .set_size(Size::Physical(PhysicalSize::new(
            rect.width.round().max(1.0) as u32,
            rect.height.round().max(1.0) as u32,
        )))
        .map_err(|error| error.to_string())?;
    window
        .set_position(Position::Physical(PhysicalPosition::new(
            rect.x.round() as i32,
            rect.y.round() as i32,
        )))
        .map_err(|error| error.to_string())
}

async fn animate_window(
    window: &WebviewWindow,
    from: WindowRect,
    to: WindowRect,
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let progress = (started.elapsed().as_secs_f64() / PHASE_TIME.as_secs_f64()).min(1.0);
        set_rect(window, interpolate(from, to, ease_out_cubic(progress)))?;
        if progress >= 1.0 {
            return Ok(());
        }
        tokio::time::sleep(FRAME_TIME).await;
    }
}

fn physical_rect(window: &WebviewWindow) -> Result<WindowRect, String> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let size = window.outer_size().map_err(|error| error.to_string())?;
    Ok(WindowRect {
        x: position.x as f64,
        y: position.y as f64,
        width: size.width as f64,
        height: size.height as f64,
    })
}

fn monitor_for(window: &WebviewWindow) -> Result<Monitor, String> {
    window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no monitor is available for the Relay window".to_string())
}

fn dock_rect(monitor: &Monitor) -> WindowRect {
    let scale = monitor.scale_factor();
    let position = monitor.position();
    let size = monitor.size();
    let width = DOCK_WALL_THICKNESS * scale;
    let height = DOCK_WALL_LENGTH * scale;
    WindowRect {
        x: position.x as f64 + size.width as f64 - width - EDGE_GAP * scale,
        y: position.y as f64 + (size.height as f64 - height) / 2.0,
        width,
        height,
    }
}

fn snap_rect_to_bounds(
    current: WindowRect,
    monitor_left: f64,
    monitor_top: f64,
    monitor_width: f64,
    monitor_height: f64,
) -> (WindowRect, &'static str) {
    snap_rect_to_scaled_bounds(
        current,
        monitor_left,
        monitor_top,
        monitor_width,
        monitor_height,
        1.0,
    )
}

fn snap_rect_to_scaled_bounds(
    current: WindowRect,
    monitor_left: f64,
    monitor_top: f64,
    monitor_width: f64,
    monitor_height: f64,
    scale: f64,
) -> (WindowRect, &'static str) {
    let monitor_right = monitor_left + monitor_width;
    let monitor_bottom = monitor_top + monitor_height;
    let center_x = current.x + current.width / 2.0;
    let center_y = current.y + current.height / 2.0;
    let distances = [
        (center_x - monitor_left, "left"),
        (monitor_right - center_x, "right"),
        (center_y - monitor_top, "top"),
        (monitor_bottom - center_y, "bottom"),
    ];
    let edge = distances
        .into_iter()
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, edge)| edge)
        .unwrap_or("right");
    let (width, height) = if matches!(edge, "left" | "right") {
        (DOCK_WALL_THICKNESS * scale, DOCK_WALL_LENGTH * scale)
    } else {
        (DOCK_WALL_LENGTH * scale, DOCK_WALL_THICKNESS * scale)
    };
    let x = match edge {
        "left" => monitor_left,
        "right" => monitor_right - width,
        _ => {
            (center_x - width / 2.0).clamp(monitor_left, (monitor_right - width).max(monitor_left))
        }
    };
    let y = match edge {
        "top" => monitor_top,
        "bottom" => monitor_bottom - height,
        _ => {
            (center_y - height / 2.0).clamp(monitor_top, (monitor_bottom - height).max(monitor_top))
        }
    };

    (
        WindowRect {
            x,
            y,
            width,
            height,
        },
        edge,
    )
}

fn snap_rect_to_nearest_side(current: WindowRect, monitor: &Monitor) -> (WindowRect, &'static str) {
    snap_rect_to_scaled_bounds(
        current,
        monitor.position().x as f64,
        monitor.position().y as f64,
        monitor.size().width as f64,
        monitor.size().height as f64,
        monitor.scale_factor(),
    )
}

fn dock_edge_for_bounds(
    rect: WindowRect,
    monitor_left: f64,
    monitor_top: f64,
    monitor_width: f64,
    monitor_height: f64,
) -> &'static str {
    let monitor_right = monitor_left + monitor_width;
    let monitor_bottom = monitor_top + monitor_height;
    [
        ((rect.x - monitor_left).abs(), "left"),
        ((monitor_right - (rect.x + rect.width)).abs(), "right"),
        ((rect.y - monitor_top).abs(), "top"),
        ((monitor_bottom - (rect.y + rect.height)).abs(), "bottom"),
    ]
    .into_iter()
    .min_by(|left, right| left.0.total_cmp(&right.0))
    .map(|(_, edge)| edge)
    .unwrap_or("right")
}

fn dock_menu_rect_for_bounds(
    attached: WindowRect,
    edge: &str,
    monitor_left: f64,
    monitor_top: f64,
    monitor_width: f64,
    monitor_height: f64,
    scale: f64,
) -> WindowRect {
    let monitor_right = monitor_left + monitor_width;
    let monitor_bottom = monitor_top + monitor_height;
    let width = (DOCK_MENU_WIDTH * scale).min(monitor_width);
    let height = (DOCK_MENU_HEIGHT * scale).min(monitor_height);
    let center_x = attached.x + attached.width / 2.0;
    let center_y = attached.y + attached.height / 2.0;
    let x = match edge {
        "left" => monitor_left,
        "right" => monitor_right - width,
        _ => (center_x - width / 2.0).clamp(monitor_left, monitor_right - width),
    };
    let y = match edge {
        "top" => monitor_top,
        "bottom" => monitor_bottom - height,
        _ => (center_y - height / 2.0).clamp(monitor_top, monitor_bottom - height),
    };
    WindowRect {
        x,
        y,
        width,
        height,
    }
}

fn validate_hide_duration(duration_seconds: u64) -> Result<Duration, String> {
    match duration_seconds {
        3_600 | 86_400 => Ok(Duration::from_secs(duration_seconds)),
        _ => Err("unsupported dock hide duration".to_string()),
    }
}

fn offscreen_dock_rect(target: WindowRect, monitor: &Monitor) -> WindowRect {
    let monitor_left = monitor.position().x as f64;
    let monitor_top = monitor.position().y as f64;
    offscreen_rect_for_bounds(
        target,
        monitor_left,
        monitor_top,
        monitor.size().width as f64,
        monitor.size().height as f64,
    )
}

fn offscreen_rect_for_bounds(
    target: WindowRect,
    monitor_left: f64,
    monitor_top: f64,
    monitor_width: f64,
    monitor_height: f64,
) -> WindowRect {
    let monitor_right = monitor_left + monitor_width;
    let monitor_bottom = monitor_top + monitor_height;
    let distances = [
        ((target.x - monitor_left).abs(), "left"),
        ((monitor_right - (target.x + target.width)).abs(), "right"),
        ((target.y - monitor_top).abs(), "top"),
        (
            (monitor_bottom - (target.y + target.height)).abs(),
            "bottom",
        ),
    ];
    match distances
        .into_iter()
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, edge)| edge)
        .unwrap_or("right")
    {
        "left" => WindowRect {
            x: monitor_left - target.width,
            ..target
        },
        "right" => WindowRect {
            x: monitor_right,
            ..target
        },
        "top" => WindowRect {
            y: monitor_top - target.height,
            ..target
        },
        _ => WindowRect {
            y: monitor_bottom,
            ..target
        },
    }
}

fn collapsed_widget_rect(expanded: WindowRect, dock: WindowRect) -> WindowRect {
    let scale = (dock.width / expanded.width)
        .min(dock.height / expanded.height)
        .min(1.0);
    let width = expanded.width * scale;
    let height = expanded.height * scale;
    WindowRect {
        x: dock.x + (dock.width - width) / 2.0,
        y: dock.y + (dock.height - height) / 2.0,
        width,
        height,
    }
}

fn default_widget_rect(monitor: &Monitor) -> WindowRect {
    let scale = monitor.scale_factor();
    let position = monitor.position();
    let size = monitor.size();
    let width = WIDGET_WIDTH * scale;
    let height = WIDGET_HEIGHT * scale;
    WindowRect {
        x: position.x as f64 + size.width as f64 - width - 28.0 * scale,
        y: position.y as f64 + ((size.height as f64 - height) / 2.0).max(EDGE_GAP * scale),
        width,
        height,
    }
}

fn window_geometry_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(WINDOW_GEOMETRY_FILE))
        .map_err(|error| error.to_string())
}

fn load_window_geometry(path: &Path) -> Option<WindowRect> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(MAX_WINDOW_GEOMETRY_BYTES.min(512));
    file.take((MAX_WINDOW_GEOMETRY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    decode_window_geometry(&bytes)
}

fn persist_window_geometry(path: &Path, rect: WindowRect) -> Result<(), String> {
    let stored = StoredWindowGeometry {
        version: WINDOW_GEOMETRY_VERSION,
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    };
    let serialized = serde_json::to_vec(&stored).map_err(|error| error.to_string())?;
    if serialized.len() > MAX_WINDOW_GEOMETRY_BYTES {
        return Err("window geometry exceeded the local storage limit".to_string());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "window geometry path has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&serialized)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn relay_windows(app: &AppHandle) -> Result<(WebviewWindow, WebviewWindow), String> {
    let widget = app
        .get_webview_window("main")
        .ok_or_else(|| "the Relay widget window is unavailable".to_string())?;
    let dock = app
        .get_webview_window("dock")
        .ok_or_else(|| "the Relay dock window is unavailable".to_string())?;
    Ok((widget, dock))
}

#[tauri::command]
async fn collapse_to_dock(
    app: AppHandle,
    state: tauri::State<'_, TransitionState>,
    reduced_motion: bool,
) -> Result<(), String> {
    if state.mode.load(Ordering::Acquire) == MODE_DOCK {
        return Ok(());
    }
    let _guard = begin_transition(&state)?;
    state.mode.store(MODE_TRANSITIONING, Ordering::Release);

    let result = async {
        let (widget, dock) = relay_windows(&app)?;
        let expanded = physical_rect(&widget)?;
        let monitor = monitor_for(&widget)?;
        let dock_target = state
            .dock_rect
            .lock()
            .map_err(|_| "window state lock was poisoned".to_string())?
            .unwrap_or_else(|| dock_rect(&monitor));
        let dock_offscreen = offscreen_dock_rect(dock_target, &monitor);
        let collapsed = collapsed_widget_rect(expanded, dock_target);

        *state
            .expanded_rect
            .lock()
            .map_err(|_| "window state lock was poisoned".to_string())? = Some(expanded);
        if let Ok(path) = window_geometry_path(&app) {
            let _ = persist_window_geometry(&path, expanded);
        }

        if !reduced_motion {
            animate_window(&widget, expanded, collapsed).await?;
        }
        widget.hide().map_err(|error| error.to_string())?;

        set_rect(
            &dock,
            if reduced_motion {
                dock_target
            } else {
                dock_offscreen
            },
        )?;
        dock.show().map_err(|error| error.to_string())?;
        if !reduced_motion {
            animate_window(&dock, dock_offscreen, dock_target).await?;
        }
        Ok(())
    }
    .await;

    state.mode.store(
        if result.is_ok() {
            MODE_DOCK
        } else {
            MODE_WIDGET
        },
        Ordering::Release,
    );
    result
}

#[tauri::command]
async fn expand_from_dock(
    app: AppHandle,
    state: tauri::State<'_, TransitionState>,
    reduced_motion: bool,
) -> Result<(), String> {
    if state.mode.load(Ordering::Acquire) == MODE_WIDGET {
        return Ok(());
    }
    let _guard = begin_transition(&state)?;
    state.mode.store(MODE_TRANSITIONING, Ordering::Release);

    let result = async {
        let (widget, dock) = relay_windows(&app)?;
        let monitor = monitor_for(&dock).or_else(|_| monitor_for(&widget))?;
        let dock_target = state
            .dock_rect
            .lock()
            .map_err(|_| "window state lock was poisoned".to_string())?
            .unwrap_or(physical_rect(&dock)?);
        state.hidden_revision.fetch_add(1, Ordering::AcqRel);
        set_rect(&dock, dock_target)?;
        let dock_offscreen = offscreen_dock_rect(dock_target, &monitor);
        let expanded = state
            .expanded_rect
            .lock()
            .map_err(|_| "window state lock was poisoned".to_string())?
            .unwrap_or_else(|| default_widget_rect(&monitor));
        let collapsed = collapsed_widget_rect(expanded, dock_target);

        if !reduced_motion {
            animate_window(&dock, dock_target, dock_offscreen).await?;
        }
        dock.hide().map_err(|error| error.to_string())?;

        set_rect(&widget, if reduced_motion { expanded } else { collapsed })?;
        widget.show().map_err(|error| error.to_string())?;
        if !reduced_motion {
            animate_window(&widget, collapsed, expanded).await?;
        }
        Ok(())
    }
    .await;

    state.mode.store(
        if result.is_ok() {
            MODE_WIDGET
        } else {
            MODE_DOCK
        },
        Ordering::Release,
    );
    result
}

#[tauri::command]
fn prepare_dock_drag(app: AppHandle) -> Result<(), String> {
    let (_, dock) = relay_windows(&app)?;
    let monitor = monitor_for(&dock)?;
    let dragging = drag_square_rect(physical_rect(&dock)?, monitor.scale_factor());
    set_rect(&dock, dragging)
}

#[tauri::command]
fn open_dock_menu(
    app: AppHandle,
    state: tauri::State<'_, TransitionState>,
) -> Result<DockPlacement, String> {
    let (_, dock) = relay_windows(&app)?;
    let monitor = monitor_for(&dock)?;
    let attached = state
        .dock_rect
        .lock()
        .map_err(|_| "window state lock was poisoned".to_string())?
        .unwrap_or(physical_rect(&dock)?);
    let monitor_left = monitor.position().x as f64;
    let monitor_top = monitor.position().y as f64;
    let monitor_width = monitor.size().width as f64;
    let monitor_height = monitor.size().height as f64;
    let edge = dock_edge_for_bounds(
        attached,
        monitor_left,
        monitor_top,
        monitor_width,
        monitor_height,
    );
    let menu = dock_menu_rect_for_bounds(
        attached,
        edge,
        monitor_left,
        monitor_top,
        monitor_width,
        monitor_height,
        monitor.scale_factor(),
    );
    set_rect(&dock, menu)?;
    Ok(DockPlacement {
        edge,
        x: menu.x.round() as i32,
        y: menu.y.round() as i32,
    })
}

#[tauri::command]
fn close_dock_menu(
    app: AppHandle,
    state: tauri::State<'_, TransitionState>,
) -> Result<DockPlacement, String> {
    let (_, dock) = relay_windows(&app)?;
    let monitor = monitor_for(&dock)?;
    let target = state
        .dock_rect
        .lock()
        .map_err(|_| "window state lock was poisoned".to_string())?
        .unwrap_or_else(|| dock_rect(&monitor));
    let edge = dock_edge_for_bounds(
        target,
        monitor.position().x as f64,
        monitor.position().y as f64,
        monitor.size().width as f64,
        monitor.size().height as f64,
    );
    set_rect(&dock, target)?;
    Ok(DockPlacement {
        edge,
        x: target.x.round() as i32,
        y: target.y.round() as i32,
    })
}

#[tauri::command]
fn hide_dock_for(
    app: AppHandle,
    state: tauri::State<'_, TransitionState>,
    duration_seconds: u64,
) -> Result<(), String> {
    let duration = validate_hide_duration(duration_seconds)?;
    let (_, dock) = relay_windows(&app)?;
    let monitor = monitor_for(&dock)?;
    let target = state
        .dock_rect
        .lock()
        .map_err(|_| "window state lock was poisoned".to_string())?
        .unwrap_or_else(|| dock_rect(&monitor));
    set_rect(&dock, target)?;
    dock.hide().map_err(|error| error.to_string())?;
    let revision = state.hidden_revision.fetch_add(1, Ordering::AcqRel) + 1;
    let app_for_timer = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(duration).await;
        let settled_state = app_for_timer.state::<TransitionState>();
        if settled_state.hidden_revision.load(Ordering::Acquire) != revision
            || settled_state.mode.load(Ordering::Acquire) != MODE_DOCK
        {
            return;
        }
        let Ok((_, settled_dock)) = relay_windows(&app_for_timer) else {
            return;
        };
        let target = settled_state.dock_rect.lock().ok().and_then(|rect| *rect);
        if let Some(target) = target {
            let _ = set_rect(&settled_dock, target);
        }
        let _ = settled_dock.show();
    });
    Ok(())
}

#[tauri::command]
fn snap_dock_to_nearest_edge(
    app: AppHandle,
    state: tauri::State<'_, TransitionState>,
) -> Result<DockPlacement, String> {
    let (_, dock) = relay_windows(&app)?;
    let monitor = monitor_for(&dock)?;
    let current = physical_rect(&dock)?;
    let (target, edge) = snap_rect_to_nearest_side(current, &monitor);
    set_rect(&dock, target)?;
    *state
        .dock_rect
        .lock()
        .map_err(|_| "window state lock was poisoned".to_string())? = Some(target);

    Ok(DockPlacement {
        edge,
        x: target.x.round() as i32,
        y: target.y.round() as i32,
    })
}

fn validate_source(source: &str) -> Result<Url, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return Err("session source must be between 1 and 2048 characters".to_string());
    }

    let url = Url::parse(trimmed).map_err(|_| "session source must be a valid URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only http and https session sources are allowed".to_string());
    }
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("session source must have a host and cannot contain credentials".to_string());
    }
    Ok(url)
}

fn validate_directory(source: &str) -> Result<PathBuf, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return Err("session source must be between 1 and 2048 characters".to_string());
    }

    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err("session directory must be an absolute path".to_string());
    }

    let canonical = path
        .canonicalize()
        .map_err(|_| "session directory does not exist".to_string())?;
    if !canonical.is_dir() {
        return Err("session source paths must point to a directory".to_string());
    }
    Ok(canonical)
}

#[tauri::command]
fn open_session(app: AppHandle, source: String) -> Result<(), String> {
    if let Ok(url) = validate_source(&source) {
        return app
            .opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|error| error.to_string());
    }

    let directory = validate_directory(&source)?;
    app.opener()
        .open_path(directory.to_string_lossy(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discover_claude_sessions() -> Result<claude::ClaudeDiscovery, String> {
    tauri::async_runtime::spawn_blocking(claude::discover_claude_sessions)
        .await
        .map_err(|error| format!("Claude session scan failed: {error}"))
}

#[tauri::command]
fn resume_claude_session(session_id: String, cwd: String) -> Result<(), String> {
    claude::resume_claude_session(&session_id, &cwd)
}

#[tauri::command]
async fn read_claude_usage() -> Result<claude::ClaudeUsage, String> {
    tauri::async_runtime::spawn_blocking(claude::read_claude_usage)
        .await
        .map_err(|error| format!("Claude usage refresh failed: {error}"))
}

#[tauri::command]
fn enable_claude_live_usage() -> Result<(), String> {
    claude::enable_claude_live_usage()
}

#[tauri::command]
fn reauthenticate_claude() -> Result<(), String> {
    claude::reauthenticate_claude()
}

#[tauri::command]
fn get_window_mode(
    app: AppHandle,
    state: tauri::State<'_, TransitionState>,
) -> Result<WindowModeSnapshot, String> {
    let (widget, dock) = relay_windows(&app)?;
    let mode = match state.mode.load(Ordering::Acquire) {
        MODE_WIDGET => "widget",
        MODE_DOCK => "dock",
        _ => "transitioning",
    };
    let monitor = widget
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or(dock.current_monitor().map_err(|error| error.to_string())?)
        .map(|monitor| MonitorPlacement {
            x: monitor.position().x,
            y: monitor.position().y,
            width: monitor.size().width,
            height: monitor.size().height,
            scale_factor: monitor.scale_factor(),
        });

    Ok(WindowModeSnapshot {
        mode,
        widget_visible: widget.is_visible().map_err(|error| error.to_string())?,
        dock_visible: dock.is_visible().map_err(|error| error.to_string())?,
        monitor,
    })
}

fn position_initial_windows(app: &AppHandle) -> Result<(), String> {
    let (widget, dock) = relay_windows(app)?;
    let monitor = monitor_for(&widget)?;
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let geometry_path = window_geometry_path(app).ok();
    let widget_rect = geometry_path
        .as_deref()
        .and_then(load_window_geometry)
        .map(|saved| {
            fit_widget_rect_to_bounds(
                saved,
                monitor_position.x as f64,
                monitor_position.y as f64,
                monitor_size.width as f64,
                monitor_size.height as f64,
                monitor.scale_factor(),
            )
        })
        .unwrap_or_else(|| default_widget_rect(&monitor));
    let dock_target = dock_rect(&monitor);

    widget
        .set_resizable(true)
        .map_err(|error| error.to_string())?;
    widget
        .set_min_size(Some(Size::Logical(LogicalSize::new(
            WIDGET_MIN_WIDTH,
            WIDGET_MIN_HEIGHT,
        ))))
        .map_err(|error| error.to_string())?;
    widget
        .set_max_size(Some(Size::Logical(LogicalSize::new(
            WIDGET_MAX_WIDTH,
            WIDGET_MAX_HEIGHT,
        ))))
        .map_err(|error| error.to_string())?;
    set_rect(&widget, widget_rect)?;
    set_rect(&dock, dock_target)?;

    *app.state::<TransitionState>()
        .expanded_rect
        .lock()
        .map_err(|_| "window state lock was poisoned".to_string())? = Some(widget_rect);
    *app.state::<TransitionState>()
        .dock_rect
        .lock()
        .map_err(|_| "window state lock was poisoned".to_string())? = Some(dock_target);

    let widget_for_events = widget.clone();
    let app_for_events = app.clone();
    widget.on_window_event(move |event| {
        if !matches!(
            event,
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
        ) {
            return;
        }
        let state = app_for_events.state::<TransitionState>();
        if state.mode.load(Ordering::Acquire) != MODE_WIDGET {
            return;
        }
        let Ok(rect) = physical_rect(&widget_for_events) else {
            return;
        };
        if let Ok(mut expanded) = state.expanded_rect.lock() {
            *expanded = Some(rect);
        }
        let revision = state.geometry_revision.fetch_add(1, Ordering::AcqRel) + 1;
        let settled_widget = widget_for_events.clone();
        let settled_app = app_for_events.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(GEOMETRY_SETTLE_TIME).await;
            let settled_state = settled_app.state::<TransitionState>();
            if !should_persist_geometry(
                revision,
                settled_state.geometry_revision.load(Ordering::Acquire),
                settled_state.mode.load(Ordering::Acquire),
            ) {
                return;
            }
            let Ok(settled_rect) = physical_rect(&settled_widget) else {
                return;
            };
            if let Ok(path) = window_geometry_path(&settled_app) {
                let _ = persist_window_geometry(&path, settled_rect);
            }
        });
    });

    if let Some(path) = geometry_path {
        let _ = persist_window_geometry(&path, widget_rect);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(TransitionState::default())
        .setup(|app| {
            position_initial_windows(&app.handle()).map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            collapse_to_dock,
            expand_from_dock,
            prepare_dock_drag,
            open_dock_menu,
            close_dock_menu,
            hide_dock_for,
            snap_dock_to_nearest_edge,
            open_session,
            discover_claude_sessions,
            resume_claude_session,
            read_claude_usage,
            enable_claude_live_usage,
            reauthenticate_claude,
            get_window_mode
        ])
        .run(tauri::generate_context!())
        .expect("error while running Relay");
}

#[cfg(test)]
mod tests {
    use super::{
        decode_window_geometry, dock_menu_rect_for_bounds, drag_square_rect, ease_out_cubic,
        fit_widget_rect_to_bounds, offscreen_rect_for_bounds, should_persist_geometry,
        snap_rect_to_bounds, snap_rect_to_scaled_bounds, validate_directory,
        validate_hide_duration, validate_source, WindowRect, MODE_DOCK, MODE_TRANSITIONING,
        MODE_WIDGET,
    };

    #[test]
    fn cubic_easing_has_expected_bounds_and_shape() {
        assert_eq!(ease_out_cubic(-1.0), 0.0);
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert!((ease_out_cubic(0.5) - 0.875).abs() < f64::EPSILON);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert_eq!(ease_out_cubic(2.0), 1.0);
    }

    #[test]
    fn source_validation_allows_only_safe_web_urls() {
        assert!(validate_source("https://example.com/session/123?from=relay").is_ok());
        assert!(validate_source("http://localhost:3000/session").is_ok());
        assert!(validate_source("file:///C:/secrets.txt").is_err());
        assert!(validate_source("javascript:alert(1)").is_err());
        assert!(validate_source("https://user:password@example.com").is_err());
        assert!(validate_source("not a url").is_err());
        assert!(validate_source("").is_err());
    }

    #[test]
    fn directory_validation_rejects_relative_and_missing_paths() {
        assert!(validate_directory("relative/path").is_err());
        assert!(validate_directory("C:\\this-directory-should-not-exist\\relay").is_err());
    }

    #[test]
    fn dock_snaps_to_nearest_of_all_four_edges_with_edge_specific_size() {
        let left = WindowRect {
            x: 120.0,
            y: 400.0,
            width: 58.0,
            height: 58.0,
        };
        let right = WindowRect {
            x: 1800.0,
            y: 400.0,
            ..left
        };
        let top = WindowRect {
            x: 700.0,
            y: 20.0,
            ..left
        };
        let bottom = WindowRect {
            x: 700.0,
            y: 1020.0,
            ..left
        };

        let (left_target, left_edge) = snap_rect_to_bounds(left, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(left_edge, "left");
        assert_eq!(left_target.x, 0.0);
        assert_eq!((left_target.width, left_target.height), (52.0, 58.0));

        let (right_target, right_edge) = snap_rect_to_bounds(right, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(right_edge, "right");
        assert_eq!(right_target.x, 1868.0);
        assert_eq!((right_target.width, right_target.height), (52.0, 58.0));

        let (top_target, top_edge) = snap_rect_to_bounds(top, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(top_edge, "top");
        assert_eq!(top_target.y, 0.0);
        assert_eq!((top_target.width, top_target.height), (58.0, 52.0));

        let (bottom_target, bottom_edge) = snap_rect_to_bounds(bottom, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(bottom_edge, "bottom");
        assert_eq!(bottom_target.y, 1028.0);
        assert_eq!((bottom_target.width, bottom_target.height), (58.0, 52.0));
    }

    #[test]
    fn dock_handoff_exits_through_the_attached_edge() {
        let left = WindowRect {
            x: 0.0,
            y: 400.0,
            width: 52.0,
            height: 58.0,
        };
        let right = WindowRect { x: 1868.0, ..left };
        let top = WindowRect {
            x: 700.0,
            y: 0.0,
            width: 58.0,
            height: 52.0,
        };
        let bottom = WindowRect { y: 1028.0, ..top };

        assert_eq!(
            offscreen_rect_for_bounds(left, 0.0, 0.0, 1920.0, 1080.0).x,
            -52.0
        );
        assert_eq!(
            offscreen_rect_for_bounds(right, 0.0, 0.0, 1920.0, 1080.0).x,
            1920.0
        );
        assert_eq!(
            offscreen_rect_for_bounds(top, 0.0, 0.0, 1920.0, 1080.0).y,
            -52.0
        );
        assert_eq!(
            offscreen_rect_for_bounds(bottom, 0.0, 0.0, 1920.0, 1080.0).y,
            1080.0
        );
    }

    #[test]
    fn dock_drag_uses_a_centered_square_at_monitor_scale() {
        let attached = WindowRect {
            x: 0.0,
            y: 401.0,
            width: 78.0,
            height: 87.0,
        };

        let dragging = drag_square_rect(attached, 1.5);

        assert_eq!((dragging.width, dragging.height), (87.0, 87.0));
        assert_eq!(dragging.x, -4.5);
        assert_eq!(dragging.y, 401.0);
    }

    #[test]
    fn dock_dimensions_scale_once_in_physical_coordinates() {
        let dragging = WindowRect {
            x: 1700.0,
            y: 500.0,
            width: 87.0,
            height: 87.0,
        };

        let (target, edge) = snap_rect_to_scaled_bounds(dragging, 0.0, 0.0, 1920.0, 1080.0, 1.5);

        assert_eq!(edge, "right");
        assert_eq!((target.width, target.height), (78.0, 87.0));
        assert_eq!(target.x, 1842.0);
    }

    #[test]
    fn persisted_widget_geometry_is_bounded_and_validated() {
        let valid = br#"{"version":1,"x":140.0,"y":90.0,"width":420.0,"height":760.0}"#;
        let decoded = decode_window_geometry(valid).expect("valid saved geometry");
        assert_eq!(decoded.x, 140.0);
        assert_eq!((decoded.width, decoded.height), (420.0, 760.0));

        assert!(decode_window_geometry(
            br#"{"version":2,"x":140.0,"y":90.0,"width":420.0,"height":760.0}"#
        )
        .is_none());
        assert!(decode_window_geometry(
            br#"{"version":1,"x":140.0,"y":90.0,"width":40.0,"height":76.0}"#
        )
        .is_none());
        assert!(decode_window_geometry(&vec![b' '; 16 * 1024 + 1]).is_none());
    }

    #[test]
    fn restored_widget_geometry_is_clamped_to_monitor_and_size_limits() {
        let stored = WindowRect {
            x: 1800.0,
            y: -200.0,
            width: 900.0,
            height: 300.0,
        };

        let fitted = fit_widget_rect_to_bounds(stored, 0.0, 0.0, 1920.0, 1080.0, 1.0);

        assert_eq!((fitted.width, fitted.height), (720.0, 520.0));
        assert_eq!((fitted.x, fitted.y), (1200.0, 0.0));
    }

    #[test]
    fn geometry_debounce_persists_only_the_latest_settled_widget_event() {
        assert!(should_persist_geometry(7, 7, MODE_WIDGET));
        assert!(!should_persist_geometry(6, 7, MODE_WIDGET));
        assert!(!should_persist_geometry(7, 7, MODE_TRANSITIONING));
        assert!(!should_persist_geometry(7, 7, MODE_DOCK));
    }

    #[test]
    fn dock_menu_expands_inward_and_keeps_the_trigger_on_each_wall() {
        let left = WindowRect {
            x: 0.0,
            y: 400.0,
            width: 52.0,
            height: 58.0,
        };
        let right = WindowRect { x: 1868.0, ..left };
        let top = WindowRect {
            x: 700.0,
            y: 0.0,
            width: 58.0,
            height: 52.0,
        };
        let bottom = WindowRect { y: 1028.0, ..top };

        let left_menu = dock_menu_rect_for_bounds(left, "left", 0.0, 0.0, 1920.0, 1080.0, 1.0);
        let right_menu = dock_menu_rect_for_bounds(right, "right", 0.0, 0.0, 1920.0, 1080.0, 1.0);
        let top_menu = dock_menu_rect_for_bounds(top, "top", 0.0, 0.0, 1920.0, 1080.0, 1.0);
        let bottom_menu =
            dock_menu_rect_for_bounds(bottom, "bottom", 0.0, 0.0, 1920.0, 1080.0, 1.0);

        assert_eq!((left_menu.x, left_menu.width), (0.0, 190.0));
        assert_eq!((right_menu.x, right_menu.width), (1730.0, 190.0));
        assert_eq!((top_menu.y, top_menu.height), (0.0, 166.0));
        assert_eq!((bottom_menu.y, bottom_menu.height), (914.0, 166.0));
    }

    #[test]
    fn timed_dock_hide_accepts_only_the_exposed_quick_action_durations() {
        assert!(validate_hide_duration(3_600).is_ok());
        assert!(validate_hide_duration(86_400).is_ok());
        assert!(validate_hide_duration(0).is_err());
        assert!(validate_hide_duration(60).is_err());
        assert!(validate_hide_duration(u64::MAX).is_err());
    }
}
