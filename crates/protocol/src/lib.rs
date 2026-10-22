//! Wire protocol between the kvmate hub and the board's firmware. The firmware's C header is
//! generated from this crate: `cargo xtask gen-header`.
#![no_std]

#[cfg(target_endian = "big")]
compile_error!("payload structs are read and written as little-endian bytes");

mod msg;
pub mod secure;
mod wire;

pub use msg::*;
pub use wire::*;
