import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { useApp } from "../../App";
import { ipc, type CaseDetail, type ChatView, type SectionResult } from "../../ipc/client";
import { ErrorLine, SendIcon, Spinner, WarnIcon } from "../../components/ui";
import type { CaseApi } from "../CaseScreen";
import "./SectionWork.css";

type Section = CaseDetail["sections"][number];

const DERIVED = ["summary", "diagnoses", "recommendations", "dsm"];

const QUICK: [string, string][] = [
  ["להרחיב", "הרחיבי את הטיוטה של הסעיף, בלי להוסיף עובדות שאינן במקורות."],
  ["לקצר", "קצרי את הטיוטה של הסעיף ושמרי על כל הממצאים העיקריים."],
  ["לבדוק מול המקורות", "בדקי את הטיוטה מול המקורות: מה חסר, מה סותר, ומה לא מבוסס."],
  ["להציע המלצות", "הציעי המלצות שנובעות מהממצאים שבמקורות, כל אחת עם הנימוק שלה."],
];

export function SectionWork({ api, section }: { api: CaseApi; section: Section }) {
  const { fail, notify } = useApp();
  const { caseId, reload } = api;
  const [chat, setChat] = useState<ChatView[]>([]);
  const [last, setLast] = useState<SectionResult | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [own, setOwn] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const chatEnd = useRef<HTMLDivElement>(null);

  const loadChat = useCallback(async () => {
    try {
      setChat(await ipc.chat(caseId, section.key));
    } catch (e) {
      setError(fail(e as never));
    }
  }, [caseId, section.key, fail]);

  useEffect(() => {
    let alive = true;
    ipc
      .chat(caseId, section.key)
      .then((c) => alive && setChat(c))
      .catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, section.key, fail]);

  useEffect(() => {
    chatEnd.current?.scrollIntoView({ block: "end" });
  }, [chat]);

  async function ask(instruction: string, title?: string) {
    if (!instruction.trim()) return;
    setError(null);
    setBusy(true);
    try {
      const prepare = () => ipc.prepareSection(caseId, section.key, instruction);
      const prepared = await prepare();
      await api.review({
        title: title ?? `בקשה לסעיף ${section.title}`,
        prepared,
        reprepare: prepare,
        onSend: async (id) => {
          const result = await ipc.sendSection(id);
          setLast(result);
          setMessage("");
          await Promise.all([reload(), loadChat()]);
          if (result.demo) notify("מצב הדגמה: התשובה נבנתה במחשב מתוך המקורות, ושום דבר לא נשלח.");
        },
      });
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  async function act(fn: () => Promise<void>) {
    setError(null);
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  function submit(e: FormEvent) {
    e.preventDefault();
    void ask(message);
  }

  const derived = DERIVED.includes(section.key);
  const visible = section.paragraphs;
  const approved = visible.filter((p) => p.status === "approved").length;
  const pending = visible.filter((p) => p.status === "proposed").length;
  const lastWarnings = new Map((last?.paragraphs ?? []).map((p) => [p.text, p.warnings]));

  return (
    <div className="view">
      <div className="view-head">
        <div className="stack" style={{ gap: 8 }}>
          <div className="row" style={{ alignItems: "baseline" }}>
            <h1>{section.title}</h1>
            <span className="small muted">{section.part}</span>
          </div>
          <div className="row small wrap-row">
            {derived ? (
              <span className="muted">הסעיף נכתב מתוך הסעיפים שכבר אישרת.</span>
            ) : section.source_count > 0 ? (
              <span className="muted">{section.source_count === 1 ? "חומר אחד מזין את הסעיף" : `${section.source_count} חומרים מזינים את הסעיף`}</span>
            ) : (
              <span className="muted">עוד אין חומרים שמזינים את הסעיף. אפשר להוסיף ב"חומרי התיק", או לכתוב בעצמך.</span>
            )}
          </div>
        </div>
      </div>

      <div className="view-body">
        <section className="paper draft" aria-label="טיוטת הסעיף">
          <div className="draft-head">
            <span className="draft-label">טיוטת הסעיף</span>
            <span>{approved} אושרו · {pending} ממתינות</span>
          </div>

          {visible.length === 0 && (
            <div className="draft-empty">
              <p className="serif">עוד אין טיוטה לסעיף הזה.</p>
              <div className="row">
                {(derived || section.source_count > 0) && (
                  <button type="button" className="btn btn-primary" disabled={busy}
                    onClick={() => void ask(derived ? "כתבי טיוטה לסעיף מתוך הסעיפים שאושרו." : "כתבי טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.", `טיוטה לסעיף ${section.title}`)}>
                    {derived ? "טיוטה מהסעיפים שאושרו" : "טיוטה מהמקורות"}
                  </button>
                )}
                <button type="button" className="btn" onClick={() => setOwn("")}>כתיבה בעצמי</button>
              </div>
            </div>
          )}

          {visible.map((p) => {
            const warnings = p.warnings.length ? p.warnings : lastWarnings.get(p.text) ?? [];
            const isEditing = editing?.id === p.id;
            if (p.status === "approved" && !isEditing) {
              return (
                <article key={p.id} className="para">
                  <p className="serif para-text">{p.text}</p>
                  <div className="para-meta">
                    <span className="approved-mark">✓ אושר על ידך</span>
                    {p.sources.length > 0 && <span className="muted">מקור: {p.sources.join(" · ")}</span>}
                    <button type="button" className="link-small" onClick={() => setEditing({ id: p.id, text: p.text })}>עריכה</button>
                  </div>
                </article>
              );
            }
            return (
              <article key={p.id} className="para pending">
                {!isEditing && (
                  <div className="para-meta">
                    <span className="pending-mark">ממתין לאישור{p.by_ai ? " · הצעה של Claude" : ""}</span>
                    {p.sources.length > 0 && <span className="muted">מקור: {p.sources.join(" · ")}</span>}
                  </div>
                )}
                {isEditing ? (
                  <label className="edit-box">
                    <span className="visually-hidden">עריכת הפסקה</span>
                    <textarea className="textarea serif" rows={5} value={editing.text} autoFocus
                      onChange={(e) => setEditing({ id: p.id, text: e.target.value })} />
                  </label>
                ) : (
                  <p className="serif para-text">{p.text}</p>
                )}
                {warnings.map((w, i) => (
                  <div key={i} className="note-warn"><WarnIcon /><span>{w}</span></div>
                ))}
                <div className="row wrap-row">
                  {isEditing ? (
                    <>
                      <button type="button" className="btn btn-primary btn-small"
                        onClick={() => void act(async () => { await ipc.editParagraph(caseId, p.id, editing.text); await ipc.approveParagraph(caseId, p.id); setEditing(null); })}>שמירה ואישור</button>
                      <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
                    </>
                  ) : (
                    <>
                      <button type="button" className="btn btn-primary btn-small" onClick={() => void act(() => ipc.approveParagraph(caseId, p.id))}>אישור</button>
                      <button type="button" className="btn btn-small" disabled={busy}
                        onClick={() => void ask(`נסחי מחדש את הפסקה הבאה, באותו תוכן ובלי להוסיף עובדות: "${p.text}"`, "ניסוח מחדש")}>ניסוח מחדש</button>
                      <button type="button" className="btn btn-small" onClick={() => setEditing({ id: p.id, text: p.text })}>עריכה</button>
                      <button type="button" className="btn btn-small btn-ghost" onClick={() => void act(() => ipc.rejectParagraph(caseId, p.id))}>הסרה</button>
                    </>
                  )}
                </div>
              </article>
            );
          })}

          {own !== null ? (
            <div className="para pending">
              <label className="edit-box">
                <span className="visually-hidden">פסקה חדשה</span>
                <textarea className="textarea serif" rows={4} value={own} autoFocus placeholder="כתבי כאן. שמות אמיתיים נשמרים מוצפנים ומוסתרים לפני כל שליחה."
                  onChange={(e) => setOwn(e.target.value)} />
              </label>
              <div className="row">
                <button type="button" className="btn btn-primary btn-small" disabled={!own.trim()}
                  onClick={() => void act(async () => { await ipc.addOwnParagraph(caseId, section.key, own); setOwn(null); })}>הוספה לדוח</button>
                <button type="button" className="btn btn-small" onClick={() => setOwn(null)}>ביטול</button>
              </div>
            </div>
          ) : visible.length > 0 && (
            <button type="button" className="add-own" onClick={() => setOwn("")}>+ פסקה משלי</button>
          )}

          {last && (last.questions.length > 0 || last.missing.length > 0 || last.contradictions.length > 0) && (
            <div className="notes-from-claude">
              {last.contradictions.map((c, i) => <p key={`c${i}`}><b>סתירה בין מקורות:</b> {c}</p>)}
              {last.missing.map((c, i) => <p key={`m${i}`}><b>חסר מידע:</b> {c}</p>)}
              {last.questions.map((c, i) => <p key={`q${i}`}><b>שאלה:</b> {c}</p>)}
            </div>
          )}
          <ErrorLine error={error} />
        </section>

        <section className="chat" aria-label="שיחה על הסעיף">
          <div className="chat-head">שיחה על הסעיף</div>
          <div className="chat-body" aria-live="polite">
            {chat.length === 0 && <p className="muted small chat-hint">כאן מבקשים מ-Claude: טיוטה, ניסוח אחר, השוואה לממצאים. כל בקשה עוברת קודם במסך "מה יוצא מהמחשב".</p>}
            {chat.map((m, i) =>
              m.role === "user" ? (
                <div key={i} className="msg-user">
                  <div className="bubble-user">{m.text}</div>
                  <span className="shield-line">
                    {m.hidden.length ? `לפני השליחה הוסתרו ${m.hidden.length} פרטים: ${m.hidden.join(", ")}` : "לא נמצאו פרטים מזהים"}
                  </span>
                </div>
              ) : (
                <div key={i} className="bubble-claude">
                  {m.demo && <span className="chip chip-sand demo-chip">הדגמה</span>}
                  <span>{m.text}</span>
                </div>
              ),
            )}
            {busy && <div className="bubble-claude muted"><Spinner /> מכינה…</div>}
            <div ref={chatEnd} />
          </div>
          <form className="chat-foot" onSubmit={submit}>
            <div className="quick">
              {QUICK.map(([label, text]) => (
                <button key={label} type="button" className="pill-btn" disabled={busy || visible.length === 0} onClick={() => void ask(text, label)}>{label}</button>
              ))}
            </div>
            <div className="chat-input">
              <label htmlFor="chat-msg" className="visually-hidden">הודעה ל-Claude</label>
              <textarea id="chat-msg" rows={2} className="textarea grow" placeholder="כתבי ל-Claude… שמות יוסתרו אוטומטית"
                value={message} onChange={(e) => setMessage(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void ask(message); }} />
              <button type="submit" className="btn btn-primary send-btn" title="תמיד מוצג קודם מה יוצא מהמחשב" disabled={busy || !message.trim()}>שליחה <SendIcon /></button>
            </div>
          </form>
        </section>
      </div>
    </div>
  );
}
