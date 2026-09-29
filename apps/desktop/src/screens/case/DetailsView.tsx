import { useState } from "react";
import { AgeField, parseAge } from "../../components/AgeField";
import { DateField, todayIso } from "../../components/DateField";
import { useApp } from "../../App";
import { ipc } from "../../ipc/client";
import type { GrammaticalGender } from "../../ipc/generated/GrammaticalGender";
import { PeopleEditor, toIdentityInputs, type PersonRow } from "../../components/NewCaseDialog";
import { Dialog, ErrorLine } from "../../components/ui";
import type { CaseApi } from "../CaseScreen";

/** The case's details, consent and the names hidden before every send. */
export function DetailsView({ api }: { api: CaseApi }) {
  const { fail, notify, go } = useApp();
  const { detail, caseId, reload } = api;
  const m = detail.meta;
  const [code, setCode] = useState(m.code);
  const [gender, setGender] = useState<GrammaticalGender | null>(m.child_gender);
  const [years, setYears] = useState(m.age ? String(m.age.years) : "");
  const [months, setMonths] = useState(m.age ? String(m.age.months) : "0");
  const [consent, setConsent] = useState(m.consent !== null);
  const [consentDate, setConsentDate] = useState(m.consent?.given_on ?? todayIso());
  const [consentBy, setConsentBy] = useState(m.consent?.given_by ?? "שני ההורים");
  const [rows, setRows] = useState<PersonRow[]>(() => {
    const r = detail.identities.map((i) => ({ id: i.id, role: i.role, value: i.value, aliases: i.aliases.join(", ") }));
    return r.some((x) => x.role === "child") ? r : [{ id: null, role: "child", value: "", aliases: "" }, ...r];
  });
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [typed, setTyped] = useState("");

  async function save() {
    setError(null);
    if (consent && !consentDate) return setError("צריך את תאריך החתימה על ההסכמה (יום.חודש.שנה).");
    try {
      await ipc.updateCase(caseId, {
        ...m,
        code: code.trim(),
        child_gender: gender,
        age: parseAge(years, months),
        consent: consent ? { given_on: consentDate, form_version: m.consent?.form_version ?? "v1", given_by: consentBy.trim() || "ההורים" } : null,
      });
      await ipc.setIdentities(caseId, toIdentityInputs(rows));
      await reload();
      notify("פרטי התיק נשמרו.");
    } catch (e) {
      setError(fail(e as never));
    }
  }

  async function remove() {
    try {
      await ipc.deleteCase(caseId);
      notify("התיק נמחק לצמיתות, כולל מפתח ההצפנה שלו.");
      go({ name: "cases" });
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <div className="view">
      <div className="view-head">
        <div className="stack" style={{ gap: 4 }}>
          <h1>פרטי התיק ושמות להסתרה</h1>
          <p className="muted small">כל שם כאן מוחלף בתפקיד לפני כל שליחה ל-Claude, גם עם תחיליות ("ולנועם", "שנועם") וכתיב שונה.</p>
        </div>
        <button type="button" className="btn btn-primary" onClick={() => void save()}>שמירה</button>
      </div>
      <div className="view-body details">
        <div className="stack details-col">
          <div className="card details-card stack">
            <div className="row wrap">
              <div className="field">
                <label htmlFor="d-code">קוד התיק</label>
                <input id="d-code" className="input" value={code} onChange={(e) => setCode(e.target.value)} />
              </div>
              <fieldset className="field plain">
                <legend className="label">מין (להתאמה דקדוקית)</legend>
                <div className="row">
                  <label className="radio"><input type="radio" name="dg" checked={gender === "male"} onChange={() => setGender("male")} /> בן</label>
                  <label className="radio"><input type="radio" name="dg" checked={gender === "female"} onChange={() => setGender("female")} /> בת</label>
                </div>
              </fieldset>
              <AgeField idPrefix="d" years={years} months={months} onChange={(y, m) => { setYears(y); setMonths(m); }} />
            </div>
          </div>
          <div className="card consent">
            <label className="row"><input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
              <b>ההורים חתמו על הסכמה לשימוש ב-Claude בכתיבת הדוח</b></label>
            {consent && (
              <div className="row wrap">
                <div className="field">
                  <label htmlFor="d-cd">תאריך</label>
                  <DateField id="d-cd" value={consentDate} onChange={setConsentDate} notAfterToday />
                </div>
                <div className="field grow">
                  <label htmlFor="d-cb">מי חתם (תפקיד, לא שם)</label>
                  <input id="d-cb" className="input" value={consentBy} onChange={(e) => setConsentBy(e.target.value)} />
                </div>
              </div>
            )}
          </div>
          <div className="card details-card stack danger-zone">
            <b>מחיקת התיק</b>
            <p className="small muted">מוחקת את כל החומרים, הטיוטות והשיחות, ואת מפתח ההצפנה של התיק. אי אפשר לשחזר.</p>
            <button type="button" className="btn btn-danger" onClick={() => setConfirmDelete(true)}>מחיקת התיק לצמיתות</button>
          </div>
        </div>
        <div className="card details-card stack grow">
          <span className="label">שמות שיוסתרו</span>
          <p className="hint">הילד/ה, ההורים, האחים, הגננת, רופאים ומטפלים, וגם מקומות (גן, יישוב). כינויים וכתיב נוסף, בפסיקים.</p>
          <PeopleEditor rows={rows} onChange={setRows} />
        </div>
      </div>
      <ErrorLine error={error} />
      {confirmDelete && (
        <Dialog narrow title="מחיקת התיק" onClose={() => setConfirmDelete(false)}
          footer={<><span className="grow" /><button type="button" className="btn" onClick={() => setConfirmDelete(false)}>ביטול</button>
            <button type="button" className="btn btn-danger" disabled={typed.trim() !== m.code.trim()} onClick={() => void remove()}>מחיקה</button></>}>
          <div className="stack">
            <p>כדי לוודא, הקלידי את קוד התיק: <b>{m.code}</b></p>
            <label className="visually-hidden" htmlFor="del-code">קוד התיק</label>
            <input id="del-code" className="input" value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus />
          </div>
        </Dialog>
      )}
    </div>
  );
}
