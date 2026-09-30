import { useEffect, useRef, useState, type ReactNode } from "react";
import { useApp } from "../../App";
import { ipc, type CaseDetail, type ReportSettings } from "../../ipc/client";
import { kindLabel } from "../../i18n/he";
import { ageWords } from "../../components/AgeField";
import { ErrorLine } from "../../components/ui";
import { ProgressLine } from "../../components/Progress";
import type { CaseApi } from "../CaseScreen";
import "./ReportView.css";

type Section = CaseDetail["sections"][number];

/**
 * The report as the Word file will look, and the place to work on it: an A4 page with the
 * title, the child's details and every section in order. On the page: write a section with
 * Claude, approve a draft, edit any paragraph, add one of her own. Only approved paragraphs
 * go into the file; a draft waiting for approval is marked, an empty section is a gap to fill.
 */
export function ReportView({ api, focus }: { api: CaseApi; onWriteAll: () => void; focus?: { key: string; at: number } | null }) {
  const { detail } = api;
  const [settings, setSettings] = useState<ReportSettings | null>(null);
  const [showDrafts, setShowDrafts] = useState(true);

  useEffect(() => {
    ipc.reportSettings().then(setSettings).catch(() => setSettings(null));
  }, []);

  // A section chosen in the contents or the next-step bar comes into view, briefly marked.
  useEffect(() => {
    if (!focus?.key) return;
    const el = document.getElementById(`sec-${focus.key}`);
    if (!el) return;
    el.scrollIntoView({ block: "start", behavior: "smooth" });
    el.classList.add("a4-flash");
    const t = setTimeout(() => el.classList.remove("a4-flash"), 1600);
    return () => clearTimeout(t);
  }, [focus]);

  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const childLabel = detail.meta.child_gender === "female" ? "שם הילדה" : detail.meta.child_gender === "male" ? "שם הילד" : "שם הילד/ה";
  const parts = Array.from(new Set(detail.sections.map((s) => s.part)));
  const content = detail.sections.filter((s) => s.key !== "signature");
  const approved = content.filter((s) => s.approved).length;
  const waiting = content.filter((s) => s.paragraphs.some((p) => p.status === "proposed")).length;
  const empty = content.filter((s) => s.paragraphs.length === 0).length;
  const signature = detail.sections.find((s) => s.key === "signature");
  const signatureLines = signature?.paragraphs.filter((p) => p.status === "approved").flatMap((p) => p.text.split("\n")) ?? [];
  const sign = signatureLines.length ? signatureLines : settings?.signature ?? [];

  return (
    <div className="view">
      <div className="view-head">
        <div className="stack" style={{ gap: 6 }}>
          <h1>הדוח</h1>
          <span className="muted small">
            כמו בקובץ. כותבים, מאשרים ועורכים ישירות על הדף. {approved} סעיפים מאושרים ייכנסו לקובץ · {waiting} לאישור · {empty} ריקים
          </span>
        </div>
        <label className="row small">
          <input type="checkbox" checked={showDrafts} onChange={(e) => setShowDrafts(e.target.checked)} />
          להראות טיוטות שעוד לא אושרו
        </label>
      </div>
      <div className="report-desk">
        <article className="a4" style={{ fontFamily: settings?.font ? `"${settings.font}", var(--font-display)` : undefined }} aria-label="הדוח, כמו בקובץ">
          <header className="a4-header">{settings?.confidentiality}</header>
          <h1 className="a4-title">{settings?.title ?? "דוח אבחון פסיכולוגי"}</h1>
          <dl className="a4-info">
            {child && <><dt>{childLabel}</dt><dd>{child}</dd></>}
            {detail.meta.age && <><dt>גיל בזמן האבחון</dt><dd>{ageWords(detail.meta.age)}</dd></>}
            <dt>תאריך הדוח</dt><dd>{new Date().toLocaleDateString("he-IL")}</dd>
          </dl>
          {parts.map((part) => (
            <section key={part}>
              <h2 className="a4-part">{part}</h2>
              {content.filter((s) => s.part === part).map((s) => (
                <A4Section key={s.key} api={api} section={s} showDrafts={showDrafts} />
              ))}
            </section>
          ))}
          {sign.length > 0 && (
            <footer className="a4-sign">
              {sign.map((l, i) => <p key={i}>{l}</p>)}
            </footer>
          )}
        </article>
      </div>
    </div>
  );
}

const CHANGES: [string, string][] = [
  ["לקצר", "קצרי את הטיוטה של הסעיף ושמרי על כל הממצאים העיקריים."],
  ["להרחיב", "הרחיבי את הטיוטה של הסעיף, בלי להוסיף עובדות שאינן במקורות."],
  ["לנסח אחרת", "נסחי את הטיוטה של הסעיף מחדש, באותו תוכן ובלי להוסיף עובדות."],
  ["לבדוק מול החומרים", "בדקי את הטיוטה מול המקורות: מה חסר, מה סותר, ומה לא מבוסס, ותקני."],
];

const PARA_CHANGES: [string, string][] = [
  ["לקצר", "קצרי את הפסקה הבאה ושמרי על כל הממצאים שבה."],
  ["לנסח אחרת", "נסחי מחדש את הפסקה הבאה, באותו תוכן ובלי להוסיף עובדות."],
  ["להרחיב", "הרחיבי את הפסקה הבאה, רק ממה שיש במקורות."],
  ["לפשט להורים", "נסחי את הפסקה הבאה בשפה פשוטה וברורה להורים, באותו תוכן."],
];

/** A small card next to what opened it; Escape or a click outside closes it. */
function Popover({ label, onClose, children }: { label: string; onClose: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  }, [onClose]);
  // Focus moves in once, when it opens (not on every keystroke inside it).
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("button, input, textarea")?.focus();
    const away = (e: MouseEvent) => {
      // The button that opened it (in the same anchor) toggles it itself.
      const anchor = ref.current?.parentElement ?? ref.current;
      if (anchor && !anchor.contains(e.target as Node)) close.current();
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, []);
  return (
    <div ref={ref} className="a4-pop" role="dialog" aria-label={label}
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      {children}
    </div>
  );
}

function A4Section({ api, section: s, showDrafts }: { api: CaseApi; section: Section; showDrafts: boolean }) {
  const { go, fail } = useApp();
  const { caseId, reload, detail } = api;
  const [editing, setEditing] = useState<{ id: string | null; text: string } | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [open, setOpen] = useState<"sources" | "change" | null>(null);
  /** The paragraph whose "✦ with Claude" card is open. */
  const [paraPop, setParaPop] = useState<string | null>(null);
  const [wish, setWish] = useState("");
  const [sourcesChanged, setSourcesChanged] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const job = api.jobs.find((j) => j.section === s.key);
  const ok = s.paragraphs.filter((p) => p.status === "approved");
  const proposed = s.paragraphs.filter((p) => p.status === "proposed");
  // A new wording of an approved paragraph is shown beside it, not as a new draft.
  const reworded = new Map(proposed.filter((p) => p.replaces).map((p) => [p.replaces ?? "", p]));
  const pending = proposed.filter((p) => !p.replaces);
  const canWrite = s.source_count > 0 || !s.sortable;
  const busy = preparing || job !== undefined;
  const sources = detail.routing
    .filter((r) => r.feeds.includes(s.key))
    .map((r) => detail.inputs.find((i) => i.id === r.input_id))
    .filter((i) => i !== undefined);

  async function act(fn: () => Promise<unknown>) {
    setError(null);
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  async function write(instruction?: string, replaces?: string) {
    setOpen(null);
    setParaPop(null);
    setSourcesChanged(false);
    setError(null);
    setPreparing(true);
    try {
      await api.draft(s.key, instruction, replaces);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setPreparing(false);
    }
  }

  async function save() {
    if (!editing?.text.trim()) return;
    await act(async () => {
      if (editing.id) await ipc.editParagraph(caseId, editing.id, editing.text);
      else await ipc.addOwnParagraph(caseId, s.key, editing.text);
      setEditing(null);
    });
  }

  /** One paragraph on the page: click to edit, "✦" to change it with Claude. */
  const para = (p: Section["paragraphs"][number], note?: string) =>
    editing?.id === p.id ? (
      <div key={p.id}>{editor}</div>
    ) : (
      <div key={p.id} className="a4-para">
        <p className="a4-p a4-editable" tabIndex={0} title={note ?? "לחיצה לעריכה"}
          onClick={() => setEditing({ id: p.id, text: p.text })}
          onKeyDown={(e) => { if (e.key === "Enter") setEditing({ id: p.id, text: p.text }); }}>
          {p.text}
        </p>
        {canWrite && !busy && (
          <span className="a4-pop-anchor a4-para-tool">
            <button type="button" className="a4-tool" aria-expanded={paraPop === p.id} title="לשנות את הפסקה עם Claude"
              onClick={() => setParaPop(paraPop === p.id ? null : p.id)}>✦</button>
            {paraPop === p.id && (
              <Popover label="מה לשנות בפסקה" onClose={() => setParaPop(null)}>
                <b>מה לשנות בפסקה הזאת?</b>
                <div className="row wrap-row">
                  {PARA_CHANGES.map(([label, text]) => (
                    <button key={label} type="button" className="pill-btn"
                      onClick={() => void write(`${text} הפסקה: "${p.text}"`, p.id)}>{label}</button>
                  ))}
                </div>
                <label className="stack small muted" style={{ gap: 4 }}>
                  או במילים שלך
                  <textarea className="textarea" rows={2} value={wish} placeholder="למשל: לכתוב בלשון פשוטה יותר להורים"
                    onChange={(e) => setWish(e.target.value)}
                    onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey && wish.trim()) { e.preventDefault(); const w = wish; setWish(""); void write(`${w} הפסקה: "${p.text}"`, p.id); } }} />
                </label>
                <div className="a4-pop-foot">
                  <span className="small muted grow">{p.status === "approved" ? "הניסוח החדש יופיע ליד הקיים, ויחליף אותו רק אם תאשרי." : "הניסוח החדש יחליף את הפסקה הזאת בטיוטה."}</span>
                  <button type="button" className="btn btn-primary btn-small" disabled={!wish.trim()}
                    onClick={() => { const w = wish; setWish(""); void write(`${w} הפסקה: "${p.text}"`, p.id); }}>שליחה</button>
                </div>
              </Popover>
            )}
          </span>
        )}
        {reworded.get(p.id) && (
          <div className="a4-reword">
            <span className="a4-flag">ניסוח חדש של Claude לפסקה הזאת</span>
            <p className="a4-p">{reworded.get(p.id)?.text}</p>
            <span className="row">
              <button type="button" className="btn btn-primary btn-small" onClick={() => void act(() => ipc.approveParagraph(caseId, reworded.get(p.id)?.id ?? ""))}>✓ להחליף</button>
              <button type="button" className="btn btn-small" onClick={() => void act(() => ipc.rejectParagraph(caseId, reworded.get(p.id)?.id ?? ""))}>להשאיר את הקודם</button>
            </span>
          </div>
        )}
      </div>
    );

  const editor = (
    <div className="a4-edit">
      <textarea className="textarea serif" rows={Math.min(12, Math.max(3, Math.ceil((editing?.text.length ?? 0) / 90)))} autoFocus
        aria-label={`עריכה בסעיף ${s.title}`} value={editing?.text ?? ""} placeholder="כותבים כאן. שמות אמיתיים נשמרים מוצפנים ומוסתרים לפני כל שליחה."
        onChange={(e) => setEditing(editing && { ...editing, text: e.target.value })}
        onKeyDown={(e) => { if (e.key === "Escape") setEditing(null); if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void save(); }} />
      <div className="row">
        <button type="button" className="btn btn-primary btn-small" disabled={!editing?.text.trim()} onClick={() => void save()}>שמירה ואישור</button>
        <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
        <span className="small muted">Ctrl+Enter לשמירה · Esc לביטול</span>
      </div>
    </div>
  );

  return (
    <div id={`sec-${s.key}`} className={`a4-section${pending.length ? " has-pending" : ""}`}>
      <div className="a4-heading-row">
        <h3 className="a4-heading">{s.title}</h3>
        <span className="a4-tools">
          {!editing && <button type="button" className="a4-tool" onClick={() => setEditing({ id: null, text: "" })}>+ פסקה משלי</button>}
        </span>
      </div>

      {s.sortable && (
        <div className="a4-sources">
          <span>{sources.length ? "נכתב מתוך:" : "אין חומרים לסעיף."}</span>
          {sources.map((i) => <span key={i.id} className="a4-source">{i.title || kindLabel[i.kind]}</span>)}
          <span className="a4-pop-anchor">
            <button type="button" className="a4-source-edit" aria-expanded={open === "sources"} onClick={() => setOpen(open === "sources" ? null : "sources")}>
              {sources.length ? "+ חומר / הסרה" : "+ קישור חומרים"}
            </button>
            {open === "sources" && (
              <Popover label={`מאיזה חומרים לכתוב את "${s.title}"`} onClose={() => setOpen(null)}>
                <b>מאיזה חומרים לכתוב את "{s.title}"?</b>
                <span className="small muted">מסמנים ומורידים. הבחירה שלך גוברת על המיון של Claude.</span>
                <div className="a4-pop-list">
                  {detail.inputs.map((i) => {
                    const r = detail.routing.find((x) => x.input_id === i.id);
                    const on = r?.feeds.includes(s.key) ?? false;
                    const others = (r?.feeds ?? []).filter((k) => k !== s.key).length;
                    return (
                      <label key={i.id} className={on ? "a4-pop-row on" : "a4-pop-row"}>
                        <input type="checkbox" checked={on} disabled={!r}
                          onChange={() => void act(async () => {
                            if (!r) return;
                            await ipc.setInputSections(caseId, i.id, on ? r.feeds.filter((k) => k !== s.key) : [...r.feeds, s.key]);
                            setSourcesChanged(true);
                          })} />
                        <span className="grow stack" style={{ gap: 0 }}>
                          <b>{i.title || kindLabel[i.kind]}</b>
                          <span className="small muted">{kindLabel[i.kind]}{others > 0 ? ` · מזין עוד ${others} סעיפים` : ""}</span>
                        </span>
                      </label>
                    );
                  })}
                  {detail.inputs.length === 0 && <span className="small muted">עוד אין חומרים בתיק.</span>}
                </div>
                <button type="button" className="link-small" onClick={() => go({ name: "case", id: caseId, view: "materials" })}>+ להעלות מסמך חדש לתיק</button>
                <div className="a4-pop-foot">
                  {sourcesChanged && s.paragraphs.length > 0 ? (
                    <>
                      <span className="small grow">החומרים השתנו. הטיוטה הנוכחית עוד לא כוללת אותם.</span>
                      <button type="button" className="btn btn-primary btn-small" disabled={busy} onClick={() => void write()}>✦ לכתוב מחדש</button>
                    </>
                  ) : <span className="grow" />}
                  <button type="button" className="btn btn-small" onClick={() => setOpen(null)}>סגירה</button>
                </div>
              </Popover>
            )}
          </span>
        </div>
      )}

      {ok.map((p) => para(p))}

      {job && (
        <div className="a4-writing">
          <ProgressLine started={job.started} estimate={job.estimate} label={job.label} approval={job.approval} />
        </div>
      )}
      {preparing && !job && <p className="small muted">מכינה את הבקשה…</p>}

      {showDrafts && pending.length > 0 && !job && (
        <div className="a4-pending">
          <div className="a4-pending-head">
            <span className="a4-flag">טיוטה של Claude · עוד לא בדוח</span>
            <span className="row">
              <button type="button" className="btn btn-primary btn-small" onClick={() => void act(async () => { for (const p of pending) await ipc.approveParagraph(caseId, p.id); })}>✓ לאשר</button>
              <span className="a4-pop-anchor">
                <button type="button" className="btn btn-small" aria-expanded={open === "change"} disabled={busy} onClick={() => setOpen(open === "change" ? null : "change")}>✦ לשנות עם Claude</button>
                {open === "change" && (
                  <Popover label="מה לשנות בטיוטה" onClose={() => setOpen(null)}>
                    <b>מה לשנות בטיוטה?</b>
                    <div className="row wrap-row">
                      {CHANGES.map(([label, text]) => (
                        <button key={label} type="button" className="pill-btn" onClick={() => void write(text)}>{label}</button>
                      ))}
                    </div>
                    <label className="stack small muted" style={{ gap: 4 }}>
                      או במילים שלך
                      <textarea className="textarea" rows={2} value={wish} placeholder="למשל: להדגיש את מה שעוזר לו במעברים"
                        onChange={(e) => setWish(e.target.value)}
                        onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey && wish.trim()) { e.preventDefault(); const w = wish; setWish(""); void write(w); } }} />
                    </label>
                    <div className="a4-pop-foot">
                      <span className="small muted grow">הטיוטה החדשה תחליף את זו. מה שאישרת נשאר.</span>
                      <button type="button" className="btn btn-primary btn-small" disabled={!wish.trim()} onClick={() => { const w = wish; setWish(""); void write(w); }}>שליחה</button>
                    </div>
                  </Popover>
                )}
              </span>
              <button type="button" className="btn btn-small btn-ghost" onClick={() => void act(async () => { for (const p of pending) await ipc.rejectParagraph(caseId, p.id); })}>הסרה</button>
            </span>
          </div>
          {pending.map((p) => para(p, "לחיצה לעריכה (העריכה מאשרת את הפסקה)"))}
        </div>
      )}

      {editing && editing.id === null && editor}

      {s.paragraphs.length === 0 && !job && !editing && (
        canWrite ? (
          <button type="button" className="a4-gap" disabled={busy} onClick={() => void write()}>
            ✦ לכתוב עם Claude · או "+ פסקה משלי" למעלה
          </button>
        ) : (
          <button type="button" className="a4-gap" onClick={() => setOpen("sources")}>
            אין חומרים לסעיף · לקישור חומרים, או "+ פסקה משלי"
          </button>
        )
      )}
      <ErrorLine error={error} />
    </div>
  );
}
