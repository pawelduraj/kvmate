//! Byte pipes between the hub and a board: the cable, Wi-Fi, and memory for tests. The link layer
//! only needs independent read and write halves whose reads time out.

use std::io::{self, Read, Write};
use std::time::Duration;

pub type Reader = Box<dyn Read + Send>;
pub type Writer = Box<dyn Write + Send>;

pub trait Transport: Send {
    fn name(&self) -> String;
    fn split(self: Box<Self>) -> io::Result<(Reader, Writer)>;
}

pub(crate) const READ_TIMEOUT: Duration = Duration::from_millis(50);

pub(crate) fn is_timeout(e: &io::Error) -> bool {
    return matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted);
}
