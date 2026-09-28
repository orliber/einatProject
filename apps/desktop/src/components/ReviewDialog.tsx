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
  const nameSuspects = prepared.suspects.filter((s) => s.kind !== "indirect").length;
  const indirect = prepared.suspects.filter((s) => s.kind === "indirect").length;

  async function decide(s: Suspect, kind: "hide" | "is_name" | "not_a_name") {
    if (!props.caseId) return;
    setBusy("decide");
    setError(null);
    try {
      await ipc.decideSuspect(props.caseId, s.token,
        kind === "hide" ? { decision: "hide", role: s.suggested_role } : { decision: kind });
      setPrepared(await props.reprepare());
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

  const blockedOnly = prepared.blocked.filter((b) => b.code !== "unresolved_suspects");

  return (
    <Dialog title="לפני שליחה ל-Claude" subtitle={props.title} icon={<span className="review-shield"><ShieldIcon /></span>}
      onClose={props.onClose}
      footer={
        <>
          {status.review_choice_available ? (
            <label className="row small grow">
              <input type="checkbox" checked={status.review_only_suspect}
                onChange={(e) => void ipc.setReviewOnlySuspect(e.target.checked).then(refresh)} />
              להציג את המסך הזה רק כשיש חשד
            </label>
          ) : (
            <span className="small muted grow">בשבועיים הראשונים המסך הזה מוצג לפני כל שליחה.</span>
          )}
          {prepared.demo_mode && <span className="chip chip-sand">מצב הדגמה: שום דבר לא יוצא מהמחשב</span>}
          <button type="button" className="btn" onClick={props.onClose}>ביטול ועריכה</button>
          <button type="button" className="btn btn-primary" disabled={!prepared.approval_id || busy !== null} onClick={() => void send()}>
            {busy === "send" ? <><Spinner /> שולחת…</> : prepared.demo_mode ? "שליחה (הדגמה)" : "שליחה ל-Claude"}
          </button>
        </>
      }>
      <div className="stack review">
        <div className="review-cols">
          <section className="card review-col" aria-label="מה שכתוב בתיק">
            <h3 className="col-title">מה שכתוב בתיק (נשאר במחשב)</h3>
            {prepared.parts.map((p, i) => (
              <div key={i} className="review-part">
                <span className="small muted">{p.label}</span>
                <p className="serif review-text"><Segments segments={p.original} side="original" /></p>
              </div>
            ))}
          </section>
          <section className="card review-col" aria-label="מה Claude יקבל">
            <h3 className="col-title col-out">מה Claude יקבל</h3>
            {prepared.parts.map((p, i) => (
              <div key={i} className="review-part">
                <span className="small muted">{p.label}</span>
                <p className="serif review-text"><Segments segments={p.outgoing} side="outgoing" /></p>
              </div>
            ))}
          </section>
        </div>

        <div className="checks">
          <div className="check ok"><b>✓ שמות מהתיק</b><span>{c.declared_names ? `${c.declared_names} הוסתרו` : "לא הופיעו"}</span></div>
          <div className="check ok"><b>✓ ת"ז, טלפון, תאריכים</b><span>{c.patterns ? `${c.patterns} הוחלפו` : "לא נמצאו"}</span></div>
          <div className={nameSuspects ? "check warn" : "check ok"}><b>{nameSuspects ? "!" : "✓"} שמות לא מוכרים</b><span>{nameSuspects ? `${nameSuspects} לבדיקה` : "לא נמצאו"}</span></div>
          <div className={indirect ? "check warn" : "check ok"}><b>{indirect ? "!" : "✓"} פרטים עקיפים</b><span>{indirect ? `${indirect} לבדיקה` : "לא נמצאו"}</span></div>
        </div>

        {prepared.suspects.map((s) => (
          <div key={s.token} className="note-block suspect" role="alert">
            <span className="grow">
              {s.kind === "ambiguous_word" ? (
                <>המילה <b>{s.token}</b> יכולה להיות מילה רגילה או שם מהתיק. מה נכון כאן?</>
              ) : s.kind === "indirect" ? (
                <>{s.message}: <b>{s.token}</b></>
              ) : (
                <>נמצא שם שלא מופיע ברשימת התיק: <b>{s.token}</b>. עד שתחליטי, השליחה חסומה.</>
              )}
            </span>
            {props.caseId && (s.kind === "ambiguous_word" ? (
              <>
                <button type="button" className="btn btn-primary btn-small" disabled={busy !== null} onClick={() => void decide(s, "is_name")}>זה השם, להסתיר</button>
                <button type="button" className="btn btn-small" disabled={busy !== null} onClick={() => void decide(s, "not_a_name")}>מילה רגילה</button>
              </>
            ) : (
              <>
                <button type="button" className="btn btn-primary btn-small" disabled={busy !== null} onClick={() => void decide(s, "hide")}>
                  להסתיר כ{roleLabel[s.suggested_role]}
                </button>
                <button type="button" className="btn btn-small" disabled={busy !== null} onClick={() => void decide(s, "not_a_name")}>זה לא שם</button>
              </>
            ))}
          </div>
        ))}
        {!props.caseId && prepared.suspects.length > 0 && (
          <p className="note-sand">בשאלה כללית אי אפשר להחליט על שמות. כדאי לנסח את השאלה בלי השם, או לשאול על תיק.</p>
        )}
        {blockedOnly.map((b, i) => (
          <p key={i} className="error">{b.message}{b.detail ? `: ${b.detail}` : ""}</p>
        ))}
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
