// Text size for long hours at the report: the whole window scales (Ctrl + / Ctrl − / Ctrl 0,
// or Settings). Kept on this computer only; it holds nothing about any case.
const KEY = "dv.textSize";
export const TEXT_SIZES = [90, 100, 110, 125, 140] as const;
/** Said on the window when the text size changed: the page is laid out anew, as after a resize
 *  (which a zoom does not fire). */
export const TEXT_SIZE_EVENT = "dv-text-size";

export function readTextSize(): number {
  try {
    const n = Number(localStorage.getItem(KEY));
    return (TEXT_SIZES as readonly number[]).includes(n) ? n : 100;
  } catch {
    return 100;
  }
}

export function applyTextSize(percent: number): void {
  const zoom = percent === 100 ? "" : `${percent}%`;
  // A key at the end of the list, or Ctrl 0 at 100%, changes nothing: nothing is said either.
  const changed = document.documentElement.style.getPropertyValue("zoom") !== zoom;
  document.documentElement.style.setProperty("zoom", zoom);
  // Viewport units are scaled with the window: a dialog sized by `100vh` divides by this.
  document.documentElement.style.setProperty("--zoom", percent === 100 ? "" : String(percent / 100));
  try {
    localStorage.setItem(KEY, String(percent));
  } catch {
    // Not kept: applies until the window closes.
  }
  if (changed) window.dispatchEvent(new Event(TEXT_SIZE_EVENT));
}

/** One step larger (+1) or smaller (−1), within the list. */
export function stepTextSize(current: number, step: 1 | -1): number {
  const i = (TEXT_SIZES as readonly number[]).indexOf(current);
  const next = Math.min(TEXT_SIZES.length - 1, Math.max(0, (i < 0 ? 1 : i) + step));
  return TEXT_SIZES[next] ?? 100;
}
