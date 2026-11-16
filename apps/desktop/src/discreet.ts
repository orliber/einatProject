// Discreet mode (D-192): on while a family in the room may see the screen. One switch in the
// top bar, for both parts of the program: every list and heading shows a person's initials
// in place of the name. The switch stays where she left it, also after the vault locks and
// the page reloads, so a lock in the middle of a meeting does not bring the names back.
// What is kept is one word ("on"), never a name.
import { useMemo, useSyncExternalStore } from "react";

const KEY = "dv.discreet";

function stored(): boolean {
  try {
    return localStorage.getItem(KEY) === "on";
  } catch {
    return false;
  }
}

let on = stored();
const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function isDiscreet(): boolean {
  return on;
}

export function setDiscreet(next: boolean): void {
  on = next;
  try {
    if (next) localStorage.setItem(KEY, "on");
    else localStorage.removeItem(KEY);
  } catch {
    // Storage refused: the switch still works until the program closes.
  }
  listeners.forEach((l) => l());
}

/** For the tests: each starts with discreet mode off. */
export function forgetDiscreet(): void {
  setDiscreet(false);
}

export function useDiscreet(): boolean {
  return useSyncExternalStore(subscribe, isDiscreet, isDiscreet);
}

/** "נועה לוי" → "נ. ל.": the first letter of each of the first three words. */
export function initials(name: string): string {
  const letters = name.split(/\s+/).filter(Boolean).slice(0, 3).map((word) => `${Array.from(word)[0] ?? ""}.`);
  return letters.join(" ");
}

/** A name as the screen may show it now: whole, or initials in discreet mode. */
export function shownName(name: string, discreet: boolean): string {
  const full = name.trim();
  return discreet && full !== "" ? initials(full) : full;
}

/** `shownName` that follows the switch. */
export function useShownName(): (name: string) => string {
  const discreet = useDiscreet();
  return useMemo(() => (name: string) => shownName(name, discreet), [discreet]);
}
