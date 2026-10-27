import { useEffect, useState } from "react";
import { useApp } from "../App";
import { ipc, type ExportCheck } from "../ipc/client";
import type { CaseApi } from "../screens/CaseScreen";
import { Switch } from "./Switch";
import { Dialog, ErrorLine, Spinner } from "./ui";
import { timed, WaitLine } from "./Progress";
import "./ExportDialog.css";

/** "דוח אבחון X.docx" → "דוח אבחון X - להורים.docx", as the core names the parents' file. */
export function parentsFileName(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? `${name} - להורים` : `${name.slice(0, dot)} - להורים${name.slice(dot)}`;
}

/** The report, or with `letter` the short letter (EX-4), into the downloads folder. The files
 *  open without a password, as she asked (D-081); the PDF is still print-only. Beside the full
 *  report, a version for the parents (round 55, D-095): the same approved sections, a simple
 *  chart instead of the score tables, her opening sentence, and no numbers unless she turns
 *  them on here. It is made on this computer; nothing is sent for it. */
export function ExportDialog({ api, onClose, letter }: { api: CaseApi; onClose: () => void; letter?: "parents" | "school" }) {
  const { fail, go } = useApp();
  const [check, setCheck] = useState<ExportCheck | null>(null);
  const [pdf, setPdf] = useState(false);
  const [parents, setParents] = useState(false);
  // Off every time the dialog opens: numbers go to the parents only when she says so here.
  const [numbers, setNumbers] = useState(false);
  const [opening, setOpening] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The check runs when the dialog opens, and again from "לנסות שוב" when it failed: the
  // dialog never waits for an answer that is not coming.
  const [checkRun, setCheckRun] = useState(0);
  const [checkError, setCheckError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    timed("check", ipc.checkExport(api.caseId))
      .then((c) => alive && setCheck(c))
      .catch((e) => alive && setCheckError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [api.caseId, fail, checkRun]);

  const exportKind = letter ? "letter" : pdf ? "pdf" : "word";

  useEffect(() => {
    if (letter) return;
    ipc.reportSettings().then((s) => setOpening(s.parents_opening)).catch(() => setOpening(null));
  }, [letter]);

  async function run() {
    setBusy(true);
    setError(null);
    try {
      const version = parents ? { show_numbers: numbers } : null;
      setSaved(await timed("export", letter ? ipc.exportLetter(api.caseId, letter, null) : pdf ? ipc.exportPdf(api.caseId, null, version) : ipc.exportReport(api.caseId, null, version), exportKind));
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  const blocked = !letter && (check?.blocking.length ?? 0) > 0;
  const forParents = parents && !letter;
  const child = api.detail.identities.find((i) => i.role === "child")?.value;

  if (saved) {
    return (
      <Dialog narrow title={letter ? "המכתב מוכן" : forParents ? "הגרסה להורים מוכנה" : "הדוח מוכן"} onClose={onClose}
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
            {busy ? <><Spinner /> מפיקה…</> : letter ? "הפקת המכתב" : forParents ? "הפקת הגרסה להורים" : "הפקת הדוח"}
          </button>
        </>
      }>
      <div className="export">
        <div className="stack grow">
          {!letter && (
            <div className="row" role="radiogroup" aria-label="איזה דוח">
              <label className="row small"><input type="radio" name="version" checked={!parents} onChange={() => setParents(false)} /> הדוח המלא</label>
              <label className="row small"><input type="radio" name="version" checked={parents} onChange={() => setParents(true)} /> גרסה להורים</label>
            </div>
          )}
          {forParents && (
            <div className="parents-version stack">
              <span className="small">אותם סעיפים מאושרים, בלי טבלאות הציונים. במקומן טבלה פשוטה: חזק / בטווח הגיל / קשה יותר, מחושבת מהציונים. נבנית במחשב הזה, שום דבר לא נשלח.</span>
              <Switch id="parents-numbers" label="להציג ציונים בגרסה להורים" checked={numbers} onChange={setNumbers}
                hint={numbers ? "הציונים יופיעו בטבלה ובטקסט." : "בלי ציונים: משפט עם ציון יוצא מהגרסה להורים."} />
              {opening !== null && (
                <p className="small">
                  <b>משפט הפתיחה:</b> {opening.trim() ? opening : <span className="muted">(ריק)</span>}{" "}
                  <span className="muted">· משנים בהגדרות ← הדוח</span>
                </p>
              )}
            </div>
          )}
          {!check && !checkError && <WaitLine task="check" label="בודקת את הדוח" />}
          {!check && <ErrorLine error={checkError} retry={() => { setCheckError(null); setCheckRun((n) => n + 1); }} />}
          {busy && <WaitLine key={exportKind} task="export" speed={exportKind} label={letter ? "מפיקה את המכתב" : pdf ? "מפיקה את ה-PDF" : "מפיקה את קובץ ה-Word"} />}
          {check && !letter && (
            <ul className="checklist">
              <li className={check.included_sections > 0 ? "ok" : "bad"}>
                <span aria-hidden="true">{check.included_sections > 0 ? "✓" : "!"}</span>
                <span>{check.included_sections} סעיפים עם פסקאות מאושרות</span>
              </li>
              {check.parents_chart_rows > 0 && forParents && (
                <li className="ok">
                  <span aria-hidden="true">✓</span>
                  <span>במקום טבלאות הציונים: טבלה פשוטה של חזק / בטווח הגיל / קשה יותר</span>
                </li>
              )}
              {check.parents_left_out > 0 && forParents && !numbers && (
                <li className="warn">
                  <span aria-hidden="true">!</span>
                  <span>
                    {check.parents_left_out === 1 ? "משפט אחד הושמט" : `${check.parents_left_out} משפטים הושמטו`} כי היה בהם מספר שנראה כמו ציון. כדי להשאיר אותם: לנסח בלי המספר, או להציג ציונים.
                  </span>
                </li>
              )}
              {check.score_tables > 0 && !forParents && (
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
            <p className="small"><b>שם הקובץ:</b> {forParents ? parentsFileName(check.file_name) : check.file_name} <span className="muted">(בלי שם הילד) · נשמר בתיקיית ההורדות</span></p>
          )}
          {blocked && (
            <button type="button" className="btn btn-small align-start" onClick={() => { onClose(); go({ name: "case", id: api.caseId, view: api.detail.sections[0]?.key ?? "materials" }); }}>לסעיפים</button>
          )}
          <ErrorLine error={error} />
        </div>
        <aside className="page-preview" aria-hidden="true">
          <div className="mini-page serif">
            <span className="mini-conf">חסוי – מידע רפואי</span>
            <span className="mini-title">{forParents ? "דוח אבחון – גרסה להורים" : "דוח אבחון פסיכולוגי"}</span>
            {forParents && (
              <span className="mini-bands">
                <i className="on" /><i /><i />
                <i /><i className="on" /><i />
                <i /><i /><i className="on" />
              </span>
            )}
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
