// "בדיקת מערכת" (D-148), in Settings → עדכונים ועזרה: one button runs safe self-tests one after
// another, and each line says ✓ or ✗, why, and what to do. Nothing from a case is read, and the
// connection check is a secure handshake only, with nothing sent. "העתקה לאור" copies a short
// technical summary in plain ASCII: no paths, no content, no keys.
import { useRef, useState, useSyncExternalStore } from "react";
import { ipc, type CheckLine, type CheckState, type SystemCheck, type SystemCheckResult, type UiError } from "../ipc/client";
import { Spinner } from "./ui";
import "./SystemCheck.css";

/** The checks, in the order they run, with the name shown while each runs. */
export const CHECKS: { id: SystemCheck; label: string }[] = [
  { id: "reading", label: "קריאת קבצים" },
  { id: "ocr", label: "זיהוי טקסט בסריקות" },
  { id: "folders", label: "שמירה בתיקיות" },
  { id: "connection", label: "חיבור לשירות ה-AI" },
  { id: "basics", label: "פרטי המערכת" },
];

const MARK: Record<CheckState, string> = { ok: "✓", failed: "✗", warn: "!", info: "i", skipped: "–" };
const SAID: Record<CheckState, string> = { ok: "תקין", failed: "לא תקין", warn: "לתשומת לב", info: "מידע", skipped: "לא נבדק" };

/** A check that could not run at all (the core refused, the window lost it): one failed line. */
function notRun(check: SystemCheck, e: unknown): SystemCheckResult {
  const code = (e as Partial<UiError> | null)?.code ?? "internal";
  const label = CHECKS.find((c) => c.id === check)?.label ?? check;
  return {
    check,
    lines: [{
      key: "not_run",
      label,
      state: "failed",
      reason: code === "locked" ? "הכספת ננעלה באמצע הבדיקה." : "הבדיקה עצמה לא הצליחה לרוץ.",
      next_step: code === "locked" ? "פותחים את הכספת ובודקים שוב." : "בודקים שוב; אם זה חוזר, לוחצים \"העתקה לאור\".",
      detail: `${check}: not run (${code})`,
    }],
  };
}

/** ASCII only, and nothing from the first path separator on (as the core cuts it, D-120). */
function plain(s: string): string {
  const cut = s.search(/[/\\]/);
  const head = cut === -1 ? s : `${s.slice(0, cut)}<path left out>`;
  return head.replace(/[^\x20-\x7E]/g, "").slice(0, 220);
}

/** What "העתקה לאור" copies: the check, the line's key, its state and its technical detail. */
export function systemCheckSummary(results: SystemCheckResult[], at: Date = new Date()): string {
  const lines = ["Diagnostic Vault system check", `time: ${at.toISOString()}`];
  for (const r of results) {
    for (const l of r.lines) {
      lines.push(plain(`${r.check}.${l.key}: ${l.state} | ${l.detail}`));
    }
  }
  return lines.join("\n");
}

function Line({ line }: { line: CheckLine }) {
  return (
    <li className={`syscheck-line ${line.state}`}>
      <span className="syscheck-mark" aria-hidden="true">{MARK[line.state]}</span>
      <div className="syscheck-text">
        <span className="syscheck-label">
          {line.label}
          <span className="visually-hidden"> ({SAID[line.state]})</span>
        </span>
        <span className="hint">{line.reason}</span>
        {line.next_step && <span className="syscheck-next"><b>מה לעשות:</b> {line.next_step}</span>}
      </div>
    </li>
  );
}

// The run belongs to the program, not to the pane (UIA-8): it goes on while she works
// elsewhere, and the pane shows where it is, or what it found, when she comes back. In memory
// only; a lock reloads the page and it is gone.
interface Run {
  results: SystemCheckResult[];
  /** The check running now, or null. */
  running: SystemCheck | null;
}
let run: Run = { results: [], running: null };
const listeners = new Set<() => void>();
function setRun(next: Run) {
  run = next;
  listeners.forEach((l) => l());
}
function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** For the tests: each starts with no run. */
export function forgetSystemCheck() {
  run = { results: [], running: null };
}

/** Runs the checks one after another; a second start while one runs does nothing. */
async function runChecks(): Promise<void> {
  if (run.running) return;
  setRun({ results: [], running: CHECKS[0]?.id ?? null });
  for (const c of CHECKS) {
    setRun({ ...run, running: c.id });
    let r: SystemCheckResult;
    try {
      r = await ipc.systemCheck(c.id);
    } catch (e) {
      r = notRun(c.id, e);
    }
    setRun({ ...run, results: [...run.results, r] });
  }
  setRun({ ...run, running: null });
}

export function SystemCheckSettings() {
  const { results, running } = useSyncExternalStore(subscribe, () => run);
  const [copied, setCopied] = useState(false);
  const doneRef = useRef<HTMLParagraphElement>(null);

  async function start() {
    if (running) return;
    setCopied(false);
    await runChecks();
    // The result is said, and the keyboard lands on it (when she is still here).
    requestAnimationFrame(() => doneRef.current?.focus());
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(systemCheckSummary(results));
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  const step = running ? CHECKS.findIndex((c) => c.id === running) : -1;
  const all = results.flatMap((r) => r.lines);
  const bad = all.filter((l) => l.state === "failed").length;
  const attention = all.filter((l) => l.state === "warn").length;
  const finished = running === null && results.length === CHECKS.length;
  return (
    <section className="card setting" aria-labelledby="s-syscheck">
      <h2 id="s-syscheck">בדיקת מערכת</h2>
      <p className="hint">
        בודקת שהתוכנה יכולה לקרוא קבצים, לשמור בתיקיות ולהתחבר לשירות ה-AI במחשב הזה. היא לא נוגעת בתיקים
        ולא שולחת שום מידע: בחיבור נבדקת רק לחיצת יד מאובטחת. אפשר להמשיך לעבוד בזמן הבדיקה, והתוצאות מחכות כאן.
      </p>
      <div className="row syscheck-actions">
        <button type="button" className="btn" onClick={() => void start()} disabled={running !== null}>
          {running ? "בודקת…" : results.length ? "לבדוק שוב" : "בדיקת מערכת"}
        </button>
        {finished && (
          <button type="button" className="btn" onClick={() => void copy()}>העתקה לאור</button>
        )}
      </div>
      <div role="status" aria-live="polite" className="syscheck-status">
        {running && (
          <span className="syscheck-progress">
            <Spinner /> בודקת {step + 1} מתוך {CHECKS.length}: {CHECKS[step]?.label}
          </span>
        )}
        {copied && <span className="hint">הועתק. אין בו שום דבר מהתיקים, נתיבים או מפתחות; אפשר להדביק במייל או בהודעה לאור.</span>}
      </div>
      {running && <progress className="syscheck-meter" value={step} max={CHECKS.length} aria-hidden="true" />}
      {finished && (
        <p ref={doneRef} tabIndex={-1} className={bad ? "syscheck-done bad" : "syscheck-done"}>
          {bad === 1
            ? "בדיקה אחת לא עברה. לידה כתוב מה לעשות."
            : bad
            ? `${bad} בדיקות לא עברו. ליד כל אחת כתוב מה לעשות.`
            : attention
              ? "הכל עובד. יש כמה דברים לתשומת לב."
              : "הכל עובד."}
        </p>
      )}
      {results.length > 0 && (
        <div className="syscheck-groups">
          {results.map((r) => (
            <div key={r.check} className="syscheck-group">
              <h3 className="syscheck-group-title">{CHECKS.find((c) => c.id === r.check)?.label}</h3>
              <ul className="syscheck-list">
                {r.lines.map((l) => <Line key={l.key} line={l} />)}
              </ul>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
