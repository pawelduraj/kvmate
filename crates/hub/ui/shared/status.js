export function connected(s) {
  return s?.state === "connected";
}

export function reachable(s) {
  return connected(s) && (s.usb === "mounted" || s.usb === "suspended");
}

export function onOther(s) {
  return s?.target === "laptop";
}

export function onCable(s) {
  return connected(s) && s.transport === "cable";
}

const USB_TEXT = {
  detached: "doesn't see the board — check the “USB” cable",
  mounted: "sees the board",
  suspended: "asleep — a key press wakes it",
};

export function usbText(s) {
  return connected(s) ? (USB_TEXT[s.usb] ?? "—") : "no board";
}

export const FLASH_STAGES = {
  connecting: "connecting to the board's bootloader…",
  erasing: "erasing the flash memory (may take up to half a minute)…",
  writing: "writing the firmware",
  verifying: "verifying…",
  restarting: "restarting the board…",
};

export function notice(s, wifi) {
  const conn = connected(s);
  if (s.flash) {
    const f = s.flash;
    const pct = f.stage === "writing" && f.progress != null ? ` ${Math.round(f.progress * 100)}%` : "";
    const title = f.kind === "erase" ? "Removing the firmware…" : "Installing the firmware…";
    return { tone: "info", title, text: `${f.port}: ${FLASH_STAGES[f.stage] ?? f.stage}${pct}`, section: "board-fw" };
  }
  if (s.capture_error) return { tone: "warn", title: "Can't capture the keyboard and mouse", text: s.capture_error, section: "here" };
  if (!conn && (s.silent_ports ?? []).length > 0) {
    return { tone: "info", title: "Board without kvmate firmware", text: "Install it — takes about 15 s.", section: "board-fw", action: "Install firmware…" };
  }
  if (!conn && wifi && wifi.mode !== "off") {
    const text = "Over the “COM” cable or Wi-Fi — the board must be in the other computer's “USB” port, which powers it.";
    return { tone: "info", title: "Looking for the board…", text, section: "board-conn" };
  }
  if (!conn) return { tone: "info", title: "Looking for the board…", text: "Connect the board's “COM” port to this computer." };
  if (s.usb === "detached") {
    const text = "Connect the board's “USB” port (not “COM”) to the other computer or its dock. The keyboard and mouse stay here.";
    return { tone: "warn", title: "The other computer doesn't see the board", text, section: "other" };
  }
  if (s.selftest_ok === false) return { tone: "warn", title: "The board reports a self-test error", text: "Reinstall the firmware.", section: "board-fw" };
  if (s.versions_match === false) {
    return { tone: "warn", title: "The board's firmware is from another version", text: "Update it — takes a few seconds.", section: "board-fw" };
  }
  const { playback, capture } = s.audio;
  if (playback.error || capture.error) {
    const text = [playback.error, capture.error].filter(Boolean).join(" · ");
    return { tone: "warn", title: "Sound problem", text, section: playback.error ? "headphones" : "mic" };
  }
  return null;
}

export function wifiAddress(s) {
  return (s.port ?? "").replace(/^Wi-Fi\s*/, "");
}

export function lanes(id, s) {
  const a = s.audio;
  const other = onOther(s);
  const right = (flow, off = false) => (off ? "none" : flow && reachable(s) ? flow : "idle");
  switch (id) {
    case "km":
      return { l: other ? "idle" : "out", r: right(other ? "out" : null) };
    case "headphones":
      return { l: a.here_playing ? "in" : "idle", r: right(a.playback.active ? "in" : null, a.playback_mode === "off") };
    case "mic":
      return { l: a.here_recording ? "out" : "idle", r: right(a.capture.active ? "out" : null, a.capture_mode === "off") };
  }
}

export const LED_LEVELS = [3, 10, 23, 41, 64, 92, 125, 163, 207, 255];

export function ledStep(brightness) {
  return LED_LEVELS.reduce((best, v, i) => (Math.abs(v - brightness) < Math.abs(LED_LEVELS[best] - brightness) ? i : best), 0) + 1;
}

export function ledSwitch(on, settings) {
  return on && !(settings.led_brightness > 0) ? { led_on: true, led_brightness: LED_LEVELS[4] } : { led_on: on };
}

export const THEMES = [["auto", "System"], ["light", "Light"], ["dark", "Dark"]];
export const CLOSE_ACTIONS = [["tray", "Keep kvmate running in the tray"], ["quit", "Quit kvmate"]];
export const AUDIO_MODES = [["always", "Always"], ["when_laptop", "While controlling the other computer"], ["off", "Never"]];
