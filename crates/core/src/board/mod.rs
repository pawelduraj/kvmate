//! The connection to a board: finding its port, the link over a transport (the cable, Wi-Fi, or
//! memory for tests and the demo), and a simulated board.

pub(crate) mod discovery;
pub(crate) mod link;
pub(crate) mod memory;
pub(crate) mod serial;
pub(crate) mod sim;
pub(crate) mod transport;
pub(crate) mod udp;
