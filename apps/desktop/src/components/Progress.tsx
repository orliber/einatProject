// How long Claude takes: an estimate learned from the last answers on this computer, and a
// progress line that says how far along a request is and about how long is left.
import { useEffect, useState } from "react";
import "./Progress.css";

export type Task = "draft" | "sort" | "consult";

/** Seconds a request usually takes before anything was measured, by answer speed. */
const DEFAULTS: Record<Task, Record<string, number>> = {
  draft: { fast: 15, balanced: 30, thorough: 70 },
  sort: { fast: 20, balanced: 20, thorough: 20 },
  consult: { fast: 10, balanced: 20, thorough: 45 },
};
const KEY = "dv.timing.v1";
const KEEP = 8;

type Store = Partial<Record<string, number[]>>;

function load(): Store {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as Store) : {};
  } catch {
    return {};
  }
}

/** Only durations are kept (no text, no case): nothing personal, per computer. */
export function recordDuration(task: Task, speed: string, ms: number) {
  if (!(ms > 0 && ms < 15 * 60_000)) return;
  try {
    const s = load();
    const k = `${task}:${speed}`;
    s[k] = [...(s[k] ?? []), ms].slice(-KEEP);
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // Private mode or storage off: the defaults are used.
  }
}

/** Expected milliseconds for one request (the median of the last ones, or the default). */
export function estimateMs(task: Task, speed: string, demo: boolean): number {
  if (demo) return 1500;
  const seen = [...(load()[`${task}:${speed}`] ?? [])].sort((a, b) => a - b);
  if (seen.length >= 2) return seen[Math.floor(seen.length / 2)] ?? 0;
  return (DEFAULTS[task][speed] ?? DEFAULTS[task].balanced ?? 30) * 1000;
}

export function clock(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** "about 20 seconds" / "about 2 minutes", for what is left. */
export function aboutLeft(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s <= 3) return "עוד רגע";
  if (s < 60) return `נותרו כ-${Math.max(5, Math.round(s / 5) * 5)} שניות`;
  const m = Math.round(s / 60);
  return m === 1 ? "נותרה כדקה" : `נותרו כ-${m} דקות`;
}

export function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(t);
  }, [active]);
  return now;
}

/** A bar that fills toward the estimate and then waits, never claiming to be done early. */
export function ProgressLine({ started, estimate, label }: { started: number; estimate: number; label: string }) {
  const now = useNow(true);
  const elapsed = now - started;
  const share = Math.min(0.95, elapsed / Math.max(estimate, 1));
  const over = elapsed > estimate;
  return (
    <div className="progress-line" role="status" aria-live="polite">
      <div className="progress-text">
        <span className="progress-label">{label}</span>
        <span className="progress-time">
          {clock(elapsed)} · {over ? "לוקח קצת יותר מהרגיל, עוד מעט" : aboutLeft(estimate - elapsed)}
        </span>
      </div>
      <div className="progress-track" aria-hidden="true">
        <div className={over ? "progress-fill waiting" : "progress-fill"} style={{ width: `${Math.round(share * 100)}%` }} />
      </div>
    </div>
  );
}
