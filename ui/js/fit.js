/** Shrinks each tile value until it fits its tile, so wide theme fonts (Verdana, Segoe Print) never clip. */
const MIN_PX = 11;

export function fitTiles() {
  for (const el of document.querySelectorAll(".tile__value")) {
    el.style.fontSize = "";
    if (el.clientWidth === 0) continue; // in a hidden tab: measured again when it is shown
    let size = parseFloat(getComputedStyle(el).fontSize);
    while (el.scrollWidth > el.clientWidth && size > MIN_PX) {
      size -= 1;
      el.style.fontSize = `${size}px`;
    }
  }
}
