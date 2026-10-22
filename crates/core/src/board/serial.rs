use std::io;

use crate::board::transport::{READ_TIMEOUT, Reader, Transport, Writer};
use crate::platform;

pub struct SerialTransport {
    name: String,
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    pub fn open(name: &str, baud: u32) -> io::Result<Self> {
        let mut port =
            serialport::new(name, baud).timeout(READ_TIMEOUT).dtr_on_open(false).open().map_err(io::Error::from)?;
        port.write_request_to_send(false).map_err(io::Error::from)?;
        port.write_data_terminal_ready(false).map_err(io::Error::from)?;
        return Ok(Self { name: name.to_owned(), port });
    }
}

impl Transport for SerialTransport {
    fn name(&self) -> String {
        return self.name.clone();
    }

    fn split(self: Box<Self>) -> io::Result<(Reader, Writer)> {
        return platform::serial::split(self.port);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kvmate_protocol::{self as proto, Heartbeat, Msg};
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn write_latency(reader: Reader, mut writer: Writer) -> Duration {
        let stop = Arc::new(AtomicBool::new(false));
        let drain = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 4096];
                while !stop.load(Ordering::Relaxed) {
                    let _ = reader.read(&mut buf);
                }
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        let hb = Heartbeat { flags: 0 };
        let mut wire = vec![0u8; proto::MAX_ENCODED_LEN];
        let n = Msg::Heartbeat(hb).encode(0, &mut wire).unwrap();
        const WRITES: u32 = 200;
        let start = Instant::now();
        for _ in 0..WRITES {
            writer.write_all(&wire[..n]).unwrap();
        }
        let per_write = start.elapsed() / WRITES;
        stop.store(true, Ordering::Relaxed);
        drain.join().unwrap();
        return per_write;
    }

    #[test]
    #[ignore]
    fn live_serial_write_cost() {
        let port = std::env::var("KVMATE_PORT").expect("set KVMATE_PORT");
        let t = Box::new(SerialTransport::open(&port, proto::DEFAULT_BAUD).unwrap());
        let (reader, writer) = t.split().unwrap();
        let per_write = write_latency(reader, writer);
        println!("{port}: one write while another thread reads: {per_write:?}");
        assert!(per_write < Duration::from_millis(2), "writes wait for reads: {per_write:?}");
    }
}
