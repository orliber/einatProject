import { useMemo, useState, useSyncExternalStore } from "react";

/**
 * Screen state that outlives the screen: a request to the AI keeps going when she moves to
 * another screen or section, and its question, answer and notes are there when she comes
 * back. In memory only (never written anywhere), and emptied when the vault locks.
 */
const kept = new Map<string, unknown>();
const listeners = new Set<() => void>();
/** Grows on every lock: an answer that lands after the lock is not kept. */
let generation = 0;

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function emit() {
  for (const fn of listeners) fn();
}

export type SetKept<T> = (next: T | ((prev: T) => T)) => void;

/** Read and change a kept value. The setter still works after the screen has closed. */
export function setKept<T>(key: string, initial: T): SetKept<T> {
  const born = generation;
  return (next) => {
    if (born !== generation) return;
    const prev = (kept.has(key) ? kept.get(key) : initial) as T;
    const value = typeof next === "function" ? (next as (p: T) => T)(prev) : next;
    if (Object.is(value, prev) && kept.has(key)) return;
    kept.set(key, value);
    emit();
  };
}

export function getKept<T>(key: string, initial: T): T {
  // The first read stores the default, so every later read returns the same value.
  if (!kept.has(key)) kept.set(key, initial);
  return kept.get(key) as T;
}

/** Like `useState`, but the value stays while the app is unlocked. */
export function useKept<T>(key: string, initial: T): [T, SetKept<T>] {
  // `initial` is only a default: the first one counts.
  const [first] = useState(initial);
  const value = useSyncExternalStore(subscribe, () => getKept(key, first));
  const set = useMemo(() => setKept(key, first), [key, first]);
  return [value, set];
}

/** Read a kept value without giving it a default: `undefined` until a screen keeps one. */
export function useKeptValue<T>(key: string): T | undefined {
  return useSyncExternalStore(subscribe, () => kept.get(key) as T | undefined);
}

/** Forget one kept value: the next screen that reads it starts from its own default. */
export function forgetKept(key: string) {
  if (kept.delete(key)) emit();
}

/** On lock: nothing of a case stays in memory. */
export function clearKept() {
  generation += 1;
  if (kept.size === 0) return;
  kept.clear();
  emit();
}
