use std::sync::Arc;

use kvmate_core::config::{CloseAction, Theme};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

use crate::state::AppState;
use crate::{panel, platform};

const MAIN_WINDOW: &str = "main";

pub(crate) fn open(app: &AppHandle, element: Option<&str>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(element) = element {
            let _ = window.emit("select", element);
        }
        return;
    }
    let theme = app.state::<Arc<AppState>>().config.lock().unwrap().theme;
    let page = match element {
        Some(element) => format!("index.html#{element}"),
        None => "index.html".into(),
    };
    let built = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App(page.into()))
        .title("kvmate")
        .inner_size(960.0, 820.0)
        .min_inner_size(780.0, 560.0)
        .theme(native_theme(theme))
        .build();
    if let Ok(window) = built {
        platform::set_window_icons(&window.as_ref().window());
    }
}

#[tauri::command]
pub(crate) async fn open_main(element: Option<String>, app: AppHandle) {
    panel::hide(&app);
    open(&app, element.as_deref());
}

pub(crate) fn set_theme(app: &AppHandle, theme: Theme) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.set_theme(native_theme(theme));
    }
}

fn native_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::Auto => return None,
        Theme::Light => return Some(tauri::Theme::Light),
        Theme::Dark => return Some(tauri::Theme::Dark),
    }
}

pub(crate) fn on_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    match event {
        WindowEvent::CloseRequested { .. } => {
            let app = window.app_handle();
            if app.state::<Arc<AppState>>().config.lock().unwrap().on_close == CloseAction::Quit {
                app.exit(0);
            }
        }
        WindowEvent::ScaleFactorChanged { .. } => platform::set_window_icons(window),
        WindowEvent::Destroyed => platform::release_window_icons(),
        _ => {}
    }
}
