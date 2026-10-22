// window.__TAURI__ needs app.withGlobalTauri in tauri.conf.json.
export const { invoke } = window.__TAURI__.core;
export const { listen } = window.__TAURI__.event;
