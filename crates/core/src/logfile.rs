//! Copy of the log shown in the window, in a file next to config.json.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use kvmate_protocol as proto;
use serde::Serialize;

static FILE: OnceLock<Mutex<File>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    pub fn from_board(level: u8) -> Level {
        match level {
            proto::LOG_ERROR => return Level::Error,
            proto::LOG_WARN => return Level::Warn,
            proto::LOG_DEBUG => return Level::Debug,
            _ => return Level::Info,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Level::Error => return "error",
            Level::Warn => return "warn",
            Level::Info => return "info",
            Level::Debug => return "debug",
            Level::Trace => return "trace",
        }
    }
}

pub fn init(dir: &Path) {
    if FILE.get().is_some() {
        return;
    }
    let path = dir.join("kvmate.log");
    let _ = fs::create_dir_all(dir);
    let _ = fs::rename(&path, dir.join("kvmate.old.log"));
    if let Ok(file) = OpenOptions::new().create(true).write(true).truncate(true).open(&path) {
        let _ = FILE.set(Mutex::new(file));
        write(Level::Info, &format!("kvmate {} — start", env!("CARGO_PKG_VERSION")));
    }
}

pub fn write(level: Level, text: &str) {
    if let Some(file) = FILE.get() {
        let mut file = crate::lock(file);
        let _ = writeln!(file, "{} {:5} {text}", utc_time(), level.name());
    }
}

fn utc_time() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() % 86_400;
    return format!("{:02}:{:02}:{:02}.{:03}Z", secs / 3600, secs / 60 % 60, secs % 60, now.subsec_millis());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_names_as_the_window_reads_them() {
        assert_eq!(serde_json::to_string(&Level::Warn).unwrap(), r#""warn""#);
        assert_eq!(Level::from_board(proto::LOG_ERROR), Level::Error);
        assert_eq!(Level::from_board(0), Level::Info);
    }
}
