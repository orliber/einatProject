// The clinic's settings: calendar hours and working days, the usual length and fee of each
// kind of visit, places, the two message texts (with their placeholders and a live preview),
// the note templates, the assessment pathway and its package (D-198), the starter kit of a new
// family (D-199), the gap kept around a visit and the holidays (D-196, D-197), the copy of the
// calendar to Google Calendar (D-202, a switch of its own that acts at once), and the three
// thresholds of "מה דורש אותי". Everything is saved at
// once, with "שמירה": the core tidies and checks it, and its refusal is the message shown.
// What she changed and did not save yet is kept while the program is open (in memory only),
// so going to another page and back loses nothing. They are the part "הקליניקה" of the
// program's settings (D-205), no longer a tab of the clinic: the page's own look comes along.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useApp } from "../../App";
import { Segmented } from "../../components/Switch";
import { CloseIcon, ErrorLine, PlusIcon, Skeleton } from "../../components/ui";
import type { ClinicSettings } from "../../ipc/generated/ClinicSettings";
import type { NoteTemplate } from "../../ipc/generated/NoteTemplate";
import type { TaskKind } from "../../ipc/generated/TaskKind";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import { CalendarSyncSection } from "./CalendarSyncSection";
import { useClinicSettings, useReloadClinic, useSaveSettings } from "./clinicData";
import { defaultClinicSettings, fillPreview, PLACEHOLDERS, sampleValues } from "./clinicDefaults";
import { formatTime, moneyInput, parseMoney, parseTime, TASK_KIND_LABEL, TASK_KINDS, VISIT_KIND_LABEL, VISIT_KINDS } from "./format";
import { CheckField, ConfirmDialog, ReadError, Section, SelectField, TextAreaField, TextField, TimeField } from "./parts";
import "./ClinicScreen.css";
import "./ClinicSettingsTab.css";

// ------------------------------------------------------------------ the form's own shape

/** The settings as she types them: every number is the text in its box until it is saved. */
export interface SettingsDraft {
  start: string;
  end: string;
  days: number[];
  kinds: { kind: VisitKind; minutes: string; fee: string }[];
  places: string[];
  reminder: string;
  payment: string;
  templates: NoteTemplate[];
  reportDays: string;
  debtDays: string;
  waitingWeeks: string;
  buffer: string;
  holidays: boolean;
  pathway: { kind: VisitKind; gap: string }[];
  packageFee: string;
  packageName: string;
  starter: { kind: TaskKind; title: string; due: string }[];
}

/** The limits of the core, for the boxes (the core checks them again). */
const MAX_PLACES = 12;
const MAX_TEMPLATES = 20;
const PLACE_MAX = 200;
const MESSAGE_MAX = 1000;
const TEMPLATE_MAX = 5000;
const MAX_STEPS = 12;
const MAX_STARTER = 20;
/** The kinds a step of the pathway can be: every visit with the family or the child. */
const STEP_KINDS = VISIT_KINDS.filter((k) => k !== "block");

const DAY_NAMES = ["ראשון", "שני", "שלישי", "רביעי", "חמישי", "שישי", "שבת"];

export function toDraft(s: ClinicSettings): SettingsDraft {
  return {
    start: formatTime(s.day_start),
    // The end of the day may be midnight, which she writes as 00:00.
    end: s.day_end >= 1440 ? "00:00" : formatTime(s.day_end),
    days: [...s.work_days].sort((a, b) => a - b),
    kinds: s.defaults.map((d) => ({ kind: d.kind, minutes: String(d.minutes), fee: moneyInput(d.fee) })),
    places: [...s.places],
    reminder: s.reminder_text,
    payment: s.payment_text,
    templates: s.note_templates.map((t) => ({ ...t })),
    reportDays: String(s.report_days),
    debtDays: String(s.debt_days),
    waitingWeeks: String(s.waiting_weeks),
    buffer: String(s.buffer_minutes),
    holidays: s.holidays,
    pathway: s.pathway.map((p) => ({ kind: p.kind, gap: String(p.gap_days) })),
    packageFee: s.package_fee === 0 ? "" : moneyInput(s.package_fee),
    packageName: s.package_name,
    starter: s.starter_tasks.map((t) => ({ kind: t.kind, title: t.title, due: String(t.due_days) })),
  };
}

const wholeNumber = (text: string): number | null => (/^\d{1,6}$/.test(text.trim()) ? Number(text.trim()) : null);

/** The settings a draft stands for, or what in it is not even a number or an hour. Whether
 *  the values make sense together is the core's to say. */
export function fromDraft(d: SettingsDraft): { settings: ClinicSettings } | { problem: string } {
  const start = parseTime(d.start);
  if (start === null) return { problem: "שעת ההתחלה של היום ביומן לא ברורה. כותבים שעה, למשל 8:00." };
  const endTyped = parseTime(d.end);
  if (endTyped === null) return { problem: "שעת הסיום של היום ביומן לא ברורה. כותבים שעה, למשל 20:00." };
  const defaults = [];
  for (const k of d.kinds) {
    const label = VISIT_KIND_LABEL[k.kind];
    const minutes = wholeNumber(k.minutes);
    if (minutes === null) return { problem: `המשך של ${label} צריך להיות מספר של דקות, למשל 50.` };
    const fee = k.kind === "block" || k.fee.trim() === "" ? 0 : parseMoney(k.fee);
    if (fee === null) return { problem: `המחיר של ${label} צריך להיות סכום בשקלים, למשל 450 או 450.50, עד 100,000.` };
    defaults.push({ kind: k.kind, minutes, fee });
  }
  const reportDays = wholeNumber(d.reportDays);
  const debtDays = wholeNumber(d.debtDays);
  const waitingWeeks = wholeNumber(d.waitingWeeks);
  if (reportDays === null) return { problem: "מספר הימים עד שהדוח אמור להיות מוכן צריך להיות מספר שלם, למשל 21." };
  if (debtDays === null) return { problem: "מספר הימים עד שמצביעים על חוב צריך להיות מספר שלם, למשל 30." };
  if (waitingWeeks === null) return { problem: "מספר השבועות ברשימת ההמתנה צריך להיות מספר שלם, למשל 6." };
  const buffer = wholeNumber(d.buffer);
  if (buffer === null) return { problem: "הזמן הפנוי בין פגישות צריך להיות מספר של דקות, למשל 15, או 0." };
  const pathway = [];
  for (const [i, step] of d.pathway.entries()) {
    // The first step is on the day she starts from (D-198): it has no gap, and no box for one.
    const gap = i === 0 ? 0 : wholeNumber(step.gap);
    if (gap === null) return { problem: `הרווח בשלב ${i + 1} של מסלול האבחון צריך להיות מספר של ימים, למשל 7.` };
    pathway.push({ kind: step.kind, gap_days: gap });
  }
  const packageFee = d.packageFee.trim() === "" ? 0 : parseMoney(d.packageFee);
  if (packageFee === null) return { problem: "המחיר של חבילת האבחון צריך להיות סכום בשקלים, למשל 4500, עד 100,000. ריק: בלי חבילה." };
  const starter = [];
  for (const [i, t] of d.starter.entries()) {
    const due = wholeNumber(t.due);
    if (due === null) return { problem: `מספר הימים במעקב ${i + 1} של ערכת הפתיחה צריך להיות מספר שלם, למשל 14.` };
    starter.push({ kind: t.kind, title: t.title, due_days: due });
  }
  return {
    settings: {
      day_start: start,
      day_end: endTyped === 0 ? 1440 : endTyped,
      work_days: [...d.days].sort((a, b) => a - b),
      defaults,
      places: d.places,
      reminder_text: d.reminder,
      payment_text: d.payment,
      note_templates: d.templates,
      report_days: reportDays,
      debt_days: debtDays,
      waiting_weeks: waitingWeeks,
      buffer_minutes: buffer,
      holidays: d.holidays,
      pathway,
      package_fee: packageFee,
      package_name: d.packageName,
      starter_tasks: starter,
    },
  };
}

/** A list with the item at `from` moved one place up (-1) or down (+1). */
export function moved<T>(list: T[], from: number, by: -1 | 1): T[] {
  const to = from + by;
  if (from < 0 || from >= list.length || to < 0 || to >= list.length) return list;
  const next = [...list];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item as T);
  return next;
}

// What she changed and did not save, kept while the program is open: in memory, never in
// storage. Locking the vault reloads the page, so nothing of it outlives the session.
let kept: SettingsDraft | null = null;
let leaving: number | undefined;

/** The four small tabs of the settings (D-205): one subject at a time, one "שמירה" for all. */
export type SettingsGroup = "calendar" | "visits" | "pathway" | "messages";
export const SETTINGS_GROUPS: { value: SettingsGroup; label: string }[] = [
  { value: "calendar", label: "יומן ושעות" },
  { value: "visits", label: "פגישות ומחירים" },
  { value: "pathway", label: "מסלול ומשפחה חדשה" },
  { value: "messages", label: "הודעות ותזכורות" },
];
/** The tab she was on, while the program is open (in memory only, like the unsaved changes). */
let lastGroup: SettingsGroup = "calendar";

/** For the tests: each starts with nothing kept, on the first tab. */
export function forgetSettingsDraft() {
  kept = null;
  lastGroup = "calendar";
  window.clearTimeout(leaving);
}

const same = (a: SettingsDraft, b: SettingsDraft) => JSON.stringify(a) === JSON.stringify(b);

// ------------------------------------------------------------------ the tab

export function ClinicSettingsTab() {
  const settings = useClinicSettings();
  const reload = useReloadClinic();
  if (settings.error && !settings.data) return <ReadError error={settings.error.message} retry={reload} />;
  if (!settings.data) {
    return (
      <div className="stack clinic-settings" aria-busy="true">
        <Skeleton card lines={4} label="טוען את הגדרות הקליניקה" />
        <Skeleton card lines={6} label="טוען את סוגי הפגישות" />
      </div>
    );
  }
  return <SettingsForm stored={settings.data} />;
}

function SettingsForm({ stored }: { stored: ClinicSettings }) {
  const { notify } = useApp();
  const save = useSaveSettings();
  const base = useMemo(() => toDraft(stored), [stored]);
  const [draft, setDraft] = useState<SettingsDraft>(() => kept ?? base);
  /** She came back to changes she had left unsaved. */
  const [cameBack, setCameBack] = useState(() => kept !== null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [newPlace, setNewPlace] = useState("");
  const [group, setGroupState] = useState<SettingsGroup>(lastGroup);
  const setGroup = (g: SettingsGroup) => { lastGroup = g; setGroupState(g); };
  /** Every section stays in the form (one draft, one save); only the chosen tab's are shown. */
  const hide = (g: SettingsGroup) => group !== g;
  const dirty = !same(draft, base);
  const set = (patch: Partial<SettingsDraft>) => setDraft((d) => ({ ...d, ...patch }));

  useEffect(() => {
    kept = dirty ? draft : null;
  }, [draft, dirty]);

  // Leaving the part with unsaved changes: they are kept, and she is told where they wait.
  // (The timer lets a remount in development pass without a word.)
  const say = useRef(notify);
  useEffect(() => {
    say.current = notify;
  });
  useEffect(() => {
    window.clearTimeout(leaving);
    return () => {
      leaving = window.setTimeout(() => {
        if (kept !== null) say.current("השינויים בהגדרות הקליניקה עוד לא נשמרו. הם מחכים בהגדרות, בחלק \"הקליניקה\".");
      }, 0);
    };
  }, []);

  async function submit(e?: FormEvent) {
    e?.preventDefault();
    if (!dirty || busy) return;
    const out = fromDraft(draft);
    if ("problem" in out) {
      setError(out.problem);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // What comes back is what the core stored, tidied: that is what the form shows now.
      setDraft(toDraft(await save.mutateAsync(out.settings)));
      setCameBack(false);
      notify("הגדרות הקליניקה נשמרו");
    } catch (err) {
      setError((err as { message?: string }).message ?? "ההגדרות לא נשמרו. אפשר לנסות שוב.");
    } finally {
      setBusy(false);
    }
  }

  function discard() {
    setDraft(base);
    setError(null);
    setCameBack(false);
  }

  function addPlace() {
    const place = newPlace.split(/\s+/).filter(Boolean).join(" ");
    if (place === "" || draft.places.includes(place) || draft.places.length >= MAX_PLACES) return;
    set({ places: [...draft.places, place] });
    setNewPlace("");
  }

  const setKind = (kind: VisitKind, patch: { minutes?: string; fee?: string }) => set({ kinds: draft.kinds.map((k) => (k.kind === kind ? { ...k, ...patch } : k)) });
  const setTemplate = (i: number, patch: Partial<NoteTemplate>) => set({ templates: draft.templates.map((t, j) => (j === i ? { ...t, ...patch } : t)) });
  const setStep = (i: number, patch: Partial<SettingsDraft["pathway"][number]>) => set({ pathway: draft.pathway.map((p, j) => (j === i ? { ...p, ...patch } : p)) });
  const setStarter = (i: number, patch: Partial<SettingsDraft["starter"][number]>) => set({ starter: draft.starter.map((t, j) => (j === i ? { ...t, ...patch } : t)) });
  const stepKinds = STEP_KINDS.map((value) => ({ value, label: VISIT_KIND_LABEL[value] }));
  const taskKinds = TASK_KINDS.map((value) => ({ value, label: TASK_KIND_LABEL[value] }));
  const sample = sampleValues(draft.places[0] ?? "");
  const placeTaken = newPlace.trim() !== "" && draft.places.includes(newPlace.split(/\s+/).filter(Boolean).join(" "));

  return (
    <form className="stack clinic-settings" aria-label="הגדרות הקליניקה" noValidate onSubmit={(e) => void submit(e)}>
      {cameBack && dirty && (
        <p className="note-sand clinic-kept" role="status">השינויים שעשית כאן קודם עוד לא נשמרו, והם כאן כמו שהשארת אותם. אפשר לשמור אותם או לבטל.</p>
      )}

      <div className="clinic-settings-groups">
        <Segmented label="נושא בהגדרות הקליניקה" value={group} options={SETTINGS_GROUPS} onChange={setGroup} />
      </div>

      <Section title="שעות וימי עבודה" hidden={hide("calendar")}>
        <p className="hint clinic-quiet">היומן מציג את השעות האלה, ומדגיש את ימי העבודה.</p>
        <div className="clinic-form-row">
          <TimeField id="settings-start" label="היום מתחיל ב-" value={draft.start} onChange={(start) => set({ start })} />
          <TimeField id="settings-end" label="ומסתיים ב-" value={draft.end} onChange={(end) => set({ end })} />
        </div>
        <fieldset className="clinic-group">
          <legend className="label">ימי עבודה</legend>
          <div className="row clinic-days">
            {DAY_NAMES.map((name, day) => (
              <CheckField key={day} id={`settings-day-${day}`} label={name} checked={draft.days.includes(day)}
                onChange={(on) => set({ days: on ? [...draft.days, day].sort((a, b) => a - b) : draft.days.filter((x) => x !== day) })} />
            ))}
          </div>
        </fieldset>
        <div className="clinic-form-row">
          <TextField id="settings-buffer" label="דקות פנויות לפני ואחרי כל פגישה" value={draft.buffer} maxLength={3} dir="ltr" inputMode="numeric" className="clinic-threshold"
            hint="כשהתוכנה מציעה שעה פנויה, היא משאירה את הזמן הזה בין פגישה לפגישה. 0: צמוד." onChange={(buffer) => set({ buffer })} />
        </div>
        <CheckField id="settings-holidays" label="להציג את חגי ישראל ביומן" checked={draft.holidays} onChange={(holidays) => set({ holidays })}
          hint="החגים מחושבים במחשב לפי הלוח העברי. בימי חג (ראש השנה, יום כיפור, סוכות, שמחת תורה, פסח, שבועות ויום העצמאות) התוכנה לא מציעה שעות פנויות." />
      </Section>

      <Section title="מסלול האבחון" hidden={hide("pathway")} aside={<span className="muted small">{draft.pathway.length === 1 ? "שלב אחד" : `${draft.pathway.length} שלבים`}</span>}>
        <p className="hint clinic-quiet">
          השלבים שהתוכנה קובעת בלחיצה אחת מכרטיס המטופל: השלב הראשון בשעה הפנויה הראשונה מהיום שבוחרים, וכל שלב אחריו
          לפחות כמה ימים אחרי השלב שלפניו (לפחות יום אחד). לפני השמירה רואים את כל הפגישות, ואפשר לשנות כל אחת.
        </p>
        {draft.pathway.length === 0 && <p className="muted clinic-quiet">עוד אין שלבים במסלול.</p>}
        <ol className="clinic-list clinic-steps" aria-label="שלבי מסלול האבחון, לפי הסדר">
          {draft.pathway.map((step, i) => (
            <li key={i}><div className="clinic-step" role="group" aria-label={`שלב ${i + 1}: ${VISIT_KIND_LABEL[step.kind]}`}>
              <span className="clinic-step-num num" aria-hidden="true">{i + 1}</span>
              <SelectField id={`settings-step-${i}-kind`} label="סוג הפגישה" value={step.kind} options={stepKinds} onChange={(kind) => setStep(i, { kind })} />
              {i === 0
                ? <p className="clinic-step-first muted small">מהיום שבוחרים</p>
                : <TextField id={`settings-step-${i}-gap`} label="ימים אחרי השלב הקודם" value={step.gap} maxLength={3} dir="ltr"
                  inputMode="numeric" className="clinic-narrow" onChange={(gap) => setStep(i, { gap })} />}
              <div className="row clinic-place-actions">
                <button type="button" className="btn btn-small" disabled={i === 0} aria-label={`להעלות את שלב ${i + 1}`} onClick={() => set({ pathway: moved(draft.pathway, i, -1) })}>למעלה</button>
                <button type="button" className="btn btn-small" disabled={i === draft.pathway.length - 1} aria-label={`להוריד את שלב ${i + 1}`} onClick={() => set({ pathway: moved(draft.pathway, i, 1) })}>למטה</button>
                <button type="button" className="btn btn-small icon-btn" aria-label={`הסרת שלב ${i + 1}`} onClick={() => set({ pathway: draft.pathway.filter((_, j) => j !== i) })}><CloseIcon /></button>
              </div>
            </div></li>
          ))}
        </ol>
        {draft.pathway.length < MAX_STEPS
          ? <button type="button" className="btn btn-small clinic-add" onClick={() => set({ pathway: [...draft.pathway, { kind: "assessment", gap: "7" }] })}><PlusIcon /> שלב נוסף</button>
          : <p className="hint">אפשר להגדיר עד {MAX_STEPS} שלבים.</p>}
        <div className="clinic-form-row clinic-package">
          <TextField id="settings-package-fee" label="מחיר חבילת אבחון (₪)" value={draft.packageFee} maxLength={12} dir="ltr" inputMode="numeric" className="clinic-threshold"
            hint="ריק: כל פגישה במסלול מחויבת בנפרד. עם מחיר: אפשר לבחור חבילה, ואז נרשם חיוב אחד והפגישות עצמן בלי מחיר." onChange={(packageFee) => set({ packageFee })} />
          <TextField id="settings-package-name" label="השם של החבילה בחיוב" value={draft.packageName} maxLength={200} className="grow" onChange={(packageName) => set({ packageName })} />
        </div>
      </Section>

      <Section title="ערכת פתיחה למשפחה חדשה" hidden={hide("pathway")} aside={<span className="muted small">{draft.starter.length === 1 ? "מעקב אחד" : `${draft.starter.length} מעקבים`}</span>}>
        <p className="hint clinic-quiet">
          השאלונים והמסמכים שמבקשים מכל משפחה. בלחיצה אחת בכרטיס הם נפתחים כמעקבים, כל אחד עם תאריך יעד.
          שבוע לפני שיחת המשוב, מה שעוד פתוח מופיע ב"מה דורש אותי".
        </p>
        {draft.starter.length === 0 && <p className="muted clinic-quiet">עוד אין מעקבים בערכה.</p>}
        <ul className="clinic-list clinic-starter">
          {draft.starter.map((t, i) => (
            <li key={i}><div className="clinic-step" role="group" aria-label={`מעקב ${i + 1}${t.title.trim() === "" ? "" : `: ${t.title.trim()}`}`}>
              <SelectField id={`settings-starter-${i}-kind`} label="סוג" value={t.kind} options={taskKinds} onChange={(kind) => setStarter(i, { kind })} />
              <TextField id={`settings-starter-${i}-title`} label="מה לבקש" value={t.title} maxLength={200} className="grow fit-l" onChange={(title) => setStarter(i, { title })} />
              <TextField id={`settings-starter-${i}-due`} label="תוך כמה ימים" value={t.due} maxLength={3} dir="ltr" inputMode="numeric" className="clinic-narrow"
                onChange={(due) => setStarter(i, { due })} />
              <button type="button" className="btn btn-small icon-btn clinic-step-remove" aria-label={`הסרת מעקב ${i + 1}`} onClick={() => set({ starter: draft.starter.filter((_, j) => j !== i) })}><CloseIcon /></button>
            </div></li>
          ))}
        </ul>
        {draft.starter.length < MAX_STARTER
          ? <button type="button" className="btn btn-small clinic-add" onClick={() => set({ starter: [...draft.starter, { kind: "document", title: "", due: "14" }] })}><PlusIcon /> מעקב נוסף</button>
          : <p className="hint">אפשר להגדיר עד {MAX_STARTER} מעקבים.</p>}
      </Section>

      <Section title="סוגי הפגישות" hidden={hide("visits")}>
        <p className="hint clinic-quiet">פגישה חדשה מקבלת את המשך ואת המחיר שכאן. בכל פגישה אפשר לשנות אותם, ולמשפחה אפשר לקבוע מחיר משלה בכרטיס.</p>
        <div className="clinic-settings-scroll">
          <table className="clinic-settings-table">
            <caption className="visually-hidden">המשך והמחיר הרגילים של כל סוג פגישה</caption>
            <thead>
              <tr>
                <th scope="col">סוג הפגישה</th>
                <th scope="col">משך רגיל (דקות)</th>
                <th scope="col">מחיר רגיל (₪)</th>
              </tr>
            </thead>
            <tbody>
              {draft.kinds.map((k) => {
                const label = VISIT_KIND_LABEL[k.kind];
                return (
                  <tr key={k.kind}>
                    <th scope="row">{label}</th>
                    <td>
                      <input className="input num clinic-settings-num" dir="ltr" inputMode="numeric" value={k.minutes} maxLength={4}
                        aria-label={`משך של ${label}, בדקות`} onChange={(e) => setKind(k.kind, { minutes: e.target.value })} />
                    </td>
                    <td>
                      {k.kind === "block"
                        ? <span className="muted small">זמן שלך, בלי מחיר</span>
                        : (
                          <input className="input num clinic-settings-num" dir="ltr" inputMode="decimal" value={k.fee} maxLength={12}
                            aria-label={`מחיר של ${label}, בשקלים`} onChange={(e) => setKind(k.kind, { fee: e.target.value })} />
                        )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </Section>

      <Section title="מקומות" hidden={hide("visits")}>
        <p className="hint clinic-quiet">המקומות שאפשר לבחור לפגישה. הראשון ברשימה הוא זה שמוצע בפגישה חדשה.</p>
        {draft.places.length === 0
          ? <p className="muted clinic-quiet">עוד אין מקומות ברשימה.</p>
          : (
            <ol className="clinic-list clinic-places" aria-label="המקומות, לפי הסדר">
              {draft.places.map((place, i) => (
                <li key={place} className="clinic-place">
                  <span className="grow">{place}</span>
                  {i === 0 && <span className="chip chip-sand">מוצע ראשון</span>}
                  <div className="row clinic-place-actions">
                    <button type="button" className="btn btn-small" disabled={i === 0} aria-label={`להעלות את ${place}`} onClick={() => set({ places: moved(draft.places, i, -1) })}>למעלה</button>
                    <button type="button" className="btn btn-small" disabled={i === draft.places.length - 1} aria-label={`להוריד את ${place}`} onClick={() => set({ places: moved(draft.places, i, 1) })}>למטה</button>
                    <button type="button" className="btn btn-small icon-btn" aria-label={`הסרת ${place}`} onClick={() => set({ places: draft.places.filter((_, j) => j !== i) })}><CloseIcon /></button>
                  </div>
                </li>
              ))}
            </ol>
          )}
        {draft.places.length < MAX_PLACES ? (
          <div className="row clinic-place-add">
            <div className="field">
              <label htmlFor="settings-new-place">מקום חדש</label>
              <input id="settings-new-place" className="input" value={newPlace} maxLength={PLACE_MAX} placeholder="למשל: שיחת וידאו"
                aria-describedby={placeTaken ? "settings-new-place-taken" : undefined}
                onChange={(e) => setNewPlace(e.target.value)}
                onKeyDown={(e) => {
                  // Enter adds the place; it does not save the whole form.
                  if (e.key === "Enter") {
                    e.preventDefault();
                    addPlace();
                  }
                }} />
              {placeTaken && <span className="hint" id="settings-new-place-taken">המקום הזה כבר ברשימה.</span>}
            </div>
            <button type="button" className="btn clinic-place-go" disabled={newPlace.trim() === "" || placeTaken} onClick={addPlace}><PlusIcon /> הוספת מקום</button>
          </div>
        ) : <p className="hint">אפשר לשמור עד {MAX_PLACES} מקומות.</p>}
      </Section>

      <Section title="הודעות להורים" hidden={hide("messages")}>
        <p className="hint clinic-quiet">
          התוכנה לא שולחת הודעות בעצמה. היא ממלאת את הנוסח בפרטי הפגישה או היתרה, פותחת אצלך במחשב את הצ'אט עם ההורה בוואטסאפ, ב-SMS או במייל, ומשאירה את הנוסח בלוח המוגן. את מדביקה ושולחת בעצמך.
          הנוסח שהגיע עם התוכנה לא כולל את שם הילד בכוונה, כדי שהשם לא ייצא מהמחשב בלי שבחרת בזה. מי שרוצה יכולה להוסיף {"{ילד}"}.
        </p>
        <MessageEditor id="settings-reminder" label="נוסח התזכורת לפגישה" value={draft.reminder} sample={sample} onChange={(reminder) => set({ reminder })} />
        <MessageEditor id="settings-payment" label="נוסח הודעת התשלום" value={draft.payment} sample={sample} onChange={(payment) => set({ payment })} />
      </Section>

      <div className="stack clinic-settings" hidden={hide("calendar")}>
        <CalendarSyncSection />
      </div>

      <Section title="תבניות לסיכום פגישה" hidden={hide("messages")} aside={<span className="muted small">{draft.templates.length === 1 ? "תבנית אחת" : `${draft.templates.length} תבניות`}</span>}>
        <p className="hint clinic-quiet">שלד שאפשר להכניס לרשומה של סיכום פגישה ריק, כדי לא להתחיל מדף לבן.</p>
        {draft.templates.length === 0 && <p className="muted clinic-quiet">עוד אין תבניות.</p>}
        <ul className="clinic-list clinic-templates">
          {draft.templates.map((t, i) => (
            <li key={i}>
              <div className="clinic-template-item" role="group" aria-label={`תבנית ${i + 1}${t.name.trim() === "" ? "" : `: ${t.name.trim()}`}`}>
                <div className="clinic-form-row">
                  <TextField id={`settings-template-${i}-name`} label="שם התבנית" value={t.name} maxLength={200} className="grow" onChange={(name) => setTemplate(i, { name })} />
                  <button type="button" className="btn btn-small clinic-template-remove" onClick={() => set({ templates: draft.templates.filter((_, j) => j !== i) })}>הסרת התבנית</button>
                </div>
                <TextAreaField id={`settings-template-${i}-text`} label="הטקסט של התבנית" value={t.text} rows={5} maxLength={TEMPLATE_MAX} onChange={(text) => setTemplate(i, { text })} />
              </div>
            </li>
          ))}
        </ul>
        {draft.templates.length < MAX_TEMPLATES
          ? <button type="button" className="btn btn-small clinic-add" onClick={() => set({ templates: [...draft.templates, { name: "", text: "" }] })}><PlusIcon /> תבנית חדשה</button>
          : <p className="hint">אפשר לשמור עד {MAX_TEMPLATES} תבניות.</p>}
      </Section>

      <Section title="מתי להסב את תשומת הלב" hidden={hide("messages")}>
        <p className="hint clinic-quiet">שלושת המספרים האלה קובעים מתי משהו מופיע ב"מה דורש אותי".</p>
        <div className="clinic-thresholds">
          <TextField id="settings-report-days" label="ימים עד שהדוח אמור להיות מוכן" value={draft.reportDays} maxLength={5} dir="ltr" inputMode="numeric" className="clinic-threshold"
            hint="כשעברו כל כך הרבה ימים ממפגש האבחון האחרון והדוח עוד לא גמור, הילד מופיע כ״דוח מתעכב״." onChange={(reportDays) => set({ reportDays })} />
          <TextField id="settings-debt-days" label="ימים עד שמצביעים על חוב" value={draft.debtDays} maxLength={5} dir="ltr" inputMode="numeric" className="clinic-threshold"
            hint="חיוב שלא שולם כל כך הרבה ימים מופיע כ״חוב פתוח״. עד אז היתרה רק רשומה בכרטיס ובכספים." onChange={(debtDays) => set({ debtDays })} />
          <TextField id="settings-waiting-weeks" label="שבועות ברשימת ההמתנה" value={draft.waitingWeeks} maxLength={5} dir="ltr" inputMode="numeric" className="clinic-threshold"
            hint="משפחה שממתינה כל כך הרבה שבועות מופיעה כ״המתנה ארוכה״." onChange={(waitingWeeks) => set({ waitingWeeks })} />
        </div>
      </Section>

      <div className="card clinic-save-bar" role="group" aria-label="שמירת ההגדרות">
        <ErrorLine error={error} />
        <div className="row clinic-save-row">
          <button type="button" className="link-btn" onClick={() => setResetting(true)}>לחזור לברירות המחדל</button>
          <span className="grow" />
          <span className="muted small" role="status">{dirty ? "יש שינויים שעוד לא נשמרו." : "כל ההגדרות שמורות."}</span>
          {dirty && <button type="button" className="btn" disabled={busy} onClick={discard}>ביטול השינויים</button>}
          <button type="submit" className="btn btn-primary" disabled={!dirty || busy}>שמירה</button>
        </div>
      </div>

      {resetting && (
        <ConfirmDialog title="לחזור לברירות המחדל" action="להחזיר את ברירות המחדל" danger onClose={() => setResetting(false)}
          onConfirm={async () => {
            setDraft(toDraft(await save.mutateAsync(defaultClinicSettings())));
            setError(null);
            setCameBack(false);
            setResetting(false);
            notify("הגדרות הקליניקה חזרו לברירות המחדל");
          }}>
          <p>כל הגדרות הקליניקה יחזרו למצב שבו התוכנה הגיעה: שעות היומן וימי העבודה, המשך של כל סוג פגישה, רשימת המקומות, נוסחי ההודעות, התבניות לסיכום, מסלול האבחון, ערכת הפתיחה ושלושת הספים.</p>
          <p>המחירים שקבעת יתאפסו, ומה שכתבת בנוסחים ובתבניות יוחלף. המטופלים, הפגישות והכספים לא משתנים.</p>
        </ConfirmDialog>
      )}
    </form>
  );
}

// ------------------------------------------------------------------ a message text

/** A message text: the box, the placeholders as chips that go in where the cursor is, and
 *  how the message will read, filled with invented details. */
function MessageEditor({ id, label, value, sample, onChange }: {
  id: string; label: string; value: string; sample: ReturnType<typeof sampleValues>; onChange: (text: string) => void;
}) {
  const box = useRef<HTMLTextAreaElement>(null);
  /** Where the cursor goes once the text with the new placeholder is in the box. */
  const caret = useRef<number | null>(null);
  useLayoutEffect(() => {
    if (caret.current === null || !box.current) return;
    box.current.focus();
    box.current.setSelectionRange(caret.current, caret.current);
    caret.current = null;
  }, [value]);

  function insert(token: string) {
    const el = box.current;
    const from = el ? el.selectionStart : value.length;
    const to = el ? el.selectionEnd : value.length;
    const next = value.slice(0, from) + token + value.slice(to);
    if (next.length > MESSAGE_MAX) return;
    caret.current = from + token.length;
    onChange(next);
  }

  return (
    <div className="stack clinic-message">
      <div className="field">
        <label htmlFor={id}>{label}</label>
        <textarea id={id} ref={box} className="textarea" rows={3} value={value} maxLength={MESSAGE_MAX} aria-describedby={`${id}-preview`}
          onChange={(e) => onChange(e.target.value)} />
      </div>
      <div className="row clinic-placeholders" role="group" aria-label={`מילים שמתמלאות לבד, עבור ${label}`}>
        <span className="muted small">להוסיף במקום הסמן:</span>
        {PLACEHOLDERS.map((p) => (
          <button key={p.token} type="button" className="chip clinic-placeholder" title={p.means} aria-label={`להוסיף ${p.token}: ${p.means}`} onClick={() => insert(p.token)}>{p.token}</button>
        ))}
      </div>
      <div className="note-sand stack clinic-preview" id={`${id}-preview`}>
        <span className="small muted">כך ההודעה תיראה (הפרטים כאן מומצאים):</span>
        <span className="clinic-preview-text">{value.trim() === "" ? "הנוסח ריק, ולכן ההודעה תהיה ריקה." : fillPreview(value, sample)}</span>
      </div>
    </div>
  );
}
