// Scheduling a visit and editing one: who, what kind, when, where, what it costs; a repeat for
// a new one; "only this one" or "this and the following" for a visit of a series; what
// happened in it; removing one that is still planned; and every saved state of it.
//
// One dialog from the first moment to the last: while the settings and the patients are read
// it shows the shape of the form, then the form, then (when the saved visit overlaps another)
// the notice about it. The frame is the same element throughout, so the focus that moved into
// it stays in it, and goes back to whatever opened it when it closes.
import { useState } from "react";
import { useApp } from "../../App";
import { DateField } from "../../components/DateField";
import { Segmented } from "../../components/Switch";
import { Dialog, ErrorLine, Skeleton } from "../../components/ui";
import type { ClinicSettings } from "../../ipc/generated/ClinicSettings";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import type { RememberItem } from "../../ipc/generated/RememberItem";
import type { SavedVisits } from "../../ipc/generated/SavedVisits";
import type { Scope } from "../../ipc/generated/Scope";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import type { VisitStatus } from "../../ipc/generated/VisitStatus";
import type { VisitView } from "../../ipc/generated/VisitView";
import { FreeTimes, HolidayLine } from "./FreeTimes";
import { suggestStart } from "./calendarLayout";
import { RememberList, withTyped } from "./RememberList";
import { VisitPrep } from "./VisitPrep";
import { useClinicSettings, useDeleteVisit, usePatients, useReloadClinic, useSaveVisit, useToday } from "./clinicData";
import {
  addDays, formatDate, formatDateLong, formatTime, leftAloneWords, optionsWith, ownTime, parseMoney, parseTime, VISIT_KIND_LABEL, VISIT_KINDS,
  VISIT_STATUS_LABEL, VISIT_STATUSES,
} from "./format";
import {
  CheckField, ConfirmDialog, Field, MoneyField, moneyProblem, moneyText, PatientPicker, ReadError, RecordHistory, SelectField, TextAreaField,
  TextField, TimeField,
} from "./parts";

/** What is already known when a new visit is opened (from a patient's card, from an hour of the calendar). */
export interface VisitStart {
  patientId?: string;
  date?: string;
  start?: number;
  kind?: VisitKind;
}

export interface VisitDialogProps {
  /** The visit to edit; without it a new one is scheduled. */
  view?: VisitView;
  start?: VisitStart;
  onClose: () => void;
  onSaved?: (saved: SavedVisits) => void;
}

const EVERY: { value: string; label: string }[] = [
  { value: "1", label: "כל שבוע" }, { value: "2", label: "כל שבועיים" }, { value: "3", label: "כל 3 שבועות" }, { value: "4", label: "כל 4 שבועות" },
];

/** How long a kind of visit usually is, and what it costs this family. */
function defaultsFor(settings: ClinicSettings, kind: VisitKind, patient: PatientRow | undefined): { minutes: number; fee: number } {
  const d = settings.defaults.find((x) => x.kind === kind);
  return { minutes: d?.minutes ?? 60, fee: ownTime(kind, patient?.patient.id) ? 0 : (patient?.patient.session_fee ?? d?.fee ?? 0) };
}

interface Form {
  kind: VisitKind;
  patientId: string | null;
  title: string;
  date: string;
  start: string;
  minutes: string;
  place: string;
  fee: string;
  note: string;
  remember: RememberItem[];
  /** A line typed under "לא לשכוח" without "הוספה": saved with the visit, and closing asks first. */
  rememberTyped: string;
  status: VisitStatus;
  statusNote: string;
  chargeMissed: boolean;
  repeat: boolean;
  every: string;
  count: string;
  scope: Scope;
}

/** The form as it opens: the visit as it is kept, or a new one with what is already known. */
function firstForm(props: VisitDialogProps, settings: ClinicSettings, patients: PatientRow[], now: { date: string; minute: number }): Form {
  const today = now.date;
  // Today, not an hour already gone (it would show at once as "not marked"), as the calendar does.
  const existing = props.view?.visit;
  if (existing) {
    return {
      kind: existing.kind, patientId: existing.patient_id, title: existing.title, date: existing.date, start: formatTime(existing.start),
      minutes: String(existing.minutes), place: existing.place, fee: moneyText(existing.fee), note: existing.note, remember: existing.remember ?? [], rememberTyped: "", status: existing.status,
      statusNote: existing.status_note, chargeMissed: existing.charge_missed, repeat: false, every: "1", count: "8", scope: "one",
    };
  }
  const kind = props.start?.kind ?? "assessment";
  const patientId = props.start?.patientId ?? null;
  const d = defaultsFor(settings, kind, patients.find((r) => r.patient.id === patientId));
  const date = props.start?.date ?? today;
  return {
    kind, patientId, title: "", date, start: formatTime(props.start?.start ?? suggestStart(date, now, settings.day_start, settings.day_end)),
    minutes: String(d.minutes), place: settings.places[0] ?? "", fee: moneyText(d.fee), note: "", remember: [], rememberTyped: "", status: "planned", statusNote: "",
    chargeMissed: false, repeat: false, every: "1", count: "8", scope: "one",
  };
}

export function VisitDialog(props: VisitDialogProps) {
  const { view, onClose } = props;
  const now = useToday();
  const settingsRead = useClinicSettings();
  const patientsRead = usePatients(now.date);
  const reload = useReloadClinic();
  const existing = view?.visit;
  const { notify } = useApp();
  const save = useSaveVisit();
  const remove = useDeleteVisit();
  const title = existing ? "פרטי הפגישה" : "פגישה חדשה";

  // The form starts once, when what it starts from was read; a later answer about the
  // settings or the patients never starts it over.
  const [opened, setOpened] = useState<Form | null>(null);
  const [draft, setForm] = useState<Form | null>(null);
  // What she typed herself is hers: a default never overwrites it.
  const [typed, setTyped] = useState({ minutes: false, fee: false });
  const [error, setError] = useState<string | null>(null);
  // The date box keeps what she types; a free time picked on another day starts it afresh.
  const [dateRev, setDateRev] = useState(0);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  /** Set once the visit was saved and overlaps another: the dialog stays to say so. */
  const [overlapping, setOverlapping] = useState<SavedVisits | null>(null);

  if (opened === null && settingsRead.data && patientsRead.data) {
    const start = firstForm(props, settingsRead.data, patientsRead.data, now);
    setOpened(start);
    setForm(start);
  }
  if (!settingsRead.data || !patientsRead.data || opened === null || draft === null) {
    const failed = settingsRead.error ?? patientsRead.error;
    return (
      <Dialog title={title} onClose={onClose}>
        {failed && (!settingsRead.data || !patientsRead.data)
          ? <ReadError error={failed.message} retry={reload} />
          : <Skeleton lines={5} label="טוען את פרטי הפגישה" />}
      </Dialog>
    );
  }
  // From here on everything is there; the names below keep that for the functions too.
  const settings = settingsRead.data;
  const patients = patientsRead.data;
  const first = opened;
  const form = draft;

  const patientOf = (id: string | null) => patients.find((r) => r.patient.id === id);
  const set = (patch: Partial<Form>) => setForm((f) => (f === null ? f : { ...f, ...patch }));

  /** The usual length and fee follow the kind and the family, for a new visit only. */
  function withDefaults(kind: VisitKind, patientId: string | null): Partial<Form> {
    const d = defaultsFor(settings, kind, patientOf(patientId));
    // Own time has no fee: the field is hidden and the visit is saved with 0, and what she typed
    // waits there in case she picks the child again.
    const own = ownTime(kind, patientId);
    // A supervision kept without a child, given one later, becomes a visit on the child's card:
    // it takes the usual fee, unless she typed one.
    if (existing) return existing.patient_id === null && !own && !typed.fee ? { fee: moneyText(d.fee) } : {};
    return { ...(typed.minutes ? {} : { minutes: String(d.minutes) }), ...(own || typed.fee ? {} : { fee: moneyText(d.fee) }) };
  }

  // Reserved time never has a child; supervision may be about one (a kindergarten teacher, the
  // staff), and is then a visit like any other, with a fee; without one it is her own time (D-207).
  const noChild = form.kind === "block";
  const block = ownTime(form.kind, form.patientId);
  const missed = form.status === "cancelled" || form.status === "no_show";
  const inSeries = existing !== undefined && existing.series_id !== null;
  // A visit of a patient stays on that patient's card (D-177): the core moves it to no other
  // card, and does not turn it into time of her own.
  const onCard = existing !== undefined && existing.patient_id !== null;
  const kinds = onCard ? VISIT_KINDS.filter((k) => k !== "block") : VISIT_KINDS;
  const dirty = JSON.stringify(form) !== JSON.stringify(first);
  // A planned visit at an hour already gone is allowed (she may write one down after the
  // fact), but she is told, so an hour left as it opened does not slip by.
  const startMinute = parseTime(form.start);
  // Her own time is never marked, so it is not said of it.
  const passed = !block && form.status === "planned" && form.date !== "" && startMinute !== null
    && (form.date < now.date || (form.date === now.date && startMinute < now.minute))
    && (!existing || form.date !== first.date || form.start !== first.start);
  const every = Number(form.every);
  const count = Number(form.count);
  const lastOfSeries = form.repeat && form.date !== "" && Number.isInteger(count) && count >= 2 && count <= 52 ? addDays(form.date, 7 * every * (count - 1)) : null;

  /** The visit as it will be saved, or the sentence that says what is missing. */
  function build(): Visit | string {
    if (!block && form.patientId === null && form.kind !== "supervision") return "צריך לבחור מטופל לפגישה. לזמן שהוא שלך בלבד בוחרים בסוג \"זמן שמור\".";
    if (form.date === "") return "צריך לכתוב את תאריך הפגישה: יום.חודש.שנה.";
    const start = parseTime(form.start);
    if (start === null) return "צריך לכתוב את שעת ההתחלה, למשל 9:30.";
    const minutes = Number(form.minutes);
    if (!Number.isInteger(minutes) || minutes < 5 || minutes > 720) return "משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.";
    if (start + minutes > 1440) return "הפגישה צריכה להסתיים עד חצות. אפשר לקצר אותה או להקדים את שעת ההתחלה.";
    const fee = block ? 0 : parseMoney(form.fee.trim() === "" ? "0" : form.fee);
    if (fee === null) return moneyProblem(form.fee) ?? "המחיר לא תקין.";
    if (form.repeat && !(Number.isInteger(count) && count >= 2 && count <= 52)) return "סדרה של פגישות חוזרות היא בין 2 ל-52 פגישות.";
    return {
      id: existing?.id ?? "",
      patient_id: noChild ? null : form.patientId,
      kind: form.kind,
      title: form.title,
      date: form.date,
      start,
      minutes,
      place: form.place,
      status: existing ? form.status : "planned",
      status_note: missed ? form.statusNote : "",
      fee,
      charge_missed: missed && !block && form.chargeMissed,
      series_id: existing?.series_id ?? null,
      note: form.note,
      remember: withTyped(form.remember, form.rememberTyped),
    };
  }

  async function submit() {
    const visit = build();
    if (typeof visit === "string") {
      setError(visit);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const saved = await save.mutateAsync({
        visit,
        repeat: !existing && form.repeat ? { every_weeks: every, count } : null,
        scope: inSeries ? form.scope : "one",
      });
      props.onSaved?.(saved);
      // A later visit of the series that has a note, or was already marked, stayed as it was
      // (D-176): she is told how many, so she does not look for them at the new hour.
      const alone = leftAloneWords(saved.left_alone);
      notify(`${saved.visits.length > 1 ? `נשמרו ${saved.visits.length} פגישות` : "הפגישה נשמרה"}${alone === "" ? "" : `. ${alone}`}`);
      // Saved anyway: her calendar, her call. But she is told before the dialog goes.
      if (saved.overlapping > 0) setOverlapping(saved);
      else onClose();
    } catch (e) {
      setError((e as { message?: string }).message ?? "הפגישה לא נשמרה. אפשר לנסות שוב.");
    } finally {
      setBusy(false);
    }
  }

  if (overlapping) {
    const clashes = overlapping.visits.filter((v) => v.overlaps);
    return (
      <Dialog narrow title="הפגישה נשמרה" onClose={onClose}
        footer={<button type="button" className="btn btn-primary" autoFocus onClick={onClose}>הבנתי</button>}>
        <div className="note-warn clinic-overlap" role="alert">
          <span>
            {overlapping.visits.length === 1
              ? "שימי לב: הפגישה חופפת לפגישה אחרת באותו זמן. היא נשמרה כמו שהיא, ואפשר לשנות את השעה ביומן."
              : `שימי לב: ${clashes.length === 1 ? "פגישה אחת מהסדרה חופפת" : `${clashes.length} פגישות מהסדרה חופפות`} לפגישות אחרות. הכל נשמר כמו שהוא, ואפשר לשנות ביומן.`}
          </span>
        </div>
        {overlapping.visits.length > 1 && (
          <ul className="clinic-list">
            {clashes.map((v) => <li key={v.visit.id}>{formatDateLong(v.visit.date)}, <span className="num" dir="ltr">{formatTime(v.visit.start)}</span></li>)}
          </ul>
        )}
      </Dialog>
    );
  }

  return (
    <Dialog help="visit" title={title} dirty={dirty} onClose={onClose}
      footer={(close) => <>
        {existing?.status === "planned" && (
          <button type="button" className="btn btn-danger-quiet" disabled={busy} onClick={() => setConfirmDelete(true)}>מחיקת הפגישה</button>
        )}
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void submit()}>{existing ? "שמירה" : "קביעת הפגישה"}</button>
      </>}>
      <div className="stack clinic-form">
        {view?.overlaps && (
          <div className="note-warn" role="status"><span>הפגישה הזו חופפת לפגישה אחרת באותו זמן.</span></div>
        )}

        {/* Opening a visit, she first sees what not to forget for it (D-207). */}
        {existing && <RememberList id="visit-remember" items={form.remember} onChange={(remember) => set({ remember })}
          text={form.rememberTyped} onText={(rememberTyped) => set({ rememberTyped })} />}

        {/* And, while it is still ahead, what the last session left and what is open for this family. */}
        {existing && existing.status === "planned" && !block && existing.patient_id !== null && (
          <VisitPrep patientId={existing.patient_id} visit={existing} today={now.date} />
        )}

        <SelectField id="visit-kind" label="סוג הפגישה" value={form.kind}
          options={optionsWith(kinds, VISIT_KIND_LABEL, form.kind)}
          hint={form.kind === "supervision" ? "הדרכה של צוות, גננת או מטפלת אחרת. על ילד מסוים: בוחרים אותו, והפגישה נכנסת לכרטיס שלו עם מחיר. הדרכה מקצועית כללית: בלי ילד ובלי מחיר. להדרכת הורים בוחרים \"הדרכת הורים\"."
            : block ? "זמן שמור הוא שלך בלבד: כתיבת דוחות, השתלמות, יום חופש. בלי מטופל ובלי מחיר."
            : form.kind === "unknown" ? "הסוג שנשמר לפגישה הזו לא מוכר לגרסה הזו של התוכנה. כדי לשמור שינוי צריך לבחור סוג מהרשימה, או לעדכן את התוכנה."
            : undefined}
          onChange={(kind) => set({ kind, ...(kind === "block" ? { patientId: null } : {}), ...withDefaults(kind, kind === "block" ? null : form.patientId) })} />

        {!noChild && (
          <PatientPicker id="visit-patient" value={form.patientId} autoFocus={!existing && form.patientId === null} locked={onCard}
            label={form.kind === "supervision" ? "על איזה ילד (לא חובה)" : "מטופל"}
            onChange={(patientId) => set({ patientId, ...withDefaults(form.kind, patientId) })} />
        )}

        <TextField id="visit-title" label={form.kind === "supervision" ? "עם מי או על מה (לא חובה)" : block ? "מה בזמן הזה" : "כותרת (לא חובה)"} value={form.title} maxLength={200}
          placeholder={form.kind === "supervision" ? "הדרכה קבוצתית" : block ? "כתיבת דוחות" : undefined} onChange={(title) => set({ title })}
          hint={block ? undefined : "היומן יציג את הכותרת במקום שם הסוג."} />

        <div className="clinic-form-row">
          <Field id="visit-date" label="תאריך">
            <DateField key={dateRev} id="visit-date" value={form.date} onChange={(date) => set({ date })} />
          </Field>
          <TimeField id="visit-start" label="שעת התחלה" value={form.start} onChange={(start) => set({ start })} />
          <TextField id="visit-minutes" label="משך (דקות)" value={form.minutes} dir="ltr" inputMode="numeric" className="clinic-narrow"
            onChange={(minutes) => { setTyped((t) => ({ ...t, minutes: true })); set({ minutes }); }} />
        </div>
        <HolidayLine date={form.date} enabled={settings.holidays} />
        {passed && (
          <p className="note-warn" role="status">
            <span>השעה הזו כבר עברה. אפשר לשמור כך, והפגישה תופיע בין הפגישות שעוד לא סומנו.</span>
          </p>
        )}
        {(!existing || existing.status === "planned") && (
          <FreeTimes from={form.date === "" ? now.date : form.date} minutes={Number(form.minutes)} skip={existing?.id ?? null} today={now.date}
            onPick={(slot) => {
              if (slot.date !== form.date) setDateRev((n) => n + 1);
              set({ date: slot.date, start: formatTime(slot.start) });
            }} />
        )}

        <div className="clinic-form-row">
          <TextField id="visit-place" label="מקום" value={form.place} maxLength={200} list="visit-places" className="grow" onChange={(place) => set({ place })} />
          <datalist id="visit-places">{settings.places.map((p) => <option key={p} value={p} />)}</datalist>
          {!block && (
            <MoneyField id="visit-fee" label="מחיר" value={form.fee} optional hint="ריק או 0: בלי חיוב." className="clinic-narrow"
              onChange={(fee) => { setTyped((t) => ({ ...t, fee: true })); set({ fee }); }} />
          )}
        </div>

        <TextAreaField id="visit-note" label="הערה מעשית (לא חובה)" value={form.note} rows={2} maxLength={4000}
          placeholder="למשל: חדר 2, החניה מאחור" onChange={(note) => set({ note })} />

        {!existing && <RememberList id="visit-remember" items={form.remember} onChange={(remember) => set({ remember })}
          text={form.rememberTyped} onText={(rememberTyped) => set({ rememberTyped })} />}

        {!existing && (
          <fieldset className="clinic-group">
            <legend className="visually-hidden">פגישה חוזרת</legend>
            <CheckField id="visit-repeat" label="פגישה חוזרת" checked={form.repeat} onChange={(repeat) => set({ repeat })} />
            {form.repeat && (
              <div className="clinic-form-row">
                <SelectField id="visit-every" label="כל כמה זמן" value={form.every} options={EVERY} onChange={(v) => set({ every: v })} />
                <TextField id="visit-count" label="כמה פגישות בסך הכל" value={form.count} dir="ltr" inputMode="numeric" className="clinic-narrow"
                  onChange={(v) => set({ count: v })} hint={lastOfSeries ? `האחרונה: ${formatDate(lastOfSeries)}` : "בין 2 ל-52."} />
              </div>
            )}
          </fieldset>
        )}

        {inSeries && (
          <div className="field">
            <span className="label">הפגישה הזו חלק מסדרה חוזרת. על מה השינוי חל?</span>
            <Segmented label="על מה השינוי חל" value={form.scope} onChange={(scope) => set({ scope })}
              options={[{ value: "one", label: "רק הפגישה הזו" }, { value: "following", label: "זו וכל הבאות" }]} />
            <p className="hint">
              {form.scope === "following"
                ? "הסוג, השעה, המשך, המקום, המחיר וההערה (בלי רשימת \"לא לשכוח\") יעברו גם לפגישות הבאות שעוד מתוכננות, והן יזוזו באותו מספר ימים. פגישה שכבר יש לה סיכום, או שכבר סומנה (התקיימה, בוטלה, לא הגיעו), נשארת בדיוק כמו שהיא."
                : "הפגישות האחרות בסדרה לא משתנות."}
            </p>
          </div>
        )}

        {existing && !block && (
          <div className="field">
            <span className="label">מה קרה בפגישה</span>
            <Segmented label="מה קרה בפגישה" value={form.status} onChange={(status) => set({ status })}
              options={VISIT_STATUSES.map((s) => ({ value: s, label: VISIT_STATUS_LABEL[s] }))} />
            {form.status === "unknown" && (
              <p className="hint">המצב שנשמר לפגישה הזו לא מוכר לגרסה הזו של התוכנה. כדי לשמור שינוי צריך לבחור מצב מהרשימה, או לעדכן את התוכנה.</p>
            )}
            {existing.status !== "planned" && form.status === "planned" && (
              <p className="hint">חזרה ל"מתוכננת" היא תיקון של סימון שנעשה בטעות. היא אפשרית רק כשלפגישה אין סיכום, וכשמה שהמשפחה כבר שילמה לא עולה על מה שנשאר לחייב בלי הפגישה הזו.</p>
            )}
            {missed && (
              <>
                <TextField id="visit-status-note" label={form.status === "cancelled" ? "למה בוטלה (לא חובה)" : "מה קרה (לא חובה)"} value={form.statusNote}
                  maxLength={200} onChange={(statusNote) => set({ statusNote })} />
                <CheckField id="visit-charge-missed" label="לחייב בכל זאת" checked={form.chargeMissed} onChange={(chargeMissed) => set({ chargeMissed })}
                  hint="מחיר הפגישה ייכנס לחשבון של המשפחה, אף שהיא לא התקיימה." />
              </>
            )}
          </div>
        )}

        <ErrorLine error={error} />
        {existing && <RecordHistory recordId={existing.id} />}
      </div>

      {confirmDelete && existing && (
        <ConfirmDialog title="מחיקת הפגישה" action="מחיקה" danger onClose={() => setConfirmDelete(false)}
          onConfirm={async () => {
            await remove.mutateAsync({ id: existing.id, scope: inSeries ? form.scope : "one" });
            notify(inSeries && form.scope === "following" ? "הפגישות נמחקו" : "הפגישה נמחקה");
            onClose();
          }}>
          <p>
            {inSeries && form.scope === "following"
              ? "הפגישה הזו וכל הפגישות שאחריה בסדרה, שעוד מתוכננות, יימחקו מהיומן."
              : "הפגישה תימחק מהיומן."}
            {" "}בהיסטוריה יישאר רישום שהיא נמחקה.
          </p>
        </ConfirmDialog>
      )}
    </Dialog>
  );
}
