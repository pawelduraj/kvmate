import { $, el, switchInput } from "../shared/dom.js";
import { AUDIO_MODES, CLOSE_ACTIONS, LED_LEVELS, THEMES, connected, ledStep, ledSwitch, onCable, usbText, wifiAddress } from "../shared/status.js";
import { invoke } from "../shared/tauri.js";
import { ROWS, S, boardLine, saveAudio, saveSettings, setWifi } from "./panel.js";

let pageKey = "";

function group(text) {
  return el("div", { class: "group", text });
}

function note(text, cls = "") {
  return el("p", { class: `note ${cls}`, text });
}

function radio(label, hint, on, onclick, disabled = false) {
  return el(
    "button",
    { class: `radio${on ? " on" : ""}`, "aria-pressed": String(on), disabled, onclick },
    el("i"),
    el("span", {}, el("span", { text: label }), hint ? el("small", { text: hint }) : null),
  );
}

function toggle(label, hint, on, onchange) {
  const input = switchInput();
  input.checked = Boolean(on);
  input.addEventListener("change", () => onchange(input.checked));
  return el("label", { class: "plain" }, el("span", {}, el("span", { text: label }), hint ? el("small", { text: hint }) : null), input);
}

function action(label, onclick, primary = false, disabled = false) {
  return el("button", { class: `small${primary ? " primary" : ""}`, text: label, onclick, disabled });
}

function link(label, element) {
  return el("button", { class: "link", text: label, onclick: () => invoke("open_main", { element }) });
}

function kmPage() {
  const st = S.settings;
  return [
    group("Switch shortcut"),
    ...(st?.hotkeys ?? []).map((h) => radio(h.label, null, st.hotkey === h.id, () => saveSettings({ hotkey: h.id }))),
    note("The BOOT button on the board switches too (hold: always back here)."),
    group("Other"),
    toggle("Ctrl+Alt+End works as Ctrl+Alt+Del", "on the other computer", st?.ctrl_alt_end_as_delete, (v) => saveSettings({ ctrl_alt_end_as_delete: v })),
  ];
}

const SOUND = {
  headphones: {
    key: "playback",
    title: "Other computer's sound",
    always: "you hear both computers at once",
    off: "the other computer plays through its own speakers",
    device: "Headphones",
    deviceKey: "output_device",
    list: "outputs",
    defaultKey: "default_output",
  },
  mic: {
    key: "capture",
    title: "Microphone for the other computer",
    always: "e.g. a call on the other computer while you work here",
    off: "the microphone is for this computer only",
    device: "Microphone",
    deviceKey: "input_device",
    list: "inputs",
    defaultKey: "default_input",
  },
};

function soundPage(kind) {
  const d = SOUND[kind];
  const mode = S.audio?.[d.key];
  const hints = { always: d.always, when_laptop: "together with the keyboard and mouse", off: d.off };
  const parts = [
    group(d.title),
    ...AUDIO_MODES.map(([value, label]) => radio(label, hints[value], mode === value, () => saveAudio({ [d.key]: value }))),
    group(d.device),
  ];
  if (!S.devices) {
    parts.push(note("looking for devices…"));
  } else {
    const chosen = S.audio?.[d.deviceKey] ?? null;
    const names = S.devices[d.list];
    const def = S.devices[d.defaultKey];
    const choices = [["", "System default", def ? `now: ${def}` : null], ...names.map((n) => [n, n, null])];
    if (chosen && !names.includes(chosen)) choices.push([chosen, chosen, "not found"]);
    for (const [value, label, hint] of choices) {
      const r = radio(label, hint, value === (chosen ?? ""), () => saveAudio({ [d.deviceKey]: value || null }));
      r.classList.add("device");
      parts.push(r);
    }
  }
  if (kind === "headphones") {
    const a = S.status.audio;
    const volume = !a.playback.active ? "— (the other computer isn't playing anything)" : a.host_volume_percent == null ? "muted" : `${a.host_volume_percent}%`;
    parts.push(note(`Volume on the other computer: ${volume}`, "line"));
  }
  return parts;
}

function boardPage() {
  const s = S.status;
  const st = S.settings;
  const wifi = S.wifi;
  const conn = connected(s);
  const cable = onCable(s);
  const mode = wifi?.mode ?? "off";
  const how = !conn ? boardLine() : s.transport === "wifi" ? `via Wi-Fi (${wifiAddress(s)})` : `“COM” cable (${s.port ?? "?"})`;
  const bundled = s.bundled_firmware;
  const silent = !conn && (s.silent_ports ?? []).length > 0;
  let fwText;
  let fwButton = null;
  if (s.flash) {
    fwText = `installing${s.flash.progress != null ? ` ${Math.round(s.flash.progress * 100)}%` : "…"}`;
  } else if (!bundled) {
    fwText = "this build of the app has no firmware";
  } else if (silent) {
    fwText = "the board has no kvmate firmware";
    fwButton = link("Install…", "board-fw");
  } else if (!conn) {
    fwText = "—";
  } else {
    fwText = `${s.firmware ?? "?"} · ${s.firmware_current ? "up to date" : "differs from the app's"}`;
    fwButton = cable ? action(s.firmware_current ? "Reinstall" : "Update", () => invoke("install_firmware", { port: null }), !s.firmware_current) : null;
    if (!cable) fwText += " · updates by cable only";
  }
  const ledOn = st?.led_on && st.led_brightness > 0;
  const step = ledStep(st?.led_brightness ?? 0);
  return [
    note(`Connected: ${how}`, "line strong"),
    group("Board Wi-Fi (wireless)"),
    radio("Off", "the board works over the “COM” cable only", mode === "off", () => setWifi("off"), !cable),
    radio("Board's own network", mode === "board_network" && wifi.network_ssid ? `“${wifi.network_ssid}” · the Wi-Fi adapter connects by itself` : "this computer's Wi-Fi adapter connects by itself", mode === "board_network", () => setWifi("board_network"), !cable),
    radio("Via router", mode === "router" && wifi.router_ssid ? `“${wifi.router_ssid}” · pick another network in the window` : "pick the network in the window", mode === "router", () => invoke("open_main", { element: "board-conn" })),
    cable ? null : note("Wi-Fi can be changed only with the board on the “COM” cable."),
    S.error ? note(S.error, "error") : null,
    group("Firmware and LED"),
    el("div", { class: "plain" }, el("span", {}, el("span", { text: "Firmware" }), el("small", { text: fwText })), fwButton),
    toggle("LED", "blue: this computer, green: the other one", ledOn, (v) => saveSettings(ledSwitch(v, S.settings))),
    ledOn
      ? el(
          "div",
          { class: "plain" },
          el("span", {}, el("span", { text: "Brightness" }), el("small", { text: `${step} of 10` })),
          el(
            "div",
            { class: "levels", role: "radiogroup", "aria-label": "LED brightness" },
            ...LED_LEVELS.map((level, i) => {
              const bar = el("i");
              bar.style.height = `${6 + (i + 1) * 2}px`;
              return el("button", { class: i < step ? "on" : "", role: "radio", "aria-checked": String(i + 1 === step), title: `${i + 1} of 10`, onclick: () => saveSettings({ led_on: true, led_brightness: level }) }, bar);
            }),
          ),
        )
      : null,
    link("Details and log in the window…", "board-fw"),
  ];
}

function otherPage() {
  return [
    note(`Board's “USB” port: ${usbText(S.status)}`, "line strong"),
    toggle("Keep it awake", "while you control this computer, nudges its mouse by 0 px every 30 s", S.settings?.jiggle, (v) => saveSettings({ jiggle: v })),
    group("Send to it"),
    el(
      "div",
      { class: "buttons" },
      action("Ctrl+Alt+Del", () => invoke("send_combo", { combo: "ctrl_alt_del" })),
      action("Lock it (Win+L)", () => invoke("send_combo", { combo: "lock" })),
    ),
  ];
}

function herePage() {
  const st = S.settings;
  const captureError = S.status.capture_error;
  return [
    toggle("Start kvmate at sign-in", "waits in the tray", st?.autostart, (v) => saveSettings({ autostart: v })),
    group("Theme"),
    ...THEMES.map(([value, label]) => radio(label, null, st?.theme === value, () => saveSettings({ theme: value }))),
    group("When the window closes"),
    ...CLOSE_ACTIONS.map(([value, label]) => radio(label, null, st?.on_close === value, () => saveSettings({ on_close: value }))),
    note(captureError ? `Keyboard and mouse capture: ${captureError}` : "Keyboard and mouse capture works.", captureError ? "error" : ""),
  ];
}

const PAGES = { km: kmPage, headphones: () => soundPage("headphones"), mic: () => soundPage("mic"), board: boardPage, other: otherPage, here: herePage };

function pageData() {
  const s = S.status;
  const a = s.audio;
  return JSON.stringify([
    S.sub,
    S.settings,
    S.audio,
    S.devices,
    S.wifi,
    S.error,
    S.sub === "headphones" ? [a.playback.active, a.host_volume_percent] : null,
    S.sub === "board" ? [s.state, s.transport, s.port, s.firmware, s.firmware_current, s.flash, s.silent_ports, Boolean(s.bundled_firmware)] : null,
    S.sub === "other" ? [s.state, s.usb] : null,
    S.sub === "here" ? s.capture_error : null,
  ]);
}

export function renderSub() {
  $("main-view").hidden = S.sub !== null;
  $("sub-view").hidden = S.sub === null;
  if (S.sub === null) {
    pageKey = "";
    return;
  }
  $("sub-title").textContent = ROWS.find((r) => r.id === S.sub).title;
  const key = pageData();
  if (key === pageKey) return;
  pageKey = key;
  $("sub-body").replaceChildren(...PAGES[S.sub]().filter(Boolean));
}
