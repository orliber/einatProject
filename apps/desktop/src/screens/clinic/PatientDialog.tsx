// A patient's card as a form: a new family (active, or onto the waiting list) and the details
// of one that exists. Names and phones stay in the vault; nothing here is sent anywhere.
import { useRef, useState } from "react";
import { useApp } from "../../App";
import { ageFrom, ageOf } from "../../components/AgeField";
import { DateField } from "../../components/DateField";
import { Segmented } from "../../components/Switch";
import { CloseIcon, Dialog, ErrorLine, PlusIcon } from "../../components/ui";
import type { Contact } from "../../ipc/generated/Contact";
import type { ContactRole } from "../../ipc/generated/ContactRole";
import type { Patient } from "../../ipc/generated/Patient";
import type { PatientStatus } from "../../ipc/generated/PatientStatus";
import { useSavePatient, useToday } from "./clinicData";
import { CONTACT_ROLE_LABEL, CONTACT_ROLES, emailReachable, optionsWith, parseMoney, PATIENT_STATUS_LABEL, phoneReachable } from "./format";
import { CheckField, Field, MoneyField, moneyProblem, moneyText, SelectField, TextAreaField, TextField } from "./parts";

export interface PatientDialogProps {
  /** The card to edit; without it a new one is opened. */
  patient?: Patient;
  /** A new card starts on the waiting list (opened from the waiting tab). */
  waiting?: boolean;
  onClose: () => void;
  onSaved?: (patient: Patient) => void;
}

/** The most contacts a card keeps (the core's limit). */
const MAX_CONTACTS = 6;

type Gender = "" | "male" | "female";

interface Form {
  name: string;
  birth: string;
  /** A birth date typed but not a real date: never saved as "no date". */
  birthBad: boolean;
  gender: Gender;
  status: PatientStatus;
  waitingSince: string;
  waitingBad: boolean;
  urgent: boolean;
  contacts: Contact[];
  framework: string;
  referredBy: string;
  reason: string;
  notes: string;
  fee: string;
}

const emptyContact = (role: ContactRole): Contact => ({ role, name: "", phone: "", email: "" });

export function PatientDialog({ patient, waiting = false, onClose, onSaved }: PatientDialogProps) {
  const { notify } = useApp();
  const now = useToday();
  const save = useSavePatient();
  const [first] = useState<Form>(() => patient
    ? {
      name: patient.name, birth: patient.birth_date ?? "", birthBad: false, gender: patient.gender ?? "", status: patient.status,
      waitingSince: patient.waiting_since ?? "", waitingBad: false, urgent: patient.urgent, contacts: patient.contacts, framework: patient.framework,
      referredBy: patient.referred_by, reason: patient.reason, notes: patient.notes, fee: moneyText(patient.session_fee),
    }
    : {
      name: "", birth: "", birthBad: false, gender: "", status: waiting ? "waiting" : "active", waitingSince: waiting ? now.date : "", waitingBad: false, urgent: false,
      contacts: [emptyContact("mother")], framework: "", referredBy: "", reason: "", notes: "", fee: "",
    });
  const [form, setForm] = useState(first);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** She pressed save: what is missing or wrong shows by its field, not only at the foot. */
  const [tried, setTried] = useState(false);
  /** The contact boxes she has left: a phone or an email that cannot be reached shows then. */
  const [left, setLeft] = useState<ReadonlySet<string>>(new Set());
  const form_ = useRef<HTMLDivElement>(null);
  const set = (patch: Partial<Form>) => setForm((f) => ({ ...f, ...patch }));
  const setContact = (i: number, patch: Partial<Contact>) => set({ contacts: form.contacts.map((c, j) => (j === i ? { ...c, ...patch } : c)) });
  const dirty = JSON.stringify(form) !== JSON.stringify(first);

  const age = form.birth === "" ? null : ageFrom(form.birth, now.date);
  // "הסתיים" is where a card goes, not where it starts.
  const statuses: PatientStatus[] = patient ? ["active", "waiting", "closed"] : ["active", "waiting"];

  const nameProblem = tried && form.name.trim() === "" ? "צריך לכתוב את שם הילד או הילדה." : null;
  const onList = form.status === "waiting";
  /** Warnings only: a card may keep a number she will fix later; messages to it will not open. */
  const phoneWarning = (c: Contact, i: number) => (tried || left.has(`contact-${i}-phone`)) && c.phone.trim() !== "" && !phoneReachable(c.phone)
    ? "המספר לא נראה תקין, ולא תהיה אפשרות לשלוח אליו הודעה." : null;
  const emailWarning = (c: Contact, i: number) => (tried || left.has(`contact-${i}-email`)) && c.email.trim() !== "" && !emailReachable(c.email)
    ? "הכתובת לא נראית תקינה, ולא תהיה אפשרות לשלוח אליה מייל." : null;

  /** Moves to a field that needs her, and shows it on the screen. */
  function toField(id: string) {
    const box = form_.current?.querySelector<HTMLElement>(`#${id}`);
    box?.focus();
    box?.scrollIntoView?.({ block: "center" });
  }

  async function submit() {
    setTried(true);
    if (form.name.trim() === "") {
      setError(null);
      toField("patient-name");
      return;
    }
    // A wrong date is not "no date": saving it would erase the date the card had. The field
    // says what is wrong ("תאריך לא תקין"), and the focus goes to it.
    if (form.birthBad) {
      setError(null);
      toField("patient-birth");
      return;
    }
    if (onList && form.waitingBad) {
      setError(null);
      toField("patient-waiting-since");
      return;
    }
    const fee = form.fee.trim() === "" ? null : parseMoney(form.fee);
    if (form.fee.trim() !== "" && fee === null) {
      setError(moneyProblem(form.fee, true));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const saved = await save.mutateAsync({
        id: patient?.id ?? "",
        name: form.name,
        birth_date: form.birth === "" ? null : form.birth,
        gender: form.gender === "" ? null : form.gender,
        contacts: form.contacts,
        framework: form.framework,
        referred_by: form.referredBy,
        reason: form.reason,
        notes: form.notes,
        status: form.status,
        waiting_since: onList ? (form.waitingSince === "" ? now.date : form.waitingSince) : (patient?.waiting_since ?? null),
        urgent: onList && form.urgent,
        session_fee: fee,
        // The link to a case and the day the card was opened are the vault's to keep.
        case_id: patient?.case_id ?? null,
        created_on: patient?.created_on ?? now.date,
      });
      onSaved?.(saved);
      notify(patient ? "הכרטיס נשמר" : onList ? "המשפחה נוספה לרשימת ההמתנה" : "המטופל נוסף");
      onClose();
    } catch (e) {
      setError((e as { message?: string }).message ?? "הכרטיס לא נשמר. אפשר לנסות שוב.");
      setBusy(false);
    }
  }

  return (
    <Dialog help="patient" title={patient ? "פרטי המטופל" : "מטופל חדש"} dirty={dirty} onClose={onClose}
      subtitle="הפרטים נשמרים מוצפנים בכספת שבמחשב, ולא נשלחים לשום מקום."
      footer={(close) => <>
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void submit()}>{patient ? "שמירה" : "הוספה"}</button>
      </>}>
      <div className="stack clinic-form" ref={form_}>
        <div className="clinic-form-row">
          <TextField id="patient-name" label="שם הילד או הילדה" value={form.name} maxLength={120} autoFocus={!patient} className="grow"
            problem={nameProblem}
            onChange={(name) => set({ name })} />
          <SelectField<Gender> id="patient-gender" label="מין" value={form.gender} onChange={(gender) => set({ gender })}
            options={[{ value: "", label: "לא צוין" }, { value: "male", label: "בן" }, { value: "female", label: "בת" }]} />
        </div>

        <div className="clinic-form-row">
          <Field id="patient-birth" label="תאריך לידה">
            <DateField id="patient-birth" value={form.birth} onChange={(birth, birthBad) => set({ birth, birthBad })} notAfterToday checked={tried} />
          </Field>
          <p className="clinic-age" role="status">{age ? `היום: ${ageOf(age, form.gender === "" ? null : form.gender)}` : ""}</p>
        </div>

        <div className="field">
          <span className="label">איפה המשפחה עכשיו</span>
          <Segmented label="איפה המשפחה עכשיו" value={form.status} onChange={(status) => set({ status, waitingBad: false })}
            options={statuses.map((s) => ({ value: s, label: PATIENT_STATUS_LABEL[s] }))} />
          {form.status === "waiting" && (
            <div className="clinic-form-row">
              <Field id="patient-waiting-since" label="ממתינים מאז" hint="ריק: מהיום.">
                <DateField id="patient-waiting-since" value={form.waitingSince} onChange={(waitingSince, waitingBad) => set({ waitingSince, waitingBad })} notAfterToday checked={tried} />
              </Field>
              <CheckField id="patient-urgent" label="דחוף: להכניס ראשונים" checked={form.urgent} onChange={(urgent) => set({ urgent })} />
            </div>
          )}
          {form.status === "closed" && <p className="hint">כרטיס שהסתיים יוצא מהרשימה הפעילה, וכל מה שנרשם בו נשאר שמור.</p>}
          {form.status === "unknown" && <p className="hint">המצב שנשמר לכרטיס הזה לא מוכר לגרסה הזו של התוכנה. כדי לשמור צריך לבחור מצב מהרשימה, או לעדכן את התוכנה.</p>}
        </div>

        <fieldset className="clinic-group">
          <legend className="label">אנשי קשר</legend>
          {form.contacts.length === 0 && <p className="hint">עוד אין אנשי קשר בכרטיס.</p>}
          {form.contacts.map((c, i) => (
            <div key={i} className="clinic-contact" role="group" aria-label={`איש קשר ${i + 1}`}
              onBlur={(e) => { const id = e.target.id; if (id && !left.has(id)) setLeft(new Set(left).add(id)); }}>
              <SelectField<ContactRole> id={`contact-${i}-role`} label="קשר" value={c.role} onChange={(role) => setContact(i, { role })}
                options={optionsWith(CONTACT_ROLES, CONTACT_ROLE_LABEL, c.role)} />
              <TextField id={`contact-${i}-name`} label="שם" value={c.name} maxLength={120} onChange={(name) => setContact(i, { name })} />
              <TextField id={`contact-${i}-phone`} label="טלפון" value={c.phone} maxLength={200} dir="ltr" inputMode="tel" problem={phoneWarning(c, i)} onChange={(phone) => setContact(i, { phone })} />
              <TextField id={`contact-${i}-email`} label="מייל" value={c.email} maxLength={200} dir="ltr" inputMode="email" problem={emailWarning(c, i)} onChange={(email) => setContact(i, { email })} />
              <button type="button" className="btn icon-btn" aria-label={`הסרת איש קשר ${i + 1}`}
                onClick={() => set({ contacts: form.contacts.filter((_, j) => j !== i) })}><CloseIcon /></button>
            </div>
          ))}
          {form.contacts.length < MAX_CONTACTS
            ? (
              <button type="button" className="btn btn-small clinic-add" onClick={() => set({ contacts: [...form.contacts, emptyContact(form.contacts.some((c) => c.role === "mother") ? (form.contacts.some((c) => c.role === "father") ? "other" : "father") : "mother")] })}>
                <PlusIcon /> איש קשר
              </button>
            )
            : <p className="hint">אפשר לשמור עד {MAX_CONTACTS} אנשי קשר.</p>}
        </fieldset>

        <div className="clinic-form-row">
          <TextField id="patient-framework" label="מסגרת חינוכית" value={form.framework} maxLength={200} placeholder="גן או בית ספר" className="grow"
            onChange={(framework) => set({ framework })} />
          <TextField id="patient-referred" label="מי הפנה" value={form.referredBy} maxLength={200} className="grow" onChange={(referredBy) => set({ referredBy })} />
        </div>

        <TextAreaField id="patient-reason" label="סיבת הפנייה" value={form.reason} rows={2} maxLength={4000} hint="בשורה או שתיים: למה המשפחה פנתה."
          onChange={(reason) => set({ reason })} />
        <TextAreaField id="patient-notes" label="הערות" value={form.notes} rows={2} maxLength={4000} onChange={(notes) => set({ notes })} />

        <MoneyField id="patient-fee" label="מחיר קבוע לפגישה (לא חובה)" value={form.fee} optional
          hint="ריק: לפי המחיר של סוג הפגישה, מהגדרות הקליניקה." onChange={(fee) => set({ fee })} />

        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
