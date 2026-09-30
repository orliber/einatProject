import { useEffect, useState } from "react";
import { useApp } from "../../App";
import { ipc, type CaseDetail, type ReportSettings } from "../../ipc/client";
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
export function ReportView({ api, onWriteAll }: { api: CaseApi; onWriteAll: () => void }) {
  const { detail } = api;
  const [settings, setSettings] = useState<ReportSettings | null>(null);
  const [showDrafts, setShowDrafts] = useState(true);

  useEffect(() => {
    ipc.reportSettings().then(setSettings).catch(() => setSettings(null));
  }, []);

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
            עובדים ישירות על הדף: כותבים, מאשרים ועורכים. {approved} סעיפים מאושרים ייכנסו לקובץ · {waiting} עם טיוטה לאישור · {empty} ריקים
          </span>
        </div>
        <div className="row">
          <label className="row small">
            <input type="checkbox" checked={showDrafts} onChange={(e) => setShowDrafts(e.target.checked)} />
            להראות טיוטות שעוד לא אושרו
          </label>
          {empty > 0 && <button type="button" className="btn btn-primary" onClick={onWriteAll}>כתיבת הסעיפים הריקים עם Claude</button>}
        </div>
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

function A4Section({ api, section: s, showDrafts }: { api: CaseApi; section: Section; showDrafts: boolean }) {
  const { go, fail } = useApp();
  const { caseId, reload } = api;
  const [editing, setEditing] = useState<{ id: string | null; text: string } | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const job = api.jobs.find((j) => j.section === s.key);
  const ok = s.paragraphs.filter((p) => p.status === "approved");
  const pending = s.paragraphs.filter((p) => p.status === "proposed");
  const canWrite = s.source_count > 0 || !s.sortable;
  const busy = preparing || job !== undefined;

  async function act(fn: () => Promise<unknown>) {
    setError(null);
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  async function write(instruction?: string) {
    setError(null);
    setPreparing(true);
    try {
      await api.draft(s.key, instruction);
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
    <div className={`a4-section${pending.length ? " has-pending" : ""}`}>
      <div className="a4-heading-row">
        <h3 className="a4-heading">{s.title}</h3>
        <span className="a4-tools">
          {canWrite && !busy && (
            <button type="button" className="a4-tool" onClick={() => void write()}>
              {s.paragraphs.length ? "↻ טיוטה חדשה" : "✦ כתיבה עם Claude"}
            </button>
          )}
          {!editing && <button type="button" className="a4-tool" onClick={() => setEditing({ id: null, text: "" })}>+ פסקה משלי</button>}
          <button type="button" className="a4-tool" onClick={() => go({ name: "case", id: caseId, view: s.key })}>פתיחת הסעיף ←</button>
        </span>
      </div>

      {ok.map((p) =>
        editing?.id === p.id ? (
          <div key={p.id}>{editor}</div>
        ) : (
          <p key={p.id} className="a4-p a4-editable" tabIndex={0} title="לחיצה לעריכה"
            onClick={() => setEditing({ id: p.id, text: p.text })}
            onKeyDown={(e) => { if (e.key === "Enter") setEditing({ id: p.id, text: p.text }); }}>
            {p.text}
          </p>
        ),
      )}

      {job && (
        <div className="a4-writing">
          <ProgressLine started={job.started} estimate={job.estimate} label={job.label} />
        </div>
      )}
      {preparing && !job && <p className="small muted">מכינה את הבקשה…</p>}

      {showDrafts && pending.length > 0 && !job && (
        <div className="a4-pending">
          <div className="a4-pending-head">
            <span className="a4-flag">טיוטה של Claude · עוד לא אושרה ולא תיכנס לקובץ</span>
            <span className="row">
              <button type="button" className="btn btn-primary btn-small" onClick={() => void act(() => ipc.approveSection(caseId, s.key))}>
                ✓ אישור הטיוטה
              </button>
              <button type="button" className="btn btn-small btn-ghost" onClick={() => void act(async () => { for (const p of pending) await ipc.rejectParagraph(caseId, p.id); })}>
                הסרה
              </button>
            </span>
          </div>
          {pending.map((p) =>
            editing?.id === p.id ? (
              <div key={p.id}>{editor}</div>
            ) : (
              <p key={p.id} className="a4-p a4-editable" tabIndex={0} title="לחיצה לעריכה (העריכה מאשרת את הפסקה)"
                onClick={() => setEditing({ id: p.id, text: p.text })}
                onKeyDown={(e) => { if (e.key === "Enter") setEditing({ id: p.id, text: p.text }); }}>
                {p.text}
              </p>
            ),
          )}
        </div>
      )}

      {editing && editing.id === null && editor}

      {s.paragraphs.length === 0 && !job && !editing && (
        canWrite ? (
          <button type="button" className="a4-gap" disabled={busy} onClick={() => void write()}>
            ✦ הסעיף עוד לא נכתב · לכתיבה עם Claude
          </button>
        ) : (
          <button type="button" className="a4-gap" onClick={() => go({ name: "case", id: caseId, view: s.key })}>
            אין חומרים לסעיף · לקישור חומרים או לכתיבה בעצמך
          </button>
        )
      )}
      <ErrorLine error={error} />
    </div>
  );
}
