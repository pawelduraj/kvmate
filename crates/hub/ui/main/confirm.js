import { $ } from "../shared/dom.js";

export function ask(message, yesLabel, onYes) {
  $("confirm-text").textContent = message;
  $("confirm-yes").textContent = yesLabel;
  $("confirm-yes").onclick = () => {
    $("confirm").hidden = true;
    onYes();
  };
  $("confirm").hidden = false;
}

$("confirm-no").addEventListener("click", () => ($("confirm").hidden = true));
