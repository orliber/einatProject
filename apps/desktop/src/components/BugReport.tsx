import { useContext, useEffect, useState } from "react";
import { AppContext, type Route } from "../App";
import { ipc, type UiError } from "../ipc/client";
import { Dialog, Skeleton } from "./ui";

/** What a bug report carries (U6): nothing from a case, only what Or needs to find the fault. */
export interface BugFacts {
  version: string;
  commit: string;
  platform: string;
  /** Where she was: the screen and, in a case, the tab. Never the case itself. */
  screen: string;
  failure: Pick<UiError, "code" | "details"> | null;
  crashes: string[];
}

const SCREENS: Record<Route["name"], string> = {
  cases: "רשימת התיקים",
  case: "תיק",
  consult: "התייעצות",
  style: "הסגנון שלי",
  settings: "הגדרות",
};

export function screenName(route: Route | undefined): string {
  if (!route) return "לא ידוע";
  return route.name === "case" ? `${SCREENS.case} (לשונית ${route.view})` : SCREENS[route.name];
}

/** The text she sees, may edit, and copies. Plain text only. */
export function bugReportText(f: BugFacts): string {
  const lines = [
    "דוח תקלה מכספת האבחון",
    `גרסה: ${f.version} (${f.commit})`,
    `מערכת: ${f.platform}`,
    `מסך: ${f.screen}`,
    `שעה: ${new Date().toISOString()}`,
  ];
  if (f.failure) {
    lines.push(`קוד שגיאה: ${f.failure.code}`);
    if (f.failure.details.length) lines.push("פרטים:", ...f.failure.details);
  } else {
    lines.push("קוד שגיאה: אין");
  }
  lines.push(f.crashes.length ? "קריסות אחרונות:" : "קריסות אחרונות: אין", ...f.crashes);
  return lines.join("\n");
}

/**
 * The report for Or, shown in full before it is copied. She can delete anything in it first;
 * only what is in the box is copied.
 */
export function BugReportDialog({ failure, onClose }: { failure: BugFacts["failure"]; onClose: () => void }) {
  const app = useContext(AppContext);
  const [text, setText] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const screen = screenName(app?.route);
  useEffect(() => {
    let live = true;
    void Promise.all([
      ipc.ping().catch(() => null),
      ipc.recentCrashes().catch(() => [] as string[]),
    ]).then(([p, crashes]) => {
      if (!live) return;
      setText(bugReportText({
        version: p?.core_version ?? "לא ידוע",
        commit: p?.build_commit ?? "לא ידוע",
        platform: p?.platform ?? "לא ידוע",
        screen,
        failure,
        crashes,
      }));
    });
    return () => { live = false; };
  }, [failure, screen]);
  return (
    <Dialog title="דוח תקלה לאור" onClose={onClose}
      subtitle="זה כל מה שיועתק. אין בו שום דבר מהתיקים: רק הגרסה, המסך, קוד השגיאה והקריסות האחרונות."
      footer={
        <>
          <span className="grow muted small">{copied ? "הועתק. אפשר להדביק במייל או בהודעה לאור." : ""}</span>
          <button type="button" className="btn" onClick={onClose}>סגירה</button>
          <button type="button" className="btn btn-primary" disabled={text === null}
            onClick={() => void navigator.clipboard.writeText(text ?? "").then(() => setCopied(true), () => undefined)}>
            העתקה
          </button>
        </>
      }>
      {text === null ? (
        <Skeleton lines={6} label="מכין את הדוח" />
      ) : (
        <>
          <p className="hint">אם בכל זאת מופיע כאן שם או פרט מזהה, אפשר למחוק אותו כאן לפני ההעתקה.</p>
          <textarea className="textarea bug-report" dir="auto" rows={12} aria-label="תוכן הדוח" spellCheck={false}
            value={text} onChange={(e) => { setText(e.target.value); setCopied(false); }} />
        </>
      )}
    </Dialog>
  );
}

/** In settings: the same report, when something went wrong without an error on screen. */
export function BugReportSettings() {
  const [open, setOpen] = useState(false);
  return (
    <section className="card setting" aria-labelledby="s-bug">
      <h2 id="s-bug">משהו לא עובד?</h2>
      <p className="hint">דוח קצר לאור, בלי שום דבר מהתיקים. רואים אותו לפני ההעתקה.</p>
      <button type="button" className="btn align-start" onClick={() => setOpen(true)}>דוח תקלה לאור</button>
      {open && <BugReportDialog failure={null} onClose={() => setOpen(false)} />}
    </section>
  );
}
