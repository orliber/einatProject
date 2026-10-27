// "The layers of protection" (S4, L6; stage 5, D-078, in the approved design of round 50,
// D-087): a grid of small cards, one per layer, with what the program found just now; the
// layers that depend on her are marked "את". Above them the emergency stop, below them what
// each layer does and against what, and the same list as a PDF. The texts come from the core,
// which makes the PDF from the same list, so the page and the paper say the same thing.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ipc, type SecurityLayer, type SecurityStatus } from "../ipc/client";
import { useApp } from "../App";
import { ErrorLine } from "./ui";
import { timed, WaitLine } from "./Progress";

const COUNT = ["אפס", "אחת", "שתיים", "שלוש", "ארבע", "חמש", "שש", "שבע", "שמונה", "תשע", "עשר", "אחת-עשרה", "שתים-עשרה", "שלוש-עשרה", "ארבע-עשרה"];
const count = (n: number) => COUNT[n] ?? String(n);

/** Where she fixes a layer that is hers. */
const ACTION: Record<string, { label: string; pane: "backup" } | { label: string; below: string }> = {
  backup: { label: "לגבות", pane: "backup" },
  // A forgotten password is Google's (D-103): turned on in its card, just below on this page.
  forgot_password: { label: "להפעלה", below: "s-google" },
};

function Card({ l, you }: { l: SecurityLayer; you: boolean }) {
  const { go } = useApp();
  const warn = l.state === "warn";
  // Google needs her only while it is off (D-103).
  const action = you && (l.key !== "forgot_password" || warn) ? ACTION[l.key] : undefined;
  return (
    <li className={["layer", warn ? "warn" : l.state === "info" ? "info" : "ok", you ? "you" : ""].filter(Boolean).join(" ")}>
      <span className="layer-mark" aria-hidden="true">{you ? "את" : warn ? "!" : l.state === "info" ? "i" : "✓"}</span>
      <span className="visually-hidden">{warn ? "לבדוק: " : l.state === "ok" ? "תקין: " : "לידיעה: "}</span>
      <div className="layer-text">
        <strong>{l.title}</strong>
        <span className="small muted">{l.now}</span>
      </div>
      {action && (
        <button type="button" className="link-btn small layer-when" onClick={() => {
          if ("pane" in action) go({ name: "settings", pane: action.pane });
          else document.getElementById(action.below)?.scrollIntoView({ block: "start" });
        }}>{action.label}</button>
      )}
    </li>
  );
}

export function SecuritySettings() {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["security"], queryFn: () => timed("check", ipc.securityStatus(), "security") });
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  async function stop(stopped: boolean) {
    setError(null);
    try {
      qc.setQueryData<SecurityStatus>(["security"], await ipc.setSendStopped(stopped));
    } catch (e) {
      setError(fail(e as never));
    }
  }

  async function savePdf() {
    setError(null);
    setSaving(true);
    try {
      // The path is isolated, so its slashes do not mix with the Hebrew around it.
      notify(`נשמר: ⁦${await ipc.exportSecurityPdf()}⁩`);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setSaving(false);
    }
  }

  const s = q.data;
  const all = s ? [...s.layers, ...s.yours] : [];
  const bad = all.filter((l) => l.state === "warn");
  const title = !s ? "שכבות ההגנה"
    : bad.length === 0 ? `${count(all.length)} שכבות הגנה, כולן תקינות`
      : `${all.length - bad.length} מתוך ${all.length} תקינות: ${bad.map((l) => l.title).join(", ")}`;
  // The stop is always there, also while the list loads or failed to load (review of
  // stage 5, U7). Sending cannot be turned back on while the gate or the rules fail (U1).
  const stopped = s?.send_stopped ?? false;
  const broken = s?.layers.some((l) => (l.key === "privacy_gate" || l.key === "send_rules") && l.state === "warn") ?? false;
  return (
    <section className="card setting" aria-labelledby="s-sec">
      <div className="layers-head">
        <div>
          <h2 id="s-sec">{title}</h2>
          {s && (
            <span className="hint">
              השכבות שכתוב בהן "נבדק עכשיו" נוסו בידי התוכנה ממש עכשיו; בשאר מוצג המצב הנוכחי. {s.yours.length === 1 ? "אחת תלויה בך." : `${count(s.yours.length)} תלויות בך.`}
            </span>
          )}
        </div>
        <button type="button" className="btn btn-small" disabled={q.isFetching} onClick={() => { void qc.invalidateQueries({ queryKey: ["security"] }); }}>
          {q.isFetching && !q.isPending ? "בודקת…" : "לבדוק שוב"}
        </button>
      </div>
      <div className="row">
        <p className="hint grow">
          {s ? `עצירת חירום מכבה כל שליחה ל-AI עד שמדליקים אותה מחדש כאן. עד ${s.daily_limit} בקשות ביום, ולא יותר מ-${s.max_request_kb} KB לבקשה.` : "עצירת חירום מכבה כל שליחה ל-AI."}
        </p>
        <button type="button" className={stopped ? "btn" : "btn btn-danger"} disabled={stopped && broken}
          title={stopped && broken ? "השער או כללי השליחה לא עברו את הבדיקה, ולכן אי אפשר להפעיל שליחה." : undefined}
          onClick={() => void stop(!stopped)}>
          {stopped ? "להפעיל שליחה מחדש" : "עצירת חירום: לכבות שליחה"}
        </button>
      </div>
      {stopped && broken && <p className="hint">אי אפשר להפעיל שליחה מחדש: השער או כללי השליחה לא עברו את הבדיקה עכשיו. כדאי לפנות לאור.</p>}
      {q.isFetching && <WaitLine task="check" speed="security" delay={300} label="בודקת עכשיו את שכבות ההגנה" />}
      {q.isError && <ErrorLine error={fail(q.error as never)} retry={() => void qc.invalidateQueries({ queryKey: ["security"] })} />}
      {s && (
        <ul className="layers">
          {s.layers.map((l) => <Card key={l.key} l={l} you={false} />)}
          {s.yours.map((l) => <Card key={l.key} l={l} you />)}
        </ul>
      )}
      {s && (
        <details className="layers-more">
          <summary>מה כל שכבה עושה, ומפני מה</summary>
          <dl className="layers-what">
            {all.map((l) => (
              <div key={l.key}>
                <dt>{l.title}</dt>
                <dd>{l.what} מגינה מפני: {l.against}</dd>
              </div>
            ))}
          </dl>
          <p className="hint">שכבה של התוכנה שנכשלת עוצרת את השליחה מיד, ושום בקשה לא יוצאת עד שאור בודק. שכבה שתלויה בך מסומנת "את", וכשצריך לתקן אותה כתוב בה מה עושים ואיפה.</p>
        </details>
      )}
      {s && (
        <div className="row">
          <p className="hint grow">אותו עמוד כקובץ PDF, להורים או למכון. אין בו שום פרט על ילד.</p>
          <button type="button" className="btn" disabled={saving} onClick={() => void savePdf()}>
            {saving ? "שומרת…" : "לשמור כ-PDF"}
          </button>
        </div>
      )}
      <ErrorLine error={error} />
    </section>
  );
}
