use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
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
const DOCK_WIDTH: f64 = 52.0;
const DOCK_HEIGHT: f64 = 96.0;
const EDGE_GAP: f64 = 0.0;
const FRAME_TIME: Duration = Duration::from_millis(16);
const PHASE_TIME: Duration = Duration::from_millis(110);

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

#[derive(Default)]
struct TransitionState {
    active: Arc<AtomicBool>,
    mode: AtomicU8,
    expanded_rect: Mutex<Option<WindowRect>>,
    dock_rect: Mutex<Option<WindowRect>>,
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
    let width = DOCK_WIDTH * scale;
    let height = DOCK_HEIGHT * scale;
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
    let monitor_right = monitor_left + monitor_width;
    let monitor_bottom = monitor_top + monitor_height;
    let current_center = current.x + current.width / 2.0;
    let monitor_center = monitor_left + monitor_width / 2.0;
    let edge = if current_center < monitor_center {
        "left"
    } else {
        "right"
    };
    let x = if edge == "left" {
        monitor_left
    } else {
        monitor_right - current.width
    };
    let y = current.y.clamp(
        monitor_top,
        (monitor_bottom - current.height).max(monitor_top),
    );

    (WindowRect { x, y, ..current }, edge)
}

fn snap_rect_to_nearest_side(current: WindowRect, monitor: &Monitor) -> (WindowRect, &'static str) {
    snap_rect_to_bounds(
        current,
        monitor.position().x as f64,
        monitor.position().y as f64,
        monitor.size().width as f64,
        monitor.size().height as f64,
    )
}

fn offscreen_dock_rect(target: WindowRect, monitor: &Monitor) -> WindowRect {
    let monitor_left = monitor.position().x as f64;
    let monitor_right = monitor_left + monitor.size().width as f64;
    let distance_to_left = (target.x - monitor_left).abs();
    let distance_to_right = (monitor_right - (target.x + target.width)).abs();
    let x = if distance_to_left < distance_to_right {
        monitor_left - target.width - EDGE_GAP * monitor.scale_factor()
    } else {
        monitor_right + EDGE_GAP * monitor.scale_factor()
    };

    WindowRect { x, ..target }
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
        let dock_target = physical_rect(&dock)?;
        *state
            .dock_rect
            .lock()
            .map_err(|_| "window state lock was poisoned".to_string())? = Some(dock_target);
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
    let widget_rect = default_widget_rect(&monitor);
    let dock_target = dock_rect(&monitor);

    widget
        .set_size(Size::Logical(LogicalSize::new(WIDGET_WIDTH, WIDGET_HEIGHT)))
        .map_err(|error| error.to_string())?;
    widget
        .set_position(Position::Physical(PhysicalPosition::new(
            widget_rect.x.round() as i32,
            widget_rect.y.round() as i32,
        )))
        .map_err(|error| error.to_string())?;
    dock.set_size(Size::Logical(LogicalSize::new(DOCK_WIDTH, DOCK_HEIGHT)))
        .map_err(|error| error.to_string())?;
    dock.set_position(Position::Physical(PhysicalPosition::new(
        dock_target.x.round() as i32,
        dock_target.y.round() as i32,
    )))
    .map_err(|error| error.to_string())?;
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
        ease_out_cubic, snap_rect_to_bounds, validate_directory, validate_source, WindowRect,
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
    fn dock_snaps_to_nearest_side_and_stays_inside_monitor_height() {
        let left = WindowRect {
            x: 120.0,
            y: -30.0,
            width: 52.0,
            height: 96.0,
        };
        let right = WindowRect {
            x: 1500.0,
            y: 1050.0,
            ..left
        };

        let (left_target, left_edge) = snap_rect_to_bounds(left, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(left_edge, "left");
        assert_eq!(left_target.x, 0.0);
        assert_eq!(left_target.y, 0.0);

        let (right_target, right_edge) = snap_rect_to_bounds(right, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(right_edge, "right");
        assert_eq!(right_target.x, 1868.0);
        assert_eq!(right_target.y, 984.0);
    }
}
