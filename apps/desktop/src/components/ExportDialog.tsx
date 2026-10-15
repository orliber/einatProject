import { useEffect, useState } from "react";
import { useApp } from "../App";
import { ipc, type ExportCheck } from "../ipc/client";
import type { CaseApi } from "../screens/CaseScreen";
import { Dialog, ErrorLine, Spinner } from "./ui";
import "./ExportDialog.css";

/** The report, or with `letter` the short letter (EX-4), into the downloads folder. The files
 *  open without a password, as she asked (D-081); the PDF is still print-only. */
export function ExportDialog({ api, onClose, letter }: { api: CaseApi; onClose: () => void; letter?: "parents" | "school" }) {
  const { fail, go } = useApp();
  const [check, setCheck] = useState<ExportCheck | null>(null);
  const [pdf, setPdf] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ipc.checkExport(api.caseId).then(setCheck).catch((e) => setError(fail(e as never)));
  }, [api.caseId, fail]);

  async function run() {
    setBusy(true);
    setError(null);
    try {
      setSaved(await (letter ? ipc.exportLetter(api.caseId, letter, null) : pdf ? ipc.exportPdf(api.caseId, null) : ipc.exportReport(api.caseId, null)));
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  const blocked = !letter && (check?.blocking.length ?? 0) > 0;
  const child = api.detail.identities.find((i) => i.role === "child")?.value;

  if (saved) {
    return (
      <Dialog narrow title={letter ? "המכתב מוכן" : "הדוח מוכן"} onClose={onClose}
        footer={<><span className="grow" /><button type="button" className="btn btn-primary" onClick={onClose}>סגירה</button></>}>
        <div className="stack">
          <p>הקובץ נשמר:</p>
          <p className="saved-path" dir="ltr">{saved}</p>
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog title={letter ? "הפקת מכתב Word" : "הפקת דוח Word"} subtitle={`${api.detail.meta.code}${child ? ` · ${child}` : ""} · רק פסקאות שאישרת נכנסות ל${letter ? "מכתב" : "דוח"}`} onClose={onClose}
      footer={
        <>
          <span className="grow" />
          <button type="button" className="btn" onClick={onClose}>ביטול</button>
          <button type="button" className="btn btn-primary" disabled={!check || blocked || busy} onClick={() => void run()}>
            {busy ? <><Spinner /> מפיקה…</> : letter ? "הפקת המכתב" : "הפקת הדוח"}
          </button>
        </>
      }>
      <div className="export">
        <div className="stack grow">
          {!check && <p className="muted"><Spinner /> בודקת את הדוח…</p>}
          {check && !letter && (
            <ul className="checklist">
              <li className={check.included_sections > 0 ? "ok" : "bad"}>
                <span aria-hidden="true">{check.included_sections > 0 ? "✓" : "!"}</span>
                <span>{check.included_sections} סעיפים עם פסקאות מאושרות</span>
              </li>
              {check.score_tables > 0 && (
                <li className="ok">
                  <span aria-hidden="true">✓</span>
                  <span>{check.score_tables === 1 ? "טבלת הציונים תצורף כנספח" : `${check.score_tables} טבלאות ציונים יצורפו כנספח`}</span>
                </li>
              )}
              {!blocked ? (
                <li className="ok"><span aria-hidden="true">✓</span><span>השמות חזרו, ולא נשארו תפקידים או "חסר מידע"</span></li>
              ) : (
                check.blocking.map((b, i) => <li key={i} className="bad"><span aria-hidden="true">!</span><span>{b}</span></li>)
              )}
              {check.empty_sections.length > 0 && (
                <li className="warn">
                  <span aria-hidden="true">!</span>
                  <details className="grow empty-list">
                    <summary>{check.empty_sections.length === 1 ? "סעיף אחד ריק לא ייכלל בדוח" : `${check.empty_sections.length} סעיפים ריקים לא ייכללו בדוח`}</summary>
                    <span className="small muted">{check.empty_sections.join(" · ")}</span>
                  </details>
                </li>
              )}
            </ul>
          )}

          {!letter && (
            <div className="row" role="radiogroup" aria-label="סוג הקובץ">
              <label className="row small"><input type="radio" checked={!pdf} onChange={() => setPdf(false)} /> Word (אפשר להמשיך לערוך)</label>
              <label className="row small"><input type="radio" checked={pdf} onChange={() => setPdf(true)} /> PDF להדפסה בלבד (אי אפשר לשנות)</label>
            </div>
          )}
          {pdf && !letter && <span className="small muted">את ה-PDF אי אפשר לשנות. בהגדרות אפשר לבדוק אם עותק הוא המקורי.</span>}
          {/* D-081: the file goes out without a password, and she is told so. */}
          <span className="small muted">הקובץ נפתח בלי סיסמה: כל מי שיקבל אותו יוכל לקרוא אותו.</span>
          {check && !letter && (
            <p className="small"><b>שם הקובץ:</b> {check.file_name} <span className="muted">(בלי שם הילד) · נשמר בתיקיית ההורדות</span></p>
          )}
          {blocked && (
            <button type="button" className="btn btn-small align-start" onClick={() => { onClose(); go({ name: "case", id: api.caseId, view: api.detail.sections[0]?.key ?? "materials" }); }}>לסעיפים</button>
          )}
          <ErrorLine error={error} />
        </div>
        <aside className="page-preview" aria-hidden="true">
          <div className="mini-page serif">
            <span className="mini-conf">חסוי – מידע רפואי</span>
            <span className="mini-title">דוח אבחון פסיכולוגי</span>
            {api.detail.sections.filter((s) => s.approved).slice(0, 5).map((s) => (
              <span key={s.key} className="mini-section">
                <b>{s.title}</b>
                <i /><i /><i className="short" />
              </span>
            ))}
            <span className="mini-foot">עמוד 1</span>
          </div>
        </aside>
      </div>
    </Dialog>
  );
}
