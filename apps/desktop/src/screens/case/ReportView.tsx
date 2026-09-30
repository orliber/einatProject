import { useEffect, useState } from "react";
import { useApp } from "../../App";
import { ipc, type ReportSettings } from "../../ipc/client";
import { ageWords } from "../../components/AgeField";
import type { CaseApi } from "../CaseScreen";
import "./ReportView.css";

/**
 * The report as the Word file will look: an A4 page with the title, the child's details, the
 * parts and sections in order. Only approved paragraphs go into the file; a draft waiting for
 * approval is shown marked, and an empty section as a gap to fill, each one click away.
 */
export function ReportView({ api, onWriteAll }: { api: CaseApi; onWriteAll: () => void }) {
  const { go } = useApp();
  const { detail, caseId } = api;
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
  const open = (key: string) => go({ name: "case", id: caseId, view: key });
  const writing = new Set(api.jobs.map((j) => j.section));

  return (
    <div className="view">
      <div className="view-head">
        <div className="stack" style={{ gap: 6 }}>
          <h1>הדוח כמו בוורד</h1>
          <span className="muted small">
            {approved} סעיפים מאושרים ייכנסו לקובץ · {waiting} עם טיוטה לאישור · {empty} ריקים
          </span>
        </div>
        <div className="row">
          <label className="row small">
            <input type="checkbox" checked={showDrafts} onChange={(e) => setShowDrafts(e.target.checked)} />
            להראות גם טיוטות שעוד לא אושרו
          </label>
          {empty > 0 && <button type="button" className="btn" onClick={onWriteAll}>כתיבת הסעיפים הריקים</button>}
        </div>
      </div>
      <div className="report-desk">
        <article className="a4" style={{ fontFamily: settings?.font ? `"${settings.font}", var(--font-display)` : undefined }} aria-label="תצוגה של קובץ הדוח">
          <header className="a4-header">{settings?.confidentiality}</header>
          <h1 className="a4-title">{settings?.title ?? "דוח אבחון פסיכולוגי"}</h1>
          <dl className="a4-info">
            {child && <><dt>{childLabel}</dt><dd>{child}</dd></>}
            {detail.meta.age && <><dt>גיל בזמן האבחון</dt><dd>{ageWords(detail.meta.age)}</dd></>}
            <dt>תאריך הדוח</dt><dd>{new Date().toLocaleDateString("he-IL")}</dd>
          </dl>
          {parts.map((part) => {
            const sections = content.filter((s) => s.part === part);
            return (
              <section key={part}>
                <h2 className="a4-part">{part}</h2>
                {sections.map((s) => {
                  const ok = s.paragraphs.filter((p) => p.status === "approved");
                  const pending = s.paragraphs.filter((p) => p.status === "proposed");
                  return (
                    <div key={s.key} className="a4-section">
                      <h3 className="a4-heading">
                        <button type="button" onClick={() => open(s.key)} title="פתיחת הסעיף">{s.title}</button>
                      </h3>
                      {ok.map((p) => <p key={p.id} className="a4-p">{p.text}</p>)}
                      {showDrafts && pending.length > 0 && (
                        <div className="a4-pending">
                          <span className="a4-flag">טיוטה של Claude · עוד לא אושרה ולא תיכנס לקובץ</span>
                          {pending.map((p) => <p key={p.id} className="a4-p">{p.text}</p>)}
                          <button type="button" className="btn btn-small" onClick={() => open(s.key)}>לאישור בסעיף</button>
                        </div>
                      )}
                      {s.paragraphs.length === 0 && (
                        <button type="button" className="a4-gap" onClick={() => open(s.key)}>
                          {writing.has(s.key) ? "Claude כותב את הסעיף…" : s.source_count > 0 || !s.sortable ? "עוד לא נכתב · לכתיבה" : "אין חומרים לסעיף · הסעיף לא ייכנס לקובץ"}
                        </button>
                      )}
                    </div>
                  );
                })}
              </section>
            );
          })}
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
