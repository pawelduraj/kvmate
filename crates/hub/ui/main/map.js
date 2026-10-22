import { $, el, icon } from "../shared/dom.js";
import { connected, lanes, onOther, reachable, usbText } from "../shared/status.js";
import { invoke } from "../shared/tauri.js";
import { S } from "./state.js";

export const ROWS = [
  { id: "km", icons: ["keyboard", "mouse"], name: "Keyboard and mouse" },
  { id: "headphones", icons: ["headphones"], name: "Headphones" },
  { id: "mic", icons: ["mic"], name: "Microphone" },
];

function computerColumn(side) {
  const here = side === "here";
  return el(
    "div",
    { class: `computer ${side}`, id: `col-${side}` },
    el(
      "button",
      { class: "col-main", onclick: () => show(here ? "here" : "other") },
      icon(here ? "desktop" : "laptop", 36),
      el("b", { text: here ? "This computer" : "Other computer" }),
      el("small", { id: `${side}-sub` }),
    ),
    el("span", { class: "tag", id: `${side}-tag`, text: "in control" }),
    el("button", { class: "go", id: `${side}-go`, text: here ? "Switch here" : "Switch to it", onclick: () => invoke("set_target", { target: here ? "pc" : "laptop" }) }),
  );
}

export function buildMap() {
  const rows = [
    el(
      "div",
      { class: "bus", id: "bus" },
      el(
        "button",
        { class: "bus-main", title: "Board: connection", onclick: () => show("board-conn") },
        icon("board", 26),
        el("b", { text: "Board" }),
        el("small", { class: "chip", text: "ESP32-S3" }),
        el("span", { class: "bus-state" }, el("i"), el("span", { id: "bus-label" })),
      ),
      el("button", { class: "bus-btn", id: "bus-conn", text: "Connection", onclick: () => show("board-conn") }),
      el("button", { class: "bus-btn", id: "bus-fw", text: "Firmware", onclick: () => show("board-fw") }),
    ),
  ];
  ROWS.forEach((spec, i) => {
    const row = String(i + 1);
    for (const lane of ["l", "a", "b"]) {
      const e = el("div", { class: `lane ${lane}`, "data-lane": `${spec.id}|${lane}` });
      e.style.gridRow = row;
      rows.push(e);
    }
    const device = el(
      "div",
      { class: "device", "data-device": spec.id },
      el(
        "button",
        { class: "device-main", onclick: () => show(spec.id), title: spec.name },
        el("span", { class: "device-icons" }, ...spec.icons.map((name) => icon(name))),
        el("span", {}, el("b", { text: spec.name }), el("small", { "data-status": spec.id })),
        icon("chev", 16),
      ),
    );
    device.style.gridRow = row;
    rows.push(device);
  });
  const grid = el("div", { class: "map-rows" }, ...rows);
  grid.style.gridTemplateRows = `repeat(${ROWS.length}, auto)`;
  $("map").replaceChildren(computerColumn("here"), el("div", { class: "map-mid" }, grid), computerColumn("other"));
}

function rowStatus(id, s) {
  const a = s.audio;
  const other = onOther(s);
  switch (id) {
    case "km":
      return other ? "→ other computer" : "→ this computer";
    case "headphones": {
      const here = a.here_playing;
      const there = a.playback.active;
      const waiting = !there && a.playback_mode === "when_laptop" && !other && a.playback.peer_active;
      return here && there ? "both computers playing" : there ? "other computer playing" : here ? "this computer playing" : waiting ? "the other one plays — you'll hear it while controlling it" : "silence";
    }
    case "mic": {
      const here = a.here_recording;
      const there = a.capture.active;
      const waiting = !there && a.capture_mode === "when_laptop" && !other && a.capture.peer_active;
      return here && there ? "both computers recording" : there ? "other computer recording" : here ? "this computer recording" : waiting ? "the other one gets it while you control it" : "nobody recording";
    }
  }
}

export function updateMap() {
  const s = S.status;
  if (!s || !$("col-here")) return;
  for (const spec of ROWS) {
    const { l, r } = lanes(spec.id, s);
    const lane = (k) => document.querySelector(`[data-lane="${CSS.escape(`${spec.id}|${k}`)}"]`);
    lane("l").className = `lane l here ${l}`;
    lane("a").className = `lane a other ${r}`;
    lane("b").className = `lane b other ${r}`;
    const status = document.querySelector(`[data-status="${CSS.escape(spec.id)}"]`);
    if (status) status.textContent = rowStatus(spec.id, s);
    const device = document.querySelector(`[data-device="${CSS.escape(spec.id)}"]`);
    device?.classList.toggle("sel", S.sel === spec.id);
  }
  const conn = connected(s);
  const other = onOther(s);
  $("col-here").classList.toggle("on", !other);
  $("col-other").classList.toggle("on", other);
  $("col-here").classList.toggle("sel", S.sel === "here");
  $("col-other").classList.toggle("sel", S.sel === "other");
  $("col-other").classList.toggle("broken", conn && s.usb === "detached");
  $("here-tag").hidden = other;
  $("other-tag").hidden = !other;
  $("here-go").hidden = !other;
  $("other-go").hidden = other;
  $("other-go").disabled = !reachable(s);
  $("here-sub").textContent = s.capture_error ? "capture not working" : "keyboard, mouse and headphones are here";
  $("other-sub").textContent = usbText(s);
  const silent = !conn && (s.silent_ports ?? []).length > 0;
  $("bus").className = `bus ${silent ? "warn" : !conn ? "off" : ""} ${S.sel === "board-conn" || S.sel === "board-fw" ? "sel" : ""}`;
  $("bus-conn").classList.toggle("on", S.sel === "board-conn");
  $("bus-fw").classList.toggle("on", S.sel === "board-fw");
  $("bus-label").textContent = conn
    ? s.demo ? "demo" : s.transport === "wifi" ? "via Wi-Fi" : `cable · ${s.port ?? ""}`
    : silent
      ? "no firmware"
      : s.state === "connecting" ? "connecting…" : "searching…";
}

export function show(id, smooth = true) {
  const card = S.cards.get(id);
  if (!card) return;
  S.sel = id;
  S.spyAfter = performance.now() + 1000;
  updateMap();
  card.scrollIntoView({ block: "start", behavior: smooth ? "smooth" : "auto" });
}

export function spy() {
  if (performance.now() < S.spyAfter) return;
  const top = $("main").getBoundingClientRect().top + 40;
  let current = $("sections").firstElementChild?.dataset.section ?? null;
  for (const card of $("sections").children) {
    if (card.getBoundingClientRect().top > top) break;
    current = card.dataset.section;
  }
  if (current !== S.sel) {
    S.sel = current;
    updateMap();
  }
}

$("main").addEventListener("scroll", spy, { passive: true });
$("main").addEventListener("scrollend", () => {
  S.spyAfter = 0;
  spy();
});

function fitRoom() {
  const last = $("sections").lastElementChild;
  const room = last ? $("main").clientHeight - last.offsetHeight - 16 : 0;
  $("sections").style.paddingBottom = `${Math.max(24, room)}px`;
}

const roomWatch = new ResizeObserver(fitRoom);
roomWatch.observe($("main"));
roomWatch.observe($("sections"));
