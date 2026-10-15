import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { useKept } from "../../kept";
import { useApp } from "../../App";
import { ipc, type CaseDetail, type ChatView, type ParagraphCheck, type SectionResult } from "../../ipc/client";
import { useQuery } from "@tanstack/react-query";
import { VerdictNote } from "../../components/VerdictNote";
import { ErrorLine, SendIcon, Spinner, WarnIcon } from "../../components/ui";
import { ProgressLine } from "../../components/Progress";
import { Why } from "../../components/Why";
import { Similar } from "../../components/Similar";
import { useHoldUnsaved } from "../../components/Unsaved";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { kindLabel } from "../../i18n/he";
import { useRotatingPlaceholder } from "../../components/Rotating";
import { DRAFT_HINTS } from "../../i18n/suggestions";
import { ParagraphVersions } from "../../components/ParagraphVersions";
import { SectionGuide } from "../../components/SectionGuide";
import { OwnSectionPanel } from "../../components/OwnSection";
import { MarkedText, StyleHintView, applyMark } from "./StyleHintView";
import { isCombo, KEYS } from "../../shortcuts";
import type { CaseApi } from "../CaseScreen";
import "./SectionWork.css";
import { useAi } from "../../ai";

type Section = CaseDetail["sections"][number];

const DERIVED = ["summary", "diagnoses", "recommendations", "dsm"];
/** A line of the guide no paragraph of the first draft answers (round 29): it says so itself. */
const GUIDE_MISSING = "חסרה פסקה לפי ההנחיה";

const QUICK: [string, string][] = [
  ["להרחיב", "הרחיבי את הטיוטה של הסעיף, בלי להוסיף עובדות שאינן במקורות."],
  ["לקצר", "קצרי את הטיוטה של הסעיף ושמרי על כל הממצאים העיקריים."],
  ["לבדוק מול החומרים", "בדקי את הטיוטה מול המקורות: מה חסר, מה סותר, ומה לא מבוסס."],
  ["להציע המלצות", "הציעי המלצות שנובעות מהממצאים שבמקורות, כל אחת עם הנימוק שלה."],
];

export function SectionWork({ api, section }: { api: CaseApi; section: Section }) {
  const { fail, notify, go } = useApp();
  const ai = useAi();
  const { caseId, reload } = api;
  const [chat, setChat] = useState<ChatView[]>([]);
  // Kept while she works in another section or screen (the answer arrives there too):
  // the request she typed, the one on its way, and what came back with the last answer.
  const keep = `case:${caseId}:section:${section.key}`;
  const [last, setLast] = useKept<SectionResult | null>(`${keep}:last`, null);
  // The request being typed lives in its box (F1a); `message` is the kept, debounced copy.
  const [message, setMessage] = useKept(`${keep}:message`, "");
  const [messageEmpty, setMessageEmpty] = useState(() => !message.trim());
  const messageBox = useRef<LocalTextHandle>(null);
  /** The last text the box reported, to tell its own reports from changes made elsewhere. */
  const reported = useRef(message);
  const [asked, setAsked] = useKept<string | null>(`${keep}:asked`, null);
  const chatHint = `${useRotatingPlaceholder(DRAFT_HINTS, !messageEmpty)} · שמות יוסתרו אוטומטית`;
  const [busy, setBusy] = useState(false);
  /** A paragraph being edited: the text is in `editBox`; `text` is its debounced copy, for the lock. */
  const [editing, setEditing] = useKept<{ id: string; text: string } | null>(`${keep}:editing`, null);
  const editBox = useRef<LocalTextHandle>(null);
  /** A paragraph of her own being written (in `ownBox`), with its debounced copy, or `null`. */
  // Both kept while the vault is open: leaving and coming back finds the editor with her text.
  const [own, setOwn] = useKept<string | null>(`${keep}:own`, null);
  const [ownEmpty, setOwnEmpty] = useState(() => !(own ?? "").trim());
  const ownBox = useRef<LocalTextHandle>(null);
  const startOwn = () => { setOwnEmpty(true); setOwn(""); };
  const [error, setError] = useState<string | null>(null);
  const [linking, setLinking] = useState(false);
  const [linked, setLinked] = useState(false);
  /** The paragraph whose earlier wordings are open (UX-4). */
  const [history, setHistory] = useState<string | null>(null);
  /** Paragraphs whose sources are open ("למה כתבת את זה?", AI-7). */
  const [why, setWhy] = useState<string | null>(null);
  const [similar, setSimilar] = useState<string | null>(null);
  /** What the check against the sources said, per proposed paragraph (until the draft changes). */
  const [checks, setChecks] = useKept<Map<string, ParagraphCheck>>(`${keep}:checks`, new Map());
  const verifyOn = useQuery({ queryKey: ["verify-stage"], queryFn: ipc.verifyStageOn }).data === true;
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

  // The kept request changed elsewhere (cleared when its answer came, maybe while another visit
  // of this screen was open): the box takes it, unless she has typed in it since its last report.
  useEffect(() => {
    const box = messageBox.current;
    if (!box || message === reported.current) return;
    if (box.value() === reported.current) box.set(message);
    else reported.current = message;
  }, [message]);

  // One request at a time per section: while it is prepared, reviewed or written.
  const job = api.jobs.find((j) => j.section === section.key);
  const holdLive = useHoldUnsaved(caseId, `פסקה בסעיף "${section.title}"`, editing?.text ?? own);
  const [preparing, setPreparing] = useState(false);
  const working = preparing || busy || job !== undefined;

  /** The send of the last request, kept while it failed: "לנסות שוב" sends the same approved
   *  request again and takes in its answer like the first time (the core kept the approval). */
  const sending = useRef<(() => Promise<void>) | null>(null);
  const [resend, setResend] = useState<(() => void) | null>(null);

  function failed(e: unknown) {
    const again = sending.current;
    setResend(() => (again ? () => void sendAgain(again) : null));
    setError(fail(e as never));
  }

  async function sendAgain(send: () => Promise<void>) {
    setError(null);
    setResend(null);
    setBusy(true);
    try {
      await send();
      sending.current = null;
    } catch (e) {
      failed(e);
    } finally {
      setBusy(false);
    }
  }

  /** `replaces`: rewrite that one paragraph; otherwise the answer is the section's new draft.
   *  `points`: her own section, worded from her points only (round 30). Resolves `true` once
   *  the answer came. */
  async function ask(instruction: string, title?: string, replaces?: string, points = false): Promise<boolean> {
    if (!instruction.trim()) return false;
    setError(null);
    setResend(null);
    sending.current = null;
    setPreparing(true);
    let went = false;
    try {
      const prepare = () => points
        ? ipc.prepareFromPoints(caseId, section.key, instruction)
        : ipc.prepareSection(caseId, section.key, instruction, replaces);
      const prepared = await prepare();
      setPreparing(false);
      setBusy(true);
      const send = async (id: string) => {
        setAsked(instruction);
        await api.track(
          { label: replaces ? `${ai} מנסח את הפסקה מחדש` : `${ai} כותב את "${section.title}"`, task: "draft", section: section.key, approval: id },
          () => ipc.sendSection(id),
          async (result) => {
            went = true;
            setLast(result);
            // Only the request that was sent is cleared, not one she typed meanwhile.
            setMessage((m) => (m === instruction ? "" : m));
            await Promise.all([reload(), loadChat()]);
            if (result.demo) notify("מצב הדגמה: התשובה נבנתה במחשב, בלי שליחה.");
          },
        ).finally(() => setAsked(null));
      };
      await api.review({
        title: title ?? `בקשה לסעיף ${section.title}`,
        prepared,
        reprepare: prepare,
        onSend: async (id) => {
          sending.current = () => send(id);
          await send(id);
          sending.current = null;
        },
      });
    } catch (e) {
      failed(e);
    } finally {
      setPreparing(false);
      setBusy(false);
    }
    return went;
  }

  /** The check of the proposed paragraphs against the sources: its own request and review. */
  async function verify() {
    setError(null);
    setResend(null);
    setPreparing(true);
    try {
      const prepare = () => ipc.prepareVerify(caseId, section.key);
      const prepared = await prepare();
      setPreparing(false);
      setBusy(true);
      await api.review({
        title: `בדיקת הטיוטה של ${section.title} מול המקורות`,
        prepared,
        reprepare: prepare,
        onSend: async (id) => {
          await api.track(
            { label: `${ai} בודק את "${section.title}" מול המקורות`, task: "draft", approval: id },
            () => ipc.sendVerify(id),
            (result) => {
              setChecks(new Map(result.draft_ids.map((d, i) => [d, result.checks[i]] as [string, ParagraphCheck])));
              if (result.demo) notify("מצב הדגמה: נבדקו רק מספרים שאין במקורות, בלי שליחה.");
            },
          );
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

  /** The request as it is in the box now (never the debounced copy). */
  const typed = () => messageBox.current?.value() ?? message;

  function submit(e: FormEvent) {
    e.preventDefault();
    if (!working) void ask(typed());
  }

  const derived = DERIVED.includes(section.key);
  /** Hers to write (round 30): nothing of the case is sent for it but her points. */
  const mine = section.mode === "self";
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
            {mine ? (
              <span className="muted">הסעיף הזה שלך: {ai} לא כותב אותו מהחומרים, ושום חומר לא נשלח בשבילו.</span>
            ) : derived ? (
              <span className="muted">הסעיף נכתב מתוך הסעיפים שכבר אישרת.</span>
            ) : (
              <>
                <span className="muted">{sources.length ? `${ai} כותב מתוך:` : "עוד אין חומרים שמזינים את הסעיף."}</span>
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
            מסמנים את החומרים לסעיף. הבחירה שלך גוברת על המיון.
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
                טיוטה חדשה מ-{ai}
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
          {/* Inside the scrolling draft: on a laptop the open guide never pushes the
              conversation's foot off the screen (the review of stage 3). */}
          <SectionGuide caseId={caseId} sectionKey={section.key} guide={section.guide} isDefault={section.guide_is_default}
            learnedFrom={section.guide_learned_from} caseGuide={section.case_guide} onSaved={reload} />

          {job && (
            <div className="writing-card">
              <ProgressLine started={job.started} estimate={job.estimate} label={job.label} approval={job.approval} />
              <span className="small muted">
                הבקשה נשלחה (בלי שמות). אפשר לעבוד בינתיים בסעיפים אחרים.
              </span>
            </div>
          )}
          {preparing && <p className="muted small"><Spinner /> מסתירה שמות ובודקת מה יוצא…</p>}

          {pending > 0 && !job && (
            <div className="draft-banner">
              <div className="grow stack" style={{ gap: 2 }}>
                <b>טיוטה של {ai} מחכה לאישורך</b>
                <span className="small">
                  מאשרים את כולה או פסקה-פסקה. רק מה שאושר נכנס לדוח.
                </span>
              </div>
              {verifyOn && !derived && (
                <button type="button" className="btn" disabled={working} onClick={() => void verify()}>
                  בדיקה מול המקורות
                </button>
              )}
              <button type="button" className="btn btn-primary" onClick={() => void act(async () => { await ipc.approveSection(caseId, section.key); })}>
                אישור כל הטיוטה ({pending})
              </button>
            </div>
          )}
          {pending === 0 && approved > 0 && !job && (
            <div className="draft-banner done">
              <span className="grow"><b>✓ הסעיף אושר.</b></span>
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
                {mine
                  ? "את כותבת את הסעיף הזה: פסקה משלך, או נקודות שמנוסחות לפסקה (בצד)."
                  : derived
                  ? `${ai} יכין טיוטה מהסעיפים שאישרת.`
                  : section.source_count > 0
                    ? `${ai} יכין טיוטה מהחומרים של הסעיף. לפני השליחה תראי מה יוצא.`
                    : "אין עדיין חומרים לסעיף. מוסיפים בחומרי התיק, או כותבים לבד."}
              </p>
              <div className="row">
                {!mine && (derived || section.source_count > 0) && (
                  <button type="button" className="btn btn-primary" disabled={working}
                    onClick={() => void ask(draftInstruction, `טיוטה לסעיף ${section.title}`)}>
                    {derived ? "כתיבת טיוטה מהסעיפים שאושרו" : `כתיבת טיוטה עם ${ai}`}
                  </button>
                )}
                <button type="button" className="btn" onClick={startOwn}>כתיבה בעצמי</button>
              </div>
            </div>
          )}

          {visible.map((p) => {
            const warnings = p.warnings.length ? p.warnings : lastWarnings.get(p.text) ?? [];
            const isEditing = editing?.id === p.id;
            const check = checks.get(p.id);
            if (p.status === "approved" && !isEditing) {
              return (
                <article key={p.id} className="para">
                  <p className="serif para-text">{p.text}</p>
                  <div className="para-meta">
                    <span className="approved-mark">✓ אושר על ידך</span>
                    {p.sources.length > 0 && <span className="muted">מקור: {p.sources.join(" · ")}</span>}
                    {p.by_ai && <button type="button" className="link-small" aria-expanded={why === p.id} onClick={() => setWhy(why === p.id ? null : p.id)}>למה כתבת את זה?</button>}
                    <button type="button" className="link-small" onClick={() => setEditing({ id: p.id, text: p.text })}>עריכה</button>
                    {p.has_versions && (
                      <button type="button" className="link-small" aria-expanded={history === p.id} onClick={() => setHistory(history === p.id ? null : p.id)}>גרסאות קודמות</button>
                    )}
                  </div>
                  {history === p.id && (
                    <ParagraphVersions caseId={caseId} draftId={p.id} onRestored={async () => { setHistory(null); await reload(); }} />
                  )}
                  {warnings.map((w, i) => (
                    <div key={i} className="note-warn"><WarnIcon /><span>{w}</span></div>
                  ))}
                  {why === p.id && <Why caseId={caseId} draftId={p.id} />}
                </article>
              );
            }
            return (
              <article key={p.id} className="para pending">
                {!isEditing && (
                  <div className="para-meta">
                    <span className="pending-mark">{p.by_ai ? `הצעה של ${ai} · ממתינה לאישור` : "ממתינה לאישור"}</span>
                    {p.sources.length > 0 && <span className="muted">מקור: {p.sources.join(" · ")}</span>}
                    {p.by_ai && <button type="button" className="link-small" aria-expanded={why === p.id} onClick={() => setWhy(why === p.id ? null : p.id)}>למה כתבת את זה?</button>}
                    <button type="button" className="link-small" aria-expanded={similar === p.id} onClick={() => setSimilar(similar === p.id ? null : p.id)}>פסקה דומה מהדוחות שלי</button>
                  </div>
                )}
                {!isEditing && why === p.id && <Why caseId={caseId} draftId={p.id} />}
                {similar === p.id && <Similar caseId={caseId} draftId={p.id} />}
                {!isEditing && check && <VerdictNote check={check} />}
                {isEditing ? (
                  <label className="edit-box">
                    <span className="visually-hidden">עריכת הפסקה</span>
                    <LocalText key={p.id} ref={editBox} className="textarea serif" rows={5} initial={editing.text} autoFocus onLive={holdLive}
                      onText={(text) => setEditing((e) => (e?.id === p.id ? { id: p.id, text } : e))} />
                  </label>
                ) : (
                  <p className="serif para-text"><MarkedText text={p.text} marks={p.style?.marks ?? []} /></p>
                )}
                {warnings.map((w, i) => (
                  <div key={i} className="note-warn"><WarnIcon /><span>{w}</span></div>
                ))}
                {!isEditing && p.style && !mine && (
                  <StyleHintView hint={p.style} busy={working}
                    onCloser={(how) => void ask(`נסחי מחדש את הפסקה הבאה כך שתישמע כמו הכתיבה שלי: ${how}. באותו תוכן ובלי להוסיף עובדות: "${p.text}"`, "קירוב פסקה לסגנון שלך", p.id)}
                    onApply={(m) => void act(() => ipc.editParagraph(caseId, p.id, applyMark(p.text, m)))} />
                )}
                <div className="row wrap-row">
                  {isEditing ? (
                    <>
                      <button type="button" className="btn btn-primary btn-small"
                        onClick={() => {
                          const text = editBox.current?.value() ?? editing.text;
                          void act(async () => { await ipc.editParagraph(caseId, p.id, text); await ipc.approveParagraph(caseId, p.id); setEditing(null); });
                        }}>שמירה ואישור</button>
                      <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
                      <button type="button" className="link-small" aria-expanded={similar === p.id} onClick={() => setSimilar(similar === p.id ? null : p.id)}>פסקה דומה מהדוחות שלי</button>
                    </>
                  ) : (
                    <>
                      <button type="button" className="btn btn-primary btn-small" onClick={() => void act(() => ipc.approveParagraph(caseId, p.id))}>אישור</button>
                      {!mine && (
                        <button type="button" className="btn btn-small" disabled={working}
                          onClick={() => void ask(`נסחי מחדש את הפסקה הבאה, באותו תוכן ובלי להוסיף עובדות: "${p.text}"`, "ניסוח מחדש של פסקה אחת", p.id)}>ניסוח מחדש</button>
                      )}
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
                <LocalText ref={ownBox} className="textarea serif" rows={4} initial={own} autoFocus onLive={holdLive} placeholder="כתבי כאן. שמות מוסתרים לפני כל שליחה."
                  onText={(text) => setOwn((o) => (o === null ? null : text))} onEmptyChange={setOwnEmpty} />
              </label>
              <div className="row">
                <button type="button" className="btn btn-primary btn-small" disabled={ownEmpty}
                  onClick={() => {
                    const text = ownBox.current?.value() ?? own;
                    if (!text.trim()) return;
                    void act(async () => { await ipc.addOwnParagraph(caseId, section.key, text); setOwn(null); });
                  }}>הוספה לדוח</button>
                <button type="button" className="btn btn-small" onClick={() => setOwn(null)}>ביטול</button>
              </div>
            </div>
          ) : visible.length > 0 && (
            <div className="row wrap-row">
              <button type="button" className="add-own" onClick={startOwn}>+ פסקה משלי</button>
              {!mine && (derived || section.source_count > 0) && (
                <button type="button" className="add-own" disabled={working}
                  onClick={() => void ask(draftInstruction, `טיוטה חדשה לסעיף ${section.title}`)}>
                  ↻ טיוטה חדשה מ-{ai} (מחליפה את מה שלא אושר)
                </button>
              )}
            </div>
          )}

          {last && (last.questions.length > 0 || last.missing.length > 0 || last.contradictions.length > 0) && (
            <div className="notes-from-claude">
              {last.contradictions.map((c, i) => <p key={`c${i}`}><b>סתירה בין החומרים:</b> {c}</p>)}
              {last.missing.map((c, i) => <p key={`m${i}`}>{c.startsWith(GUIDE_MISSING) ? c : <><b>חסר מידע:</b> {c}</>}</p>)}
              {last.questions.map((c, i) => <p key={`q${i}`}><b>שאלה:</b> {c}</p>)}
            </div>
          )}
          <ErrorLine error={error} retry={resend ?? undefined} />
        </section>

        {mine ? (
          <OwnSectionPanel caseId={caseId} sectionKey={section.key} ai={ai} working={working}
            onWord={(points) => ask(points, `ניסוח "${section.title}" מהנקודות שלך`, undefined, true)} />
        ) : (
        <section className="chat" aria-label="שיחה על הסעיף">
          <div className="chat-head">שיחה על הסעיף</div>
          <div className="chat-body" aria-live="polite">
            {chat.length === 0 && <p className="muted small chat-hint">כאן מבקשים מ-{ai} שינויים בטיוטה: לקצר, להרחיב, להדגיש.</p>}
            {chat.map((m, i) =>
              m.role === "user" ? (
                <div key={i} className="msg-user">
                  <div className="bubble-user">{m.text}</div>
                  <span className="shield-line">
                    {m.hidden.length ? `הוסתרו ${m.hidden.length}: ${m.hidden.join(", ")}` : "לא נמצאו פרטים מזהים"}
                  </span>
                </div>
              ) : (
                <div key={i} className="bubble-claude">
                  {m.demo && <span className="chip chip-sand demo-chip">הדגמה</span>}
                  <span>{m.text}</span>
                </div>
              ),
            )}
            {job && asked && (
              <div className="msg-user">
                <div className="bubble-user">{asked}</div>
                <span className="shield-line">נשלחה, מחכה לתשובה</span>
              </div>
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
              <label htmlFor="chat-msg" className="visually-hidden">הודעה ל-{ai}</label>
              <LocalText id="chat-msg" ref={messageBox} rows={2} className="textarea grow" placeholder={chatHint}
                initial={message} onEmptyChange={setMessageEmpty}
                onText={(text) => { reported.current = text; setMessage(text); }}
                onKeyDown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !working) void ask(typed()); }} />
              <button type="submit" className="btn btn-primary send-btn" title="תמיד מוצג קודם מה יוצא מהמחשב" disabled={working || messageEmpty}>שליחה <SendIcon /></button>
            </div>
          </form>
        </section>
        )}
      </div>
    </div>
  );
}
