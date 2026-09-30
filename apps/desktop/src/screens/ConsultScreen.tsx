import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { TopBar } from "../components/TopBar";
import { ReviewDialog } from "../components/ReviewDialog";
import { CasePicker } from "../components/CasePicker";
import { ErrorLine, Spinner } from "../components/ui";
import { ConfirmDialog } from "./library/LibraryDialogs";
import { ipc, type ConsultationSummary, type Prepared } from "../ipc/client";
import "./ConsultScreen.css";

interface Turn {
  role: "user" | "assistant";
  text: string;
  /** What the filter hid before sending (user turns). */
  hidden?: string[];
  demo?: boolean;
  /** Unix seconds. */
  at: number;
  /** A user turn still waiting for the review screen or for the answer. */
  pending?: boolean;
}

const IDEAS = [
  "איך לנסח המלצה לליווי רגשי בגן בלי להבהיל את ההורים?",
  "מה ההבדל בין WPPSI-IV ל-WISC-V בגיל 6, ומתי עדיף כל אחד?",
  "אילו שאלונים מתאימים להערכת ויסות חושי בגיל 5?",
];

const now = () => Math.floor(Date.now() / 1000);

function clock(at: number): string {
  return new Date(at * 1000).toLocaleTimeString("he-IL", { hour: "2-digit", minute: "2-digit" });
}

/** "היום", "אתמול", or the date, for the list of conversations. */
function when(at: number): string {
  const d = new Date(at * 1000);
  const today = new Date();
  const days = Math.round((new Date(today.toDateString()).getTime() - new Date(d.toDateString()).getTime()) / 86_400_000);
  if (days <= 0) return clock(at);
  if (days === 1) return "אתמול";
  return d.toLocaleDateString("he-IL");
}

/** "Claude is writing…" with the seconds waited, so a long answer never looks stuck. */
function Typing({ since }: { since: number }) {
  const [t, setT] = useState(() => Date.now());
  useEffect(() => {
    const i = window.setInterval(() => setT(Date.now()), 1000);
    return () => window.clearInterval(i);
  }, []);
  const seconds = Math.max(0, Math.round((t - since) / 1000));
  return (
    <div className="chat-row chat-row-ai" role="status">
      <span className="chat-avatar" aria-hidden="true">C</span>
      <div className="chat-bubble chat-bubble-ai chat-typing">
        <span className="dots" aria-hidden="true"><i /><i /><i /></span>
        <span className="muted small">Claude כותב{seconds >= 3 ? ` · ${seconds} שניות` : "…"}</span>
        {seconds >= 25 && <span className="muted small">תשובה מעמיקה לוקחת לפעמים עד דקה. אפשר לבחור "מהיר" בהגדרות.</span>}
      </div>
    </div>
  );
}

export function ConsultScreen({ caseId }: { caseId?: string | undefined }) {
  const { fail, status } = useApp();
  const qc = useQueryClient();
  const cases = useQuery({ queryKey: ["cases"], queryFn: ipc.listCases });
  const saved = useQuery({ queryKey: ["consultations"], queryFn: ipc.consultations });
  // The case a new conversation is about (null: a general question).
  const [scope, setScope] = useState<string | null>(caseId ?? null);
  const [conversation, setConversation] = useState<string | null>(null);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [waitingSince, setWaitingSince] = useState<number | null>(null);
  const [review, setReview] = useState<{ prepared: Prepared; text: string } | null>(null);
  const [copied, setCopied] = useState<number | null>(null);
  const [deleting, setDeleting] = useState<ConsultationSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const end = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const caseList = cases.data ?? [];
  const selectedCase = caseList.find((c) => c.id === scope);

  // Follow the conversation down, like any chat.
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end", behavior: "smooth" });
  }, [turns.length, waitingSince]);
  // The box grows with the text, up to a few lines.
  useEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  }, [message]);

  function startNew(nextScope: string | null = scope) {
    setConversation(null);
    setTurns([]);
    setScope(nextScope);
    setError(null);
    input.current?.focus();
  }

  async function open(c: ConsultationSummary) {
    setError(null);
    try {
      const view = await ipc.consultation(c.id);
      setConversation(view.id);
      setScope(view.case_id);
      setTurns(view.turns.map((t) => ({ role: t.role === "user" ? "user" : "assistant", text: t.text, hidden: t.hidden, demo: t.demo, at: t.at })));
    } catch (e) {
      setError(fail(e as never));
    }
  }

  const dropPending = () => setTurns((all) => all.filter((t) => !t.pending));

  async function send(approvalId: string, hidden: string[]) {
    setWaitingSince(Date.now());
    try {
      const r = await ipc.sendConsult(approvalId);
      setConversation(r.conversation_id);
      setTurns((all) => [
        ...all.map((t) => (t.pending ? { ...t, pending: false, hidden } : t)),
        { role: "assistant", text: r.answer, demo: r.demo, at: now() },
      ]);
      await qc.invalidateQueries({ queryKey: ["consultations"] });
    } catch (err) {
      dropPending();
      setError(fail(err as never));
    } finally {
      setWaitingSince(null);
      input.current?.focus();
    }
  }

  async function ask(e?: FormEvent) {
    e?.preventDefault();
    const text = message.trim();
    if (!text || busy || waitingSince) return;
    setError(null);
    setBusy(true);
    // The question shows at once, as in any chat; it is marked until it is sent.
    setTurns((all) => [...all, { role: "user", text, at: now(), pending: true }]);
    setMessage("");
    try {
      const prepared = await ipc.prepareConsult(scope, conversation, text);
      const clean = prepared.approval_id && !prepared.suspects.length && !prepared.blocked.length;
      if (status.review_only_suspect && clean && prepared.approval_id) {
        await send(prepared.approval_id, prepared.hidden);
      } else {
        setReview({ prepared, text });
      }
    } catch (err) {
      dropPending();
      setMessage(text);
      setError(fail(err as never));
    } finally {
      setBusy(false);
    }
  }

  async function copy(i: number, text: string) {
    // Answers about a case may hold names: only the protected clipboard (out of clipboard
    // history, cleared after a minute). No fallback to the ordinary one.
    try {
      await ipc.copySecret(text);
    } catch (err) {
      setError(fail(err as never));
      return;
    }
    setCopied(i);
    window.setTimeout(() => setCopied((c) => (c === i ? null : c)), 2000);
  }

  const locked = conversation !== null; // An open conversation stays with its case.
  // While a question is on its way, the conversation on screen must stay the same one.
  const inFlight = busy || waitingSince !== null || review !== null;
  const title = selectedCase ? `שאלה על ${selectedCase.child_name ?? selectedCase.meta.code}` : "שאלה מקצועית";

  return (
    <div className="page page-fit">
      <TopBar active="consult" />
      <div className="consult">
        <aside className="consult-side">
          <div className="row consult-side-head">
            <h1 className="grow">התייעצות</h1>
            <button type="button" className="btn btn-small" disabled={inFlight} onClick={() => startNew(null)}>+ שיחה חדשה</button>
          </div>

          <div className="stack consult-scope" role="radiogroup" aria-label="על מה השאלה?">
            <span className="label">על מה השאלה?</span>
            <button type="button" role="radio" aria-checked={scope === null} disabled={locked || inFlight}
              className={scope === null ? "scope on" : "scope"} onClick={() => setScope(null)}>
              <b>שאלה מקצועית כללית</b><span className="small muted">בלי שום פרט מתיק</span>
            </button>
            <button type="button" role="radio" aria-checked={scope !== null} disabled={locked || inFlight || !caseList.length}
              className={scope !== null ? "scope on" : "scope"} onClick={() => setScope(scope ?? caseList[0]?.id ?? null)}>
              <b>על תיק</b>
              <span className="small muted">{caseList.length ? "נשלחים רק סעיפים שאישרת, אחרי הסתרה" : cases.isLoading ? "טוען את התיקים…" : "עוד אין תיקים"}</span>
            </button>
            {scope !== null && !locked && (
              <div className="field consult-case">
                <label htmlFor="consult-case">התיק</label>
                <CasePicker id="consult-case" value={scope} onChange={setScope} />
              </div>
            )}
            {locked && <span className="hint">שיחה שמורה נשארת עם התיק שלה. לשאלה על תיק אחר: "שיחה חדשה".</span>}
          </div>

          <nav className="consult-history" aria-label="שיחות קודמות">
            <span className="label">שיחות קודמות</span>
            {saved.data?.length === 0 && <span className="muted small">השיחות נשמרות כאן, מוצפנות בכספת.</span>}
            <ul>
              {(saved.data ?? []).map((c) => (
                <li key={c.id} className={c.id === conversation ? "on" : undefined}>
                  <button type="button" className="history-item" disabled={inFlight} onClick={() => void open(c)} aria-current={c.id === conversation ? "true" : undefined}>
                    <span className="history-title">{c.title || "שיחה"}</span>
                    <span className="history-meta">{c.case_label ?? "כללי"} · {when(c.updated_at)}</span>
                  </button>
                  <button type="button" className="icon-btn history-delete" disabled={inFlight} aria-label={`מחיקת השיחה ${c.title}`} onClick={() => setDeleting(c)}>×</button>
                </li>
              ))}
            </ul>
          </nav>
        </aside>

        <section className="card consult-chat" aria-label="השיחה">
          <div className="consult-body" aria-live="polite">
            {turns.length === 0 && (
              <div className="consult-empty">
                <h2>{title}</h2>
                <p className="muted">כלי אבחון, ניסוח, ספרות מקצועית או שיקולים קליניים. Claude מבחין בין ידע מבוסס לדעה, וההחלטה המקצועית נשארת שלך. השיחות נשמרות מוצפנות בכספת, ושיחה על תיק נמחקת יחד איתו.</p>
                <span className="label ideas-label">אפשר להתחיל מאחת מאלה</span>
                <div className="ideas">
                  {IDEAS.map((q) => (
                    <button key={q} type="button" className="idea" onClick={() => { setMessage(q); input.current?.focus(); }}>{q}</button>
                  ))}
                </div>
              </div>
            )}
            {turns.map((t, i) =>
              t.role === "user" ? (
                <div key={i} className={t.pending ? "chat-row chat-row-me pending" : "chat-row chat-row-me"}>
                  <div className="chat-col">
                    <div className="chat-bubble chat-bubble-me">{t.text}</div>
                    <span className="chat-meta">
                      {t.pending ? "ממתינה לשליחה…" : clock(t.at)}
                      {!t.pending && t.hidden && (
                        <> · <span className="shield-note">🔒 {t.hidden.length ? `הוסתרו לפני השליחה: ${t.hidden.join(", ")}` : "לא נמצאו פרטים מזהים"}</span></>
                      )}
                    </span>
                  </div>
                </div>
              ) : (
                <div key={i} className="chat-row chat-row-ai">
                  <span className="chat-avatar" aria-hidden="true">C</span>
                  <div className="chat-col">
                    <div className="chat-bubble chat-bubble-ai serif">
                      {t.demo && <span className="chip chip-sand chat-demo">הדגמה</span>}
                      {t.text.split(/\n{2,}/).map((p, j) => <p key={j}>{p}</p>)}
                    </div>
                    <span className="chat-meta">
                      Claude · {clock(t.at)}
                      <button type="button" className="link-btn chat-copy" onClick={() => void copy(i, t.text)}>{copied === i ? "הועתק ✓" : "העתקה"}</button>
                    </span>
                  </div>
                </div>
              ),
            )}
            {waitingSince && <Typing since={waitingSince} />}
            {busy && !waitingSince && !review && (
              <div className="chat-row chat-row-ai">
                <span className="chat-avatar" aria-hidden="true">C</span>
                <div className="chat-bubble chat-bubble-ai chat-typing"><Spinner /> <span className="muted small">בודקת מה יוצא מהמחשב…</span></div>
              </div>
            )}
            <div ref={end} />
          </div>
          <form className="chat-composer" onSubmit={(e) => void ask(e)}>
            <label htmlFor="consult-q" className="visually-hidden">שאלה ל-Claude</label>
            <div className="chat-input">
              <textarea id="consult-q" ref={input} rows={1} className="grow" placeholder="כתבי שאלה… שמות יוסתרו אוטומטית"
                value={message} onChange={(e) => setMessage(e.target.value)}
                onKeyDown={(e) => {
                  // Enter sends, Shift+Enter is a new line (as in every chat).
                  if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                    e.preventDefault();
                    void ask();
                  }
                }} />
              <button type="submit" className="chat-send" aria-label="שליחה" disabled={busy || !!waitingSince || !message.trim()}>
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M19 12H5M11 6l-6 6 6 6" /></svg>
              </button>
            </div>
            <span className="chat-hint muted small">Enter לשליחה · Shift+Enter לשורה חדשה · לפני כל שליחה עוברים על מה שיוצא מהמחשב</span>
          </form>
          <ErrorLine error={error} />
        </section>
      </div>
      {review && (
        <ReviewDialog title={scope ? "שאלה על תיק" : "שאלה כללית"} caseId={scope} prepared={review.prepared}
          reprepare={() => ipc.prepareConsult(scope, conversation, review.text)}
          onClose={() => { dropPending(); setMessage(review.text); setReview(null); }}
          onSend={async (id) => { const hidden = review.prepared.hidden; setReview(null); await send(id, hidden); }} />
      )}
      {deleting && (
        <ConfirmDialog danger title="מחיקת שיחה" action="מחיקה"
          text={`השיחה "${deleting.title}" תימחק מהכספת. אי אפשר לשחזר אותה.`}
          onClose={() => setDeleting(null)}
          onConfirm={async () => {
            await ipc.deleteConsultation(deleting.id);
            if (deleting.id === conversation) startNew();
            setDeleting(null);
            await qc.invalidateQueries({ queryKey: ["consultations"] });
          }} />
      )}
    </div>
  );
}
