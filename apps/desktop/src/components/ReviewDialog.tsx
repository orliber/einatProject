import { useState } from "react";
import { useApp } from "../App";
import { roleLabel } from "../i18n/he";
import { ipc, type Prepared } from "../ipc/client";
import type { Suspect } from "../ipc/generated/Suspect";
import { Dialog, ErrorLine, Segments, ShieldIcon, Spinner } from "./ui";
import "./ReviewDialog.css";

/**
 * "What leaves the computer": both sides, the checks, and a decision for every suspect.
 * Sending is possible only with an approval id (the gate cleared this exact payload).
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
  const [busy, setBusy] = useState<"send" | "decide" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const c = prepared.checks;

  async function decide(s: Suspect, kind: "hide" | "is_name" | "not_a_name") {
    if (!props.caseId) return;
    setBusy("decide");
    setError(null);
    try {
      await ipc.decideSuspect(props.caseId, s.token,
        kind === "hide" ? { decision: "hide", role: s.suggested_role } : { decision: kind });
      const next = await props.reprepare();
      setPrepared(next);
      // A question that comes back after an answer must say so, never look frozen.
      if (next.suspects.some((x) => x.token === s.token && x.kind === s.kind)) {
        setError(`ההחלטה על "${s.token}" נשמרה, אבל המילה עדיין מסומנת. אפשר לבחור אפשרות אחרת, או לחזור לעריכה ולשנות את הניסוח.`);
      }
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(null);
    }
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

  // While a question is open the gate's reasons repeat it; they matter only once none is left.
  const blockedOnly = prepared.suspects.length ? [] : prepared.blocked.filter((b) => b.code !== "unresolved_suspects");
  const question = prepared.suspects[0];
  const context = question ? contextOf(prepared, question.token) : null;

  return (
    <Dialog title={question ? (prepared.suspects.length === 1 ? "שאלה אחת לפני ששולחים" : `${prepared.suspects.length} שאלות לפני ששולחים`) : "לפני שליחה ל-Claude"}
      subtitle={<>{props.title} · שום דבר לא נשלח עד שתלחצי "שליחה"</>}
      icon={<span className="review-shield"><ShieldIcon /></span>}
      onClose={props.onClose}
      footer={
        <>
          <span className="small muted grow">
            {question ? "השליחה תיפתח אחרי שתעני על השאלות." : status.review_choice_available ? (
              <label className="row">
                <input type="checkbox" checked={status.review_only_suspect}
                  onChange={(e) => void ipc.setReviewOnlySuspect(e.target.checked).then(refresh)} />
                להציג את המסך הזה רק כשיש שאלה
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
        {question && prepared.suspects.length > 1 && <span className="small muted">שאלה 1 מתוך {prepared.suspects.length}. כל תשובה נזכרת בתיק הזה.</span>}
        {question && (
          <section className="question" role="alert" aria-label="שאלה לפני שליחה">
            <h3 className="question-title">
              {question.kind === "ambiguous_word" ? <>המילה <b>{question.token}</b>: מילה רגילה או שם?</>
                : question.kind === "indirect" ? <>{question.message}</>
                : <>מי זה <b className="q-name">{question.token}</b>?</>}
            </h3>
            {context && (
              <p className="serif question-context">"…{context.before}<mark>{context.token}</mark>{context.after}…"</p>
            )}
            {props.caseId ? (
              <div className="question-actions">
                {question.kind === "ambiguous_word" ? (
                  <>
                    <button type="button" className="btn btn-primary btn-big" disabled={busy !== null} onClick={() => void decide(question, "is_name")}>זה השם, להסתיר</button>
                    <button type="button" className="btn btn-big" disabled={busy !== null} onClick={() => void decide(question, "not_a_name")}>מילה רגילה, להשאיר</button>
                  </>
                ) : (
                  <>
                    <button type="button" className="btn btn-primary btn-big" disabled={busy !== null} onClick={() => void decide(question, "hide")}>
                      {question.kind === "indirect" ? "להסתיר" : "להסתיר את השם"}
                    </button>
                    <button type="button" className="btn btn-ghost btn-big" disabled={busy !== null} onClick={() => void decide(question, "not_a_name")}>
                      {question.kind === "indirect" ? "להשאיר כמו שזה" : "זה לא שם, להשאיר"}
                    </button>
                  </>
                )}
              </div>
            ) : null}
            {props.caseId && question.kind !== "ambiguous_word" && question.kind !== "indirect" && (
              <span className="small muted">אם מסתירים, Claude יקבל במקומו: "{roleLabel[question.suggested_role]}". אפשר לשנות תפקיד ב"פרטים ושמות להסתרה".</span>
            )}
            {!props.caseId && (
              <p className="note-sand">בשאלה כללית אי אפשר להחליט על שמות. כדאי לנסח את השאלה בלי השם, או לשאול מתוך תיק.</p>
            )}
          </section>
        )}

        {blockedOnly.map((b, i) => (
          <p key={i} className="error">{b.message}{b.detail ? `: ${b.detail}` : ""}</p>
        ))}
        {blockedOnly.length > 0 && (
          <p className="note-sand small">
            הבדיקה האחרונה עצרה את השליחה כדי להגן על הפרטיות, ושום דבר לא יצא. אם מה שנעצר נמצא בטקסט שלך,
            חוזרים לעריכה ומתקנים. אם לא ברור מאיפה זה בא, אפשר להעתיק את ההודעה הזאת ולשלוח לאור.
          </p>
        )}
        <ErrorLine error={error} />

        <ul className="review-checks">
          <li><span aria-hidden="true">✓</span>{c.declared_names ? `${c.declared_names} שמות מהתיק יוחלפו בתפקיד ("הילד", "האם", "הגננת")` : "אין שמות מהתיק בטקסט"}</li>
          <li><span aria-hidden="true">✓</span>{c.patterns ? `${c.patterns} פרטים מזהים יוחלפו (תאריכים הופכים ל"לפני כחודש")` : "אין ת\"ז, טלפון או תאריך"}</li>
          {!question && !blockedOnly.length && <li><span aria-hidden="true">✓</span>אין שאלות פתוחות. אפשר לשלוח.</li>}
        </ul>

        <details className="full-text" open={!question && prepared.parts.length <= 2}>
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

/** A few words around the first place the token appears, from the case side of the text. */
function contextOf(p: Prepared, token: string): { before: string; token: string; after: string } | null {
  for (const part of p.parts) {
    const text = part.original.map((s) => s.text).join("");
    const at = text.indexOf(token);
    if (at >= 0) {
      const before = text.slice(Math.max(0, at - 60), at);
      const after = text.slice(at + token.length, at + token.length + 60);
      return { before: before.replace(/^\S*\s/, ""), token, after: after.replace(/\s\S*$/, "") };
    }
  }
  return null;
}
