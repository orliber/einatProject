import { useState } from "react";
import { useApp } from "../App";
import { ipc, type Prepared } from "../ipc/client";
import { AutoHiddenCard } from "./AutoHiddenCard";
import { Dialog, ErrorLine, Segments, ShieldIcon, Spinner } from "./ui";
import "./ReviewDialog.css";

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
  const { status, fail, refresh } = useApp();
  const [prepared, setPrepared] = useState(props.prepared);
  const [busy, setBusy] = useState<"send" | null>(null);
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

  return (
    <Dialog title="לפני שליחה ל-Claude"
      subtitle={<>{props.title} · שום דבר לא נשלח עד שתלחצי "שליחה"</>}
      icon={<span className="review-shield"><ShieldIcon /></span>}
      onClose={props.onClose}
      footer={
        <>
          <span className="small muted grow">
            {status.review_choice_available ? (
              <label className="row">
                <input type="checkbox" checked={status.review_only_suspect}
                  onChange={(e) => void ipc.setReviewOnlySuspect(e.target.checked).then(refresh)} />
                להציג את המסך הזה רק כשמשהו הוסתר אוטומטית
              </label>
            ) : "בשבועיים הראשונים המסך הזה מוצג לפני כל שליחה."}
          </span>
          {prepared.demo_mode && <span className="chip chip-sand">מצב הדגמה: שום דבר לא יוצא מהמחשב</span>}
          <button type="button" className="btn" onClick={props.onClose}>חזרה לעריכה</button>
          <button type="button" className="btn btn-primary btn-send" disabled={!prepared.approval_id || busy !== null} onClick={() => void send()}>
            {busy === "send" ? <><Spinner /> שולחת…</> : prepared.demo_mode ? "שליחה (הדגמה)" : "שליחה ל-Claude"}
          </button>
        </>
      }>
      <div className="stack review">
        <AutoHiddenCard items={prepared.auto_hidden} caseId={props.caseId} onChanged={rebuild} disabled={busy !== null} />

        {blocked.map((b, i) => (
          <p key={i} className="error">{b.message}{b.detail ? `: ${b.detail}` : ""}</p>
        ))}
        {blocked.length > 0 && (
          <p className="note-sand small">
            הבדיקה האחרונה עצרה את השליחה כדי להגן על הפרטיות, ושום דבר לא יצא. אם מה שנעצר נמצא בטקסט שלך,
            חוזרים לעריכה ומתקנים. אם לא ברור מאיפה זה בא, אפשר להעתיק את ההודעה הזאת ולשלוח לאור.
          </p>
        )}
        <ErrorLine error={error} />

        <ul className="review-checks">
          <li><span aria-hidden="true">✓</span>{c.declared_names ? `${c.declared_names} שמות מהתיק יוחלפו בתפקיד ("הילד", "האם", "הגננת")` : "אין שמות מהתיק בטקסט"}</li>
          <li><span aria-hidden="true">✓</span>{c.patterns ? `${c.patterns} פרטים מזהים יוחלפו (תאריכים הופכים ל"לפני כחודש")` : "אין ת\"ז, טלפון או תאריך"}</li>
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
            <section className="card review-col" aria-label="מה Claude יקבל">
              <h4 className="col-title col-out">מה Claude יקבל</h4>
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
