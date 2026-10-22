use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

pub(crate) fn enabled(app: &AppHandle) -> bool {
    return app.autolaunch().is_enabled().unwrap_or(false);
}

pub(crate) fn set(app: &AppHandle, on: bool) -> Result<(), String> {
    if on == enabled(app) {
        return Ok(());
    }
    let autolaunch = app.autolaunch();
    let result = if on { autolaunch.enable() } else { autolaunch.disable() };
    return result.map_err(|e| format!("autostart: {e}"));
}
