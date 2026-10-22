//! The main window's icons come from the .ico in the executable, in the sizes Windows asks for at the
//! window's DPI. Tauri gives the window only its 32 px image, which the taskbar has to enlarge, and
//! so blur, from 150% scaling up.

use std::ptr::null;
use std::sync::Mutex;

use tauri::Window;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SendMessageW,
    WM_SETICON,
};

const APP_ICON: usize = 32512;

static LOADED: Mutex<[usize; 2]> = Mutex::new([0; 2]);

pub(crate) fn set_window_icons(window: &Window) {
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(hwnd) = target.hwnd() else { return };
        let mut loaded = LOADED.lock().unwrap();
        // SAFETY: on the main thread with the main window's handle; an icon is destroyed only after
        // the window has been given its replacement.
        unsafe {
            let module = GetModuleHandleW(null());
            let dpi = GetDpiForWindow(hwnd.0);
            for (slot, (kind, metric)) in loaded.iter_mut().zip([(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)]) {
                let size = GetSystemMetricsForDpi(metric, dpi);
                let icon = LoadImageW(module, APP_ICON as *const u16, IMAGE_ICON, size, size, LR_DEFAULTCOLOR);
                if icon.is_null() {
                    continue;
                }
                SendMessageW(hwnd.0, WM_SETICON, kind as usize, icon as isize);
                if *slot != 0 {
                    DestroyIcon(*slot as _);
                }
                *slot = icon as usize;
            }
        }
    });
}

pub(crate) fn release_window_icons() {
    for slot in LOADED.lock().unwrap().iter_mut() {
        if *slot != 0 {
            // SAFETY: the window that showed the icon is gone.
            unsafe { DestroyIcon(*slot as _) };
            *slot = 0;
        }
    }
}
