// Suggestions that change now and then (D-036): a few at a time from a longer list, so the
// ideas stay fresh without crowding the screen. Paused while the window is hidden, and while
// `paused` (the mouse is on them, or she is typing).
import { useEffect, useState } from "react";

/** `count` items from `pool`, moving to the next ones every `ms`. */
export function useRotating<T>(pool: readonly T[], count = 1, ms = 7000, paused = false): T[] {
  const [start, setStart] = useState(() => Math.floor(Math.random() * Math.max(pool.length, 1)));
  useEffect(() => {
    if (paused || pool.length <= count) return;
    const t = setInterval(() => {
      if (!document.hidden) setStart((s) => (s + count) % pool.length);
    }, ms);
    return () => clearInterval(t);
  }, [pool.length, count, ms, paused]);
  if (pool.length === 0) return [];
  return Array.from({ length: Math.min(count, pool.length) }, (_, i) => pool[(start + i) % pool.length] as T);
}

/** One changing "למשל: …" line for an empty box. */
export function useRotatingPlaceholder(pool: readonly string[], typing: boolean): string {
  return `למשל: ${useRotating(pool, 1, 6000, typing)[0] ?? ""}`;
}
