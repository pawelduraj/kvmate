//! What differs between systems in the hub. Each system's module provides:
//! - `set_window_icons(window)`: the main window's title bar and taskbar icons for its DPI,
//! - `release_window_icons()`: frees them once the main window is gone.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use self::windows::*;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub(crate) use self::other::*;
