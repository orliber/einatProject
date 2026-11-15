// The clinic's settings: calendar hours and working days, the usual length and fee of each
// kind of visit, places, the two message texts (with their placeholders and a live preview),
// the note templates, and the three thresholds of "מה דורש אותי". Everything is saved at
// once, with "שמירה": the core tidies and checks it, and its refusal is the message shown.
// What she changed and did not save yet is kept while the program is open (in memory only),
// so going to another tab and back loses nothing.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useApp } from "../../App";
import { CloseIcon, ErrorLine, PlusIcon, Skeleton } from "../../components/ui";
import type { ClinicSettings } from "../../ipc/generated/ClinicSettings";
import type { NoteTemplate } from "../../ipc/generated/NoteTemplate";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import { useClinicSettings, useReloadClinic, useSaveSettings } from "./clinicData";
import { defaultClinicSettings, fillPreview, PLACEHOLDERS, sampleValues } from "./clinicDefaults";
import { formatTime, moneyInput, parseMoney, parseTime, VISIT_KIND_LABEL } from "./format";
import { CheckField, ConfirmDialog, Section, TextAreaField, TextField, TimeField } from "./parts";
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
}

/** The limits of the core, for the boxes (the core checks them again). */
const MAX_PLACES = 12;
const MAX_TEMPLATES = 20;
const PLACE_MAX = 200;
const MESSAGE_MAX = 1000;
const TEMPLATE_MAX = 5000;

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

/** For the tests: each starts with nothing kept. */
export function forgetSettingsDraft() {
  kept = null;
  window.clearTimeout(leaving);
}

const same = (a: SettingsDraft, b: SettingsDraft) => JSON.stringify(a) === JSON.stringify(b);

// ------------------------------------------------------------------ the tab

export function ClinicSettingsTab() {
  const settings = useClinicSettings();
  const reload = useReloadClinic();
  if (settings.error && !settings.data) return <ErrorLine error={settings.error.message} retry={reload} />;
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
  const dirty = !same(draft, base);
  const set = (patch: Partial<SettingsDraft>) => setDraft((d) => ({ ...d, ...patch }));

  useEffect(() => {
    kept = dirty ? draft : null;
  }, [draft, dirty]);

  // Leaving the tab with unsaved changes: they are kept, and she is told where they wait.
  // (The timer lets a remount in development pass without a word.)
  const say = useRef(notify);
  useEffect(() => {
    say.current = notify;
  });
  useEffect(() => {
    window.clearTimeout(leaving);
    return () => {
      leaving = window.setTimeout(() => {
        if (kept !== null) say.current("השינויים בהגדרות הקליניקה עוד לא נשמרו. הם מחכים בלשונית \"הגדרות\".");
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
  const sample = sampleValues(draft.places[0] ?? "");
  const placeTaken = newPlace.trim() !== "" && draft.places.includes(newPlace.split(/\s+/).filter(Boolean).join(" "));

  return (
    <form className="stack clinic-settings" aria-label="הגדרות הקליניקה" noValidate onSubmit={(e) => void submit(e)}>
      {cameBack && dirty && (
        <p className="note-sand clinic-kept" role="status">השינויים שעשית כאן קודם עוד לא נשמרו, והם כאן כמו שהשארת אותם. אפשר לשמור אותם או לבטל.</p>
      )}

      <Section title="שעות וימי עבודה">
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
      </Section>

      <Section title="סוגי הפגישות">
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

      <Section title="מקומות">
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

      <Section title="הודעות להורים">
        <p className="hint clinic-quiet">
          התוכנה לא שולחת הודעות. היא ממלאת את הנוסח בפרטי הפגישה או היתרה ומעתיקה אותו, ואת מדביקה אותו בהודעה בעצמך.
          הנוסח שהגיע עם התוכנה לא כולל את שם הילד בכוונה, כדי שהשם לא ייצא מהמחשב בלי שבחרת בזה. מי שרוצה יכולה להוסיף {"{ילד}"}.
        </p>
        <MessageEditor id="settings-reminder" label="נוסח התזכורת לפגישה" value={draft.reminder} sample={sample} onChange={(reminder) => set({ reminder })} />
        <MessageEditor id="settings-payment" label="נוסח הודעת התשלום" value={draft.payment} sample={sample} onChange={(payment) => set({ payment })} />
      </Section>

      <Section title="תבניות לסיכום פגישה" aside={<span className="muted small">{draft.templates.length === 1 ? "תבנית אחת" : `${draft.templates.length} תבניות`}</span>}>
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

      <Section title="מתי להסב את תשומת הלב">
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
          <p>כל הגדרות הקליניקה יחזרו למצב שבו התוכנה הגיעה: שעות היומן וימי העבודה, המשך של כל סוג פגישה, רשימת המקומות, נוסחי ההודעות, התבניות לסיכום ושלושת הספים.</p>
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
