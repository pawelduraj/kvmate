import { $, applyTheme, el, icon, showNotice } from "../shared/dom.js";
import { THEMES, connected, lanes, notice, onOther, reachable, usbText } from "../shared/status.js";
import { invoke, listen } from "../shared/tauri.js";
import { renderSub } from "./pages.js";

export const ROWS = [
  { id: "km", icon: "keyboard", title: "Keyboard and mouse", lanes: true },
  { id: "headphones", icon: "headphones", title: "Headphones", lanes: true },
  { id: "mic", icon: "mic", title: "Microphone", lanes: true },
  { id: "board", icon: "board", title: "Board" },
  { id: "other", icon: "laptop", title: "Other computer" },
  { id: "here", icon: "desktop", title: "This computer" },
];

export const S = {
  status: null,
  settings: null,
  audio: null,
  devices: null,
  wifi: null,
  sub: null,
  error: null,
};

let bannerSection = null;

function rowText(id) {
  const s = S.status;
  const a = s.audio;
  const st = S.settings;
  switch (id) {
    case "km":
      return `shortcut: ${s.hotkey || "2× Right Ctrl"}`;
    case "headphones": {
      const here = a.here_playing;
      const there = a.playback.active;
      const text = here && there ? "both computers are playing" : there ? "the other computer is playing" : here ? "this computer is playing" : "silence";
      const volume = there && a.host_volume_percent != null ? ` · ${a.host_volume_percent}%` : "";
      return `${text}${volume}`;
    }
    case "mic": {
      const here = a.here_recording;
      const there = a.capture.active;
      return here && there ? "both computers are recording" : there ? "the other computer is recording" : here ? "this computer is recording" : "nobody is recording";
    }
    case "board":
      return boardLine();
    case "other":
      return [usbText(s), st?.jiggle ? "kept awake" : null].filter(Boolean).join(" · ");
    default:
      return [st?.autostart ? "starts at sign-in" : "no autostart", `theme: ${(THEMES.find(([v]) => v === st?.theme) ?? THEMES[0])[1].toLowerCase()}`].join(" · ");
  }
}

export function boardLine() {
  const s = S.status;
  if (s.flash) return "installing firmware…";
  if (!connected(s)) {
    if ((s.silent_ports ?? []).length) return "board without kvmate firmware";
    return s.state === "connecting" ? "connecting…" : "looking for the board";
  }
  const how = s.transport === "wifi" ? "via Wi-Fi" : `cable · ${s.port ?? "?"}`;
  return `${how} · firmware ${s.firmware ?? "?"}`;
}

function buildRows() {
  $("rows").replaceChildren(
    ...ROWS.map((r) =>
      el(
        "button",
        { class: "row", "data-sub": r.id, onclick: () => openSub(r.id) },
        icon(r.icon),
        el("span", { class: "text" }, el("b", { text: r.title }), el("small", { id: `${r.id}-sub` })),
        r.lanes
          ? el(
              "span",
              { id: `${r.id}-lanes`, class: "lanes", "aria-hidden": "true" },
              icon("here", 14, "end here"),
              el("span", { class: "lane left" }),
              el("span", { class: "dot" }),
              el("span", { class: "lane right" }),
              icon("other", 14, "end other"),
            )
          : null,
        icon("chev", 14, "chev"),
      ),
    ),
  );
}

function render() {
  const s = S.status;
  if (!s) return;
  const conn = connected(s);
  const other = onOther(s);

  const n = notice(s, S.wifi);
  const warn = n?.tone === "warn";
  $("state").className = `state ${warn ? "warn" : !conn ? "" : other ? "other" : "here"}`;
  const label = !conn ? (s.state === "connecting" ? "connecting…" : "no board") : warn ? "problem" : other ? "controlling the other computer" : "controlling this computer";
  $("state-text").textContent = label + (s.demo ? " (demo)" : "");
  bannerSection = showNotice(n);

  $("t-here").classList.toggle("active", !other);
  $("t-other").classList.toggle("active", other);
  $("t-here").setAttribute("aria-pressed", String(!other));
  $("t-other").setAttribute("aria-pressed", String(other));
  $("t-other").disabled = !reachable(s) && !other;
  $("t-here-sub").textContent = other ? "switch back" : "in control";
  $("t-other-sub").textContent = other ? "in control" : !conn ? "no board" : s.usb === "detached" ? "doesn't see the board" : s.usb === "suspended" ? "asleep — a key press wakes it" : "switch";

  for (const row of ROWS) {
    $(`${row.id}-sub`).textContent = rowText(row.id);
    if (row.lanes) {
      const { l, r } = lanes(row.id, s);
      $(`${row.id}-lanes`).className = `lanes l-${l} r-${r}`;
    }
  }

  $("foot").textContent = conn
    ? `${s.transport === "wifi" ? "Wi-Fi" : s.port ?? "?"} · firmware ${s.firmware ?? "?"}`
    : s.state === "connecting"
      ? `${s.port ?? (s.transport === "wifi" ? "Wi-Fi" : "?")} · connecting`
      : "looking for the board";

  renderSub();
}

async function openSub(name) {
  const sound = name === "headphones" || name === "mic";
  S.sub = name;
  S.error = null;
  if (sound) S.devices = null;
  render();
  if (!sound) return;
  const found = await invoke("audio_devices");
  if (S.sub !== name) return;
  S.devices = found;
  render();
}

function closeSub() {
  S.sub = null;
  render();
}

export async function saveAudio(change) {
  S.audio = { ...S.audio, ...change };
  render();
  try {
    await invoke("set_audio_config", { audio: S.audio });
  } catch {
    loadConfig();
  }
}

export async function saveSettings(change) {
  S.settings = { ...S.settings, ...change };
  if (change.theme) applyTheme(change.theme);
  render();
  try {
    await invoke("set_settings", { settings: change });
  } catch {
    loadConfig();
  }
}

export async function setWifi(mode) {
  if (!S.wifi || S.wifi.mode === mode) return;
  S.error = null;
  try {
    if (mode === "off") await invoke("wifi_forget");
    else await invoke("wifi_setup", { mode, ssid: null, password: null, saved: null });
  } catch (e) {
    S.error = String(e);
  }
  await loadWifi();
}

async function loadConfig() {
  [S.settings, S.audio] = await Promise.all([invoke("get_settings"), invoke("get_audio_config")]);
  applyTheme(S.settings.theme);
  render();
}

async function loadWifi() {
  S.wifi = await invoke("get_wifi");
  render();
}

for (const button of document.querySelectorAll(".target")) {
  button.addEventListener("click", () => invoke("set_target", { target: button.dataset.target }));
}
$("back").addEventListener("click", closeSub);
$("open").addEventListener("click", () => invoke("open_main", { element: null }));
$("quit").addEventListener("click", () => invoke("quit_app"));
$("banner-action").addEventListener("click", () => invoke("open_main", { element: bannerSection }));
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (S.sub !== null) closeSub();
  else invoke("panel_hide");
});

let lastHeight = 0;
new ResizeObserver(() => {
  const height = Math.ceil($("panel").getBoundingClientRect().height);
  if (height !== lastHeight) {
    lastHeight = height;
    invoke("panel_fit", { height });
  }
}).observe($("panel"));

applyTheme("auto");
buildRows();
listen("status", (event) => {
  S.status = event.payload;
  render();
});
listen("config", () => {
  loadConfig();
  loadWifi();
});
invoke("get_status").then((s) => {
  S.status = s;
  render();
});
loadConfig();
loadWifi();
