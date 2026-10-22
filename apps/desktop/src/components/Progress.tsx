// How long Claude takes: an estimate learned from the last answers on this computer, and a
// progress line that says how far along a request is and about how long is left.
import { useEffect, useState } from "react";
import { ipc } from "../ipc/client";
import "./Progress.css";

/** Requests to Claude ("draft", "sort", "consult"), and the waits that stay on this computer. */
export type Task = "draft" | "sort" | "consult" | LocalTask;
/** Waits here, measured per computer: preparing a request, reading a file, a backup… */
export type LocalTask = "prepare" | "read" | "check" | "export" | "backup" | "backup-check" | "restore" | "unlock" | "create" | "update";

/** Seconds a wait usually takes before anything was measured: by answer speed for Claude,
 *  under "any" for the waits on this computer. A first guess, said as one ("בדרך כלל"). */
const DEFAULTS: Record<Task, Partial<Record<string, number>>> = {
  draft: { fast: 15, balanced: 30, thorough: 70 },
  sort: { fast: 20, balanced: 20, thorough: 20 },
  consult: { fast: 10, balanced: 20, thorough: 45 },
  prepare: { any: 4 },
  read: { small: 4, medium: 10, large: 30, any: 8 },
  check: { any: 3 },
  export: { any: 6 },
  backup: { any: 10 },
  "backup-check": { any: 6 },
  restore: { any: 12 },
  unlock: { any: 3 },
  create: { any: 5 },
  update: { any: 45 },
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
export function estimateMs(task: Task, speed = "any", demo = false): number {
  if (demo) return 1500;
  const seen = [...(load()[`${task}:${speed}`] ?? [])].sort((a, b) => a - b);
  if (seen.length >= 2) return seen[Math.floor(seen.length / 2)] ?? 0;
  const d = DEFAULTS[task];
  return (d[speed] ?? d.balanced ?? d.any ?? 30) * 1000;
}

/** A file's size, in the few groups its reading time is kept by. */
export function sizeBucket(bytes: number): string {
  return bytes < 300_000 ? "small" : bytes < 3_000_000 ? "medium" : "large";
}

/** Whether the estimate comes from this computer's own past waits, or is still the first guess. */
export function learned(task: Task, speed = "any"): boolean {
  return (load()[`${task}:${speed}`]?.length ?? 0) >= 2;
}

/** Waits for `work` and keeps how long it took when it went well (a failure says nothing
 *  about the next time), so the next wait of this kind can say how long is left. */
export async function timed<T>(task: Task, work: Promise<T>, speed = "any"): Promise<T> {
  const started = Date.now();
  const result = await work;
  recordDuration(task, speed, Date.now() - started);
  return result;
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

/** How many words Claude has written so far, asked once a second while a request is on its way. */
export function useWords(approval: string | undefined): number {
  const [words, setWords] = useState(0);
  useEffect(() => {
    if (!approval) return;
    let alive = true;
    const ask = () => ipc.sendProgress(approval).then((n) => alive && setWords(n)).catch(() => undefined);
    const t = setInterval(() => void ask(), 1000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [approval]);
  return words;
}

/** "כ-120 מילים עד עכשיו", once there are any. */
export function wordsSoFar(n: number): string {
  if (n < 5) return "";
  return `נכתבו כ-${Math.round(n / 10) * 10 || n} מילים`;
}

/** "about 20 seconds" / "about 2 minutes", for a whole wait. */
export function aboutTotal(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `כ-${Math.max(5, Math.round(s / 5) * 5)} שניות`;
  const m = Math.round(s / 60);
  return m === 1 ? "כדקה" : `כ-${m} דקות`;
}

/** What is left, said honestly: from past waits, a first guess, or past the usual time. */
export function leftText(elapsed: number, estimate: number, known = true): string {
  if (elapsed > estimate) return known ? "לוקח קצת יותר מהרגיל, עוד מעט" : "עדיין עובדת, בפעם הראשונה זה לוקח יותר";
  return known ? aboutLeft(estimate - elapsed) : `בדרך כלל ${aboutTotal(estimate)}`;
}

/** A bar that fills toward the estimate and then waits, never claiming to be done early. */
export function ProgressLine({ started, estimate, label, approval, known = true, small = false }: { started: number; estimate: number; label: string; approval?: string | undefined; known?: boolean; small?: boolean }) {
  const now = useNow(true);
  const words = useWords(approval);
  const elapsed = now - started;
  const share = Math.min(0.95, elapsed / Math.max(estimate, 1));
  const over = elapsed > estimate;
  return (
    <div className={small ? "progress-line small" : "progress-line"} role="status" aria-live="polite">
      <div className="progress-text">
        <span className="progress-label">{label}</span>
        <span className="progress-time">
          {clock(elapsed)} · {words >= 5 ? `${wordsSoFar(words)}${over ? "" : ` · ${aboutLeft(estimate - elapsed)}`}` : leftText(elapsed, estimate, known)}
        </span>
      </div>
      <div className="progress-track" aria-hidden="true">
        <div className={over ? "progress-fill waiting" : "progress-fill"} style={{ width: `${Math.round(share * 100)}%` }} />
      </div>
    </div>
  );
}

/**
 * The one wait line for every wait in the app: what is happening now, the time so far and
 * about how long is left, learned from this computer's past waits of the same kind. Shown
 * only after a moment, so a quick step does not flash a bar. `started` is when the wait began
 * (the default: when the line appeared), so a line shown again keeps counting. Before this
 * kind of wait was measured twice, it says "בדרך כלל" and never pretends to know.
 */
export function WaitLine({ task, label, speed = "any", demo = false, started, approval, delay = 700, guess }: { task: Task; label: string; speed?: string; demo?: boolean; started?: number; approval?: string | undefined; delay?: number; guess?: number }) {
  const [since] = useState(() => started ?? Date.now());
  // `guess`: a first guess from what is known of this wait (a download's size), until it was measured.
  const [estimate] = useState(() => (guess !== undefined && !learned(task, speed) ? guess : estimateMs(task, speed, demo)));
  const [known] = useState(() => demo || learned(task, speed));
  const [shown, setShown] = useState(delay <= 0);
  useEffect(() => {
    if (shown) return;
    const t = setTimeout(() => setShown(true), Math.max(0, delay - (Date.now() - since)));
    return () => clearTimeout(t);
  }, [shown, delay, since]);
  if (!shown) return <span className="visually-hidden" role="status">{label}</span>;
  return <ProgressLine started={since} estimate={estimate} label={label} approval={approval} known={known} small />;
}

/** "45% · נותרו כ-2 דקות", for a download that says how many bytes came: by its own pace. */
export function byPace(started: number, now: number, done: number, total: number): string {
  if (total <= 0) return "";
  const percent = Math.floor((done * 100) / total);
  const elapsed = now - started;
  if (done <= 0 || elapsed < 3000) return `${percent}%`;
  return `${percent}% · ${aboutLeft(((total - done) * elapsed) / done)}`;
}
