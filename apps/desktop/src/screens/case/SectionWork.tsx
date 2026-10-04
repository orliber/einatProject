import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { useApp } from "../../App";
import { ipc, type CaseDetail, type ChatView, type SectionResult } from "../../ipc/client";
import { ErrorLine, SendIcon, Spinner, WarnIcon } from "../../components/ui";
import { ProgressLine } from "../../components/Progress";
import { useHoldUnsaved } from "../../components/Unsaved";
import { kindLabel } from "../../i18n/he";
import { useRotatingPlaceholder } from "../../components/Rotating";
import { DRAFT_HINTS } from "../../i18n/suggestions";
import { ParagraphVersions } from "../../components/ParagraphVersions";
import { isCombo, KEYS } from "../../shortcuts";
import type { CaseApi } from "../CaseScreen";
import "./SectionWork.css";

type Section = CaseDetail["sections"][number];

const DERIVED = ["summary", "diagnoses", "recommendations", "dsm"];

const QUICK: [string, string][] = [
  ["להרחיב", "הרחיבי את הטיוטה של הסעיף, בלי להוסיף עובדות שאינן במקורות."],
  ["לקצר", "קצרי את הטיוטה של הסעיף ושמרי על כל הממצאים העיקריים."],
  ["לבדוק מול החומרים", "בדקי את הטיוטה מול המקורות: מה חסר, מה סותר, ומה לא מבוסס."],
  ["להציע המלצות", "הציעי המלצות שנובעות מהממצאים שבמקורות, כל אחת עם הנימוק שלה."],
];

export function SectionWork({ api, section }: { api: CaseApi; section: Section }) {
  const { fail, notify, go } = useApp();
  const { caseId, reload } = api;
  const [chat, setChat] = useState<ChatView[]>([]);
  const [last, setLast] = useState<SectionResult | null>(null);
  const [message, setMessage] = useState("");
  const chatHint = `${useRotatingPlaceholder(DRAFT_HINTS, message.length > 0)} · שמות יוסתרו אוטומטית`;
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [own, setOwn] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [linking, setLinking] = useState(false);
  const [linked, setLinked] = useState(false);
  /** The paragraph whose earlier wordings are open (UX-4). */
  const [history, setHistory] = useState<string | null>(null);
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

  // One request at a time per section: while it is prepared, reviewed or written.
  const job = api.jobs.find((j) => j.section === section.key);
  useHoldUnsaved(caseId, `פסקה בסעיף "${section.title}"`, editing?.text ?? own);
  const [preparing, setPreparing] = useState(false);
  const working = preparing || busy || job !== undefined;

  /** `replaces`: rewrite that one paragraph; otherwise the answer is the section's new draft. */
  async function ask(instruction: string, title?: string, replaces?: string) {
    if (!instruction.trim()) return;
    setError(null);
    setPreparing(true);
    try {
      const prepare = () => ipc.prepareSection(caseId, section.key, instruction, replaces);
      const prepared = await prepare();
      setPreparing(false);
      setBusy(true);
      await api.review({
        title: title ?? `בקשה לסעיף ${section.title}`,
        prepared,
        reprepare: prepare,
        onSend: async (id) => {
          const result = await api.track(
            { label: replaces ? "Claude מנסח את הפסקה מחדש" : `Claude כותב את "${section.title}"`, task: "draft", section: section.key, approval: id },
            () => ipc.sendSection(id),
          );
          setLast(result);
          setMessage("");
          await Promise.all([reload(), loadChat()]);
          if (result.demo) notify("מצב הדגמה: התשובה נבנתה במחשב מתוך החומרים, ושום דבר לא נשלח.");
        },
      });
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setPreparing(false);
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

  // Ctrl+Shift+Enter: approve the whole draft waiting here (UX-5).
  const canApproveAll = pending > 0 && !job && editing === null && own === null;
  const approveAll = useRef<() => void>(() => undefined);
  useEffect(() => {
    approveAll.current = () => void act(async () => { await ipc.approveSection(caseId, section.key); });
  });
  useEffect(() => {
    if (!canApproveAll) return;
    const onKey = (e: KeyboardEvent) => {
      if (!isCombo(e, KEYS.approveDraft)) return;
      e.preventDefault();
      approveAll.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [canApproveAll]);
  const lastWarnings = new Map((last?.paragraphs ?? []).map((p) => [p.text, p.warnings]));
  // What the section is written from: the materials that feed it (D-022).
  const sources = api.detail.routing
    .filter((r) => r.feeds.includes(section.key))
    .map((r) => api.detail.inputs.find((i) => i.id === r.input_id))
    .filter((i) => i !== undefined);
  const order = api.detail.sections.filter((s) => s.key !== section.key && (s.source_count > 0 || DERIVED.includes(s.key)));
  const after = api.detail.sections.findIndex((s) => s.key === section.key);
  const nextSection =
    order.find((s) => api.detail.sections.indexOf(s) > after && !s.approved) ?? order.find((s) => !s.approved);
  const draftInstruction = derived ? "כתבי טיוטה לסעיף מתוך הסעיפים שאושרו." : "כתבי טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.";

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
            ) : (
              <>
                <span className="muted">{sources.length ? "Claude כותב מתוך:" : "עוד אין חומרים שמזינים את הסעיף."}</span>
                {sources.map((i) => (
                  <span key={i.id} className="source-chip">{kindLabel[i.kind] ?? ""} · {i.title}</span>
                ))}
                {section.sortable && (
                  <button type="button" className="source-chip source-link" aria-expanded={linking} onClick={() => setLinking(!linking)}>
                    {linking ? "סגירה" : sources.length ? "✎ שינוי החומרים" : "+ קישור חומרים לסעיף"}
                  </button>
                )}
              </>
            )}
          </div>
        </div>
      </div>

      {linking && (
        <section className="link-panel" aria-label="החומרים שהסעיף נכתב מהם">
          <p className="small muted">
            מסמנים את החומרים שהסעיף ייכתב מהם. חומר שמסומן כאן נשלח לסעיף הזה (אחרי הסתרה), והבחירה שלך גוברת על המיון.
          </p>
          <ul className="link-list">
            {api.detail.inputs.map((i) => {
              const r = api.detail.routing.find((x) => x.input_id === i.id);
              const on = r?.feeds.includes(section.key) ?? false;
              const others = (r?.feeds ?? []).filter((k) => k !== section.key).length;
              return (
                <li key={i.id}>
                  <label className="link-row">
                    <input type="checkbox" checked={on} disabled={!r}
                      onChange={() => void act(async () => {
                        if (!r) return;
                        const feeds = on ? r.feeds.filter((k) => k !== section.key) : [...r.feeds, section.key];
                        await ipc.setInputSections(caseId, i.id, feeds);
                        setLinked(true);
                      })} />
                    <span className="grow">
                      <b>{i.title || kindLabel[i.kind]}</b>
                      <span className="small muted"> · {kindLabel[i.kind]}{others > 0 ? ` · מזין עוד ${others} סעיפים` : ""}</span>
                    </span>
                  </label>
                </li>
              );
            })}
            {api.detail.inputs.length === 0 && <li className="muted small">עוד אין חומרים בתיק.</li>}
          </ul>
          {linked && visible.length > 0 && (
            <div className="row">
              <span className="small grow">החומרים עודכנו. כדי שהטיוטה תיכתב גם מהם:</span>
              <button type="button" className="btn btn-small btn-primary" disabled={working}
                onClick={() => { setLinked(false); setLinking(false); void ask(draftInstruction, `טיוטה חדשה לסעיף ${section.title}`); }}>
                טיוטה חדשה מ-Claude
              </button>
            </div>
          )}
          <button type="button" className="link-small" onClick={() => go({ name: "case", id: caseId, view: "materials" })}>להוספת חומר חדש לתיק ←</button>
        </section>
      )}

      <div className="view-body">
        <section className="paper draft" aria-label="טיוטת הסעיף">
          <div className="draft-head">
            <span className="draft-label">טיוטת הסעיף</span>
            <span>{approved > 0 && `${approved} פסקאות אושרו`}{approved > 0 && pending > 0 && " · "}{pending > 0 && `${pending} ממתינות לאישורך`}</span>
          </div>

          {job && (
            <div className="writing-card">
              <ProgressLine started={job.started} estimate={job.estimate} label={job.label} approval={job.approval} />
              <span className="small muted">
                השמות הוסתרו והבקשה נשלחה. אפשר להמשיך לעבוד בינתיים בסעיפים אחרים; הטיוטה תופיע כאן, ותחליף את מה שעוד לא אישרת.
              </span>
            </div>
          )}
          {preparing && <p className="muted small"><Spinner /> מסתירה שמות ובודקת מה יוצא…</p>}

          {pending > 0 && !job && (
            <div className="draft-banner">
              <div className="grow stack" style={{ gap: 2 }}>
                <b>טיוטה של Claude מחכה לאישורך</b>
                <span className="small">
                  קוראים, ואז מאשרים את כולה או פסקה-פסקה. רק מה שאושר נכנס לדוח. בקשה חדשה (למשל "לקצר") מחליפה את הטיוטה הזאת, ומה שכבר אישרת נשאר.
                </span>
              </div>
              <button type="button" className="btn btn-primary" onClick={() => void act(async () => { await ipc.approveSection(caseId, section.key); })}>
                אישור כל הטיוטה ({pending})
              </button>
            </div>
          )}
          {pending === 0 && approved > 0 && !job && (
            <div className="draft-banner done">
              <span className="grow"><b>✓ הסעיף אושר.</b> הוא ייכנס לדוח כמו שהוא כאן.</span>
              {nextSection && (
                <button type="button" className="btn btn-primary" onClick={() => go({ name: "case", id: caseId, view: nextSection.key })}>
                  לסעיף הבא: {nextSection.title} ←
                </button>
              )}
            </div>
          )}

          {visible.length === 0 && !job && (
            <div className="draft-empty">
              <p className="serif">עוד אין טיוטה לסעיף הזה.</p>
              <p className="hint">
                {derived
                  ? "Claude יכין טיוטה מהסעיפים שכבר אישרת. את קוראת ומאשרת כל פסקה."
                  : section.source_count > 0
                    ? "Claude יכין טיוטה מהחומרים שמזינים את הסעיף. לפני השליחה תראי בדיוק מה יוצא, ואחר כך מאשרים פסקה אחר פסקה."
                    : "עוד אין חומרים שמזינים את הסעיף. אפשר להוסיף אותם בחומרי התיק, או לכתוב בעצמך."}
              </p>
              <div className="row">
                {(derived || section.source_count > 0) && (
                  <button type="button" className="btn btn-primary" disabled={working}
                    onClick={() => void ask(draftInstruction, `טיוטה לסעיף ${section.title}`)}>
                    {derived ? "כתיבת טיוטה מהסעיפים שאושרו" : "כתיבת טיוטה עם Claude"}
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
                    {p.has_versions && (
                      <button type="button" className="link-small" aria-expanded={history === p.id} onClick={() => setHistory(history === p.id ? null : p.id)}>גרסאות קודמות</button>
                    )}
                  </div>
                  {history === p.id && (
                    <ParagraphVersions caseId={caseId} draftId={p.id} onRestored={async () => { setHistory(null); await reload(); }} />
                  )}
                </article>
              );
            }
            return (
              <article key={p.id} className="para pending">
                {!isEditing && (
                  <div className="para-meta">
                    <span className="pending-mark">{p.by_ai ? "הצעה של Claude · ממתינה לאישור" : "ממתינה לאישור"}</span>
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
                      <button type="button" className="btn btn-small" disabled={working}
                        onClick={() => void ask(`נסחי מחדש את הפסקה הבאה, באותו תוכן ובלי להוסיף עובדות: "${p.text}"`, "ניסוח מחדש של פסקה אחת", p.id)}>ניסוח מחדש</button>
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
            <div className="row wrap-row">
              <button type="button" className="add-own" onClick={() => setOwn("")}>+ פסקה משלי</button>
              {(derived || section.source_count > 0) && (
                <button type="button" className="add-own" disabled={working}
                  onClick={() => void ask(draftInstruction, `טיוטה חדשה לסעיף ${section.title}`)}>
                  ↻ טיוטה חדשה מ-Claude (מחליפה את מה שלא אושר)
                </button>
              )}
            </div>
          )}

          {last && (last.questions.length > 0 || last.missing.length > 0 || last.contradictions.length > 0) && (
            <div className="notes-from-claude">
              {last.contradictions.map((c, i) => <p key={`c${i}`}><b>סתירה בין החומרים:</b> {c}</p>)}
              {last.missing.map((c, i) => <p key={`m${i}`}><b>חסר מידע:</b> {c}</p>)}
              {last.questions.map((c, i) => <p key={`q${i}`}><b>שאלה:</b> {c}</p>)}
            </div>
          )}
          <ErrorLine error={error} />
        </section>

        <section className="chat" aria-label="שיחה על הסעיף">
          <div className="chat-head">שיחה על הסעיף</div>
          <div className="chat-body" aria-live="polite">
            {chat.length === 0 && <p className="muted small chat-hint">כאן מבקשים מ-Claude שינויים בטיוטה: לקצר, להרחיב, להדגיש משהו. כל בקשה עוברת קודם במסך "מה יוצא מהמחשב", והתשובה מחליפה את מה שעוד לא אישרת.</p>}
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
            {job && <div className="bubble-claude muted"><Spinner /> כותב…</div>}
            <div ref={chatEnd} />
          </div>
          <form className="chat-foot" onSubmit={submit}>
            <div className="quick">
              {QUICK.map(([label, text]) => (
                <button key={label} type="button" className="pill-btn" disabled={working || visible.length === 0} onClick={() => void ask(text, label)}>{label}</button>
              ))}
            </div>
            <div className="chat-input">
              <label htmlFor="chat-msg" className="visually-hidden">הודעה ל-Claude</label>
              <textarea id="chat-msg" rows={2} className="textarea grow" placeholder={chatHint}
                value={message} onChange={(e) => setMessage(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void ask(message); }} />
              <button type="submit" className="btn btn-primary send-btn" title="תמיד מוצג קודם מה יוצא מהמחשב" disabled={working || !message.trim()}>שליחה <SendIcon /></button>
            </div>
          </form>
        </section>
      </div>
    </div>
  );
}
