use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::board::transport::{READ_TIMEOUT, Reader, Transport, Writer};

pub struct MemoryTransport {
    name: String,
    tx: Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
    read_timeout: Duration,
}

impl MemoryTransport {
    pub fn pair(name: &str) -> (Self, Self) {
        let (a_tx, b_rx) = mpsc::channel();
        let (b_tx, a_rx) = mpsc::channel();
        return (
            Self { name: name.to_owned(), tx: a_tx, rx: a_rx, read_timeout: READ_TIMEOUT },
            Self { name: format!("{name} (board side)"), tx: b_tx, rx: b_rx, read_timeout: READ_TIMEOUT },
        );
    }

    pub fn with_read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        return self;
    }
}

impl Transport for MemoryTransport {
    fn name(&self) -> String {
        return self.name.clone();
    }

    fn split(self: Box<Self>) -> io::Result<(Reader, Writer)> {
        return Ok((
            Box::new(MemoryReader { rx: self.rx, pending: Vec::new(), pos: 0, timeout: self.read_timeout }),
            Box::new(MemoryWriter { tx: self.tx }),
        ));
    }
}

struct MemoryReader {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
    pos: usize,
    timeout: Duration,
}

impl Read for MemoryReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.pending.len() {
            match self.rx.recv_timeout(self.timeout) {
                Ok(chunk) => {
                    self.pending = chunk;
                    self.pos = 0;
                }
                Err(RecvTimeoutError::Timeout) => return Err(io::ErrorKind::TimedOut.into()),
                Err(RecvTimeoutError::Disconnected) => return Err(io::ErrorKind::BrokenPipe.into()),
            }
        }
        let n = buf.len().min(self.pending.len() - self.pos);
        buf[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
        self.pos += n;
        return Ok(n);
    }
}

struct MemoryWriter {
    tx: Sender<Vec<u8>>,
}

impl Write for MemoryWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tx.send(buf.to_vec()).map_err(|_| return io::Error::from(io::ErrorKind::BrokenPipe))?;
        return Ok(buf.len());
    }

    fn flush(&mut self) -> io::Result<()> {
        return Ok(());
    }
}
