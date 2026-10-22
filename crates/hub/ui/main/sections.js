import { $, el, icon, switchInput, text } from "../shared/dom.js";
import { AUDIO_MODES, CLOSE_ACTIONS, THEMES, connected, usbText } from "../shared/status.js";
import { invoke } from "../shared/tauri.js";
import { renderBoard } from "./board.js";
import { loadDevices, syncControls } from "./controls.js";
import { ROWS } from "./map.js";
import { S } from "./state.js";
import { connectionInView } from "./wifi.js";

const SECTIONS = ["here", ...ROWS.map((r) => r.id), "board-conn", "board-fw", "other"];

const sight = new IntersectionObserver(
  (items) => {
    let devices = false;
    for (const item of items) {
      const id = item.target.dataset.section;
      if (!item.isIntersecting) {
        S.visible.delete(id);
        continue;
      }
      S.visible.add(id);
      if (id === "headphones" || id === "mic") devices = true;
      if (id === "board-conn") connectionInView();
    }
    if (devices) loadDevices();
  },
  { root: $("main") },
);

function sectionKey(id) {
  const s = S.status;
  switch (id) {
    case "board-conn":
    case "board-fw":
    case "km":
      return id;
    case "headphones":
    case "mic":
      return `${id}|${usbName()}`;
    case "here":
      return [id, s?.capture_error ?? "", S.configPath].join("|");
    case "other":
      return [id, usbName(), S.settings?.usb_manufacturer, connected(s) && s.usb === "detached"].join("|");
  }
}

function typing(card) {
  const e = document.activeElement;
  return card.contains(e) && e.tagName === "INPUT" && e.type !== "checkbox";
}

export function renderSections() {
  let built = false;
  const cards = SECTIONS.map((id) => {
    let card = S.cards.get(id);
    if (!card) {
      card = el("section", { class: "card detail", "data-section": id });
      S.cards.set(id, card);
      sight.observe(card);
    }
    const key = sectionKey(id);
    if (card.dataset.key !== key && !typing(card)) {
      buildSection(card, id);
      card.dataset.key = key;
      built = true;
    }
    return card;
  });
  if (!$("sections").children.length) $("sections").replaceChildren(...cards);
  if (built) syncControls();
  renderBoard();
  updateLive();
}

$("sections").addEventListener("focusout", () => setTimeout(() => S.status && renderSections()));

function buildSection(card, id) {
  if (id === "board-conn") {
    card.replaceChildren(
      el(
        "div",
        { class: "detail-head" },
        el("h2", {}, icon("board", 22), "Board connection"),
        el("p", { id: "board-summary" }),
        el("span", {}, el("span", { id: "board-state", class: "pill" })),
      ),
      $("conn-panel"),
    );
  } else if (id === "board-fw") {
    card.replaceChildren(el("div", { class: "detail-head" }, el("h2", {}, icon("board", 22), "ESP32-S3 board"), el("p", { id: "fw-summary" })), $("fw-panel"));
  } else {
    const parts = sectionParts(id);
    card.replaceChildren(parts[0], el("div", { class: "rows" }, ...parts.slice(1)));
  }
}

function head(iconName, title, description) {
  return el("div", { class: "detail-head" }, el("h2", {}, icon(iconName, 22), title), el("p", { text: description }));
}

function row(label, hint, ...controls) {
  return el("div", { class: "row" }, el("span", {}, label, hint ? el("small", { text: hint }) : null), ...controls);
}

function checkRow(label, hint, input) {
  return el("label", { class: "row check" }, el("span", {}, label, hint ? el("small", { text: hint }) : null), input);
}

function settingSelect(key, label, options) {
  return el("select", { "data-setting": key, "aria-label": label }, ...options.map(([value, name]) => el("option", { value, text: name })));
}

function sendButtons() {
  return el(
    "span",
    { class: "actions" },
    el("button", { class: "btn", text: "Ctrl+Alt+Del", onclick: () => invoke("send_combo", { combo: "ctrl_alt_del" }) }),
    el("button", { class: "btn", text: "Lock (Win+L)", onclick: () => invoke("send_combo", { combo: "lock" }) }),
  );
}

function modeSeg(key) {
  return el("div", { class: "seg", role: "group", "data-audio-mode": key }, ...AUDIO_MODES.map(([mode, label]) => el("button", { "data-mode": mode, text: label })));
}

function sectionParts(id) {
  const s = S.status;
  switch (id) {
    case "km":
      return [
        head("keyboard", "Keyboard and mouse", "They always go together to one computer. Switch with the shortcut, the “Switch” button by a computer, the BOOT button on the board (hold: always back here) or the tray menu."),
        row("Switch shortcut", "two quick presses of one key", el("select", { "data-setting": "hotkey", "aria-label": "Switch shortcut" })),
        checkRow("Ctrl+Alt+End works as Ctrl+Alt+Del on the other computer", null, switchInput({ "data-setting": "ctrl_alt_end_as_delete" })),
        el("p", { class: "note", text: "Mouse movement goes without this computer's acceleration — the other one applies its own, as with a regular mouse. Ctrl+Alt+Del and Win+L pressed on the keyboard are always handled by this computer — Windows doesn't let them be captured; send them to the other one with the buttons in “Other computer”." }),
      ];
    case "headphones":
      return [
        head("headphones", "Headphones", `This computer always plays in them. The other one sees them as the “${usbName()}” headphones.`),
        row("Other computer's sound", null, modeSeg("playback")),
        row("Headphones", null, el("select", { "data-audio-device": "output_device", "aria-label": "Headphones" })),
        row("Now", null, el("span", { id: "live-playback", class: "note" })),
        row(
          "Volume on the other computer",
          "as its volume slider shows it",
          el("span", { class: "meter-wrap" }, el("span", { class: "meter" }, el("span", { id: "volume" })), el("span", { id: "volume-text", class: "note" })),
        ),
        el("p", { class: "note", text: "kvmate doesn't mute this computer — to hear only the other one, mute this one." }),
      ];
    case "mic":
      return [
        head("mic", "Microphone", `This computer always has it. The other one sees it as the “${usbName()}” microphone.`),
        row("Microphone for the other computer", "“Always”: e.g. a call on the other one while you work here", modeSeg("capture")),
        row("Microphone", null, el("select", { "data-audio-device": "input_device", "aria-label": "Microphone" })),
        row("Now", null, el("span", { id: "live-capture", class: "note" })),
      ];
    case "here":
      return [
        head("desktop", "This computer", "kvmate runs here, and the keyboard, mouse, headphones and microphone are connected here."),
        row("Keyboard and mouse capture", null, el("span", { class: s?.capture_error ? "error-text" : "ok-text", text: s?.capture_error ?? "working" })),
        checkRow("Start kvmate at sign-in", "waits in the tray", switchInput({ "data-setting": "autostart" })),
        row("Theme", null, settingSelect("theme", "Theme", THEMES)),
        row("When the window is closed", null, settingSelect("on_close", "When the window is closed", CLOSE_ACTIONS)),
        S.configPath ? el("p", { class: "note", text: `App settings and log: ${S.configPath}` }) : null,
      ].filter(Boolean);
    case "other":
      return [
        head("laptop", "Other computer", `It sees a regular keyboard, mouse, headphones and microphone named “${usbName()}” — nothing is installed on it.`),
        row("Board's “USB” port", null, el("span", { id: "other-usb" })),
        connected(s) && s.usb === "detached"
          ? el(
              "ol",
              { class: "steps note" },
              el("li", { text: "The board's “USB” port (not “COM”) → the other computer or its dock." }),
              el("li", { text: "The cable must carry data — a charge-only one won't do." }),
              el("li", { text: "Push the plug all the way in — the board reports when the computer detects it." }),
            )
          : null,
        checkRow("Keep it awake", "while you control this computer, moves its mouse by 0 px every 30 s — the cursor stays put", switchInput({ "data-setting": "jiggle" })),
        row("Send to it", "works while you control this computer too", sendButtons()),
        ...usbNamesForm(),
      ].filter(Boolean);
  }
}

function usbName() {
  return S.settings?.usb_name || "kvmate";
}

function usbNamesForm() {
  const st = S.settings ?? {};
  const error = el("p", { class: "note error-text", hidden: true });
  const save = async () => {
    const change = { usb_name: $("usb-name").value.trim(), usb_manufacturer: $("usb-manufacturer").value.trim() };
    if (change.usb_name === S.settings.usb_name && change.usb_manufacturer === S.settings.usb_manufacturer) return;
    try {
      await invoke("set_settings", { settings: change });
      S.settings = { ...S.settings, ...change };
      renderSections();
    } catch (e) {
      error.hidden = false;
      error.textContent = String(e);
    }
  };
  const field = (id, value, label) =>
    el("input", { class: "field", id, value: value ?? "", maxlength: 31, autocomplete: "off", spellcheck: "false", "aria-label": label, onkeydown: (e) => e.key === "Enter" && save() });
  return [
    el("h3", { class: "sub", text: "How the board appears to it" }),
    row("Device name", "it shows the keyboard, mouse, headphones and microphone under it", field("usb-name", st.usb_name, "Device name")),
    row("Manufacturer", null, field("usb-manufacturer", st.usb_manufacturer, "Manufacturer")),
    el("div", { class: "actions wifi-actions" }, el("button", { class: "btn", text: "Change names", onclick: save })),
    error,
    el("p", {
      class: "note",
      text: "Up to 31 characters, plain ASCII only (no accented letters) — USB can't carry others. After a change the board briefly disconnects from the other computer, and its Windows sees it as a new device: you may need to pick the headphones and microphone again (e.g. in Teams). The VID/PID numbers stay the same.",
    }),
  ];
}

function describeStream(stream, mode) {
  if (!connected(S.status)) return "waiting for the board";
  if (mode === "off") return "off";
  if (stream.active) {
    if (stream.error) return `error: ${stream.error}`;
    if (!stream.device) return "opening the device…";
    const parts = [stream.device];
    if (stream.buffer_ms != null) parts.push(`buffer ${Math.round(stream.buffer_ms)} ms`);
    if (stream.drift_ppm != null) parts.push(`clock ${stream.drift_ppm >= 0 ? "+" : ""}${Math.round(stream.drift_ppm)} ppm`);
    return parts.join(" · ");
  }
  if (mode === "when_laptop" && stream.peer_active) return "paused — you are controlling this computer";
  return "unused — the other computer isn't playing / recording";
}

function updateLive() {
  const s = S.status;
  if (!s) return;
  const a = s.audio;
  text("live-playback", describeStream(a.playback, a.playback_mode));
  text("live-capture", describeStream(a.capture, a.capture_mode));
  if ($("volume")) {
    const pct = a.playback.active ? a.host_volume_percent : null;
    $("volume").style.width = `${pct ?? 0}%`;
    text("volume-text", !a.playback.active ? "nothing playing" : pct == null ? "muted" : `${pct}%`);
  }
  if ($("other-usb")) {
    $("other-usb").textContent = usbText(s);
    $("other-usb").className = connected(s) && s.usb === "detached" ? "warn-text" : "";
  }
}
