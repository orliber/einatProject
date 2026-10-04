import { useEffect, useState } from "react";
import { useApp } from "../App";
import { ipc, type ExportCheck } from "../ipc/client";
import type { CaseApi } from "../screens/CaseScreen";
import { Dialog, ErrorLine, Spinner } from "./ui";
import "./ExportDialog.css";

const WORDS = [
  "נהר", "ענן", "שקד", "אורן", "גשר", "מגדל", "חצב", "סלע", "נחל", "כרמל", "תמר", "אלון", "ברוש", "יונה", "שחף",
  "ארבל", "גפן", "דולב", "הדס", "זית", "חרוב", "טל", "יסמין", "כלנית", "לוטם", "מרווה", "נרקיס", "סחלב", "עירית", "פיקוס",
  "צבר", "קורנית", "רקפת", "שיזף", "תאנה", "אגם", "בריכה", "גבעה", "דיונה", "הר", "ואדי", "זריחה", "חוף", "טיפה", "ים",
];

/** A passphrase the parents can type from a phone call: four words and two digits. */
export function makePassphrase(): string {
  const r = new Uint32Array(5);
  crypto.getRandomValues(r);
  const w = (i: number) => WORDS[(r[i] ?? 0) % WORDS.length];
  return `${w(0)}-${w(1)}-${(r[4] ?? 0) % 90 + 10}-${w(2)}-${w(3)}`;
}

/** The report, or with `letter` the short letter (EX-4): same password and folder. */
export function ExportDialog({ api, onClose, letter }: { api: CaseApi; onClose: () => void; letter?: "parents" | "school" }) {
  const { fail, go } = useApp();
  const [check, setCheck] = useState<ExportCheck | null>(null);
  const [protect, setProtect] = useState(true);
  const [password, setPassword] = useState(makePassphrase);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  /** Seconds until the copied password is cleared from the clipboard. */
  const [copied, setCopied] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ipc.checkExport(api.caseId).then(setCheck).catch((e) => setError(fail(e as never)));
  }, [api.caseId, fail]);

  async function run() {
    setBusy(true);
    setError(null);
    try {
      const pw = protect ? password : null;
      setSaved(await (letter ? ipc.exportLetter(api.caseId, letter, pw) : ipc.exportReport(api.caseId, pw)));
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  async function copy() {
    try {
      setCopied(await ipc.copySecret(password));
    } catch {
      setCopied(null);
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
          {protect && (
            <div className="note-sand stack" style={{ gap: 6 }}>
              <b>הסיסמה לקובץ: <span className="mono">{password}</span></b>
              <span>מוסרים אותה להורים בטלפון, לא באותו מייל. התוכנה לא שומרת אותה.</span>
            </div>
          )}
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
          <button type="button" className="btn btn-primary" disabled={!check || blocked || busy || (protect && password.length < 10)} onClick={() => void run()}>
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
                <li className="ok"><span aria-hidden="true">✓</span><span>השמות חזרו, ולא נשארו תפקידים במקום שמות או הערות "חסר מידע"</span></li>
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

          <div className="card protect stack">
            <label className="row"><input type="checkbox" checked={protect} onChange={(e) => setProtect(e.target.checked)} />
              <b>הגנה בסיסמה</b> <span className="muted">(מומלץ כששולחים במייל)</span></label>
            {protect && (
              <>
                <div className="row">
                  <label htmlFor="exp-pw" className="visually-hidden">סיסמה לקובץ</label>
                  <input id="exp-pw" className="input grow mono pw" value={password} onChange={(e) => setPassword(e.target.value)} />
                  <button type="button" className="btn" onClick={() => void copy()}>{copied ? "הועתק" : "העתקה"}</button>
                  {copied && <span className="small muted" role="status">לא נשמר בהיסטוריית הלוח, ויימחק מהלוח בעוד {copied} שניות.</span>}
                  <button type="button" className="btn" onClick={() => { setPassword(makePassphrase()); setCopied(null); }}>סיסמה חדשה</button>
                </div>
                <span className="small muted">את הסיסמה מוסרים להורים בטלפון, לא באותו מייל. ההצפנה היא של Word עצמו, ולכן הקובץ נפתח בכל מחשב.</span>
              </>
            )}
          </div>
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
