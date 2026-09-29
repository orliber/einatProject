import { useEffect, useState, type FormEvent } from "react";
import { AgeField, parseAge } from "./AgeField";
import { DateField, todayIso } from "./DateField";
import { useApp } from "../App";
import { personRoles, roleLabel } from "../i18n/he";
import { ipc, type IdentityInput, type NameMatch } from "../ipc/client";
import type { GrammaticalGender } from "../ipc/generated/GrammaticalGender";
import type { Role } from "../ipc/generated/Role";
import { Dialog, ErrorLine } from "./ui";
import "./NewCaseDialog.css";

export interface PersonRow {
  id: string | null;
  role: Role;
  value: string;
  aliases: string;
}

export function toIdentityInputs(rows: PersonRow[]): IdentityInput[] {
  return rows
    .filter((r) => r.value.trim())
    .map((r) => ({
      id: r.id,
      role: r.role,
      value: r.value.trim(),
      aliases: r.aliases.split(",").map((a) => a.trim()).filter(Boolean),
    }));
}

/** Rows of people whose names are hidden: role, name, other spellings or nicknames. */
export function PeopleEditor({ rows, onChange }: { rows: PersonRow[]; onChange: (rows: PersonRow[]) => void }) {
  const set = (i: number, patch: Partial<PersonRow>) => onChange(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <div className="people">
      <div className="person-row person-head" aria-hidden="true">
        <span>תפקיד</span><span>שם</span><span>כינויים וכתיב נוסף</span><span />
      </div>
      {rows.map((r, i) => (
        <div key={i} className="person-row">
          <label className="visually-hidden" htmlFor={`role-${i}`}>תפקיד</label>
          <select id={`role-${i}`} className="select" value={r.role} onChange={(e) => set(i, { role: e.target.value as Role })}
            disabled={r.role === "child"}>
            {(r.role === "child" ? (["child"] as Role[]) : personRoles).map((role) => (
              <option key={role} value={role}>{roleLabel[role]}</option>
            ))}
          </select>
          <label className="visually-hidden" htmlFor={`name-${i}`}>שם</label>
          <input id={`name-${i}`} className="input" placeholder={r.role === "child" ? "למשל: נועם" : "שם"} value={r.value} onChange={(e) => set(i, { value: e.target.value })} />
          <label className="visually-hidden" htmlFor={`alias-${i}`}>כינויים</label>
          <input id={`alias-${i}`} className="input" placeholder={r.role === "child" ? "למשל: נועמי, נעמי" : "בפסיקים, לא חובה"} value={r.aliases}
            onChange={(e) => set(i, { aliases: e.target.value })} />
          {r.role !== "child" ? (
            <button type="button" className="btn icon-btn" aria-label={`הסרת ${roleLabel[r.role]}`} title="הסרה" onClick={() => onChange(rows.filter((_, j) => j !== i))}>×</button>
          ) : <span />}
        </div>
      ))}
      <button type="button" className="btn btn-small add-person"
        onClick={() => onChange([...rows, { id: null, role: "mother", value: "", aliases: "" }])}>+ הוספת אדם</button>
    </div>
  );
}

function nextCode(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}${String(d.getDate()).padStart(2, "0")}`;
}

/** Names already in another case (D-023): a sibling, a family seen before, the same child twice. */
export function useNameMatches(caseId: string | null, rows: PersonRow[]): NameMatch[] {
  const [found, setFound] = useState<NameMatch[]>([]);
  const names = rows.flatMap((r) => [r.value, ...r.aliases.split(",")]).map((n) => n.trim()).filter((n) => n.length >= 2);
  const key = names.join("|");
  useEffect(() => {
    let alive = true;
    const t = window.setTimeout(() => {
      const list = key ? key.split("|") : [];
      (list.length ? ipc.findNameMatches(caseId, list) : Promise.resolve([]))
        .then((m) => alive && setFound(m))
        .catch(() => undefined);
    }, 350);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [caseId, key]);
  return found;
}

export function NameMatches({ found }: { found: NameMatch[] }) {
  if (!found.length) return null;
  return (
    <div className="name-matches" role="status">
      <b>שמות שכבר מופיעים בתיקים אחרים</b>
      <ul>
        {found.slice(0, 6).map((m) => (
          <li key={`${m.case_id}-${m.typed}-${m.value}`}>
            "{m.typed}": {roleLabel[m.role]} בתיק {m.case_code}{m.child_name ? ` (${m.child_name})` : ""}{m.trashed ? ", בסל המחזור" : ""}
          </li>
        ))}
      </ul>
      <span className="small">אם זו אותה משפחה (למשל אח או אחות), כדאי לבדוק את התיק הקודם. אם זה צירוף מקרים, אפשר להמשיך.</span>
    </div>
  );
}

export function NewCaseDialog({ onClose, onCreated, folderId = null }: { onClose: () => void; onCreated: (id: string) => void; folderId?: string | null }) {
  const { fail } = useApp();
  const [code, setCode] = useState(`תיק-${nextCode()}`);
  const [gender, setGender] = useState<GrammaticalGender>("male");
  const [years, setYears] = useState("");
  const [months, setMonths] = useState("0");
  const [consent, setConsent] = useState(false);
  const [consentDate, setConsentDate] = useState(todayIso());
  const [consentBy, setConsentBy] = useState("שני ההורים");
  const [rows, setRows] = useState<PersonRow[]>([
    { id: null, role: "child", value: "", aliases: "" },
    { id: null, role: "mother", value: "", aliases: "" },
    { id: null, role: "father", value: "", aliases: "" },
  ]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const matches = useNameMatches(null, rows);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (!rows[0]?.value.trim()) return setError("צריך את שם הילד/ה, כדי שיוסתר בכל מקום.");
    if (consent && !consentDate) return setError("צריך את תאריך החתימה על ההסכמה (יום.חודש.שנה).");
    setBusy(true);
    try {
      const id = await ipc.createCase(
        {
          code: code.trim(),
          age: parseAge(years, months),
          child_gender: gender,
          current_section: null,
          retention_until: null,
          consent: consent ? { given_on: consentDate, form_version: "v1", given_by: consentBy.trim() || "ההורים" } : null,
        },
        toIdentityInputs(rows),
      );
      if (folderId) await ipc.moveCase(id, folderId);
      onCreated(id);
    } catch (err) {
      setError(fail(err as never));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog title="תיק חדש" subtitle="השמות נשמרים מוצפנים ומוסתרים לפני כל שליחה ל-Claude." onClose={onClose}
      footer={
        <>
          <span className="grow" />
          <button type="button" className="btn" onClick={onClose}>ביטול</button>
          <button type="submit" form="new-case" className="btn btn-primary" disabled={busy}>פתיחת התיק</button>
        </>
      }>
      <form id="new-case" className="stack form-grid" onSubmit={submit}>
        <div className="row wrap">
          <div className="field">
            <label htmlFor="code">קוד התיק</label>
            <input id="code" className="input" value={code} onChange={(e) => setCode(e.target.value)} />
          </div>
          <fieldset className="field plain">
            <legend className="label">מין (להתאמה דקדוקית בדוח)</legend>
            <div className="row">
              <label className="radio"><input type="radio" name="g" checked={gender === "male"} onChange={() => setGender("male")} /> בן</label>
              <label className="radio"><input type="radio" name="g" checked={gender === "female"} onChange={() => setGender("female")} /> בת</label>
            </div>
          </fieldset>
          <AgeField idPrefix="nc" years={years} months={months} onChange={(y, m) => { setYears(y); setMonths(m); }} />
        </div>

        <div className="stack" style={{ gap: 8 }}>
          <span className="label">שמות שיוסתרו</span>
          <p className="hint">הילד/ה, ההורים, האחים, הגננת וכל מי שמופיע בחומרים. כינויים וכתיב נוסף ("נועמי", "נעמי") מוסתרים גם הם.</p>
          <PeopleEditor rows={rows} onChange={setRows} />
          <NameMatches found={matches} />
        </div>

        <div className="card consent">
          <label className="row"><input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
            <b>ההורים חתמו על הסכמה לשימוש ב-Claude בכתיבת הדוח</b></label>
          <p className="hint">בלי הסכמה רשומה אפשר לעבוד בתיק, אבל לא לשלוח ממנו ל-Claude.</p>
          {consent && (
            <div className="row wrap">
              <div className="field">
                <label htmlFor="cdate">תאריך</label>
                <DateField id="cdate" value={consentDate} onChange={setConsentDate} notAfterToday />
              </div>
              <div className="field grow">
                <label htmlFor="cby">מי חתם (תפקיד, לא שם)</label>
                <input id="cby" className="input" value={consentBy} onChange={(e) => setConsentBy(e.target.value)} />
              </div>
            </div>
          )}
        </div>
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}
