//! Core of the kvmate hub, independent of the GUI.

pub mod audio;
mod board;
pub mod config;
mod encoding;
pub mod firmware;
mod input;
pub mod logfile;
mod platform;
pub mod supervisor;
pub mod switcher;
pub mod wifi;

pub use kvmate_protocol as protocol;

use std::sync::{Mutex, MutexGuard, PoisonError};

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    return m.lock().unwrap_or_else(PoisonError::into_inner);
}

pub fn hub_version() -> [u8; 3] {
    return [
        env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0),
        env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0),
        env!("CARGO_PKG_VERSION_PATCH").parse().unwrap_or(0),
    ];
}
