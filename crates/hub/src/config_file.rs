//! A missing config.json is created with the defaults, so it can be edited. If the file can't be
//! read or parsed, the defaults are used. A broken file is copied first, because the next settings
//! change saves over it.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use kvmate_core::config::Config;
use tauri::{AppHandle, Manager};

const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";

pub(crate) fn config_path(app: &AppHandle) -> Option<PathBuf> {
    return app.path().app_config_dir().ok().map(|dir| return dir.join("config.json"));
}

#[tauri::command]
pub(crate) fn config_location(app: AppHandle) -> Option<String> {
    return config_path(&app).map(|p| return p.display().to_string());
}

pub(crate) fn save(app: &AppHandle, config: &Config) -> Result<(), String> {
    return write(&config_path(app).ok_or("no configuration directory")?, config);
}

pub(crate) fn load(app: &AppHandle) -> (Config, Option<String>) {
    match config_path(app) {
        Some(path) => return load_from(&path),
        None => return (Config::default(), Some("no configuration directory, using the default settings".into())),
    }
}

fn load_from(path: &Path) -> (Config, Option<String>) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let config = Config::default();
            let note = match write(path, &config) {
                Ok(()) => format!("created {}", path.display()),
                Err(e) => e,
            };
            return (config, Some(note));
        }
        Err(e) => {
            return (
                Config::default(),
                Some(format!("can't read {}: {e} — using the default settings", path.display())),
            );
        }
    };
    let json = bytes.strip_prefix(BYTE_ORDER_MARK).unwrap_or(&bytes);
    match serde_json::from_slice::<Config>(json) {
        Ok(config) => return (config, None),
        Err(e) => {
            let copy = path.with_extension("broken.json");
            let kept = match fs::copy(path, &copy) {
                Ok(_) => format!(" (the file is kept as {})", copy.display()),
                Err(_) => String::new(),
            };
            let note = format!("error in {}: {e} — using the default settings{kept}", path.display());
            return (Config::default(), Some(note));
        }
    }
}

fn write(path: &Path, config: &Config) -> Result<(), String> {
    let text = serde_json::to_string_pretty(config).map_err(|e| return e.to_string())?;
    let temp = path.with_extension("json.tmp");
    let replace = || -> std::io::Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        return fs::rename(&temp, path);
    };
    return replace().map_err(|e| return format!("can't write {}: {e}", path.display()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_in_new_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kvmate-config-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        return dir.join("config.json");
    }

    #[test]
    fn a_missing_file_is_created() {
        let path = path_in_new_dir("missing");
        let (config, note) = load_from(&path);
        assert!(note.unwrap().starts_with("created"));
        assert_eq!(load_from(&path), (config, None));
    }

    #[test]
    fn a_broken_file_is_copied_before_anything_saves_over_it() {
        let path = path_in_new_dir("broken");
        fs::write(&path, "{ \"jiggle\": tru").unwrap();
        let (config, note) = load_from(&path);
        assert_eq!(config, Config::default());
        assert!(note.unwrap().contains("config.broken.json"), "the note says where the copy is");
        assert_eq!(fs::read_to_string(path.with_extension("broken.json")).unwrap(), "{ \"jiggle\": tru");
    }

    #[test]
    fn a_file_that_cant_be_read_is_not_replaced() {
        let path = path_in_new_dir("unreadable");
        fs::create_dir(&path).unwrap();
        let (_, note) = load_from(&path);
        assert!(note.unwrap().starts_with("can't read"));
        assert!(path.is_dir());
    }

    #[test]
    fn a_byte_order_mark_is_skipped() {
        let path = path_in_new_dir("bom");
        let config = Config { jiggle: !Config::default().jiggle, ..Config::default() };
        write(&path, &config).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        let text = fs::read(&path).unwrap();
        fs::write(&path, [BYTE_ORDER_MARK, &text].concat()).unwrap();
        assert_eq!(load_from(&path), (config, None));
    }
}
