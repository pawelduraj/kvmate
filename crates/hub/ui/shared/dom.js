export function $(id) {
  return document.getElementById(id);
}

export function el(tag, props = {}, ...children) {
  const e = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value == null || value === false) continue;
    if (key === "class") e.className = value;
    else if (key === "text") e.textContent = value;
    else if (key.startsWith("on")) e.addEventListener(key.slice(2), value);
    else e.setAttribute(key, value === true ? "" : value);
  }
  for (const child of children.flat()) if (child != null) e.append(child);
  return e;
}

export function text(id, value) {
  const e = $(id);
  if (e) e.textContent = value ?? "—";
}

export function showNotice(n) {
  $("banner").hidden = !n;
  if (!n) return null;
  $("banner").className = n.tone === "info" ? "banner info" : "banner";
  $("banner-title").textContent = n.title;
  $("banner-text").textContent = n.text;
  $("banner-action").hidden = !n.section;
  $("banner-action").textContent = n.action ?? "Show";
  return n.section ?? null;
}

export function switchInput(props = {}) {
  return el("input", { class: "tgl", type: "checkbox", role: "switch", ...props });
}

const ICONS = {
  desktop: "M3 4h14a.5.5 0 0 1 .5.5v8.5a.5.5 0 0 1-.5.5H3a.5.5 0 0 1-.5-.5V4.5A.5.5 0 0 1 3 4zM7 17h6M10 13.5V17",
  laptop: "M4.5 4.5h11a.5.5 0 0 1 .5.5v8H4V5a.5.5 0 0 1 .5-.5zM2 15.5h16",
  keyboard: "M3 6h14a.5.5 0 0 1 .5.5v7a.5.5 0 0 1-.5.5H3a.5.5 0 0 1-.5-.5v-7A.5.5 0 0 1 3 6zM5.5 8.5h1M8.5 8.5h1M11.5 8.5h1M14 8.5h.5M6.5 11.5h7",
  mouse: "M10 2.5c2.5 0 4.5 2 4.5 4.5v6a4.5 4.5 0 0 1-9 0V7c0-2.5 2-4.5 4.5-4.5zM10 5.5v3",
  headphones: "M3.5 13v-2.5a6.5 6.5 0 0 1 13 0V13M3.5 12.5h3v4.5h-3zM13.5 12.5h3v4.5h-3z",
  mic: "M7.5 5a2.5 2.5 0 0 1 5 0v4a2.5 2.5 0 0 1-5 0zM4.5 9.5a5.5 5.5 0 0 0 11 0M10 15v2.5",
  board: "M6 5h8a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM8 2.5V5M12 2.5V5M8 15v2.5M12 15v2.5M2.5 8H5M2.5 12H5M15 8h2.5M15 12h2.5",
  chev: "M8 5l5 5-5 5",
  here: "M3 4h14v9.5H3zM7 17h6",
  other: "M4.5 4.5h11v8.5h-11zM2 15.5h16",
};

export function icon(name, size = 20, cls = "") {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 20 20");
  svg.setAttribute("width", size);
  svg.setAttribute("height", size);
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("class", cls ? `ico ${cls}` : "ico");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", ICONS[name]);
  svg.append(path);
  return svg;
}

const systemDark = matchMedia("(prefers-color-scheme: dark)");
let chosenTheme = "auto";

export function applyTheme(theme) {
  chosenTheme = theme;
  const dark = theme === "dark" || (theme !== "light" && systemDark.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

systemDark.addEventListener("change", () => applyTheme(chosenTheme));
