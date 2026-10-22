//! Serial ports: one shared port, and never two requests in the driver at once. A blocking read
//! in one thread made every write from the other wait about 16 ms, and real parallel reads and
//! writes crashed the CH343 driver (blue screen). So reads only take bytes already waiting.

use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::board::transport::{READ_TIMEOUT, Reader, Writer};
use crate::lock;

type SharedPort = Arc<Mutex<Box<dyn serialport::SerialPort>>>;

const POLL_BUSY: Duration = Duration::from_millis(1);
const POLL_IDLE: Duration = Duration::from_millis(5);
const BUSY_FOR: Duration = Duration::from_millis(100);

pub fn split(port: Box<dyn serialport::SerialPort>) -> io::Result<(Reader, Writer)> {
    let port = Arc::new(Mutex::new(port));
    return Ok((
        Box::new(PolledReader { port: port.clone(), last_data: Instant::now() }),
        Box::new(LockedWriter { port }),
    ));
}

struct PolledReader {
    port: SharedPort,
    last_data: Instant,
}

impl Read for PolledReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let deadline = Instant::now() + READ_TIMEOUT;
        loop {
            {
                let mut port = lock(&self.port);
                let waiting = port.bytes_to_read().map_err(io::Error::from)? as usize;
                if waiting > 0 {
                    let take = waiting.min(buf.len());
                    let n = port.read(&mut buf[..take])?;
                    self.last_data = Instant::now();
                    return Ok(n);
                }
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(if self.last_data.elapsed() < BUSY_FOR { POLL_BUSY } else { POLL_IDLE });
        }
    }
}

struct LockedWriter {
    port: SharedPort,
}

impl Write for LockedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        return lock(&self.port).write(buf);
    }

    fn flush(&mut self) -> io::Result<()> {
        return Ok(());
    }
}
