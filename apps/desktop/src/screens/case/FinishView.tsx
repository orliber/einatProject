import { Fragment, useEffect, useState } from "react";
import { useApp } from "../../App";
import { ipc, type ExportCheck, type ReportSettings } from "../../ipc/client";
import { ageWords } from "../../components/AgeField";
import { ErrorLine, Spinner } from "../../components/ui";
import type { CaseApi } from "../CaseScreen";
import "./FinishView.css";

const MARK = /\[חסר[^\]]*\]/;

/** A paragraph with its "[חסר: …]" markers highlighted. */
function Marked({ text }: { text: string }) {
  const parts = text.split(/(\[חסר[^\]]*\])/g);
  return <>{parts.map((p, i) => (p.startsWith("[חסר") ? <mark key={i} className="a4-mark">{p}</mark> : <Fragment key={i}>{p}</Fragment>))}</>;
}

/**
 * The last stage: the report as the file will be, always in view (almost empty, full, or with
 * things to complete), beside what is checked before the file is made. A missing-information
 * marker is completed right here; it never stops the file, it only asks her to confirm.
 */
export function FinishView({ api, onExport, onOpen }: { api: CaseApi; onExport: () => void; onOpen: (key: string) => void }) {
  const { fail } = useApp();
  const { detail, caseId, reload } = api;
  const [check, setCheck] = useState<ExportCheck | null>(null);
  const [settings, setSettings] = useState<ReportSettings | null>(null);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [withMarks, setWithMarks] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    ipc.checkExport(caseId).then((c) => alive && setCheck(c)).catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, detail, fail]);
  useEffect(() => {
    ipc.reportSettings().then(setSettings).catch(() => setSettings(null));
  }, []);

  const content = detail.sections.filter((s) => s.key !== "signature");
  const parts = Array.from(new Set(content.map((s) => s.part)));
  const waiting = content.filter((s) => s.paragraphs.some((p) => p.status === "proposed" && !p.replaces));
  const toComplete = content.flatMap((s) =>
    s.paragraphs.filter((p) => p.status === "approved" && MARK.test(p.text)).map((p) => ({ section: s, p })),
  );
  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const blocked = (check?.blocking.length ?? 0) > 0;
  const nothing = check !== null && check.included_sections === 0;
  const needsConfirm = toComplete.length > 0 && !withMarks;
  const ready = check !== null && !blocked && !nothing && !needsConfirm;

  async function save() {
    if (!editing) return;
    setError(null);
    try {
      await ipc.editParagraph(caseId, editing.id, editing.text);
      setEditing(null);
      await reload();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <div className="view finish">
      <div className="finish-body">
        <section className="finish-side" aria-label="לפני שהקובץ יוצא">
          <div className="stack" style={{ gap: 4 }}>
            <h1>{ready ? "הדוח מוכן להוצאה" : nothing ? "עוד אין מה להוציא" : "כמעט מוכן"}</h1>
            <span className="muted">משמאל: הדוח כמו שייצא. כאן: מה עוד כדאי לעשות לפני שהקובץ יוצא.</span>
          </div>
          {!check && !error && <p className="muted"><Spinner /> בודקת…</p>}
          {check && (
            <ul className="finish-checks">
              <li className={check.included_sections > 0 ? "ok" : "warn"}>
                <span className="finish-mark" aria-hidden="true">{check.included_sections > 0 ? "✓" : "!"}</span>
                <span className="grow">
                  {check.included_sections > 0 ? `${check.included_sections} סעיפים מאושרים ייכנסו לקובץ.` : "עוד אין סעיף מאושר. רק מה שאישרת נכנס לקובץ."}
                  {check.empty_sections.length > 0 && <span className="small muted"> {check.empty_sections.length} ריקים לא ייכנסו.</span>}
                </span>
                <button type="button" className="link-small" onClick={() => onOpen("")}>לדוח</button>
              </li>
              {waiting.length > 0 && (
                <li className="info">
                  <span className="finish-mark" aria-hidden="true">i</span>
                  <span className="grow">טיוטות שעוד לא אושרו לא ייכנסו: {waiting.map((s) => s.title).join(", ")}.</span>
                  <button type="button" className="link-small" onClick={() => onOpen(waiting[0]?.key ?? "")}>לאישור</button>
                </li>
              )}
              {check.blocking.map((b) => (
                <li key={b} className="warn">
                  <span className="finish-mark" aria-hidden="true">!</span>
                  <span className="grow">{b}</span>
                  <button type="button" className="link-small" onClick={() => onOpen(content.find((s) => b.startsWith(`${s.title}:`))?.key ?? "")}>לתיקון</button>
                </li>
              ))}
              {toComplete.length > 0 && (
                <li className="warn complete">
                  <span className="finish-mark" aria-hidden="true">!</span>
                  <div className="grow stack" style={{ gap: 8 }}>
                    <b>{toComplete.length === 1 ? "מקום אחד מסומן להשלמה" : `${toComplete.length} מקומות מסומנים להשלמה`}</b>
                    {toComplete.map(({ section, p }) =>
                      editing?.id === p.id ? (
                        <div key={p.id} className="stack" style={{ gap: 6 }}>
                          <span className="small muted">{section.title}</span>
                          <textarea className="textarea serif" rows={4} autoFocus value={editing.text}
                            onChange={(e) => setEditing({ id: p.id, text: e.target.value })} />
                          <div className="row">
                            <button type="button" className="btn btn-primary btn-small" onClick={() => void save()}>שמירה</button>
                            <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
                          </div>
                        </div>
                      ) : (
                        <button key={p.id} type="button" className="complete-item" onClick={() => setEditing({ id: p.id, text: p.text })}>
                          <span className="small muted">{section.title} · להשלמה כאן</span>
                          <span className="serif"><Marked text={p.text} /></span>
                        </button>
                      ),
                    )}
                    <label className="row small">
                      <input type="checkbox" checked={withMarks} onChange={(e) => setWithMarks(e.target.checked)} />
                      להוציא בכל זאת: הסימונים יישארו בקובץ, ואשלים אותם בוורד
                    </label>
                  </div>
                </li>
              )}
              {!blocked && check.included_sections > 0 && (
                <li className="ok">
                  <span className="finish-mark" aria-hidden="true">✓</span>
                  <span className="grow">השמות חוזרים למקומם בקובץ.</span>
                </li>
              )}
            </ul>
          )}
          <ErrorLine error={error} />
          <section className="card finish-file">
            <b>קובץ Word מוגן בסיסמה</b>
            <span className="muted small">הסיסמה נוצרת לבד. שולחים אותה להורים בנפרד מהקובץ, למשל בהודעה.</span>
            <button type="button" className="btn btn-primary btn-big" disabled={!ready} onClick={onExport}>הפקת הקובץ</button>
            {!ready && check && (
              <span className="small muted">
                {nothing ? "צריך לפחות סעיף מאושר אחד." : blocked ? "קודם מתקנים את מה שמסומן למעלה." : "משלימים את הסימונים, או מסמנים \"להוציא בכל זאת\"."}
              </span>
            )}
          </section>
        </section>

        <div className="finish-preview" aria-label="תצוגה מקדימה של הקובץ">
          <article className="a4 a4-small" style={{ fontFamily: settings?.font ? `"${settings.font}", var(--font-display)` : undefined }}>
            <header className="a4-header">{settings?.confidentiality}</header>
            <h1 className="a4-title">{settings?.title ?? "דוח אבחון פסיכולוגי"}</h1>
            <dl className="a4-info">
              {child && <><dt>שם הילד/ה</dt><dd>{child}</dd></>}
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
                    return (
                      <div key={s.key} className={ok.length ? "a4-section" : "a4-section a4-out"}>
                        <h3 className="a4-heading">{s.title}</h3>
                        {ok.length ? ok.map((p) => <p key={p.id} className="a4-p"><Marked text={p.text} /></p>)
                          : <p className="a4-out-note">לא ייכנס לקובץ (אין פסקה מאושרת)</p>}
                      </div>
                    );
                  })}
                </section>
              );
            })}
          </article>
        </div>
      </div>
    </div>
  );
}
