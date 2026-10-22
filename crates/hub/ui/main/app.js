import { $, applyTheme, showNotice } from "../shared/dom.js";
import { notice } from "../shared/status.js";
import { invoke, listen } from "../shared/tauri.js";
import { renderLogs } from "./board.js";
import { loadConfig, loadDevices } from "./controls.js";
import { buildMap, show, spy, updateMap } from "./map.js";
import { renderSections } from "./sections.js";
import { S } from "./state.js";
import { loadWifi, renderConnection } from "./wifi.js";

let bannerSection = null;

function onStatus() {
  bannerSection = showNotice(notice(S.status, S.wifi));
  if (!$("col-here")) buildMap();
  updateMap();
  renderSections();
  renderConnection();
  renderLogs();
}

$("banner-action").addEventListener("click", () => show(bannerSection));
listen("select", (event) => show(event.payload));
listen("status", (event) => {
  S.status = event.payload;
  onStatus();
});
listen("config", () => {
  loadConfig();
  loadWifi();
});

applyTheme("auto");
invoke("config_location").then((path) => {
  S.configPath = path ? path.replace(/[\\/]config\.json$/, "") : null;
  $("log-path").textContent = path ? `Full log with timestamps: ${path.replace(/config\.json$/, "kvmate.log")}` : "";
});
Promise.all([loadConfig(), invoke("get_status")]).then(([, status]) => {
  S.status = status;
  onStatus();
  spy();
  show(decodeURIComponent(location.hash.slice(1)), false);
});
loadDevices();
loadWifi();
