use std::cell::Cell;
use std::mem::size_of;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::menu::CheckMenuItem;
use tauri::window::Color;
use tauri::{
    App, AppHandle, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalRect, PhysicalSize,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::Graphics::Gdi::{
    CreateEllipticRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromWindow,
    SetWindowRgn, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowLongW,
    GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed, SetWindowPos,
    ShowWindow, GWL_STYLE, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
    WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCDESTROY, WM_SIZE, WS_CAPTION,
};

use crate::{lock, AppState, QUICK_HEIGHT, QUICK_WIDTH};

pub(crate) const LABEL: &str = "usage-widget";
const STRIP_WIDTH: f64 = 304.0;
const STRIP_HEIGHT: f64 = 48.0;
const BALL_DIAMETER: f64 = 72.0;
const DOCK_DISTANCE: f64 = 40.0;
const DOCK_INSET: f64 = 6.0;
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const SUBCLASS_ID: usize = 1;

type WorkArea = PhysicalRect<i32, u32>;

thread_local! {
    static OBSERVING_DRAG: Cell<bool> = const { Cell::new(false) };
    static DRAG_ORIGIN: Cell<Option<PhysicalPosition<i32>>> = const { Cell::new(None) };
    static DROP_ORIGIN: Cell<Option<PhysicalPosition<i32>>> = const { Cell::new(None) };
    static COMPACT: Cell<bool> = const { Cell::new(true) };
    static APPLIED_REGION: Cell<Option<(PhysicalSize<u32>, bool)>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum DockEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Fractions of the space the widget can travel within the monitor's work area.
/// A dock edge selects the compact shape and fixes its offset from that edge.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Placement {
    monitor: Option<String>,
    x: f64,
    y: f64,
    #[serde(default)]
    dock: Option<DockEdge>,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            monitor: None,
            x: 1.0,
            y: 0.12,
            dock: Some(DockEdge::Right),
        }
    }
}

pub(crate) fn menu_item(app: &App) -> tauri::Result<CheckMenuItem<tauri::Wry>> {
    CheckMenuItem::with_id(
        app,
        LABEL,
        "显示用量小组件 / Show Usage Widget",
        true,
        lock(&app.state::<AppState>().settings).usage_widget_enabled,
        None::<&str>,
    )
}

pub(crate) fn toggle(app: &AppHandle, item: &CheckMenuItem<tauri::Wry>) {
    let state = app.state::<AppState>();
    let enabled = !lock(&state.settings).usage_widget_enabled;
    state.update_settings(|settings| settings.usage_widget_enabled = enabled);
    let _ = item.set_checked(enabled);
    refresh(app);
}

pub(crate) fn start(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut next_attempt = Instant::now();
        loop {
            if lock(&handle.state::<AppState>().settings).usage_widget_enabled {
                // Building from a tray callback can deadlock WebView2, so the
                // worker creates the webview on demand outside the callback.
                if handle.get_webview_window(LABEL).is_none() && Instant::now() >= next_attempt {
                    if let Err(error) = create_window(&handle) {
                        eprintln!("Could not open the usage widget: {error}");
                        next_attempt = Instant::now() + Duration::from_secs(5);
                    }
                }
                let app = handle.clone();
                let (done, complete) = std::sync::mpsc::channel();
                if handle
                    .run_on_main_thread(move || {
                        refresh(&app);
                        let _ = done.send(());
                    })
                    .is_err()
                    || complete.recv().is_err()
                {
                    break;
                }
            } else {
                next_attempt = Instant::now();
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

fn create_window(app: &AppHandle) -> Result<(), String> {
    let widget = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Willow Usage Widget")
        .inner_size(BALL_DIAMETER, BALL_DIAMETER)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .background_color(Color(16, 26, 23, 255))
        .focused(false)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;

    // A window can only be subclassed on its owning thread.
    let (done, complete) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let installed = widget.hwnd().is_ok_and(|handle| unsafe {
            SetWindowSubclass(handle.0, Some(widget_subclass), SUBCLASS_ID, 0) != 0
        });
        OBSERVING_DRAG.set(installed);
        if !installed {
            let _ = widget.destroy();
        }
        let _ = done.send(installed);
    })
    .map_err(|error| error.to_string())?;
    if !complete.recv().map_err(|error| error.to_string())? {
        return Err("could not observe native widget dragging".into());
    }
    Ok(())
}

unsafe extern "system" fn widget_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass: usize,
    _: usize,
) -> LRESULT {
    // Let Windows manage the position for the entire native move loop.
    match message {
        WM_ENTERSIZEMOVE => DRAG_ORIGIN.set(window_position(hwnd)),
        WM_EXITSIZEMOVE => {
            if let Some(origin) = DRAG_ORIGIN.take() {
                if DROP_ORIGIN.get().is_none() {
                    DROP_ORIGIN.set(Some(origin));
                }
            }
        }
        WM_NCDESTROY => {
            OBSERVING_DRAG.set(false);
            DRAG_ORIGIN.set(None);
            DROP_ORIGIN.set(None);
            APPLIED_REGION.set(None);
            unsafe { RemoveWindowSubclass(hwnd, Some(widget_subclass), subclass) };
        }
        _ => {}
    }
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if message == WM_SIZE {
        // This also tracks physical size changes when a drag crosses between DPIs.
        apply_region(hwnd);
    }
    result
}

fn window_bounds(hwnd: HWND) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
    let mut bounds = RECT::default();
    (unsafe { GetWindowRect(hwnd, &mut bounds) } != 0).then(|| {
        (
            PhysicalPosition::new(bounds.left, bounds.top),
            PhysicalSize::new(
                (bounds.right - bounds.left) as u32,
                (bounds.bottom - bounds.top) as u32,
            ),
        )
    })
}

fn window_position(hwnd: HWND) -> Option<PhysicalPosition<i32>> {
    window_bounds(hwnd).map(|(position, _)| position)
}

fn apply_region(hwnd: HWND) -> bool {
    let Some((_, size)) = window_bounds(hwnd) else {
        return false;
    };
    if size.width == 0 || size.height == 0 {
        return false;
    }
    let compact = COMPACT.get();
    if APPLIED_REGION.get() == Some((size, compact)) {
        return true;
    }
    let w = size.width as i32;
    let h = size.height as i32;
    let region = unsafe {
        if compact {
            CreateEllipticRgn(0, 0, w, h)
        } else {
            CreateRoundRectRgn(0, 0, w, h, h, h)
        }
    };
    if region.is_null() {
        return false;
    }
    APPLIED_REGION.set(Some((size, compact)));
    // Windows owns the region once SetWindowRgn succeeds.
    if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
        unsafe { DeleteObject(region) };
        APPLIED_REGION.set(None);
        return false;
    }
    true
}

fn refresh(app: &AppHandle) {
    if !OBSERVING_DRAG.get() {
        return;
    }
    let Some(widget) = app.get_webview_window(LABEL) else {
        return;
    };
    let Ok(handle) = widget.hwnd() else {
        return;
    };
    let hwnd = handle.0;
    let state = app.state::<AppState>();
    if !lock(&state.settings).usage_widget_enabled {
        remember_position(&widget, hwnd, &state);
        set_visible(hwnd, false);
        return;
    }
    if DRAG_ORIGIN.get().is_some() {
        return;
    }

    remember_position(&widget, hwnd, &state);
    let placement = lock(&state.settings)
        .usage_widget_position
        .clone()
        .unwrap_or_default();
    if let Some(monitor) = preferred_monitor(&widget, &placement) {
        let compact = placement.dock.is_some();
        let area = monitor.work_area();
        let scale = monitor.scale_factor();
        let size = widget_size(area, scale, compact);
        let position = restore_position(area, size, &placement, pixels(DOCK_INSET, scale));
        COMPACT.set(compact);
        if window_bounds(hwnd) != Some((position, size)) {
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    position.x,
                    position.y,
                    size.width as i32,
                    size.height as i32,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }
        set_visible(hwnd, apply_region(hwnd) && !foreground_is_fullscreen());
    }
}

fn remember_position(widget: &WebviewWindow, hwnd: HWND, state: &AppState) {
    let Some(origin) = DROP_ORIGIN.take() else {
        return;
    };
    let (Ok(Some(monitor)), Some((position, size))) =
        (widget.current_monitor(), window_bounds(hwnd))
    else {
        return;
    };
    if position == origin {
        return;
    }
    let mut placement =
        placement_from_drop(monitor.work_area(), monitor.scale_factor(), position, size);
    placement.monitor = monitor.name().cloned();
    state.update_settings(|settings| settings.usage_widget_position = Some(placement));
}

fn placement_from_drop(
    area: &WorkArea,
    scale: f64,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> Placement {
    let start_x = position.x - area.position.x;
    let start_y = position.y - area.position.y;
    let dock = [
        (DockEdge::Left, start_x),
        (
            DockEdge::Right,
            area.size.width as i32 - start_x - size.width as i32,
        ),
        (DockEdge::Top, start_y),
        (
            DockEdge::Bottom,
            area.size.height as i32 - start_y - size.height as i32,
        ),
    ]
    .into_iter()
    .min_by_key(|(_, distance)| (*distance).max(0))
    .filter(|(_, distance)| *distance <= pixels(DOCK_DISTANCE, scale))
    .map(|(edge, _)| edge);
    let target = widget_size(area, scale, dock.is_some());
    let with_center = |start: i32, actual: u32, target: u32, available: u32| {
        let offset = f64::from(start) + (f64::from(actual) - f64::from(target)) / 2.0;
        let travel = available - target;
        if travel == 0 {
            0.0
        } else {
            (offset / f64::from(travel)).clamp(0.0, 1.0)
        }
    };
    Placement {
        monitor: None,
        x: with_center(start_x, size.width, target.width, area.size.width),
        y: with_center(start_y, size.height, target.height, area.size.height),
        dock,
    }
}

fn preferred_monitor(widget: &WebviewWindow, placement: &Placement) -> Option<Monitor> {
    if let Some(name) = placement.monitor.as_ref() {
        if let Ok(Some(monitor)) = widget.current_monitor() {
            if monitor.name() == Some(name) {
                return Some(monitor);
            }
        }
        if let Ok(monitors) = widget.available_monitors() {
            if let Some(monitor) = monitors
                .into_iter()
                .find(|monitor| monitor.name() == Some(name))
            {
                return Some(monitor);
            }
        }
    }
    widget.primary_monitor().ok().flatten()
}

fn pixels(length: f64, scale: f64) -> i32 {
    (length * scale).round() as i32
}

fn widget_size(area: &WorkArea, scale: f64, compact: bool) -> PhysicalSize<u32> {
    if compact {
        let diameter = (pixels(BALL_DIAMETER, scale) as u32)
            .min(area.size.width)
            .min(area.size.height);
        PhysicalSize::new(diameter, diameter)
    } else {
        PhysicalSize::new(
            area.size.width.min(pixels(STRIP_WIDTH, scale) as u32),
            area.size.height.min(pixels(STRIP_HEIGHT, scale) as u32),
        )
    }
}

fn fitted_size(width: f64, height: f64, monitor: &Monitor) -> PhysicalSize<u32> {
    let PhysicalSize { width, height } =
        LogicalSize::new(width, height).to_physical(monitor.scale_factor());
    let available = monitor.work_area().size;
    PhysicalSize::new(available.width.min(width), available.height.min(height))
}

fn restore_position(
    area: &WorkArea,
    size: PhysicalSize<u32>,
    placement: &Placement,
    inset: i32,
) -> PhysicalPosition<i32> {
    let width = (area.size.width - size.width) as i32;
    let height = (area.size.height - size.height) as i32;
    let mut x = (f64::from(width) * placement.x.clamp(0.0, 1.0)).round() as i32;
    let mut y = (f64::from(height) * placement.y.clamp(0.0, 1.0)).round() as i32;
    if let Some(edge) = placement.dock {
        let inset_x = inset.min(width / 2);
        let inset_y = inset.min(height / 2);
        x = x.clamp(inset_x, width - inset_x);
        y = y.clamp(inset_y, height - inset_y);
        match edge {
            DockEdge::Left => x = inset_x,
            DockEdge::Right => x = width - inset_x,
            DockEdge::Top => y = inset_y,
            DockEdge::Bottom => y = height - inset_y,
        }
    }
    PhysicalPosition::new(area.position.x + x, area.position.y + y)
}

fn set_visible(hwnd: HWND, visible: bool) {
    if (unsafe { IsWindowVisible(hwnd) } != 0) != visible {
        // Restoring the widget must leave the foreground app focused.
        unsafe { ShowWindow(hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
    }
}

fn foreground_is_fullscreen() -> bool {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return false;
    }
    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(foreground, &mut process_id) };
    if process_id == std::process::id()
        || foreground == unsafe { GetDesktopWindow() }
        || foreground == unsafe { GetShellWindow() }
        || unsafe { IsIconic(foreground) } != 0
        || is_shell_surface(foreground)
    {
        return false;
    }

    // A maximized window that still has a title bar is ordinary desktop use,
    // even when the taskbar is configured to hide.
    let style = unsafe { GetWindowLongW(foreground, GWL_STYLE) } as u32;
    if unsafe { IsZoomed(foreground) } != 0 && style & WS_CAPTION == WS_CAPTION {
        return false;
    }

    let display = unsafe { MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if display.is_null() || unsafe { GetMonitorInfoW(display, &mut info) } == 0 {
        return false;
    }

    // DWM bounds omit resize borders and use physical screen coordinates.
    let mut bounds = RECT::default();
    let result = unsafe {
        DwmGetWindowAttribute(
            foreground,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut bounds as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    };
    if result < 0 && unsafe { GetWindowRect(foreground, &mut bounds) } == 0 {
        return false;
    }
    let monitor = info.rcMonitor;
    bounds.left <= monitor.left + 1
        && bounds.top <= monitor.top + 1
        && bounds.right >= monitor.right - 1
        && bounds.bottom >= monitor.bottom - 1
}

fn is_shell_surface(hwnd: HWND) -> bool {
    let mut name = [0; 64];
    let len = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    matches!(
        String::from_utf16_lossy(&name[..len as usize]).as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

#[tauri::command]
pub(crate) fn show_widget_details(window: WebviewWindow) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("the usage widget must open its own quick panel".into());
    }
    let quick = window
        .get_webview_window("quick")
        .ok_or("the quick panel is unavailable")?;

    let show = || {
        if let Some(monitor) = window.current_monitor()? {
            let anchor = PhysicalRect {
                position: window.outer_position()?,
                size: window.outer_size()?,
            };
            position_quick_panel(&quick, &monitor, anchor, false)?;
        }
        focus_quick_panel(&quick)
    };
    show().map_err(|error| error.to_string())
}

pub(crate) fn show_tray_details(
    quick: &WebviewWindow,
    click: PhysicalPosition<f64>,
    rect: &tauri::Rect,
) -> tauri::Result<()> {
    if let Some(monitor) = quick.monitor_from_point(click.x, click.y)? {
        let scale = monitor.scale_factor();
        let anchor = PhysicalRect {
            position: rect.position.to_physical(scale),
            size: rect.size.to_physical(scale),
        };
        // The quick panel may last have been shown on a monitor with a different DPI.
        position_quick_panel(quick, &monitor, anchor, true)?;
    }
    focus_quick_panel(quick)
}

fn position_quick_panel(
    quick: &WebviewWindow,
    monitor: &Monitor,
    anchor: WorkArea,
    from_tray: bool,
) -> tauri::Result<()> {
    let size = fitted_size(QUICK_WIDTH, QUICK_HEIGHT, monitor);
    let position = quick_panel_position(
        monitor.work_area(),
        size,
        anchor,
        pixels(8.0, monitor.scale_factor()),
        from_tray,
    );
    quick.set_position(position)?;
    if quick.inner_size().ok() != Some(size) {
        quick.set_size(size)?;
    }
    Ok(())
}

fn quick_panel_position(
    area: &WorkArea,
    size: PhysicalSize<u32>,
    anchor: WorkArea,
    gap: i32,
    from_tray: bool,
) -> PhysicalPosition<i32> {
    let width = size.width as i32;
    let height = size.height as i32;
    let right = area.position.x + area.size.width as i32;
    let bottom = area.position.y + area.size.height as i32;
    let below = anchor.position.y + anchor.size.height as i32 + gap;
    let above = anchor.position.y - height - gap;
    let (preferred_y, alternate_y) = if from_tray {
        (above, below)
    } else {
        (below, above)
    };
    let y = if (area.position.y..=bottom - height).contains(&preferred_y) {
        preferred_y
    } else {
        alternate_y.clamp(area.position.y, bottom - height)
    };
    let x = if from_tray {
        anchor.position.x + (anchor.size.width as i32 - width) / 2
    } else {
        anchor.position.x + anchor.size.width as i32 - width
    };
    PhysicalPosition::new(x.clamp(area.position.x, right - width), y)
}

fn focus_quick_panel(quick: &WebviewWindow) -> tauri::Result<()> {
    quick.show()?;
    quick.set_focus()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    const AREA: WorkArea = WorkArea {
        position: PhysicalPosition::new(-1800, 240),
        size: PhysicalSize::new(1800, 1000),
    };

    #[test]
    fn docks_at_the_nearest_edge_and_preserves_tangential_center() {
        let scale = 1.5;
        let size = widget_size(&AREA, scale, false);
        assert_eq!(size, PhysicalSize::new(456, 72));
        let release = PhysicalPosition::new(-504, 380);
        let placement = placement_from_drop(&AREA, scale, release, size);
        assert_eq!(placement.dock, Some(DockEdge::Right));
        let ball = widget_size(&AREA, scale, true);
        assert_eq!(ball, PhysicalSize::new(108, 108));
        assert_eq!(
            restore_position(&AREA, ball, &placement, 9),
            PhysicalPosition::new(-117, 362),
        );

        let top = placement_from_drop(&AREA, scale, PhysicalPosition::new(-1000, 252), size);
        assert_eq!(top.dock, Some(DockEdge::Top));
        assert_eq!(
            restore_position(&AREA, ball, &top, 9),
            PhysicalPosition::new(-826, 249)
        );
    }

    #[test]
    fn undocks_away_from_edges_and_expands_around_the_release_center() {
        let ball = widget_size(&AREA, 1.0, true);
        let release = PhysicalPosition::new(-700, 500);
        let placement = placement_from_drop(&AREA, 1.0, release, ball);
        assert!(placement.dock.is_none());
        let strip = widget_size(&AREA, 1.0, false);
        assert_eq!(
            restore_position(&AREA, strip, &placement, 6),
            PhysicalPosition::new(-816, 512),
        );
    }

    #[test]
    fn default_is_a_compact_right_dock_with_space_above_it() {
        let placement = Placement::default();
        let size = widget_size(&AREA, 1.5, true);
        assert_eq!(placement.dock, Some(DockEdge::Right));
        assert_eq!(
            restore_position(&AREA, size, &placement, 9),
            PhysicalPosition::new(-117, 347)
        );
    }

    #[test]
    fn shared_quick_panel_uses_the_destination_display_geometry() {
        let size = PhysicalSize::new(540, 690);
        let tray = WorkArea {
            position: PhysicalPosition::new(-60, 1248),
            size: PhysicalSize::new(24, 24),
        };
        let widget = WorkArea {
            position: PhysicalPosition::new(-117, 347),
            size: PhysicalSize::new(108, 108),
        };
        assert_eq!(
            quick_panel_position(&AREA, size, tray, 12, true),
            PhysicalPosition::new(-540, 546)
        );
        assert_eq!(
            quick_panel_position(&AREA, size, widget, 12, false),
            PhysicalPosition::new(-549, 467)
        );
    }

    #[test]
    fn new_widget_settings_are_optional_and_round_trip() {
        let mut settings: Settings = serde_json::from_str(r#"{"language":"zh"}"#).unwrap();
        assert!(!settings.usage_widget_enabled);
        assert!(settings.usage_widget_position.is_none());
        settings.usage_widget_enabled = true;
        settings.usage_widget_position = Some(Placement {
            monitor: Some(r"\\.\DISPLAY2".into()),
            x: 1.0,
            y: 0.5,
            dock: Some(DockEdge::Right),
        });
        let value = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&value).unwrap();
        let placement = restored.usage_widget_position.unwrap();
        assert!(restored.usage_widget_enabled);
        assert_eq!(placement.monitor.as_deref(), Some(r"\\.\DISPLAY2"));
        assert_eq!((placement.x, placement.y), (1.0, 0.5));
        assert_eq!(placement.dock, Some(DockEdge::Right));

        let legacy: Placement =
            serde_json::from_str(r#"{"monitor":null,"x":0.6,"y":0.4}"#).unwrap();
        assert_eq!((legacy.x, legacy.y), (0.6, 0.4));
        assert!(legacy.dock.is_none());
    }
}
