import { $, applyTheme, el } from "../shared/dom.js";
import { LED_LEVELS, ledStep, ledSwitch } from "../shared/status.js";
import { invoke } from "../shared/tauri.js";
import { updateMap } from "./map.js";
import { renderSections } from "./sections.js";
import { S } from "./state.js";

function fillDeviceSelect(select) {
  const key = select.dataset.audioDevice;
  const list = S.devices ? (key === "output_device" ? S.devices.outputs : S.devices.inputs) : [];
  const def = S.devices ? (key === "output_device" ? S.devices.default_output : S.devices.default_input) : null;
  const chosen = S.audio?.[key] ?? null;
  const options = [new Option(def ? `System default (${def})` : "System default", "")];
  for (const name of list) options.push(new Option(name, name));
  if (chosen && !list.includes(chosen)) options.push(new Option(`${chosen} (not found)`, chosen));
  select.replaceChildren(...options);
  select.value = chosen ?? "";
}

export function syncControls() {
  const settings = S.settings;
  for (const e of document.querySelectorAll("[data-setting]")) {
    const key = e.dataset.setting;
    if (key === "hotkey" && settings && e.options.length === 0) {
      e.replaceChildren(...settings.hotkeys.map((h) => new Option(h.label, h.id)));
    }
    const value = settings?.[key];
    if (value === undefined) continue;
    if (e.type === "checkbox") e.checked = Boolean(value);
    else e.value = String(value);
  }
  for (const group of document.querySelectorAll("[data-audio-mode]")) {
    for (const b of group.querySelectorAll("button[data-mode]")) {
      const on = S.audio?.[group.dataset.audioMode] === b.dataset.mode;
      b.classList.toggle("on", on);
      b.setAttribute("aria-pressed", String(on));
    }
  }
  for (const select of document.querySelectorAll("select[data-audio-device]")) fillDeviceSelect(select);
  renderLed();
}

function renderLed() {
  const st = S.settings;
  if (!st) return;
  const box = $("led-levels");
  if (box.children.length === 0) {
    box.append(
      ...LED_LEVELS.map((_, i) => {
        const bar = el("i");
        bar.style.height = `${8 + (i + 1) * 2}px`;
        return el("button", { role: "radio", "data-level": i + 1, title: `${i + 1} of 10`, "aria-label": `${i + 1} of 10` }, bar);
      }),
    );
  }
  const on = st.led_on && st.led_brightness > 0;
  $("led-on").checked = on;
  $("led-row").hidden = !on;
  const level = ledStep(st.led_brightness);
  for (const b of box.children) {
    const n = Number(b.dataset.level);
    b.classList.toggle("on", n <= level);
    b.setAttribute("aria-checked", String(n === level));
  }
  $("led-text").textContent = `${level} of 10`;
}

$("led-on").addEventListener("change", () => saveSettings(ledSwitch($("led-on").checked, S.settings)));

$("led-levels").addEventListener("click", (event) => {
  const b = event.target.closest("button[data-level]");
  if (b) saveSettings({ led_brightness: LED_LEVELS[Number(b.dataset.level) - 1] });
});

async function saveSettings(change) {
  if (!S.settings) return;
  S.settings = { ...S.settings, ...change };
  if (change.theme) applyTheme(change.theme);
  syncControls();
  updateMap();
  const error = $("settings-error");
  try {
    await invoke("set_settings", { settings: change });
    error.hidden = true;
  } catch (e) {
    error.hidden = false;
    error.textContent = `Settings not saved: ${e}`;
    loadConfig();
  }
}

async function saveAudio(change) {
  S.audio = { ...S.audio, ...change };
  syncControls();
  const error = $("settings-error");
  try {
    await invoke("set_audio_config", { audio: S.audio });
    error.hidden = true;
  } catch (e) {
    error.hidden = false;
    error.textContent = `Sound settings not saved: ${e}`;
    loadConfig();
  }
}

document.addEventListener("change", (event) => {
  const t = event.target;
  if (t.dataset.setting) {
    saveSettings({ [t.dataset.setting]: t.type === "checkbox" ? t.checked : t.value });
  } else if (t.dataset.audioDevice) {
    saveAudio({ [t.dataset.audioDevice]: t.value || null });
  }
});

document.addEventListener("click", (event) => {
  const button = event.target.closest("[data-audio-mode] button[data-mode]");
  if (button) saveAudio({ [button.parentElement.dataset.audioMode]: button.dataset.mode });
});

export async function loadConfig() {
  const [settings, audio] = await Promise.all([invoke("get_settings"), invoke("get_audio_config")]);
  S.settings = settings;
  S.audio = audio;
  applyTheme(settings.theme);
  syncControls();
  updateMap();
  if (S.status) renderSections();
}

export async function loadDevices() {
  S.devices = await invoke("audio_devices");
  syncControls();
}
