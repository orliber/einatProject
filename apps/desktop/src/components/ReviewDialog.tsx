import { useState } from "react";
import { useApp } from "../App";
import { ipc, type Prepared } from "../ipc/client";
import type { SuspectDecision } from "../ipc/generated/SuspectDecision";
import { AutoHiddenCard } from "./AutoHiddenCard";
import { Dialog, ErrorLine, Segments, ShieldIcon, Spinner } from "./ui";
import "./ReviewDialog.css";
import { useAi } from "../ai";

/** What each block means and what to do next, in her words. */
const guidance: Record<string, string> = {
  unknown_name: "נראה כמו שם בלי תפקיד ידוע. אפשר להכריע כאן.",
  past_report_name: "המילה מופיעה כשם בדוח ישן. אפשר להכריע כאן.",
  foreign_tag: "תגית שנשארה מפעם קודמת. הכפתור מחליף אותה במילה רגילה ובודק שוב.",
  identity: "שם מהתיק לא הוסתר. הכפתור פותח את פרטי התיק לבדיקה.",
  pattern: "יש פרט מזהה (ת\"ז, טלפון, מייל) בטקסט. מסירים אותו בעריכה.",
  too_large: "הבקשה גדולה מדי. מקצרים, או מסמנים פחות חומרים.",
};

/**
 * "What leaves the computer": both sides, the checks, and the card of what was hidden without
 * asking (D-042: no questions). Sending is possible only with an approval id (the gate
 * cleared this exact payload).
 */
export function ReviewDialog(props: {
  title: string;
  caseId: string | null;
  prepared: Prepared;
  /** Build the request again after decisions. */
  reprepare: () => Promise<Prepared>;
  onSend: (approvalId: string) => Promise<void>;
  onClose: () => void;
}) {
  const { status, fail, refresh, go } = useApp();
  const ai = useAi();
  const [prepared, setPrepared] = useState(props.prepared);
  const [busy, setBusy] = useState<"send" | "decide" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const c = prepared.checks;

  async function rebuild() {
    setPrepared(await props.reprepare());
  }

  async function send() {
    if (!prepared.approval_id) return;
    setBusy("send");
    setError(null);
    try {
      await props.onSend(prepared.approval_id);
    } catch (e) {
      setError(fail(e as never));
      setBusy(null);
    }
  }

  const blocked = prepared.blocked;

  /** One block, one answer: she says what the flagged word is and the request is rebuilt (F1). */
  async function decide(token: string, decision: SuspectDecision) {
    if (!props.caseId) return;
    setBusy("decide");
    setError(null);
    try {
      await ipc.decideSuspect(props.caseId, token, decision);
      await rebuild();
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(null);
    }
  }

  /** A tag left in the case's text: it becomes its plain word everywhere, and the check runs again. */
  async function settleTags() {
    if (!props.caseId) return;
    setBusy("decide");
    setError(null);
    try {
      await ipc.settleForeignTags(props.caseId);
      await rebuild();
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(null);
    }
  }

  return (
    <Dialog title={`לפני שליחה ל-${ai}`}
      subtitle={<>{props.title} · שום דבר לא נשלח עד שתלחצי "שליחה"</>}
      icon={<span className="review-shield"><ShieldIcon /></span>}
      onClose={props.onClose}
      footer={
        <>
          <span className="small muted grow">
            {/* Shown only because she asked to see every request (advanced); off, requests go at once. */}
            <label className="row">
              <input type="checkbox" checked={status.review_always}
                onChange={(e) => void ipc.setReviewAlways(e.target.checked).then(refresh)} />
              להציג את המסך הזה לפני כל שליחה
            </label>
          </span>
          {prepared.demo_mode && <span className="chip chip-sand">מצב הדגמה: שום דבר לא יוצא מהמחשב</span>}
          <button type="button" className="btn" onClick={props.onClose}>חזרה לעריכה</button>
          <button type="button" className="btn btn-primary btn-send" disabled={!prepared.approval_id || busy !== null} onClick={() => void send()}>
            {busy === "send" ? <><Spinner /> שולחת…</> : prepared.demo_mode ? "שליחה (הדגמה)" : `שליחה ל-${ai}`}
          </button>
        </>
      }>
      <div className="stack review">
        <AutoHiddenCard items={prepared.auto_hidden} caseId={props.caseId} onChanged={rebuild} disabled={busy !== null} />

        {blocked.map((b, i) => (
          <div key={i} className="stack" style={{ gap: 6 }}>
            <p className="error">{b.message}{b.detail ? `: ${b.detail}` : ""}</p>
            {guidance[b.code] && <span className="small muted">{guidance[b.code]}</span>}
            {props.caseId && b.detail && (b.code === "unknown_name" || b.code === "past_report_name") && (
              <div className="row">
                <button type="button" className="btn btn-small" disabled={busy !== null}
                  onClick={() => void decide(b.detail as string, { decision: "hide", role: "other" })}>
                  זה שם: להסתיר ולהמשיך
                </button>
                <button type="button" className="btn btn-small" disabled={busy !== null}
                  onClick={() => void decide(b.detail as string, { decision: "not_a_name" })}>
                  זה לא שם: להמשיך
                </button>
              </div>
            )}
            {props.caseId && b.code === "foreign_tag" && (
              <div className="row">
                <button type="button" className="btn btn-small" disabled={busy !== null} onClick={() => void settleTags()}>
                  להחליף במילה רגילה ולהמשיך
                </button>
              </div>
            )}
            {props.caseId && b.code === "identity" && (
              <div className="row">
                <button type="button" className="btn btn-small" disabled={busy !== null}
                  onClick={() => { const id = props.caseId as string; props.onClose(); go({ name: "case", id, view: "details" }); }}>
                  לפרטי התיק
                </button>
              </div>
            )}
            {!["unknown_name", "past_report_name", "foreign_tag", "identity"].includes(b.code) && (
              <div className="row">
                <button type="button" className="btn btn-small" onClick={props.onClose}>חזרה לעריכה</button>
              </div>
            )}
          </div>
        ))}
        {blocked.length > 0 && (
          <p className="note-sand small">
            השליחה נעצרה להגנת הפרטיות, ושום דבר לא יצא. ליד כל עצירה יש כפתור לתיקון.
            אם זה לא עוזר, מעתיקים את ההודעה ושולחים לאור.
          </p>
        )}
        <ErrorLine error={error} />

        <ul className="review-checks">
          <li><span aria-hidden="true">✓</span>{c.declared_names ? `${c.declared_names} שמות יוחלפו בתפקיד ("הילד", "האם")` : "אין שמות מהתיק בטקסט"}</li>
          <li><span aria-hidden="true">✓</span>{c.patterns ? `${c.patterns} פרטים מזהים יוחלפו (תאריך: "לפני כחודש")` : "אין ת\"ז, טלפון או תאריך"}</li>
          {prepared.approval_id && <li><span aria-hidden="true">✓</span>הבדיקה האחרונה עברה. אפשר לשלוח.</li>}
        </ul>

        <details className="full-text" open={prepared.parts.length <= 2}>
          <summary>להציג בדיוק מה יוצא מהמחשב</summary>
          <div className="review-cols">
            <section className="card review-col" aria-label="מה שכתוב בתיק">
              <h4 className="col-title">מה שכתוב בתיק (נשאר במחשב)</h4>
              {prepared.parts.map((p, i) => (
                <div key={i} className="review-part">
                  <span className="small muted">{p.label}</span>
                  <p className="serif review-text"><Segments segments={p.original} side="original" /></p>
                </div>
              ))}
            </section>
            <section className="card review-col" aria-label={`מה ${ai} יקבל`}>
              <h4 className="col-title col-out">מה {ai} יקבל</h4>
              {prepared.parts.map((p, i) => (
                <div key={i} className="review-part">
                  <span className="small muted">{p.label}</span>
                  <p className="serif review-text"><Segments segments={p.outgoing} side="outgoing" /></p>
                </div>
              ))}
            </section>
          </div>
        </details>
      </div>
    </Dialog>
  );
}
