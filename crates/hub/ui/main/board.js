import { $, el, text } from "../shared/dom.js";
import { FLASH_STAGES, connected, usbText, wifiAddress } from "../shared/status.js";
import { invoke } from "../shared/tauri.js";
import { ask } from "./confirm.js";
import { S } from "./state.js";

const STATE_LABELS = {
  searching: ["looking for the board", ""],
  connecting: ["connecting", "warn"],
  connected: ["connected", "other"],
};

function shortBuild(id) {
  return id ? id.slice(0, 8) : "?";
}

function installOn(port) {
  ask(`Install kvmate firmware on ${port}? Whatever is on this board now will be overwritten.`, `Install on ${port}`, () => invoke("install_firmware", { port }));
}

function boardSummary() {
  const s = S.status;
  if (s.flash) return "Installing the firmware — the keyboard and mouse stay here meanwhile.";
  if (!connected(s)) {
    if ((s.silent_ports ?? []).length) return "There is a board without kvmate firmware on a COM port — install it below.";
    const wifi = S.wifi && S.wifi.mode !== "off";
    return `Looking for the board — ${wifi ? "over the “COM” cable or Wi-Fi" : "connect its “COM” port to this computer"}. On the other computer it sits in the “USB” port, which powers it.`;
  }
  const how = s.transport === "wifi" ? `Connected via Wi-Fi (${wifiAddress(s)})` : `Connected over the “COM” cable (${s.port ?? "?"})`;
  const usb = { mounted: "The other computer sees it.", suspended: "The other computer is asleep.", detached: "The other computer doesn't see it — check the “USB” cable." }[s.usb] ?? "";
  return `${how}. ${usb}`;
}

export function renderBoard() {
  const s = S.status;
  const conn = connected(s);
  let [label, tone] = STATE_LABELS[s.state] ?? [s.state, ""];
  if (!conn && !s.flash && (s.silent_ports ?? []).length > 0) [label, tone] = ["no kvmate firmware", "warn"];
  if (conn) [label, tone] = [s.transport === "wifi" ? "via Wi-Fi" : "cable", "other"];
  if ($("board-state")) {
    $("board-state").className = `pill ${tone}`;
    $("board-state").textContent = label;
    $("board-summary").textContent = boardSummary();
  }
  if ($("fw-summary")) {
    $("fw-summary").textContent = !s.firmware
      ? conn ? "The board didn't report its firmware version." : "The firmware version shows once the board is connected."
      : `Firmware ${s.firmware}${s.firmware_current ? " — the same as in the app." : " — different from the one in the app."} Updates go over the “COM” cable.`;
  }
  text("port", s.port);
  text("bridge", s.bridge);
  text("usb", usbText(s));
  text("firmware", s.firmware ? `${s.firmware} · build ${shortBuild(s.build_id)}` : null);
  const bundled = s.bundled_firmware;
  text("fw-bundled", bundled ? `${bundled.version} · build ${shortBuild(bundled.build_id)} · ${bundled.built}` : "not in this build");
  text("selftest", s.selftest_ok == null ? null : s.selftest_ok ? "OK" : "FAILED");
  text("mac", s.mac);

  const flashing = s.flash != null;
  const onCable = s.transport !== "wifi";
  $("fw-row").hidden = !conn;
  if (conn) {
    $("fw-state").textContent = !bundled
      ? "This build of the app has no firmware, so it can't install it."
      : !onCable
        ? "The firmware is installed over the “COM” cable: connect it to update or remove the firmware."
        : s.firmware_current
        ? "The board has exactly the firmware that is in the app."
        : "The board has different firmware than the app. Updating takes a few seconds; meanwhile the keyboard and mouse return to this computer.";
  }
  const install = $("fw-install");
  install.textContent = s.firmware_current ? "Reinstall" : "Update firmware";
  install.classList.toggle("primary", !s.firmware_current);
  install.disabled = flashing || !bundled || !onCable;
  $("fw-erase").disabled = flashing || !onCable;

  const silent = s.silent_ports ?? [];
  $("silent").hidden = conn || silent.length === 0 || flashing;
  const silentKey = silent.map((p) => p.port).join(",") + String(Boolean(bundled));
  if ($("silent-list").dataset.key !== silentKey) {
    $("silent-list").dataset.key = silentKey;
    $("silent-list").replaceChildren(
      ...silent.map((p) =>
        el(
          "li",
          {},
          el("span", { text: p.bridge ? `${p.port} (${p.bridge})` : p.port }),
          el("button", { class: "btn primary", text: "Install kvmate firmware", disabled: !bundled, onclick: () => installOn(p.port) }),
        ),
      ),
    );
  }

  $("fw-progress").hidden = !flashing;
  if (flashing) {
    const f = s.flash;
    const what = f.kind === "erase" ? "Removing the firmware" : "Installing the firmware";
    const pct = f.stage === "writing" && f.progress != null ? ` ${Math.round(f.progress * 100)}%` : "";
    $("fw-stage").textContent = `${what} (${f.port}): ${FLASH_STAGES[f.stage] ?? f.stage}${pct}`;
    const bar = $("fw-bar");
    if (f.progress != null) bar.value = f.progress;
    else bar.removeAttribute("value");
  }
  const last = s.last_flash;
  $("fw-result-row").hidden = !last || flashing;
  if (last) {
    $("fw-result").textContent = last.text;
    $("fw-result").className = last.ok ? "ok-text" : "error-text";
  }
}

$("fw-install").addEventListener("click", () => invoke("install_firmware", { port: null }));
$("fw-erase").addEventListener("click", () =>
  ask(
    "Remove the kvmate firmware from the board? The whole flash memory will be erased — the board can then be used for " +
      "other projects, and kvmate won't work until you install the firmware again (through the “COM” port).",
    "Remove firmware",
    () => invoke("erase_firmware", { port: null }),
  ),
);

let logFilter = "all";
let logKey = "";

function logLines() {
  const lines = S.status?.logs ?? [];
  return logFilter === "all" ? lines : lines.filter((l) => l.level === "warn" || l.level === "error");
}

export function renderLogs() {
  if (!$("log-box").open) return;
  for (const b of document.querySelectorAll("[data-filter]")) {
    b.classList.toggle("on", b.dataset.filter === logFilter);
    b.setAttribute("aria-pressed", String(b.dataset.filter === logFilter));
  }
  const lines = logLines();
  const key = `${logFilter}|${S.status?.log_total ?? 0}`;
  if (key === logKey) return;
  logKey = key;
  const box = $("logs");
  const atEnd = box.scrollTop + box.clientHeight >= box.scrollHeight - 8;
  box.replaceChildren(...lines.map((line) => el("li", { class: line.level, text: line.text })));
  if (atEnd) box.scrollTop = box.scrollHeight;
}

for (const b of document.querySelectorAll("[data-filter]")) {
  b.addEventListener("click", () => {
    logFilter = b.dataset.filter;
    renderLogs();
    $("logs").scrollTop = 1e9;
  });
}
$("log-box").addEventListener("toggle", () => {
  logKey = "";
  renderLogs();
  $("logs").scrollTop = 1e9;
});
$("log-copy").addEventListener("click", () => navigator.clipboard.writeText(logLines().map((l) => `${l.level} ${l.text}`).join("\n")));
