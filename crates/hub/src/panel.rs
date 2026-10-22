//! The tray panel. Linux has none: there the tray icon only has its menu, because AppIndicator
//! doesn't report clicks.
//!
//! After use the panel is only hidden, so it opens instantly next time. After [`KEEP_HIDDEN`]
//! unused it is destroyed, so no WebView stays around in the background.

use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

const PANEL: &str = "panel";
const WIDTH: f64 = 360.0;
const REOPEN_GUARD: Duration = Duration::from_millis(300);
const KEEP_HIDDEN: Duration = Duration::from_secs(120);

#[derive(Default)]
pub(crate) struct PanelState(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    anchor: Option<(f64, f64, f64, f64)>,
    hidden_at: Option<Instant>,
    generation: u64,
    show_when_fitted: bool,
}

fn lock(app: &AppHandle) -> MutexGuard<'_, Inner> {
    return app.state::<PanelState>().inner().0.lock().unwrap();
}

pub(crate) fn toggle(app: &AppHandle, rect: Rect) {
    let position = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    lock(app).anchor = Some((position.x, position.y, size.width, size.height));

    if let Some(window) = app.get_webview_window(PANEL) {
        if window.is_visible().unwrap_or(false) {
            hide(app);
        } else if lock(app).hidden_at.is_none_or(|t| return t.elapsed() >= REOPEN_GUARD) {
            let height = window.inner_size().map(|s| return s.height as f64 / window.scale_factor().unwrap_or(1.0));
            place(&window, height.unwrap_or(420.0));
            show(app, &window);
        }
        return;
    }

    let built = WebviewWindowBuilder::new(app, PANEL, WebviewUrl::App("panel.html".into()))
        .title("kvmate")
        .inner_size(WIDTH, 420.0)
        .resizable(false)
        .decorations(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(true)
        .visible(false)
        .build();
    let Ok(window) = built else { return };
    lock(app).show_when_fitted = true;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            hide(&handle);
        }
    });
}

#[tauri::command]
pub(crate) fn panel_fit(height: f64, app: AppHandle) {
    let Some(window) = app.get_webview_window(PANEL) else { return };
    let height = height.clamp(160.0, 900.0);
    let _ = window.set_size(LogicalSize::new(WIDTH, height));
    place(&window, height);
    let first = std::mem::take(&mut lock(&app).show_when_fitted);
    if first {
        show(&app, &window);
    }
}

#[tauri::command]
pub(crate) fn panel_hide(app: AppHandle) {
    hide(&app);
}

fn show(app: &AppHandle, window: &WebviewWindow) {
    {
        let mut state = lock(app);
        state.generation += 1;
        state.hidden_at = None;
    }
    let _ = window.show();
    let _ = window.set_focus();
}

pub(crate) fn hide(app: &AppHandle) {
    let Some(window) = app.get_webview_window(PANEL) else { return };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let _ = window.hide();
    let generation = {
        let mut state = lock(app);
        state.hidden_at = Some(Instant::now());
        state.generation += 1;
        state.generation
    };
    let handle = app.clone();
    thread::spawn(move || {
        thread::sleep(KEEP_HIDDEN);
        if lock(&handle).generation == generation {
            if let Some(window) = handle.get_webview_window(PANEL) {
                let _ = window.destroy();
            }
        }
    });
}

fn place(window: &WebviewWindow, height: f64) {
    let Some((ax, ay, aw, ah)) = lock(window.app_handle()).anchor else { return };
    let monitor = window
        .monitor_from_point(ax + aw / 2.0, ay + ah / 2.0)
        .ok()
        .flatten()
        .or_else(|| return window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let scale = monitor.scale_factor();
    let (w, h) = (WIDTH * scale, height * scale);
    let work = monitor.work_area();
    let (wx, wy) = (work.position.x as f64, work.position.y as f64);
    let (ww, wh) = (work.size.width as f64, work.size.height as f64);
    let margin = 12.0 * scale;
    let x = (ax + aw / 2.0 - w / 2.0).clamp(wx + margin, (wx + ww - w - margin).max(wx + margin));
    let y = if ay > wy + wh / 2.0 { wy + wh - h - margin } else { wy + margin };
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}
