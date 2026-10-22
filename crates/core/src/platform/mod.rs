//! Everything that depends on the operating system. Each system's module provides the same
//! modules, and the rest of the crate uses them only through here:
//!
//! - `input`: capturing the keyboard and mouse, and hiding them from this computer;
//! - `audio_activity`: whether other programs on this computer play or record;
//! - `wlan`: this computer's Wi-Fi card and the networks it knows;
//! - `secret`: encrypting secrets for the current user;
//! - `usb`: whether a USB device is plugged into this computer;
//! - `serial`: a serial port split into a reader and a writer.
//!
//! Only Windows is written. `other` covers the rest: serial ports and plain secrets work, the
//! other modules are stand-ins.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use self::windows::*;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub(crate) use self::other::*;
