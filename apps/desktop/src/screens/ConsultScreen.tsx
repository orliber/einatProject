import { useEffect, useRef, useState, type FormEvent } from "react";
import { useApp } from "../App";
import { TopBar } from "../components/TopBar";
import { ReviewDialog } from "../components/ReviewDialog";
import { ErrorLine, Spinner } from "../components/ui";
import { ipc, type CaseSummary, type Prepared } from "../ipc/client";
import "./ConsultScreen.css";

interface Turn {
  role: "user" | "assistant";
  text: string;
  note?: string;
  demo?: boolean;
}

const IDEAS = [
  "איך לנסח המלצה לליווי רגשי בגן בלי להבהיל את ההורים?",
  "מה ההבדל בין WPPSI-IV ל-WISC-V בגיל 6, ומתי עדיף כל אחד?",
  "אילו שאלונים מתאימים להערכת ויסות חושי בגיל 5?",
];

export function ConsultScreen({ caseId }: { caseId?: string | undefined }) {
  const { fail, status } = useApp();
  const [cases, setCases] = useState<CaseSummary[]>([]);
  const [scope, setScope] = useState<string | null>(caseId ?? null);
  const [turns, setTurns] = useState<Record<string, Turn[]>>({});
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [review, setReview] = useState<{ prepared: Prepared; text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const end = useRef<HTMLDivElement>(null);
  const key = scope ?? "";
  const list = turns[key] ?? [];

  useEffect(() => {
    ipc.listCases().then(setCases).catch(() => undefined);
  }, []);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [list.length]);

  async function send(approvalId: string, text: string, hidden: string[]) {
    const r = await ipc.sendConsult(approvalId);
    setTurns((t) => ({
      ...t,
      [key]: [
        ...(t[key] ?? []),
        { role: "user", text, note: hidden.length ? `לפני השליחה הוסתרו ${hidden.length} פרטים: ${hidden.join(", ")}` : "לא נמצאו פרטים מזהים" },
        { role: "assistant", text: r.answer, demo: r.demo },
      ],
    }));
    setMessage("");
  }

  async function ask(e?: FormEvent) {
    e?.preventDefault();
    const text = message.trim();
    if (!text) return;
    setError(null);
    setBusy(true);
    try {
      const prepared = await ipc.prepareConsult(scope, text);
      const clean = prepared.approval_id && !prepared.suspects.length && !prepared.blocked.length;
      if (status.review_only_suspect && clean && prepared.approval_id) {
        await send(prepared.approval_id, text, prepared.hidden);
      } else {
        setReview({ prepared, text });
      }
    } catch (err) {
      setError(fail(err as never));
    } finally {
      setBusy(false);
    }
  }

  const selectedCase = cases.find((c) => c.id === scope);

  return (
    <div className="page">
      <TopBar active="consult" />
      <div className="consult">
        <aside className="consult-side">
          <h1>התייעצות</h1>
          <fieldset className="plain stack" style={{ gap: 8 }}>
            <legend className="label consult-legend">על מה השאלה?</legend>
            <label className={scope === null ? "scope on" : "scope"}>
              <input type="radio" name="scope" checked={scope === null} onChange={() => setScope(null)} />
              <span className="stack" style={{ gap: 0 }}><b>שאלה מקצועית כללית</b><span className="small muted">בלי שום פרט מתיק</span></span>
            </label>
            <label className={scope !== null ? "scope on" : "scope"}>
              <input type="radio" name="scope" checked={scope !== null} disabled={!cases.length}
                onChange={() => setScope(cases[0]?.id ?? null)} />
              <span className="stack grow" style={{ gap: 4 }}>
                <b>על תיק</b>
                <span className="small muted">נשלחים רק סעיפים שאישרת, אחרי הסתרה</span>
                {scope !== null && (
                  <>
                    <label className="visually-hidden" htmlFor="consult-case">התיק</label>
                    <select id="consult-case" className="select" value={scope} onChange={(e) => setScope(e.target.value)}>
                      {cases.map((c) => <option key={c.id} value={c.id}>{c.meta.code}{c.child_name ? ` · ${c.child_name}` : ""}</option>)}
                    </select>
                  </>
                )}
              </span>
            </label>
          </fieldset>
          <div className="stack" style={{ gap: 8 }}>
            <span className="label">רעיונות לשאלות</span>
            {IDEAS.map((q) => (
              <button key={q} type="button" className="idea" onClick={() => setMessage(q)}>{q}</button>
            ))}
          </div>
        </aside>

        <section className="card consult-chat" aria-label="השיחה">
          <div className="consult-body" aria-live="polite">
            {list.length === 0 && (
              <div className="consult-empty">
                <h2>{selectedCase ? `שאלה על ${selectedCase.meta.code}` : "שאלה מקצועית"}</h2>
                <p className="muted">כלי אבחון, ניסוח, ספרות מקצועית או שיקולים קליניים. Claude מבחין בין ידע מבוסס לדעה, וההחלטה המקצועית נשארת שלך. השיחה נשמרת רק עד נעילת התוכנה.</p>
              </div>
            )}
            {list.map((t, i) =>
              t.role === "user" ? (
                <div key={i} className="msg-user">
                  <div className="bubble-user">{t.text}</div>
                  {t.note && <span className="shield-line">{t.note}</span>}
                </div>
              ) : (
                <div key={i} className="answer serif">
                  {t.demo && <span className="chip chip-sand">הדגמה</span>}
                  {t.text.split(/\n{2,}/).map((p, j) => <p key={j}>{p}</p>)}
                </div>
              ),
            )}
            {busy && <div className="answer muted"><Spinner /> מכינה…</div>}
            <div ref={end} />
          </div>
          <form className="consult-foot" onSubmit={(e) => void ask(e)}>
            <label htmlFor="consult-q" className="visually-hidden">שאלה ל-Claude</label>
            <textarea id="consult-q" rows={2} className="textarea grow" placeholder="שאלה מקצועית… שמות יוסתרו אוטומטית"
              value={message} onChange={(e) => setMessage(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void ask(); }} />
            <button type="submit" className="btn btn-primary" disabled={busy || !message.trim()}>שליחה</button>
          </form>
          <ErrorLine error={error} />
        </section>
      </div>
      {review && (
        <ReviewDialog title={scope ? "שאלה על תיק" : "שאלה כללית"} caseId={scope} prepared={review.prepared}
          reprepare={() => ipc.prepareConsult(scope, review.text)}
          onClose={() => setReview(null)}
          onSend={async (id) => { await send(id, review.text, review.prepared.hidden); setReview(null); }} />
      )}
    </div>
  );
}
