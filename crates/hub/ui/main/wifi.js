import { $, el } from "../shared/dom.js";
import { connected, onCable } from "../shared/status.js";
import { invoke } from "../shared/tauri.js";
import { ask } from "./confirm.js";
import { S } from "./state.js";

let draft = null;
let nameDirty = false;
let password = null;
let saved = null;
let scanAsked = false;

const MODE_NOTES = {
  off: "the board works only over the “COM” cable",
  board_network: "this computer's Wi-Fi adapter connects straight to the board; internet stays on the network cable",
  router: "the board joins your Wi-Fi network (2.4 GHz); this computer's Wi-Fi adapter stays free",
};

function signal(rssi) {
  return rssi == null ? "" : rssi >= -55 ? "very good" : rssi >= -67 ? "good" : rssi >= -75 ? "weak" : "very weak";
}

function boardWifiText() {
  const s = S.status;
  const w = s?.wifi?.board;
  if (!connected(s)) return "no board";
  if (!w) return "the board's firmware doesn't know Wi-Fi yet — update it (below)";
  const strength = w.rssi != null ? ` · signal ${signal(w.rssi)} (${w.rssi} dBm)` : "";
  if (w.mode === "off") return "Wi-Fi off";
  if (w.mode === "router") {
    if (w.state === "ready") return `on “${w.ssid}” · ${w.ip ?? "?"}${strength}`;
    if (w.state === "failed") {
      if (w.fail === "not_found") return `can't see “${w.ssid}” — out of range? not a 2.4 GHz network?`;
      if (w.fail === "auth") return `can't connect to “${w.ssid}” — wrong password?`;
      return `can't connect to “${w.ssid}” (retrying)`;
    }
    return `connecting to “${w.ssid}”…`;
  }
  if (w.state === "ready") return `network “${w.ssid}” is up (channel ${w.channel})${w.client ? ` · this computer connected${strength}` : " · nobody connected"}`;
  return `starting network “${w.ssid}”…`;
}

export function renderConnection() {
  const s = S.status;
  if (!s || !S.wifi) return;
  const conn = connected(s);
  const cable = onCable(s);
  const mode = draft ?? S.wifi.mode;
  for (const b of document.querySelectorAll("[data-wifi-mode]")) {
    b.classList.toggle("on", b.dataset.wifiMode === mode);
    b.setAttribute("aria-checked", String(b.dataset.wifiMode === mode));
  }
  $("wifi-mode-note").textContent = MODE_NOTES[mode];
  $("wifi-router").hidden = mode !== "router";
  $("wifi-own").hidden = mode !== "board_network";

  const ssid = $("wifi-ssid");
  if (!ssid.value && S.wifi.router_ssid && document.activeElement !== ssid) ssid.value = S.wifi.router_ssid;
  const nets = s.wifi?.networks ?? [];
  const netKey = nets.map((n) => n.ssid).join("|");
  if ($("wifi-networks").dataset.key !== netKey) {
    $("wifi-networks").dataset.key = netKey;
    $("wifi-networks").replaceChildren(
      ...nets.map((n) => el("option", { value: n.ssid, label: `${n.auth === "enterprise" ? "enterprise, not supported" : n.auth === "open" ? "open" : "password"} · signal ${signal(n.rssi)} · channel ${n.channel}` })),
    );
  }
  const scanning = Boolean(s.wifi?.scanning);
  if (mode === "router" && S.visible.has("board-conn") && cable && !scanning && nets.length === 0 && !scanAsked) {
    scanAsked = true;
    invoke("wifi_scan");
  }
  renderSaved(nets, scanning);
  $("wifi-scan").disabled = scanning || !conn;
  $("wifi-scan").textContent = scanning ? "Scanning…" : "Scan";
  $("wifi-scan-note").textContent = scanning
    ? "the board is scanning (a few seconds)…"
    : nets.length
      ? `the board sees ${nets.length} ${nets.length === 1 ? "network" : "networks"} — pick one from the list or type a name`
      : "click “Scan” and the board shows the networks it sees";

  const own = S.wifi.mode === "board_network" && S.wifi.network_ssid;
  $("wifi-own-pass").textContent = own ? (password ?? "••••••••••••••••") : "generated when saved";
  const name = $("wifi-name");
  $("wifi-name-rows").hidden = mode === "off";
  if (document.activeElement !== name && !nameDirty) name.value = S.wifi.name;
  name.placeholder = S.wifi.default_name;
  $("wifi-name-title").textContent = mode === "router" ? "Board name in the router" : "Board network name";
  $("wifi-name-note").textContent =
    mode === "router"
      ? "the router lists it among its devices in lowercase, without spaces or accented letters"
      : `shows in the list of nearby networks; empty = “${S.wifi.default_name}”`;
  $("wifi-show").hidden = !own;
  $("wifi-show").textContent = password ? "Hide" : "Show";

  $("wifi-state").hidden = mode === "off" && S.wifi.mode === "off" && (s.wifi?.board?.mode ?? "off") === "off";
  $("wifi-board").textContent = boardWifiText();
  const adapter = mode === "board_network" ? s.wifi?.adapter : null;
  $("wifi-adapter").textContent = adapter ?? "";

  const nothingToSave = mode === "off" && S.wifi.mode === "off";
  const resave = mode === "board_network" && S.wifi.mode === "board_network" && S.wifi.paired && name.value.trim() === S.wifi.name;
  $("wifi-save").hidden = nothingToSave;
  $("wifi-save").disabled = !cable;
  $("wifi-save").textContent = resave ? "Save again (new password)" : "Save to board";
  $("wifi-save").classList.toggle("primary", !resave);
  $("wifi-forget").hidden = S.wifi.mode === "off" && !S.wifi.paired;
  $("wifi-hint").textContent = cable || nothingToSave
    ? ""
    : conn
      ? "The board is on Wi-Fi now: changes need the “COM” cable (the board accepts Wi-Fi settings only over the cable)."
      : "Connect the board with the “COM” cable to save Wi-Fi settings to it.";
}

function renderSaved(nets, scanning) {
  const list = (saved ?? [])
    .filter((n) => n.usable)
    .map((n) => ({ ...n, seen: nets.find((x) => x.ssid === n.ssid) ?? null }))
    .sort((x, y) => Number(Boolean(y.seen)) - Number(Boolean(x.seen)) || Number(y.connected) - Number(x.connected));
  $("wifi-saved").hidden = list.length === 0;
  $("wifi-manual-title").textContent = list.length ? "Other network" : "Network";
  const cable = onCable(S.status);
  const current = S.wifi?.mode === "router" ? S.wifi.router_ssid : null;
  const key = JSON.stringify([list.map((n) => [n.ssid, n.connected, n.seen?.rssi ?? null]), nets.length > 0, scanning, cable, current]);
  if ($("wifi-saved-list").dataset.key === key) return;
  $("wifi-saved-list").dataset.key = key;
  $("wifi-saved-list").replaceChildren(
    ...list.map((n) => {
      const note = n.seen
        ? `the board sees it · signal ${signal(n.seen.rssi)} · channel ${n.seen.channel}`
        : scanning
          ? "the board is scanning…"
          : nets.length
            ? "the board doesn't see it — a 5 GHz network or too far"
            : "the board hasn't scanned yet";
      const inUse = n.ssid === current;
      return el(
        "li",
        {},
        el("span", {}, el("b", { text: n.ssid }), el("small", { text: [n.connected ? "this computer is on it now" : null, note].filter(Boolean).join(" · ") })),
        inUse
          ? el("span", { class: "pill other", text: "saved to board" })
          : el("button", { class: "btn", text: "Connect board", disabled: !cable || (nets.length > 0 && !n.seen), onclick: () => useSaved(n.ssid) }),
      );
    }),
  );
}

export async function connectionInView() {
  if (saved === null) {
    try {
      saved = await invoke("wifi_saved");
    } catch {
      saved = [];
    }
  }
  renderConnection();
}

async function useSaved(ssid) {
  wifiError(null);
  try {
    await invoke("wifi_setup", { mode: "router", ssid, password: null, saved: true, name: $("wifi-name").value });
    $("wifi-pass").value = "";
    $("wifi-ssid").value = ssid;
    draft = null;
    nameDirty = false;
    await loadWifi();
  } catch (e) {
    wifiError(String(e));
  }
}

export async function loadWifi() {
  S.wifi = await invoke("get_wifi");
  if (draft === S.wifi.mode) draft = null;
  if (S.wifi.mode !== "board_network") password = null;
  renderConnection();
}

function wifiError(message) {
  $("wifi-error").hidden = !message;
  $("wifi-error").textContent = message ?? "";
}

for (const b of document.querySelectorAll("[data-wifi-mode]")) {
  b.addEventListener("click", () => {
    draft = b.dataset.wifiMode;
    wifiError(null);
    renderConnection();
  });
}

$("wifi-scan").addEventListener("click", () => invoke("wifi_scan"));

$("wifi-name").addEventListener("input", () => {
  nameDirty = true;
  renderConnection();
});

$("wifi-show").addEventListener("click", async () => {
  password = password ? null : await invoke("wifi_password");
  renderConnection();
});

$("wifi-save").addEventListener("click", async () => {
  const mode = draft ?? S.wifi.mode;
  wifiError(null);
  try {
    if (mode === "off") {
      await invoke("wifi_forget");
    } else {
      await invoke("wifi_setup", { mode, ssid: $("wifi-ssid").value, password: $("wifi-pass").value, name: $("wifi-name").value });
    }
    $("wifi-pass").value = "";
    draft = null;
    password = null;
    nameDirty = false;
    await loadWifi();
  } catch (e) {
    wifiError(String(e));
  }
});

$("wifi-forget").addEventListener("click", () =>
  ask(
    "Forget Wi-Fi? The app deletes the pairing key, and the board — if it's on the cable now — its Wi-Fi settings. After that the board works only over the cable until you set up Wi-Fi again.",
    "Forget Wi-Fi",
    async () => {
      wifiError(null);
      try {
        const cleared = await invoke("wifi_forget");
        if (!cleared) wifiError("The app forgot Wi-Fi, but the board wasn't on the cable: it keeps its settings until you connect it with the cable and click this again (or remove the firmware).");
        draft = null;
        password = null;
        await loadWifi();
      } catch (e) {
        wifiError(String(e));
      }
    },
  ),
);
