// The session note of a visit, or a note on its own (a phone call). Two texts, kept apart as
// the law keeps them apart (Patient's Rights Law, §17; the code of ethics, 3.14; D-171): the
// record, which the family may ask to see, and her personal note, which is hers alone. Only
// the record can go to the patient's diagnostic case.
import { useState } from "react";
import { useApp } from "../../App";
import { DateField } from "../../components/DateField";
import { Dialog, ErrorLine, Skeleton } from "../../components/ui";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { SessionNote } from "../../ipc/generated/SessionNote";
import type { Visit } from "../../ipc/generated/Visit";
import { useClinicSettings, useNoteToCase, usePatientCard, useReloadClinic, useSaveNote, useToday } from "./clinicData";
import { formatDate, formatTime, VISIT_KIND_LABEL } from "./format";
import { Field, ReadError, RecordHistory, useDiscreet, useShownName } from "./parts";

export interface NoteDialogProps {
  patientId: string;
  /** The visit the note documents; without it (and without `noteId`) it is a note on its own. */
  visit?: Visit;
  /** An existing note to open (from the card's list of notes). */
  noteId?: string;
  onClose: () => void;
  onSaved?: (note: SessionNote) => void;
}

/** The most text each of the two parts holds (the core's limit). */
const NOTE_MAX = 20_000;

export function NoteDialog(props: NoteDialogProps) {
  const now = useToday();
  const card = usePatientCard(props.patientId, now.date);
  const settings = useClinicSettings();
  const reload = useReloadClinic();
  if (!card.data || !settings.data) {
    const failed = card.error ?? settings.error;
    return (
      <Dialog title="סיכום פגישה" onClose={props.onClose}>
        {failed
          ? <ReadError error={failed.message} retry={reload} />
          : <Skeleton lines={6} label="טוען את הסיכום" />}
      </Dialog>
    );
  }
  // One note for a visit: opening a visit that has one opens that one.
  const note = card.data.notes.find((n) => (props.noteId ? n.id === props.noteId : props.visit !== undefined && n.visit_id === props.visit.id)) ?? null;
  const visit = props.visit ?? card.data.visits.find((v) => v.visit.id === note?.visit_id)?.visit ?? null;
  // The form keeps what she types: a fresh answer about the card (after a save, or as a minute
  // passes) never starts it over. It reads the stored note once, when it opens.
  return <NoteForm {...props} card={card.data} note={note} visit={visit} templates={settings.data.note_templates} today={now.date} />;
}

function NoteForm(props: Omit<NoteDialogProps, "visit"> & {
  card: PatientCard; note: SessionNote | null; visit: Visit | null; templates: { name: string; text: string }[]; today: string;
}) {
  const { card, visit, onClose } = props;
  const { notify } = useApp();
  const shown = useShownName();
  const discreet = useDiscreet();
  const save = useSaveNote();
  const toCase = useNoteToCase();
  /** The note as the vault holds it now: what was opened, then what each save gave back. */
  const [stored, setStored] = useState(props.note);
  const [record, setRecord] = useState(stored?.record ?? "");
  const [personal, setPersonal] = useState(stored?.personal ?? "");
  const [date, setDate] = useState(stored?.date ?? visit?.date ?? props.today);
  const [template, setTemplate] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** In discreet mode the personal note waits for her to ask for it, as on the card. */
  const [personalShown, setPersonalShown] = useState(false);
  const personalOpen = !discreet || personalShown;

  const dirty = record !== (stored?.record ?? "") || personal !== (stored?.personal ?? "") || (visit === null && date !== (stored?.date ?? props.today));
  const empty = record.trim() === "" && personal.trim() === "";
  const linked = card.case !== null && !card.case.in_trash ? card.case : null;
  const sent = stored !== null && stored.case_input_id !== null;
  const title = visit ? `סיכום ${VISIT_KIND_LABEL[visit.kind]}` : "סיכום שיחה";
  const subtitle = `${shown(card.patient.name)} · ${visit ? `${formatDate(visit.date)}, ${formatTime(visit.start)}` : "לא צמוד לפגישה"}`;

  /** Saves what is typed; answers the saved note, or `null` when it was refused. */
  async function persist(): Promise<SessionNote | null> {
    if (empty) {
      setError("הסיכום ריק. צריך לכתוב משהו ברשומה או בתרשומת האישית.");
      return null;
    }
    if (date === "") {
      setError("צריך לכתוב את תאריך הסיכום: יום.חודש.שנה.");
      return null;
    }
    setError(null);
    try {
      const saved = await save.mutateAsync({
        id: stored?.id ?? "",
        patient_id: card.patient.id,
        visit_id: visit?.id ?? null,
        date: visit?.date ?? date,
        record,
        personal,
        case_input_id: stored?.case_input_id ?? null,
      });
      setStored(saved);
      setRecord(saved.record);
      setPersonal(saved.personal);
      props.onSaved?.(saved);
      return saved;
    } catch (e) {
      setError((e as { message?: string }).message ?? "הסיכום לא נשמר. אפשר לנסות שוב.");
      return null;
    }
  }

  async function submit() {
    setBusy(true);
    const saved = await persist();
    setBusy(false);
    if (!saved) return;
    notify("הסיכום נשמר");
    onClose();
  }

  async function sendToCase() {
    setBusy(true);
    try {
      // What goes to the case is what is saved: first save, then copy.
      const saved = dirty || stored === null ? await persist() : stored;
      if (!saved) return;
      setError(null);
      setStored(await toCase.mutateAsync(saved.id));
      notify("הרשומה הועתקה לחומרי התיק. התרשומת האישית נשארה כאן בלבד.");
    } catch (e) {
      setError((e as { message?: string }).message ?? "הרשומה לא הועברה. אפשר לנסות שוב.");
    } finally {
      setBusy(false);
    }
  }

  function insertTemplate() {
    const chosen = props.templates.find((t) => t.name === template);
    // Only into an empty record: a skeleton never lands on top of what she wrote.
    if (chosen && record.trim() === "") setRecord(chosen.text);
  }

  return (
    <Dialog help="visit-note" title={title} subtitle={subtitle} dirty={dirty} onClose={onClose}
      footer={(close) => <>
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || !dirty} onClick={() => void submit()}>שמירה</button>
      </>}>
      <div className="stack clinic-form">
        {visit === null && (
          <Field id="note-date" label="תאריך">
            <DateField id="note-date" value={date} onChange={setDate} notAfterToday />
          </Field>
        )}

        <div className="clinic-note-part">
          <header className="clinic-note-head">
            <h3><label htmlFor="note-record">רשומה</label></h3>
            <p className="hint" id="note-record-hint">זו הרשומה הרשמית של הפגישה. המשפחה רשאית לבקש לעיין בה.</p>
          </header>
          {props.templates.length > 0 && (
            <div className="row clinic-template">
              <label htmlFor="note-template" className="small">תבנית</label>
              <select id="note-template" className="select" value={template} onChange={(e) => setTemplate(e.target.value)}>
                <option value="">בחירת תבנית</option>
                {props.templates.map((t) => <option key={t.name} value={t.name}>{t.name}</option>)}
              </select>
              <button type="button" className="btn btn-small" disabled={template === "" || record.trim() !== ""} onClick={insertTemplate}>הוספת התבנית</button>
              {record.trim() !== "" && template !== "" && <span className="hint">תבנית נכנסת רק לרשומה ריקה.</span>}
            </div>
          )}
          <textarea id="note-record" className="textarea clinic-note-text" rows={9} value={record} maxLength={NOTE_MAX}
            aria-describedby="note-record-hint" autoFocus onChange={(e) => setRecord(e.target.value)} />
        </div>

        <div className="clinic-note-part clinic-note-personal">
          <header className="clinic-note-head">
            <h3><label htmlFor="note-personal">תרשומת אישית</label></h3>
            <p className="hint" id="note-personal-hint">שלך בלבד: מחשבות והשערות לעצמך. היא לא מודפסת, לא מיוצאת ולא עוברת לתיק האבחון.</p>
          </header>
          {personalOpen ? (
            <textarea id="note-personal" className="textarea clinic-note-text" rows={5} value={personal} maxLength={NOTE_MAX}
              aria-describedby="note-personal-hint" onChange={(e) => setPersonal(e.target.value)} />
          ) : (
            <p className="row clinic-note-hidden">
              <span className="muted">התרשומת האישית מוסתרת במצב דיסקרטי.</span>
              <button type="button" className="btn btn-small" onClick={() => setPersonalShown(true)}>להציג</button>
            </p>
          )}
        </div>

        {linked ? (
          <div className="note-sand clinic-to-case">
            <div className="stack grow">
              <span>
                {sent
                  ? "הרשומה כבר הועברה לחומרי התיק. העברה נוספת תחליף את העותק שבתיק בנוסח שכאן."
                  : "אפשר להעתיק את הרשומה לחומרי תיק האבחון, והיא תשמש לכתיבת הדוח."}
                {" "}רק הרשומה מועתקת. התרשומת האישית נשארת כאן.
              </span>
              <span className="row">
                <button type="button" className="btn btn-small" disabled={busy || record.trim() === ""} onClick={() => void sendToCase()}>
                  {sent ? "להעביר שוב לחומרי התיק" : "להעביר לחומרי התיק"}
                </button>
                {sent && <span className="chip chip-ok">הועברה לתיק {linked.code}</span>}
              </span>
            </div>
          </div>
        ) : (
          <p className="hint">
            {card.case?.in_trash
              ? "תיק האבחון של המטופל נמצא בסל המחזור, ולכן אי אפשר להעביר אליו את הרשומה."
              : "למטופל עוד אין תיק אבחון. אחרי שנפתח תיק מכרטיס המטופל, אפשר להעביר אליו את הרשומה."}
          </p>
        )}

        <ErrorLine error={error} />
        {stored && <RecordHistory recordId={stored.id} />}
      </div>
    </Dialog>
  );
}
