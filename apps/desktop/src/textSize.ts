// Text size for long hours at the report: the whole window scales (Ctrl + / Ctrl − / Ctrl 0,
// or Settings). Kept on this computer only; it holds nothing about any case.
const KEY = "dv.textSize";
export const TEXT_SIZES = [90, 100, 110, 125, 140] as const;

export function readTextSize(): number {
  try {
    const n = Number(localStorage.getItem(KEY));
    return (TEXT_SIZES as readonly number[]).includes(n) ? n : 100;
  } catch {
    return 100;
  }
}

export function applyTextSize(percent: number): void {
  document.documentElement.style.setProperty("zoom", percent === 100 ? "" : `${percent}%`);
  try {
    localStorage.setItem(KEY, String(percent));
  } catch {
    // Not kept: applies until the window closes.
  }
}

/** One step larger (+1) or smaller (−1), within the list. */
export function stepTextSize(current: number, step: 1 | -1): number {
  const i = (TEXT_SIZES as readonly number[]).indexOf(current);
  const next = Math.min(TEXT_SIZES.length - 1, Math.max(0, (i < 0 ? 1 : i) + step));
  return TEXT_SIZES[next] ?? 100;
}
