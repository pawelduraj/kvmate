use std::sync::Arc;

use kvmate_core::firmware::FirmwareImage;
use kvmate_core::supervisor::Command;

use crate::state::AppState;

static FIRMWARE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/kvmate-firmware.bin"));

pub(crate) fn bundled_firmware() -> (Option<FirmwareImage>, Option<String>) {
    match FirmwareImage::new(FIRMWARE) {
        None => {
            return (None, Some("this build of the app has no firmware (build it: idf.py build in firmware/)".into()));
        }
        Some(Err(e)) => return (None, Some(format!("the bundled firmware is invalid: {e}"))),
        Some(Ok(f)) => {
            let app = env!("CARGO_PKG_VERSION");
            let note = (f.info.version != app)
                .then(|| format!("the bundled firmware is version {}, the app {app}", f.info.version));
            return (Some(f), note);
        }
    }
}

#[tauri::command]
pub(crate) fn install_firmware(port: Option<String>, state: tauri::State<'_, Arc<AppState>>) {
    state.command(Command::InstallFirmware { port });
}

#[tauri::command]
pub(crate) fn erase_firmware(port: Option<String>, state: tauri::State<'_, Arc<AppState>>) {
    state.command(Command::EraseFirmware { port });
}
