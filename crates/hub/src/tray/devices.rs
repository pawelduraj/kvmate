use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use kvmate_core::audio::{self, DeviceList};
use tauri::menu::{CheckMenuItem, Submenu};
use tauri::{AppHandle, Manager, Wry};

use super::{TrayMenu, check};
use crate::state::AppState;

const DEVICE_REFRESH: Duration = Duration::from_secs(4);

pub(super) struct DeviceItems {
    submenu: Submenu<Wry>,
    first: usize,
    names: Vec<Option<String>>,
    items: Vec<CheckMenuItem<Wry>>,
    generation: u32,
}

impl DeviceItems {
    pub(super) fn new(submenu: Submenu<Wry>, first: usize) -> Mutex<DeviceItems> {
        return Mutex::new(DeviceItems { submenu, first, names: vec![], items: vec![], generation: 0 });
    }
}

pub(super) fn watch(app: &AppHandle) -> tauri::Result<()> {
    let app = app.clone();
    thread::Builder::new().name("tray-devices".into()).spawn(move || {
        let mut last: Option<DeviceList> = None;
        loop {
            let list = audio::list_devices();
            if last.as_ref() != Some(&list) {
                last = Some(list.clone());
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || refresh_devices(&handle, &list));
            }
            thread::sleep(DEVICE_REFRESH);
        }
    })?;
    return Ok(());
}

fn refresh_devices(app: &AppHandle, list: &DeviceList) {
    let Some(menu) = app.try_state::<TrayMenu>() else { return };
    let config = app.state::<Arc<AppState>>().config.lock().unwrap().clone();
    let build = |slot: &Mutex<DeviceItems>,
                 prefix: &str,
                 names: &[String],
                 default: Option<&String>,
                 chosen: Option<&String>| {
        let mut guard = slot.lock().unwrap();
        let d = &mut *guard;
        for old in d.items.drain(..) {
            let _ = d.submenu.remove(&old);
        }
        d.generation += 1;
        let mut all: Vec<Option<String>> = vec![None];
        all.extend(names.iter().cloned().map(Some));
        if let Some(c) = chosen.filter(|c| return !names.contains(c)) {
            all.push(Some(c.clone()));
        }
        for (i, name) in all.iter().enumerate() {
            let text = match name {
                None => default.map_or("System default".to_string(), |d| format!("System default ({d})")),
                Some(n) if !names.contains(n) => format!("{n} (not found)"),
                Some(n) => n.clone(),
            };
            let Ok(entry) = check(app, &format!("{prefix}:{}:{i}", d.generation), &text) else { continue };
            let _ = entry.set_checked(name.as_ref() == chosen);
            let _ = d.submenu.insert(&entry, d.first + i);
            d.items.push(entry);
        }
        d.names = all;
    };
    build(&menu.outputs, "out_dev", &list.outputs, list.default_output.as_ref(), config.audio.output_device.as_ref());
    build(&menu.inputs, "in_dev", &list.inputs, list.default_input.as_ref(), config.audio.input_device.as_ref());
}

pub(super) fn device_of(slot: &Mutex<DeviceItems>, id: &str) -> Option<Option<String>> {
    let mut parts = id.split(':').skip(1);
    let (generation, index) = (parts.next()?.parse::<u32>().ok()?, parts.next()?.parse::<usize>().ok()?);
    let d = slot.lock().unwrap();
    return (d.generation == generation).then(|| return d.names.get(index).cloned()).flatten();
}

pub(super) fn sync(slot: &Mutex<DeviceItems>, chosen: &Option<String>) {
    let d = slot.lock().unwrap();
    for (name, entry) in d.names.iter().zip(&d.items) {
        let _ = entry.set_checked(name == chosen);
    }
}
