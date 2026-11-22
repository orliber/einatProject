// Browser preview and tests only: an in-memory stand-in for the clinic (docs/CLINIC.md), with
// the same rules the core keeps (crates/dv-domain/src/clinic/logic.rs): validation, series,
// overlaps, charges and balances, alerts, the assessment process, the diary, income, history
// and the messages; what may be erased and what an erased card leaves (D-174, D-179); a mark
// that stays (D-176); a record that never changes its card, and the names of a card that its
// case knows (D-177); a value of a later version that is shown and never saved (D-178). The
// refusals are the core's own sentences. Every patient, phone and text here is invented;
// nothing is stored and nothing leaves the page.
//
// Two clocks, as in the app: "today" of a request is the window's (her local day); what the
// core does by its own clock (the day a record was written, the retention period of an erase,
// the latest birth date, the time of a history line) goes by `options.clock`, read as the
// core reads it (UTC). A test gives a clock and nothing depends on the moment it runs.
import type { Age } from "../ipc/generated/Age";
import type { Alert } from "../ipc/generated/Alert";
import type { Balance } from "../ipc/generated/Balance";
import type { CaseChoice } from "../ipc/generated/CaseChoice";
import type { CaseLink } from "../ipc/generated/CaseLink";
import type { CaseMeta } from "../ipc/generated/CaseMeta";
import type { Charge } from "../ipc/generated/Charge";
import type { ClinicReply } from "../ipc/generated/ClinicReply";
import type { ClinicRequest } from "../ipc/generated/ClinicRequest";
import type { ClinicSettings } from "../ipc/generated/ClinicSettings";
import type { ClinicTask } from "../ipc/generated/ClinicTask";
import type { Channel } from "../ipc/generated/Channel";
import type { Composed } from "../ipc/generated/Composed";
import type { Contact } from "../ipc/generated/Contact";
import type { DiaryDay } from "../ipc/generated/DiaryDay";
import type { DiaryErased } from "../ipc/generated/DiaryErased";
import type { DiaryLine } from "../ipc/generated/DiaryLine";
import type { DiaryMonth } from "../ipc/generated/DiaryMonth";
import type { HistoryEntry } from "../ipc/generated/HistoryEntry";
import type { IdentityInput } from "../ipc/generated/IdentityInput";
import type { IncomeMonth } from "../ipc/generated/IncomeMonth";
import type { IncomeYear } from "../ipc/generated/IncomeYear";
import type { InputKind } from "../ipc/generated/InputKind";
import type { KindCount } from "../ipc/generated/KindCount";
import type { MessageKind } from "../ipc/generated/MessageKind";
import type { Overview } from "../ipc/generated/Overview";
import type { Patient } from "../ipc/generated/Patient";
import type { PatientCard } from "../ipc/generated/PatientCard";
import type { PatientRow } from "../ipc/generated/PatientRow";
import type { PayMethod } from "../ipc/generated/PayMethod";
import type { Payment } from "../ipc/generated/Payment";
import type { ProcessStep } from "../ipc/generated/ProcessStep";
import type { Repeat } from "../ipc/generated/Repeat";
import type { Role } from "../ipc/generated/Role";
import type { SavedVisits } from "../ipc/generated/SavedVisits";
import type { Scope } from "../ipc/generated/Scope";
import type { SessionNote } from "../ipc/generated/SessionNote";
import type { RecordingSound } from "../ipc/generated/RecordingSound";
import type { Slot } from "../ipc/generated/Slot";
import type { TaskView } from "../ipc/generated/TaskView";
import type { Today } from "../ipc/generated/Today";
import type { UiError } from "../ipc/generated/UiError";
import type { Visit } from "../ipc/generated/Visit";
import type { VisitKind } from "../ipc/generated/VisitKind";
import type { VisitStatus } from "../ipc/generated/VisitStatus";
import type { VisitView } from "../ipc/generated/VisitView";
import {
  addDays, CONTACT_ROLES, dayName, ownTime, ownVisit, daysBetween, formatDate, formatMonth, formatTime, isIsoDate, MAX_AGOROT, moneyNumber, monthOf, monthRange,
  PATIENT_STATUSES, PAY_METHOD_LABEL, PAY_METHODS, TASK_KINDS, today as localToday, VISIT_KIND_LABEL, VISIT_KINDS, VISIT_STATUS_LABEL,
  VISIT_STATUSES, weekday, weekRange,
} from "../screens/clinic/format";
import { fakeInsights } from "./fakeInsights";
import { previewHolidays } from "./previewHolidays";

// ------------------------------------------------------------------ the link to cases

/** A diagnostic case as the clinic needs to see it. */
export interface CaseInfo {
  id: string;
  code: string;
  childName: string | null;
  approved: number;
  total: number;
  inTrash: boolean;
}

/** What the clinic asks of the rest of the core: the cases, and her name for a message. */
export interface ClinicHost {
  cases(): CaseInfo[];
  createCase(meta: CaseMeta, identities: IdentityInput[]): string;
  /** Adds a material to a case; answers its id. */
  addInput(caseId: string, kind: InputKind, title: string, content: string): string;
  /** Replaces a material's title and text; `false` when it is no longer in the case. */
  updateInput(caseId: string, inputId: string, title: string, content: string): boolean;
  /** The people a case's filter knows, each with its id (the core's `identities`). */
  identities(caseId: string): IdentityInput[];
  /** Replaces the people of a case (the core's `set_identities`); one with an id keeps its tag. */
  setIdentities(caseId: string, list: IdentityInput[]): void;
  myName(): string;
}

/** Cases of its own, for a clinic that runs without the demo core (the unit tests). */
export function standInHost(): ClinicHost & { inputs: { id: string; caseId: string; kind: InputKind; title: string; content: string }[]; trash(id: string): void } {
  const cases: CaseInfo[] = [];
  const inputs: { id: string; caseId: string; kind: InputKind; title: string; content: string }[] = [];
  const people = new Map<string, IdentityInput[]>();
  let seq = 0;
  const withIds = (list: IdentityInput[]) => list.map((i) => ({ ...i, id: i.id ?? `person-local-${++seq}`, aliases: [...i.aliases] }));
  return {
    inputs,
    cases: () => cases.map((c) => ({ ...c })),
    createCase(meta, identities) {
      const id = `case-local-${++seq}`;
      cases.push({ id, code: meta.code, childName: identities.find((i) => i.role === "child")?.value ?? null, approved: 0, total: 16, inTrash: false });
      people.set(id, withIds(identities));
      return id;
    },
    identities: (caseId) => (people.get(caseId) ?? []).map((i) => ({ ...i, aliases: [...i.aliases] })),
    setIdentities(caseId, list) {
      people.set(caseId, withIds(list));
    },
    addInput(caseId, kind, title, content) {
      const id = `in-local-${++seq}`;
      inputs.push({ id, caseId, kind, title, content });
      return id;
    },
    updateInput(caseId, inputId, title, content) {
      const input = inputs.find((i) => i.id === inputId && i.caseId === caseId);
      if (!input) return false;
      input.title = title;
      input.content = content;
      return true;
    },
    myName: () => "רותם בדויה",
    trash(id) {
      const c = cases.find((x) => x.id === id);
      if (c) c.inTrash = true;
    },
  };
}

// ------------------------------------------------------------------ refusals and limits

function refuse(message: string, code = "refused"): never {
  const e: UiError = { code, message, details: [], action: { kind: "dismiss" } };
  throw e;
}

function need(ok: boolean, message: string): asserts ok {
  if (!ok) refuse(message);
}

const NAME = 120;
const SHORT = 200;
const LONG = 4_000;
const RECORD = 20_000;
const MESSAGE = 1_000;
const MAX_CONTACTS = 6;
const MAX_PLACES = 12;
const MAX_TEMPLATES = 20;
const TEMPLATE = 5_000;

/** Length as she counts it: letters, not UTF-16 units. */
const chars = (s: string) => Array.from(s).length;
const atMost = (s: string, max: number, field: string) => need(chars(s) <= max, `יש יותר מדי טקסט בשדה "${field}". אפשר לכתוב עד ${max} תווים.`);
const short = (s: string, field: string) => atMost(s, SHORT, field);
const long = (s: string, field: string) => atMost(s, LONG, field);
/** A date of the clinic as the core reads one (`parse_date`): a real day of the years 2000 to 2200. */
const clinicDate = (s: string) => isIsoDate(s) && s >= "2000-01-01" && s <= "2200-12-31";
const realDate = (s: string, field: string) => need(clinicDate(s), `צריך לבחור תאריך תקין בשדה "${field}".`);
/** The first year a birth date may carry (the core's `BIRTH_YEAR_MIN`): an older sibling, or an
 *  adult child whose parent came to consult, was born before the clinic's own days. */
export const BIRTH_YEAR_MIN = 1900;
const birthDate = (s: string) => isIsoDate(s) && s >= `${BIRTH_YEAR_MIN}-01-01` && s <= "2200-12-31";
/** A value of a later version (D-178) is shown and never written back (the core's `known`). */
const known = (ok: boolean, field: string) =>
  need(ok, `בשדה "${field}" יש ערך שהגרסה הזו של התוכנה לא מכירה. כדי לשמור צריך לבחור ערך מהרשימה, או לעדכן את התוכנה.`);
const visitKnown = (v: Visit) => {
  known(VISIT_KINDS.includes(v.kind), "סוג הפגישה");
  known(VISIT_STATUSES.includes(v.status), "מצב הפגישה");
};
const paymentKnown = (p: Payment) => known(PAY_METHODS.includes(p.method), "אמצעי התשלום");
const taskKnown = (t: ClinicTask) => known(TASK_KINDS.includes(t.kind), "סוג המשימה");
const patientKnown = (p: Patient) => {
  known(PATIENT_STATUSES.includes(p.status), "מצב הכרטיס");
  for (const c of p.contacts) known(CONTACT_ROLES.includes(c.role), "הקשר לילד");
};
/** What a value the program does not know reads as (the core's `#[serde(other)]`). */
const orUnknown = <T extends string>(choices: readonly T[], value: T): T | "unknown" => (choices.includes(value) ? value : "unknown");
/** One line, single spaces: what a name, a phone or a place is stored as. */
const cleanLine = (s: string) => s.split(/\s+/).filter(Boolean).join(" ");
const blankToNull = (s: string | null) => (s === null || s.trim() === "" ? null : s.trim());
const whole = (n: unknown, min: number, max: number): n is number => typeof n === "number" && Number.isInteger(n) && n >= min && n <= max;
const copy = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const NO_PATIENT = "המטופל שנבחר לא נמצא. ייתכן שהכרטיס נמחק.";

/** What the diary calls the one line of the cards that were erased (the core's `ERASED_LABEL`). */
export const ERASED_LABEL = "רשומות של כרטיסים שנמחקו";

/** What the erased cards counted for in one month (the core's `ErasedMonth`): numbers only.
 *  There is no field here that could hold a name, an id, a day or a text (D-179). */
interface ErasedMonth {
  held: KindCount[];
  cancelled: number;
  no_show: number;
  charged: number;
  received: { method: PayMethod; amount: number }[];
  missing_receipts: number;
}

/** A saved state of a record, with the card it was written for (the vault's history row). */
interface HistoryRow {
  entry: HistoryEntry;
  patientId: string | null;
}

/** How long a card is kept before it may be erased (D-174): the rule of a case (`retention.rs`). */
export const KEEP_YEARS_AFTER_LAST_CHANGE = 7;
export const KEEP_UNTIL_AGE = 25;
/** The calendar gives at most this many days in one answer. */
const MAX_RANGE_DAYS = 100;
const NO_NOTE_DAYS = 60;
const FEEDBACK_SOON_DAYS = 7;
const NO_NEXT_VISIT_DAYS = 14;

/** The kinds of session that need a record once they were held. */
const NOTE_KINDS: VisitKind[] = ["intake", "assessment", "observation", "feedback", "therapy", "guidance"];

/** The placeholders of a message text, as she types them. */
export const PLACEHOLDERS = ["{הורה}", "{ילד}", "{יום}", "{תאריך}", "{שעה}", "{מקום}", "{סכום}", "{שמי}"] as const;

/** The settings of a clinic nobody touched yet (as `ClinicSettings::default` in the core). */
export function defaultClinicSettings(): ClinicSettings {
  // "unknown" is only here because the table is by kind: it is never one of `VISIT_KINDS`.
  const minutes: Record<VisitKind, number> = { intake: 60, assessment: 90, observation: 60, feedback: 60, therapy: 50, guidance: 50, meeting: 30, other: 50, block: 60, supervision: 60, unknown: 60 };
  return {
    day_start: 8 * 60,
    day_end: 20 * 60,
    work_days: [0, 1, 2, 3, 4],
    defaults: VISIT_KINDS.map((kind) => ({ kind, minutes: minutes[kind], fee: 0 })),
    places: ["הקליניקה"],
    reminder_text: "שלום {הורה}, תזכורת לפגישה ביום {יום}, {תאריך}, בשעה {שעה}. להתראות, {שמי}",
    payment_text: "שלום {הורה}, היתרה לתשלום היא {סכום} ₪. תודה רבה, {שמי}",
    note_templates: [
      { name: "פגישת היכרות עם ההורים", text: "סיבת הפנייה:\n\nרקע התפתחותי:\n\nרקע משפחתי:\n\nמסגרת חינוכית:\n\nטיפולים ואבחונים קודמים:\n\nציפיות ההורים מהאבחון:\n" },
      { name: "מפגש אבחון", text: "כלים שהועברו:\n\nהתנהגות במפגש:\n\nשיתוף פעולה וקשב:\n\nרשמים ראשוניים:\n\nלהמשך:\n" },
      { name: "תצפית", text: "מקום התצפית ומשכה:\n\nמה נצפה:\n\nקשר עם ילדים ועם הצוות:\n\nדברי הצוות:\n" },
      { name: "שיחת משוב", text: "מה נמסר להורים:\n\nתגובת ההורים ושאלות שעלו:\n\nהמלצות שסוכמו:\n\nמה נמסר או נשלח:\n" },
    ],
    report_days: 21,
    debt_days: 30,
    waiting_weeks: 6,
    buffer_minutes: 15,
    holidays: true,
    pathway: [
      { kind: "intake", gap_days: 0 },
      { kind: "assessment", gap_days: 7 },
      { kind: "assessment", gap_days: 7 },
      { kind: "feedback", gap_days: 21 },
    ],
    package_fee: 0,
    package_name: "חבילת אבחון",
    starter_tasks: [
      { kind: "questionnaire", title: "שאלון להורים", due_days: 7 },
      { kind: "questionnaire", title: "שאלון למורה או לגננת", due_days: 14 },
      { kind: "document", title: "דוח או סיכום מהמסגרת החינוכית", due_days: 14 },
      { kind: "document", title: "אבחונים וסיכומים קודמים", due_days: 14 },
    ],
  };
}

const ymd = (iso: string) => iso.split("-").map(Number) as [number, number, number];
const daysInMonth = (y: number, m: number) => new Date(Date.UTC(y, m, 0)).getUTCDate();

/** Age on `on` of a child born on `birth`; `null` without a birth date, or when it is still ahead. */
export function ageOn(birth: string | null, on: string): Age | null {
  if (!birth || !birthDate(birth) || !clinicDate(on) || birth > on) return null;
  const [by, bm, bd] = ymd(birth);
  const [oy, om, od] = ymd(on);
  let months = (oy - by) * 12 + (om - bm);
  // A month is complete on the birth day, or on the month's last day when it is shorter.
  if (od < Math.min(bd, daysInMonth(oy, om))) months -= 1;
  const years = Math.floor(months / 12);
  return years > 255 ? null : { years, months: months % 12 };
}

/** The same day `years` later (29.2 becomes 28.2 in a year that has none). */
function addYears(iso: string, years: number): string {
  const [y, m, d] = ymd(iso);
  const day = Math.min(d, daysInMonth(y + years, m));
  return `${y + years}-${String(m).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

const byDateTime = (a: Visit, b: Visit) => a.date.localeCompare(b.date) || a.start - b.start || a.id.localeCompare(b.id);

/** What a visit costs the family: its fee when it was held, or when it was missed and she
 *  charges anyway; nothing otherwise (a block has no fee to begin with). */
export function visitCharge(v: Visit): number {
  const charged = v.status === "held" || ((v.status === "cancelled" || v.status === "no_show") && v.charge_missed);
  return charged ? Math.max(0, v.fee) : 0;
}

/** Two visits overlap when neither is cancelled, they are on the same date and their times meet. */
export function visitsOverlap(a: Visit, b: Visit): boolean {
  if (a.id === b.id || a.status === "cancelled" || b.status === "cancelled" || a.date !== b.date) return false;
  return a.start < b.start + b.minutes && b.start < a.start + a.minutes;
}

/** A message text with its placeholders filled in. One pass: a value is never read again as a
 *  placeholder, and anything else in braces stays as she typed it. */
export function fillMessage(text: string, values: Record<(typeof PLACEHOLDERS)[number], string>): string {
  let out = "";
  let rest = text;
  for (let at = rest.indexOf("{"); at >= 0; at = rest.indexOf("{")) {
    out += rest.slice(0, at);
    rest = rest.slice(at);
    const known = PLACEHOLDERS.find((p) => rest.startsWith(p));
    out += known ? values[known] : "{";
    rest = rest.slice(known ? known.length : 1);
  }
  return out + rest;
}

/** "label: value" when the value says something. */
const field = (label: string, value: string): string[] => (cleanLine(value) === "" ? [] : [`${label}: ${cleanLine(value)}`]);
const voidedLines = (voided: boolean, reason: string): string[] => (voided ? ["בוטל: כן", ...field("סיבת הביטול", reason)] : []);
/** Agorot as the core writes them in a sentence: "450", "450.50", no thousands mark. */
const plainMoney = (agorot: number) => moneyNumber(agorot).replace(/,/g, "");

// ------------------------------------------------------------------ the clinic

export class FakeClinic {
  private patients: Patient[] = [];
  private visits: Visit[] = [];
  private notes: SessionNote[] = [];
  /** The sound of each recording, by its id (D-208): never in a list, read only to play it. */
  private sounds = new Map<string, RecordingSound>();
  private payments: Payment[] = [];
  private charges: Charge[] = [];
  private tasks: ClinicTask[] = [];
  private settings: ClinicSettings = defaultClinicSettings();
  /** Every saved state of a visit, a session note, a charge or a payment, oldest first, by record id. */
  private history = new Map<string, HistoryRow[]>();
  /** The visits that were ever saved as something other than "planned" (D-176): what the
   *  core reads from the saved states of a visit. */
  private marked = new Set<string>();
  /** The day a record of each patient was last written (see `wrote`). */
  private lastSaved = new Map<string, string>();
  /** What the erased cards counted for, by month (D-179; the core's `ErasedTally`). */
  private tally = new Map<string, ErasedMonth>();
  /** Erased cards by the month they were opened in (the core's `ErasedCards`). */
  private erasedCards = new Map<string, number>();
  /** Rows that could not be read (D-178). Nothing in memory is ever unreadable: a test or the
   *  preview says how many there are, to see the alert (`unreadable`). */
  private unreadableRows = 0;
  private seq = 0;
  private seeded = false;
  private readonly host: ClinicHost;
  private readonly demo: boolean;
  private readonly clock: () => Date;

  /** `demo`: a working clinic of invented families (the preview); `false`: an empty one.
   *  `clock`: the core's clock (the computer's, unless a test holds it). */
  constructor(host: ClinicHost = standInHost(), options: { demo?: boolean; clock?: () => Date } = {}) {
    this.host = host;
    this.clock = options.clock ?? (() => new Date());
    const asked = typeof location === "undefined" ? null : new URLSearchParams(location.search).get("clinic");
    // `?clinic=empty` in the address: the clinic of a first day, for the demo page.
    this.demo = options.demo ?? asked !== "empty";
  }

  /** One request, answered as the core answers it. A refusal is thrown as a `UiError`. */
  answer(request: ClinicRequest): ClinicReply {
    // The demo families are written on the first request, when the demo cases already exist.
    if (!this.seeded) {
      this.seeded = true;
      if (this.demo) this.seed(localToday(this.clock()));
    }
    // What crosses the IPC is a copy on both sides: the window never holds the clinic's own records.
    return copy(this.route(copy(request)));
  }

  private route(r: ClinicRequest): ClinicReply {
    switch (r.op) {
      case "overview":
        return { kind: "overview", data: this.overview(this.today(r.today)) };
      case "calendar":
        need(isIsoDate(r.from) && isIsoDate(r.to), "צריך לבחור תאריכים תקינים ליומן.");
        need(r.to >= r.from, "תאריך הסיום ביומן צריך להיות אחרי תאריך ההתחלה.");
        need(daysBetween(r.from, r.to) < MAX_RANGE_DAYS, `אפשר להציג ביומן עד ${MAX_RANGE_DAYS} ימים בבת אחת.`);
        return { kind: "visits", data: this.visits.filter((v) => v.date >= r.from && v.date <= r.to).sort(byDateTime).map((v) => this.view(v)) };
      case "save_visit":
        return { kind: "saved", data: this.saveVisit(r.visit, r.repeat, r.scope) };
      case "delete_visit":
        this.deleteVisit(r.id, r.scope);
        return { kind: "done" };
      case "patients":
        return { kind: "patients", data: this.rows(this.today(r.today)) };
      case "patient_card":
        return { kind: "card", data: this.card(this.patient(r.id), this.today(r.today)) };
      case "save_patient":
        return { kind: "patient", data: this.savePatient(r.patient) };
      case "erase_patient":
        this.erasePatient(r.id, r.name);
        return { kind: "done" };
      case "save_note":
        return { kind: "note", data: this.saveNote(r.note) };
      case "delete_note":
        this.deleteNote(r.id);
        return { kind: "done" };
      case "note_to_case":
        return { kind: "note", data: this.noteToCase(r.id) };
      case "save_recording":
        return { kind: "note", data: this.saveRecording(r) };
      case "recording": {
        const sound = this.sounds.get(r.id) ?? refuse("ההקלטה לא נמצאה.", "not_found");
        return { kind: "sound", data: sound };
      }
      case "delete_recording": {
        const n = this.note(r.note_id);
        need(n.recordings.some((x) => x.id === r.id), "ההקלטה לא נמצאה.");
        const before = { ...n };
        n.recordings = n.recordings.filter((x) => x.id !== r.id);
        this.sounds.delete(r.id);
        this.keep(n.id, "changed", this.noteLines(n, before), n.patient_id);
        return { kind: "note", data: n };
      }
      case "save_payment":
        return { kind: "payment", data: this.savePayment(r.payment) };
      case "void_payment":
        return { kind: "payment", data: this.voidPayment(r.id, r.reason) };
      case "save_charge":
        return { kind: "charge", data: this.saveCharge(r.charge) };
      case "void_charge":
        return { kind: "charge", data: this.voidCharge(r.id, r.reason) };
      case "tasks":
        return { kind: "tasks", data: this.taskViews(this.today(r.today), false) };
      case "save_task":
        return { kind: "task", data: this.saveTask(r.task) };
      case "delete_task":
        need(this.tasks.some((t) => t.id === r.id), "המשימה לא נמצאה. ייתכן שכבר נמחקה.");
        this.tasks = this.tasks.filter((t) => t.id !== r.id);
        return { kind: "done" };
      case "diary":
        return { kind: "diary", data: this.diary(r.month) };
      case "income":
        return { kind: "income", data: this.income(r.year) };
      case "history":
        return { kind: "history", data: (this.history.get(r.record_id) ?? []).map((row) => row.entry) };
      case "settings":
        return { kind: "settings", data: this.settings };
      case "save_settings":
        this.settings = this.checkedSettings(r.settings);
        return { kind: "settings", data: this.settings };
      case "message":
        return { kind: "text", data: this.message(r.kind, r.patient_id, r.visit_id) };
      case "compose":
        return { kind: "composed", data: this.compose(r.kind, r.patient_id, r.visit_id, r.channel, r.contact) };
      case "case_choices":
        return { kind: "cases", data: this.caseChoices() };
      case "link_case":
        return { kind: "patient", data: this.linkCase(r.patient_id, r.case_id) };
      case "create_case":
        return { kind: "case", data: this.createCase(r.patient_id, this.today(r.today)) };
      case "holidays": {
        need(isIsoDate(r.from) && isIsoDate(r.to), "צריך לבחור תאריכים תקינים ליומן.");
        need(r.to >= r.from && daysBetween(r.from, r.to) <= 400, "אפשר לבקש את החגים של עד שנה אחת בכל פעם.");
        return { kind: "holidays", data: previewHolidays(r.from, r.to) };
      }
      case "free_slots":
        return { kind: "slots", data: this.freeSlots(this.today(r.today), r.from, r.minutes, r.count, r.skip_visit, []) };
      case "propose_pathway":
        return { kind: "proposed", data: this.proposePathway(r.patient_id, this.today(r.today), r.from) };
      case "save_pathway":
        return { kind: "saved", data: this.savePathway(r.patient_id, r.visits, r.package, this.today(r.today)) };
      case "starter_tasks":
        return { kind: "tasks", data: this.starterTasks(r.patient_id, this.today(r.today)) };
      case "insights": {
        const today = this.today(r.today);
        const data = fakeInsights(
          { patients: this.patients, visits: this.visits, charges: this.charges, payments: this.payments, settings: this.settings, tally: this.tally, erasedCards: this.erasedCards },
          r.from_month, r.to_month, today.date,
        );
        return { kind: "insights", data };
      }
    }
  }

  // ---------------------------------------------------------------- free times, the pathway, the starter kit (D-197..D-199)

  /** As the core's `free_slots`: working days that are not closed for a holiday, inside her
   *  hours, clear of every visit that is not cancelled by the gap; at most three a day. */
  private freeSlots(today: Today, from: string, minutes: number, count: number, skip: string | null, extra: Visit[]): Slot[] {
    need(whole(minutes, 5, 720), "משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
    need(whole(count, 1, 12), "אפשר לבקש עד 12 שעות פנויות.");
    need(clinicDate(from), "צריך לבחור תאריך תקין.");
    const s = this.settings;
    const closed = new Set(s.holidays ? previewHolidays(from > today.date ? from : today.date, addDays(from > today.date ? from : today.date, 120)).filter((h) => h.closed).map((h) => h.date) : []);
    const visits = [...this.visits.filter((v) => v.id !== skip), ...extra];
    const out: Slot[] = [];
    const first = from > today.date ? from : today.date;
    for (let i = 0; i < 120 && out.length < count; i++) {
      const date = addDays(first, i);
      if (!s.work_days.includes(weekday(date) ?? -1) || closed.has(date)) continue;
      const busy = visits.filter((v) => v.date === date && v.status !== "cancelled")
        .map((v) => [Math.max(0, v.start - s.buffer_minutes), v.start + v.minutes + s.buffer_minutes] as const);
      const floor = date === today.date ? Math.max(s.day_start, today.minute) : s.day_start;
      let start = Math.ceil(floor / 15) * 15;
      let onDay = 0;
      let last = -Infinity;
      for (; start + minutes <= s.day_end && onDay < 3 && out.length < count; start += 15) {
        if (start < last + minutes) continue;
        if (busy.some(([b0, b1]) => start < b1 && b0 < start + minutes)) continue;
        out.push({ date, start });
        last = start;
        onDay++;
      }
    }
    return out;
  }

  private proposePathway(patientId: string, today: Today, from: string): Visit[] {
    const p = this.patients.find((x) => x.id === patientId);
    if (!p) refuse(NO_PATIENT, "not_found");
    need(clinicDate(from), "צריך לבחור תאריך תקין להתחלה.");
    const s = this.settings;
    need(s.pathway.length > 0, "עוד אין מסלול אבחון. אפשר להגדיר אותו בהגדרות, בחלק \"מסלול ומשפחה חדשה\".");
    const out: Visit[] = [];
    let earliest = from;
    s.pathway.forEach((step, i) => {
      const d = s.defaults.find((x) => x.kind === step.kind);
      const minutes = d?.minutes ?? 60;
      const slot = this.freeSlots(today, earliest, minutes, 1, null, out)[0];
      if (!slot) refuse("לא נמצאה שעה פנויה לכל שלבי המסלול בחודשים הקרובים. אפשר להתחיל מתאריך אחר, או לקבוע את הפגישות אחת אחת.");
      out.push({
        id: "", patient_id: p.id, kind: step.kind, title: "", date: slot.date, start: slot.start, minutes, place: s.places[0] ?? "",
        status: "planned", status_note: "", fee: p.session_fee ?? d?.fee ?? 0, charge_missed: false, series_id: null, note: "", remember: [],
      });
      const next = s.pathway[i + 1];
      earliest = addDays(slot.date, Math.max(1, next?.gap_days ?? 0));
    });
    return out;
  }

  private savePathway(patientId: string, visits: Visit[], packaged: boolean, today: Today): SavedVisits {
    need(visits.length > 0, "אין פגישות לשמור במסלול.");
    need(visits.length <= 12, "יש יותר מדי פגישות במסלול אחד.");
    if (!this.patients.some((x) => x.id === patientId)) refuse("המטופל שנבחר לא נמצא. ייתכן שהכרטיס נמחק.");
    // All of them, or none (the core's one transaction): every check passes before the first
    // visit is written.
    const rows: Visit[] = [];
    for (const input of visits) {
      const v = this.normalVisit(input);
      need(v.id === "" && v.patient_id === patientId, "במסלול יש פגישה שלא שייכת לכרטיס הזה. אפשר לפתוח את המסלול מחדש.");
      need(v.kind !== "block", "במסלול האבחון אין זמנים שמורים, רק פגישות.");
      need(v.date >= today.date, "אחת הפגישות במסלול נקבעה לתאריך שעבר. אפשר לבחור תאריך אחר.");
      v.status = "planned";
      v.series_id = null;
      if (packaged) {
        v.fee = 0;
        v.charge_missed = false;
      }
      this.checkVisit(v);
      rows.push(v);
    }
    let charge: Charge | null = null;
    if (packaged) {
      need(this.settings.package_fee > 0, "עוד אין מחיר לחבילת האבחון. אפשר לקבוע אותו בהגדרות, בחלק \"מסלול ומשפחה חדשה\".");
      const first = rows.map((v) => v.date).sort()[0] ?? today.date;
      charge = { id: "", patient_id: patientId, date: first, amount: this.settings.package_fee, what: cleanLine(this.settings.package_name), voided: false, void_reason: "" };
      // The charge's checks (`validate_charge`), before anything is written.
      need(whole(charge.amount, 1, MAX_AGOROT), "הסכום צריך להיות גדול מאפס, ועד 100,000 ₪.");
      need(charge.what !== "", "צריך לכתוב על מה החיוב.");
      short(charge.what, "על מה החיוב");
    }
    // The waiting family's card is checked before the first write too.
    this.canActivate(patientId);
    const written = rows.map((v): Visit => ({ ...v, id: this.newId("visit") }));
    for (const w of written) {
      this.visits.push(w);
      this.keepVisit(w, "created");
    }
    this.wrote(patientId);
    this.activate(patientId);
    if (charge !== null) this.saveCharge(charge);
    // The overlaps are counted with every visit of the pathway written.
    return this.saved(written);
  }

  private starterTasks(patientId: string, today: Today): TaskView[] {
    const p = this.patients.find((x) => x.id === patientId);
    if (!p) refuse(NO_PATIENT, "not_found");
    need(this.settings.starter_tasks.length > 0, "עוד אין ערכת פתיחה. אפשר להגדיר אותה בהגדרות, בחלק \"מסלול ומשפחה חדשה\".");
    const key = (t: string) => cleanLine(t).toLowerCase();
    const have = new Set(this.tasks.filter((t) => t.patient_id === patientId && t.done_on === null).map((t) => key(t.title)));
    const made: ClinicTask[] = [];
    for (const st of this.settings.starter_tasks) {
      const title = cleanLine(st.title);
      if (title === "" || have.has(key(title))) continue;
      have.add(key(title));
      made.push(this.saveTask({ id: "", patient_id: patientId, kind: st.kind, title, due: addDays(today.date, st.due_days), done_on: null, note: "" }));
    }
    need(made.length > 0, "כל המעקבים של ערכת הפתיחה כבר פתוחים בכרטיס הזה.");
    return made.map((task) => ({ task, patient_name: p.name, overdue: false }));
  }

  // ---------------------------------------------------------------- small things

  private newId(prefix: string): string {
    return `${prefix}-${++this.seq}`;
  }

  /** The day by the core's own clock, which counts in UTC (`iso(today())` in the core). */
  private coreDay(): string {
    return this.clock().toISOString().slice(0, 10);
  }

  /** The income export of `period` ("2026-11" or "2026", D-194), as the core checks and
   *  names it. The file itself is the core's (dv-domain builds it); the preview makes none. */
  exportIncome(period: string): { fileName: string; title: string } {
    const hit = /^(\d{4})(?:-(\d{2}))?$/.exec(period);
    const year = Number(hit?.[1] ?? 0);
    const month = hit?.[2] === undefined ? null : Number(hit[2]);
    need(hit !== null && year >= 2000 && year <= 2200 && (month === null || (month >= 1 && month <= 12)), "צריך לבחור חודש או שנה לייצוא.");
    return { fileName: `הכנסות ${period}.csv`, title: month === null ? `שנת ${year}` : formatMonth(period) };
  }

  /** As if `rows` rows of the vault could not be read (D-178): the front page says so first. */
  unreadable(rows: number) {
    this.unreadableRows = Math.max(0, Math.trunc(rows));
  }

  /** Rows as an older version of the program finds them after a newer one wrote them: put in
   *  as they are, with no check, and any value this version does not know read as "unknown"
   *  (D-178). For tests of what is shown and what is refused; nothing else writes this way. */
  plant(rows: { patients?: Patient[]; visits?: Visit[]; payments?: Payment[]; tasks?: ClinicTask[] }) {
    for (const p of copy(rows.patients ?? [])) {
      this.patients.push({ ...p, status: orUnknown(PATIENT_STATUSES, p.status), contacts: p.contacts.map((c) => ({ ...c, role: orUnknown(CONTACT_ROLES, c.role) })) });
    }
    for (const v of copy(rows.visits ?? [])) {
      const read: Visit = { ...v, kind: orUnknown(VISIT_KINDS, v.kind), status: orUnknown(VISIT_STATUSES, v.status) };
      this.visits.push(read);
      this.keep(read.id, "created", this.visitLines(read), read.patient_id);
      if (read.status !== "planned") this.marked.add(read.id);
    }
    for (const x of copy(rows.payments ?? [])) {
      const read: Payment = { ...x, method: orUnknown(PAY_METHODS, x.method) };
      this.payments.push(read);
      this.keep(read.id, "created", this.paymentLines(read), read.patient_id);
    }
    for (const t of copy(rows.tasks ?? [])) this.tasks.push({ ...t, kind: orUnknown(TASK_KINDS, t.kind) });
  }

  private today(t: Today): Today {
    need(isIsoDate(t.date) && whole(t.minute, 0, 1439), "התאריך של היום לא התקבל כמו שצריך. כדאי לסגור את המסך ולפתוח אותו שוב.");
    return t;
  }

  private patient(id: string): Patient {
    return this.patients.find((p) => p.id === id) ?? refuse("המטופל שנבחר לא נמצא. ייתכן שהכרטיס נמחק.", "not_found");
  }

  private visit(id: string): Visit {
    return this.visits.find((v) => v.id === id) ?? refuse("הפגישה לא נמצאה. ייתכן שכבר נמחקה.", "not_found");
  }

  private note(id: string): SessionNote {
    return this.notes.find((n) => n.id === id) ?? refuse("הסיכום לא נמצא. ייתכן שכבר נמחק.", "not_found");
  }

  private nameOf(id: string | null): string | null {
    return id === null ? null : (this.patients.find((p) => p.id === id)?.name ?? null);
  }

  private view(v: Visit): VisitView {
    return {
      visit: v,
      patient_name: this.nameOf(v.patient_id),
      has_note: this.notes.some((n) => n.visit_id === v.id),
      overlaps: this.visits.some((o) => visitsOverlap(v, o)),
    };
  }

  private keep(recordId: string, action: "created" | "changed" | "voided" | "deleted", lines: string[], patientId: string | null) {
    const rows = this.history.get(recordId) ?? [];
    rows.push({ entry: { at: Math.floor(this.clock().getTime() / 1000), action, lines }, patientId });
    this.history.set(recordId, rows);
  }

  /** Writes a visit's state to its history, and remembers a mark (D-176). */
  private keepVisit(v: Visit, action: "created" | "changed" | "deleted") {
    this.keep(v.id, action, this.visitLines(v), v.patient_id);
    if (v.status !== "planned") this.marked.add(v.id);
  }

  /** The visit ever left "planned": by what it says now, or by a state of it that was saved. */
  private everMarked(v: Visit): boolean {
    return v.status !== "planned" || this.marked.has(v.id);
  }

  // A saved state in her words, a line for each field that says something (as the core's
  // `visit_history_lines`, `payment_history_lines` and `charge_history_lines`).

  private visitLines(v: Visit): string[] {
    return [
      ...field("מטופל", this.nameOf(v.patient_id) ?? ""),
      ...field("סוג", VISIT_KIND_LABEL[v.kind]),
      ...field("כותרת", v.title),
      ...field("תאריך", formatDate(v.date)),
      ...field("שעה", formatTime(v.start)),
      `משך: ${v.minutes} דקות`,
      ...field("מקום", v.place),
      ...field("מצב", VISIT_STATUS_LABEL[v.status]),
      ...field("מה קרה", v.status_note),
      ...(ownVisit(v) ? [] : [v.fee === 0 ? "מחיר: ללא חיוב" : `מחיר: ${plainMoney(v.fee)} ₪`]),
      ...(v.charge_missed ? ["חיוב גם אם לא התקיימה: כן"] : []),
      ...(v.series_id !== null ? ["פגישה חוזרת: כן"] : []),
      ...field("הערה", v.note),
      ...v.remember.map((i) => `לא לשכוח: ${i.done ? "✓" : "○"} ${i.text}`),
    ];
  }

  private paymentLines(p: Payment): string[] {
    return [
      ...field("מטופל", this.nameOf(p.patient_id) ?? ""),
      ...field("תאריך", formatDate(p.date)),
      `סכום: ${plainMoney(p.amount)} ₪`,
      ...field("אמצעי תשלום", PAY_METHOD_LABEL[p.method]),
      ...field("מספר קבלה", p.receipt),
      ...field("מי שילם", p.payer),
      ...field("הערה", p.note),
      ...voidedLines(p.voided, p.void_reason),
    ];
  }

  private chargeLines(c: Charge): string[] {
    return [
      ...field("מטופל", this.nameOf(c.patient_id) ?? ""),
      ...field("תאריך", formatDate(c.date)),
      `סכום: ${plainMoney(c.amount)} ₪`,
      ...field("על מה", c.what),
      ...voidedLines(c.voided, c.void_reason),
    ];
  }

  /** A saved state of a session note (as the core's `note_history_lines`): the formal record,
   *  line by line, and one line that says only whether the personal note was written or
   *  changed since the state before. Its words are never here: it is hers alone (D-171). */
  private noteLines(n: SessionNote, previous: SessionNote | null): string[] {
    const record = n.record.split("\n").map(cleanLine).filter((line) => line !== "");
    const personal = n.personal.trim();
    const before = previous === null ? null : previous.personal.trim();
    const aboutPersonal = before === null ? (personal === "" ? "אין" : "נכתבה")
      : before === personal ? (personal === "" ? "אין" : "בלי שינוי")
      : personal === "" ? "נמחקה"
      : before === "" ? "נכתבה" : "השתנתה";
    return [
      ...field("מטופל", this.nameOf(n.patient_id) ?? ""),
      ...field("תאריך", formatDate(n.date)),
      ...(record.length === 0 ? ["רשומה: ריקה"] : ["רשומה:", ...record]),
      `תרשומת אישית: ${aboutPersonal}`,
      ...(n.recordings.length > 0 ? [`הקלטות לעצמך: ${n.recordings.length}`] : []),
      ...(n.case_input_id !== null ? ["הועבר לחומרי התיק: כן"] : []),
    ];
  }

  /** The day a record of the patient was last written, by the clock (not by the date the
   *  record carries): a note corrected years later was active then (D-174). */
  private wrote(patientId: string | null) {
    if (patientId !== null) this.lastSaved.set(patientId, this.coreDay());
  }

  // ---------------------------------------------------------------- money

  /** What the patient was charged, by date: the sessions that cost, then the other charges. */
  private chargesOf(patientId: string): { date: string; amount: number }[] {
    const sessions = this.visits.filter((v) => v.patient_id === patientId && visitCharge(v) > 0).map((v) => ({ date: v.date, amount: visitCharge(v) }));
    const others = this.charges.filter((c) => c.patient_id === patientId && !c.voided).map((c) => ({ date: c.date, amount: c.amount }));
    return [...sessions, ...others].sort((a, b) => a.date.localeCompare(b.date));
  }

  private balance(patientId: string): Balance {
    const charged = this.chargesOf(patientId).reduce((sum, c) => sum + c.amount, 0);
    const paid = this.payments.filter((p) => p.patient_id === patientId && !p.voided).reduce((sum, p) => sum + p.amount, 0);
    return { charged, paid, due: charged - paid };
  }

  /** Paying the oldest charges first: the date of the oldest one still not covered. */
  private owedSince(patientId: string): string | null {
    let left = this.balance(patientId).paid;
    for (const c of this.chargesOf(patientId)) {
      if (left >= c.amount) left -= c.amount;
      else return c.date;
    }
    return null;
  }

  // ---------------------------------------------------------------- cases

  private caseOf(p: Patient): CaseInfo | null {
    return p.case_id === null ? null : (this.host.cases().find((c) => c.id === p.case_id) ?? null);
  }

  private caseLink(p: Patient): CaseLink | null {
    const c = this.caseOf(p);
    return c ? { id: c.id, code: c.code, approved: c.approved, total: c.total, in_trash: c.inTrash } : null;
  }

  /** The report's sections as the alerts and the process read them; `null`: no case. */
  private reportOf(p: Patient): { approved: number; total: number } | null {
    const c = this.caseOf(p);
    return c === null ? null : { approved: c.approved, total: c.total };
  }

  /** Every section approved; a report with no sections at all is not a finished one. */
  private reportFinished(p: Patient): boolean {
    const c = this.reportOf(p);
    return c !== null && c.total > 0 && c.approved >= c.total;
  }

  private caseChoices(): CaseChoice[] {
    const linked = new Set(this.patients.map((p) => p.case_id));
    return this.host.cases()
      .filter((c) => !c.inTrash && !linked.has(c.id))
      .map((c) => ({ id: c.id, label: c.childName ? `${c.childName} · ${c.code}` : c.code }));
  }

  private linkCase(patientId: string, caseId: string | null): Patient {
    const p = this.patient(patientId);
    if (caseId !== null) {
      const c = this.host.cases().find((x) => x.id === caseId) ?? refuse("התיק לא נמצא.", "not_found");
      need(!c.inTrash, "התיק הזה נמצא בסל המחזור. אפשר לשחזר אותו ברשימת התיקים, ואז לקשר.");
      need(!this.patients.some((x) => x.id !== p.id && x.case_id === caseId), "התיק הזה כבר מקושר למטופל אחר.");
      // The names of the card become names the case's filter knows, before the link (D-177).
      this.syncIdentities(caseId, p);
    }
    if (p.case_id !== caseId) {
      patientKnown(p);
      p.case_id = caseId;
    }
    return p;
  }

  /** The people of a card its case must know (the core's `card_people`): the child, then
   *  every contact that has a name, each once. A guardian is not always a relative, and a
   *  kind of contact this version does not know says nothing: both are the neutral role. */
  private cardPeople(p: Patient): { role: Role; name: string }[] {
    const roles: Record<Contact["role"], Role> = { mother: "mother", father: "father", guardian: "other", other: "other", unknown: "other" };
    const people: { role: Role; name: string }[] = [{ role: "child", name: cleanLine(p.name) }];
    for (const c of p.contacts) {
      const name = cleanLine(c.name);
      if (!people.some((known) => known.name === name)) people.push({ role: roles[c.role], name });
    }
    return people.filter((person) => person.name !== "");
  }

  /** Makes every name of a card a name its case knows (D-177; the core's
   *  `clinic_sync_identities`). Only adds: the child's name joins the case's child as another
   *  way the name is written (a new child when the case has none), and a contact the case
   *  does not know, by value or by alias, is added with their role. A name that left the card
   *  stays in the case. A failure of the host stops the caller: nothing is linked, saved or sent. */
  private syncIdentities(caseId: string, p: Patient) {
    const list = this.host.identities(caseId);
    const knows = (name: string) => list.some((known) => [known.value, ...known.aliases].some((written) => cleanLine(written) === name));
    let changed = false;
    for (const { role, name } of this.cardPeople(p)) {
      if (knows(name)) continue;
      changed = true;
      const child = list.find((known) => known.role === "child");
      if (child && role === "child") child.aliases.push(name);
      else list.push({ id: null, role, value: name, aliases: [] });
    }
    if (changed) this.host.setIdentities(caseId, list);
  }

  private createCase(patientId: string, today: Today): CaseLink {
    const p = this.patient(patientId);
    need(this.caseOf(p) === null, "למטופל הזה כבר יש תיק אבחון.");
    const [y = "", m = "", d = ""] = today.date.split("-");
    const taken = new Set(this.host.cases().map((c) => c.code));
    const base = `תיק-${y}-${m}${d}`;
    let code = base;
    for (let n = 2; taken.has(code); n++) code = `${base}-${n}`;
    // Every named contact is a person of the case (not only the parents), so a record sent to
    // the case later has their names masked by its filter.
    const caseId = this.host.createCase(
      { code, age: ageOn(p.birth_date, today.date), child_gender: p.gender, current_section: null, retention_until: null, consent: null },
      this.cardPeople(p).map(({ role, name }): IdentityInput => ({ id: null, role, value: name, aliases: [] })),
    );
    // As in the core: the case is there before the card is written with its link.
    patientKnown(p);
    p.case_id = caseId;
    return this.caseLink(p) ?? refuse("התיק לא נפתח. אפשר לנסות שוב.");
  }

  private noteToCase(id: string): SessionNote {
    const n = this.note(id);
    const p = this.patient(n.patient_id);
    const c = this.caseOf(p);
    need(c !== null, "למטופל עוד אין תיק אבחון. אפשר לפתוח תיק מהכרטיס, ואז להעביר את הסיכום.");
    need(!c.inTrash, "תיק האבחון נמצא בסל המחזור. אפשר לשחזר אותו ברשימת התיקים, ואז להעביר.");
    need(n.record.trim() !== "", "הרשומה ריקה, ואין מה להעביר לתיק. התרשומת האישית נשארת רק אצלך.");
    // Before the record goes: every name of the card is a name the case's filter knows (D-177).
    this.syncIdentities(c.id, p);
    const v = n.visit_id === null ? null : (this.visits.find((x) => x.id === n.visit_id) ?? null);
    const kind: InputKind = v?.kind === "intake" ? "intake" : v?.kind === "observation" ? "observation" : "session_note";
    const title = `סיכום ${v ? VISIT_KIND_LABEL[v.kind] : "שיחה"} · ${formatDate(n.date)}`;
    // Only the formal record: the personal note never leaves the clinic. The first send
    // writes the material and the note that points to it together (in the core, one
    // transaction: a failure leaves neither); a later one replaces the material's text.
    if (n.case_input_id === null || !this.host.updateInput(c.id, n.case_input_id, title, n.record)) {
      n.case_input_id = this.host.addInput(c.id, kind, title, n.record);
      this.keep(n.id, "changed", this.noteLines(n, n), n.patient_id);
      this.wrote(p.id);
    }
    return n;
  }

  // ---------------------------------------------------------------- patients

  private savePatient(input: Patient): Patient {
    // The card as it is stored (the core's `normalize_patient`). A contact with nothing in it
    // is dropped, not refused: an empty row of the form.
    const name = cleanLine(input.name);
    const birth = blankToNull(input.birth_date);
    const contacts = input.contacts
      .map((c): Contact => ({ role: c.role, name: cleanLine(c.name), phone: cleanLine(c.phone), email: c.email.trim() }))
      .filter((c) => c.name !== "" || c.phone !== "" || c.email !== "");
    const framework = cleanLine(input.framework);
    const referredBy = cleanLine(input.referred_by);
    const old = input.id === "" ? null : this.patient(input.id);
    // The link to a case and the day the card was opened are never the window's; a new card
    // that names no day was opened today, by the core's clock.
    const created = old ? old.created_on : (input.created_on.trim() || this.coreDay());
    const waitingSince = blankToNull(input.waiting_since);
    // The checks, in the core's order (`validate_patient_on`).
    need(name !== "", "צריך לכתוב את שם הילד או הילדה.");
    atMost(name, NAME, "שם");
    known(PATIENT_STATUSES.includes(input.status), "מצב הכרטיס");
    if (birth !== null) {
      need(birthDate(birth), `צריך לכתוב תאריך לידה תקין, משנת ${BIRTH_YEAR_MIN} ואילך.`);
      // The request carries no "today": the day is the core's own clock (UTC). Her local day
      // can be one ahead of it, so a birth date is refused only after the core's tomorrow.
      need(birth <= addDays(this.coreDay(), 1), "תאריך הלידה עוד לא הגיע. כדאי לבדוק את השנה שנכתבה.");
    }
    need(input.gender === null || input.gender === "male" || input.gender === "female", "המין שנבחר לא מוכר.");
    need(contacts.length <= MAX_CONTACTS, `אפשר לשמור עד ${MAX_CONTACTS} אנשי קשר למטופל.`);
    for (const c of contacts) {
      known(CONTACT_ROLES.includes(c.role), "הקשר לילד");
      atMost(c.name, NAME, "שם איש הקשר");
      short(c.phone, "טלפון");
      short(c.email, "מייל");
    }
    short(framework, "מסגרת חינוכית");
    short(referredBy, "גורם מפנה");
    long(input.reason.trim(), "סיבת הפנייה");
    long(input.notes.trim(), "הערות");
    if (waitingSince !== null) realDate(waitingSince, "ממתינים מאז");
    need(input.session_fee === null || whole(input.session_fee, 0, MAX_AGOROT), "מחיר הפגישה צריך להיות בין 0 ל-100,000 ₪.");
    realDate(created, "תאריך פתיחת הכרטיס");
    const saved: Patient = {
      id: old ? old.id : this.newId("patient"),
      name,
      birth_date: birth,
      gender: input.gender,
      contacts,
      framework,
      referred_by: referredBy,
      reason: input.reason.trim(),
      notes: input.notes.trim(),
      status: input.status,
      // A family on the list has a day it joined: the day the card was opened, unless she said otherwise.
      waiting_since: waitingSince ?? (input.status === "waiting" ? created : null),
      urgent: input.urgent === true,
      session_fee: input.session_fee,
      // Saving the card never touches the link to the case, and never moves the day it was opened.
      case_id: old ? old.case_id : null,
      created_on: created,
    };
    // The names of a card that has a case (in the recycle bin too) are names its filter must
    // know, before the card itself is written (D-177).
    if (saved.case_id !== null && this.host.cases().some((c) => c.id === saved.case_id)) this.syncIdentities(saved.case_id, saved);
    if (old) Object.assign(old, saved);
    else this.patients.push(saved);
    return old ?? saved;
  }

  /** The day from which a card that holds a record may be erased (D-174, the core's
   *  `can_erase`): seven years after the last activity in it (the latest date anything in
   *  the card carries, and the day one of its records was last written), and not before the
   *  child is 25 when the birth date is known, whichever is later. */
  private keptUntil(p: Patient, today: string): string {
    const dates = [
      p.created_on,
      this.lastSaved.get(p.id) ?? "",
      ...this.visits.filter((v) => v.patient_id === p.id).map((v) => v.date),
      ...[...this.notes, ...this.payments, ...this.charges].filter((r) => r.patient_id === p.id).map((r) => r.date),
      ...this.tasks.filter((t) => t.patient_id === p.id).flatMap((t) => [t.due ?? "", t.done_on ?? ""]),
    ].filter(clinicDate).sort();
    // A card whose dates cannot be read is kept as if it changed today.
    const afterLast = addYears(dates[dates.length - 1] ?? today, KEEP_YEARS_AFTER_LAST_CHANGE);
    const grown = p.birth_date !== null && birthDate(p.birth_date) ? addYears(p.birth_date, KEEP_UNTIL_AGE) : afterLast;
    return grown > afterLast ? grown : afterLast;
  }

  /** Something happened in the card (the core's `card_happened`, D-174 and D-176): a visit
   *  that ever left "planned" (a cancellation without a charge too: it is part of the
   *  diary), a session note, a charge or a payment (a voided one too). */
  private happened(id: string): boolean {
    return this.visits.some((v) => v.patient_id === id && this.everMarked(v))
      || [...this.notes, ...this.payments, ...this.charges].some((r) => r.patient_id === id);
  }

  private erasedMonth(date: string): ErasedMonth | null {
    if (!clinicDate(date)) return null;
    const month = monthOf(date);
    let sum = this.tally.get(month);
    if (!sum) {
      sum = { held: [], cancelled: 0, no_show: 0, charged: 0, received: [], missing_receipts: 0 };
      this.tally.set(month, sum);
    }
    return sum;
  }

  /** Folds a card that is about to be erased into the anonymous tallies (D-179; the core's
   *  `fold_erased` and `fold_erased_card`): exactly what the yearly summary and the diary
   *  count of its records, by month, and that a card was opened. Nothing else is kept. */
  private fold(p: Patient) {
    for (const v of this.visits.filter((x) => x.patient_id === p.id)) {
      // A planned visit is in no summary: it leaves no month behind.
      if (v.status === "planned" || v.status === "unknown") continue;
      const sum = this.erasedMonth(v.date);
      if (!sum) continue;
      if (v.status === "held") {
        const entry = sum.held.find((k) => k.kind === v.kind);
        if (entry) entry.count += 1;
        else sum.held.push({ kind: v.kind, count: 1 });
      } else if (v.status === "cancelled") sum.cancelled += 1;
      else sum.no_show += 1;
      sum.charged += visitCharge(v);
    }
    for (const c of this.charges.filter((x) => x.patient_id === p.id && !x.voided)) {
      const sum = this.erasedMonth(c.date);
      if (sum) sum.charged += c.amount;
    }
    for (const x of this.payments.filter((y) => y.patient_id === p.id && !y.voided)) {
      const sum = this.erasedMonth(x.date);
      if (!sum) continue;
      const entry = sum.received.find((m) => m.method === x.method);
      if (entry) entry.amount += x.amount;
      else sum.received.push({ method: x.method, amount: x.amount });
      if (x.receipt.trim() === "") sum.missing_receipts += 1;
    }
    if (clinicDate(p.created_on)) {
      const month = monthOf(p.created_on);
      this.erasedCards.set(month, (this.erasedCards.get(month) ?? 0) + 1);
    }
  }

  private erasePatient(id: string, typed: string) {
    const p = this.patient(id);
    need(cleanLine(typed) === cleanLine(p.name), "השם שהוקלד לא זהה לשם שבכרטיס. כדי למחוק צריך להקליד את השם בדיוק כמו שהוא כתוב בכרטיס.");
    // The request carries no "today": the retention period is counted by the core's own
    // clock, read once (`clinic_erase_on`).
    const today = this.coreDay();
    const happened = this.happened(id);
    if (happened) {
      const from = this.keptUntil(p, today);
      need(today >= from, `בכרטיס הזה יש פגישות, סיכומים או רישומי כספים, ורשומה כזו צריך לשמור. אפשר לסגור את הכרטיס במקום למחוק אותו. מחיקה תהיה אפשרית החל מיום ${formatDate(from)}.`);
      // What the card counted for in the summaries stays, with nothing that says whose it
      // was (D-179). A card where nothing happened was a mistake and leaves nothing.
      this.fold(p);
    }
    const gone = new Set([...this.visits, ...this.notes, ...this.payments, ...this.charges].filter((r) => r.patient_id === id).map((r) => r.id));
    const kept = new Set([...this.visits, ...this.notes, ...this.payments, ...this.charges].map((r) => r.id));
    // Only history that belongs to what is erased (D-177): every state of this card's
    // records, and the states this card's removed records left. A state of a record that
    // belongs to another card is never touched.
    for (const [record, rows] of [...this.history]) {
      if (gone.has(record)) {
        this.history.delete(record);
      } else if (!kept.has(record)) {
        const left = rows.filter((row) => row.patientId !== id);
        if (left.length === 0) this.history.delete(record);
        else if (left.length !== rows.length) this.history.set(record, left);
      }
    }
    for (const record of gone) this.marked.delete(record);
    // The recordings go with the patient (D-208).
    for (const n of this.notes) if (n.patient_id === id) for (const rec of n.recordings) this.sounds.delete(rec.id);
    this.visits = this.visits.filter((v) => v.patient_id !== id);
    this.notes = this.notes.filter((n) => n.patient_id !== id);
    this.payments = this.payments.filter((x) => x.patient_id !== id);
    this.charges = this.charges.filter((x) => x.patient_id !== id);
    this.tasks = this.tasks.filter((t) => t.patient_id !== id);
    this.lastSaved.delete(id);
    // The case, if there is one, is not the clinic's to erase.
    this.patients = this.patients.filter((x) => x.id !== id);
  }

  /** The next planned visit from today on (one earlier today is still "next" until it is marked). */
  private nextVisit(patientId: string, today: Today): Visit | undefined {
    return this.visits.filter((v) => v.patient_id === patientId && v.status === "planned" && v.date >= today.date).sort(byDateTime)[0];
  }

  private rows(today: Today): PatientRow[] {
    return [...this.patients].sort((a, b) => a.name.localeCompare(b.name, "he") || a.id.localeCompare(b.id)).map((p): PatientRow => {
      const next = this.nextVisit(p.id, today);
      const held = this.visits.filter((v) => v.patient_id === p.id && v.status === "held").map((v) => v.date).sort();
      return {
        patient: p,
        age: ageOn(p.birth_date, today.date),
        next_visit: next ? { date: next.date, start: next.start, kind: next.kind } : null,
        last_visit: held[held.length - 1] ?? null,
        due: this.balance(p.id).due,
        open_tasks: this.tasks.filter((t) => t.patient_id === p.id && t.done_on === null).length,
      };
    });
  }

  private card(p: Patient, today: Today): PatientCard {
    const newest = (a: { date: string }, b: { date: string }) => b.date.localeCompare(a.date);
    return {
      patient: p,
      age: ageOn(p.birth_date, today.date),
      visits: this.visits.filter((v) => v.patient_id === p.id).sort((a, b) => byDateTime(b, a)).map((v) => this.view(v)),
      notes: this.notes.filter((n) => n.patient_id === p.id).sort(newest),
      payments: this.payments.filter((x) => x.patient_id === p.id).sort(newest),
      charges: this.charges.filter((x) => x.patient_id === p.id).sort(newest),
      tasks: this.orderedTasks().filter((t) => t.patient_id === p.id),
      balance: this.balance(p.id),
      process: this.process(p),
      case: this.caseLink(p),
      alerts: this.alerts(today).filter((a) => a.patient_id === p.id),
    };
  }

  /** The assessment, step by step (as the core's `process`): each step done, under way or
   *  still to do. When none is under way, the first one still to do is what comes next. */
  private process(p: Patient): ProcessStep[] {
    const mine = this.visits.filter((v) => v.patient_id === p.id);
    const dates = (kind: VisitKind, status: VisitStatus) => mine.filter((v) => v.kind === kind && v.status === status).map((v) => v.date).sort();
    const step = (key: string, label: string, state: ProcessStep["state"], detail: string): ProcessStep => ({ key, label, state, detail });
    // A step that is one meeting: done once held, under way once planned.
    const meeting = (key: string, label: string, kind: VisitKind): ProcessStep => {
      const held = dates(kind, "held").pop();
      if (held) return step(key, label, "done", `התקיימה ביום ${formatDate(held)}`);
      const planned = dates(kind, "planned")[0];
      return planned ? step(key, label, "now", `נקבעה ליום ${formatDate(planned)}`) : step(key, label, "todo", "עוד לא נקבעה");
    };
    const held = dates("assessment", "held").length;
    const planned = dates("assessment", "planned");
    const sessions = held === 0 && planned.length === 0 ? step("sessions", "מפגשי אבחון", "todo", "עוד לא נקבעו מפגשים")
      : held === 0 ? step("sessions", "מפגשי אבחון", "now", `המפגש הראשון נקבע ליום ${formatDate(planned[0] ?? "")}`)
      : planned.length === 0 ? step("sessions", "מפגשי אבחון", "done", held === 1 ? "מפגש אחד התקיים" : `${held} מפגשים התקיימו`)
      : step("sessions", "מפגשי אבחון", "now", `${held} מתוך ${held + planned.length} התקיימו`);
    const steps = [meeting("intake", "פגישת היכרות", "intake"), sessions];
    if (mine.some((v) => v.kind === "observation" && v.status !== "cancelled")) {
      const observation = meeting("observation", "תצפית", "observation");
      // It exists and was neither held nor is planned: the family did not come.
      steps.push(observation.state === "todo" ? { ...observation, detail: "לא התקיימה" } : observation);
    }
    const c = this.reportOf(p);
    steps.push(step("report", "כתיבת הדוח",
      this.reportFinished(p) ? "done" : sessions.state === "done" || (c !== null && c.approved > 0) ? "now" : "todo",
      c === null ? "עוד לא נפתח תיק לדוח" : `הדוח: ${c.approved} מתוך ${c.total} סעיפים`));
    steps.push(meeting("feedback", "שיחת משוב", "feedback"));
    const { charged, paid, due } = this.balance(p.id);
    steps.push(charged === 0 && paid === 0 ? step("payment", "תשלום", "todo", "עוד לא נרשם חיוב")
      : due > 0 ? step("payment", "תשלום", "now", `נותרו ${plainMoney(due)} ₪ לתשלום`)
      : due < 0 ? step("payment", "תשלום", "done", `שולם, ויש יתרת זכות של ${plainMoney(-due)} ₪`)
      : step("payment", "תשלום", "done", "שולם במלואו"));
    if (!steps.some((x) => x.state === "now")) {
      const next = steps.find((x) => x.state === "todo");
      if (next) next.state = "now";
    }
    return steps;
  }

  // ---------------------------------------------------------------- visits

  /** The visit as it is stored (the core's `normalize_visit`). */
  private normalVisit(input: Visit): Visit {
    return {
      ...input, patient_id: blankToNull(input.patient_id), title: cleanLine(input.title), date: input.date.trim(), place: cleanLine(input.place),
      status_note: cleanLine(input.status_note), series_id: blankToNull(input.series_id), note: input.note.trim(),
      remember: (input.remember ?? []).map((i) => ({ text: cleanLine(i.text), done: i.done })).filter((i) => i.text !== ""),
    };
  }

  /** The visit by itself, checked in the core's order (`validate_visit`); that the patient
   *  exists is asked right after it, as the service does. */
  private checkVisit(v: Visit) {
    visitKnown(v);
    need(v.kind !== "block" || v.patient_id === null, "בזמן שמור אין מטופל. כדי לקבוע פגישה עם מטופל צריך לבחור סוג פגישה אחר.");
    if (ownVisit(v)) {
      need(v.fee === 0, v.kind === "supervision" ? "להדרכה בלי מטופל אין מחיר. כדי לחייב, בוחרים את הילד שעליו ההדרכה." : "לזמן שמור אין מחיר.");
    } else {
      need(v.patient_id !== null, "צריך לבחור מטופל לפגישה.");
    }
    realDate(v.date, "תאריך");
    need(whole(v.minutes, 5, 720), "משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
    need(whole(v.start, 0, 1439) && v.start + v.minutes <= 1440, "הפגישה צריכה להסתיים עד חצות. אפשר לקצר אותה או להקדים את שעת ההתחלה.");
    need(whole(v.fee, 0, MAX_AGOROT), "מחיר הפגישה צריך להיות בין 0 ל-100,000 ₪.");
    short(v.title, "כותרת");
    short(v.place, "מקום");
    short(v.status_note, "מה קרה");
    long(v.note, "הערה");
    need(v.remember.length <= 20, "אפשר לרשום עד 20 דברים שלא לשכוח לפגישה אחת.");
    v.remember.forEach((i) => short(i.text, "לא לשכוח"));
    need(v.patient_id === null || this.patients.some((p) => p.id === v.patient_id), NO_PATIENT);
  }

  private saved(written: Visit[], leftAlone = 0): SavedVisits {
    const visits = [...written].sort(byDateTime).map((v) => this.view(v));
    return { visits, overlapping: visits.filter((v) => v.overlaps).length, left_alone: leftAlone };
  }

  /** Whether `activate` would refuse the card (a value of a later version, D-178): asked
   *  before anything is written, as the core does. */
  private canActivate(patientId: string | null) {
    const patient = patientId === null ? null : this.patient(patientId);
    if (patient?.status === "waiting") patientKnown(patient);
  }

  /** A family on the waiting list that gets a visit is in the process now. */
  private activate(patientId: string | null) {
    const patient = patientId === null ? null : this.patient(patientId);
    if (patient?.status !== "waiting") return;
    patientKnown(patient);
    patient.status = "active";
  }

  private saveVisit(input: Visit, repeat: Repeat | null, scope: Scope): SavedVisits {
    const v = this.normalVisit(input);
    need(scope === "one" || scope === "following", "לא ברור על אילו פגישות השינוי חל.");
    if (v.id === "") {
      // A new visit is always planned, whatever was sent, and joins no series by itself:
      // what happened in it is marked afterwards.
      v.status = "planned";
      v.series_id = null;
      this.checkVisit(v);
      let dates = [v.date];
      let series: string | null = null;
      if (repeat !== null) {
        need(whole(repeat.every_weeks, 1, 8), "פגישה חוזרת יכולה לחזור כל שבוע ועד כל 8 שבועות.");
        need(whole(repeat.count, 2, 52), "סדרה של פגישות חוזרות היא בין 2 ל-52 פגישות.");
        dates = Array.from({ length: repeat.count }, (_, i) => addDays(v.date, 7 * repeat.every_weeks * i));
        need(dates.every(clinicDate), "הסדרה נמשכת רחוק מדי. כדאי לקצר אותה.");
        series = this.newId("series");
      }
      this.canActivate(v.patient_id);
      // What not to forget belongs to the first visit only (D-207).
      const written = dates.map((date, i): Visit => ({ ...v, id: this.newId("visit"), date, series_id: series, remember: i === 0 ? v.remember : [] }));
      for (const w of written) {
        this.visits.push(w);
        this.keepVisit(w, "created");
      }
      this.wrote(v.patient_id);
      this.activate(v.patient_id);
      return this.saved(written);
    }
    need(repeat === null, "אפשר לקבוע סדרה חוזרת רק כשקובעים פגישה חדשה.");
    const old = this.visit(v.id);
    // Which series a visit belongs to is set when the series is made.
    v.series_id = old.series_id;
    this.checkVisit(v);
    // A record never moves to another card (D-177): its history, its charge and the card's
    // balance stay where they were written. A block with no patient may still be given one.
    if (old.patient_id !== null && v.patient_id !== old.patient_id) {
      need(v.patient_id !== null, "אי אפשר להפוך פגישה של מטופל לזמן שמור. אפשר למחוק אותה (או לסמן אותה כמבוטלת, אם היא כבר סומנה) ולשמור זמן חדש ביומן.");
      refuse("אי אפשר להעביר פגישה למטופל אחר. אם היא נקבעה בכרטיס הלא נכון, אפשר למחוק אותה (או לסמן אותה כמבוטלת, אם היא כבר סומנה) ולקבוע פגישה חדשה בכרטיס הנכון.");
    }
    const noted = new Set(this.notes.map((n) => n.visit_id));
    // Taking a mark back is the correction of a wrong click only (D-176; `validate_unmark`):
    // the visit has no note, and what the family paid is not more than what is left charged
    // without it.
    if (old.status !== "planned" && v.status === "planned") {
      need(!noted.has(old.id), "לפגישה הזו יש סיכום, ולכן היא לא יכולה לחזור להיות \"מתוכננת\". אם היא לא התקיימה, אפשר לסמן אותה כמבוטלת ולכתוב מה קרה.");
      const charge = visitCharge(old);
      if (charge > 0 && old.patient_id !== null) {
        const { charged, paid } = this.balance(old.patient_id);
        need(paid <= charged - charge, "על הפגישה הזו כבר נרשם תשלום, ולכן היא לא יכולה לחזור להיות \"מתוכננת\". אם היא סומנה בטעות, אפשר לסמן אותה כמבוטלת בלי חיוב ולכתוב מה קרה, או לבטל קודם את התקבול.");
      }
    }
    // What is written: nothing when the save changes nothing, and the history gets no line.
    const changed: { visit: Visit; next: Visit }[] = [];
    if (!same(v, old)) changed.push({ visit: old, next: v });
    let leftAlone = 0;
    if (scope === "following" && old.series_id !== null) {
      const moved = daysBetween(old.date, v.date);
      const later = this.visits
        .filter((o) => o.series_id === old.series_id && o.id !== old.id && (o.date > old.date || (o.date === old.date && o.start > old.start)))
        .sort(byDateTime);
      for (const o of later) {
        // A later visit that has a note, or is no longer planned, is never moved or altered:
        // the note would stay on the old date, and a visit that happened is a line of the
        // diary. The reply says how many there were (D-176).
        if (o.status !== "planned" || noted.has(o.id)) {
          leftAlone += 1;
          continue;
        }
        // What a series shares; the status and the patient of each visit stay its own.
        const next: Visit = { ...o, kind: v.kind, title: v.title, start: v.start, minutes: v.minutes, place: v.place, fee: v.fee, note: v.note, date: addDays(o.date, moved) };
        need(clinicDate(next.date), "אחת הפגישות הבאות בסדרה יוצאת מטווח התאריכים האפשרי.");
        this.checkVisit(next);
        if (!same(next, o)) changed.push({ visit: o, next });
      }
    }
    // All of them, or none: every check passed before the first one is written.
    this.canActivate(v.patient_id);
    for (const { visit, next } of changed) {
      Object.assign(visit, next);
      this.keepVisit(visit, "changed");
      this.wrote(visit.patient_id);
    }
    this.activate(v.patient_id);
    return this.saved(changed.length === 0 ? [old] : changed.map((c) => c.visit), leftAlone);
  }

  private deleteVisit(id: string, scope: Scope) {
    const v = this.visit(id);
    // Only a visit that is planned, and was never anything else, can be removed (D-176;
    // `validate_delete`): one that was ever held, cancelled or missed stays in the diary.
    need(v.status === "planned", "אפשר למחוק רק פגישה שעדיין מתוכננת. פגישה שכבר סומנה אפשר לסמן כמבוטלת.");
    need(!this.marked.has(v.id), "הפגישה הזו כבר סומנה פעם (התקיימה, בוטלה או לא הגיעו), ולכן היא נשארת ביומן. אם היא לא תתקיים, אפשר לסמן אותה כמבוטלת.");
    const noted = new Set(this.notes.map((n) => n.visit_id));
    need(!noted.has(v.id), "לפגישה הזו יש סיכום, ולכן אי אפשר למחוק אותה. אפשר לסמן אותה כמבוטלת.");
    const gone = [v];
    if (scope === "following" && v.series_id !== null) {
      // The later ones of the series that are still planned; one that already has a record
      // written for it, or that was ever marked, stays.
      gone.push(...this.visits.filter((o) => o.series_id === v.series_id && o.id !== v.id && o.status === "planned"
        && (o.date > v.date || (o.date === v.date && o.start > v.start)) && !noted.has(o.id) && !this.marked.has(o.id)));
    }
    const ids = new Set(gone.map((g) => g.id));
    for (const g of gone) this.keep(g.id, "deleted", this.visitLines(g), g.patient_id);
    this.visits = this.visits.filter((x) => !ids.has(x.id));
  }

  // ---------------------------------------------------------------- notes, payments, charges, tasks

  private saveNote(input: SessionNote): SessionNote {
    need(input.patient_id.trim() !== "", "צריך לבחור מטופל.");
    const p = this.patient(input.patient_id);
    const note = { ...input, visit_id: blankToNull(input.visit_id), date: input.date.trim(), record: input.record.trim(), personal: input.personal.trim() };
    realDate(note.date, "תאריך");
    const old0 = note.id === "" ? null : this.note(note.id);
    need(note.record !== "" || note.personal !== "" || (old0?.recordings.length ?? 0) > 0, "הסיכום ריק. צריך לכתוב משהו ברשומה או בתרשומת האישית.");
    atMost(note.record, RECORD, "רשומה");
    atMost(note.personal, RECORD, "תרשומת אישית");
    if (note.visit_id !== null) {
      const v = this.visit(note.visit_id);
      need(v.patient_id === p.id, "הפגישה שנבחרה שייכת למטופל אחר.");
      // One note for a visit: the one that exists is opened and edited.
      need(!this.notes.some((n) => n.visit_id === v.id && n.id !== note.id), "לפגישה הזו כבר יש סיכום. אפשר לפתוח אותו ולערוך אותו.");
    }
    const old = note.id === "" ? null : this.note(note.id);
    need(old === null || old.patient_id === p.id, "אי אפשר להעביר סיכום למטופל אחר. אפשר לכתוב סיכום חדש בכרטיס הנכון.");
    const saved: SessionNote = {
      id: old ? old.id : this.newId("note"),
      patient_id: p.id,
      visit_id: note.visit_id,
      date: note.date,
      record: note.record,
      personal: note.personal,
      // Set only by "להעביר לחומרי התיק".
      case_input_id: old ? old.case_input_id : null,
      // Only their own requests change the recordings (D-208).
      recordings: old ? old.recordings : [],
    };
    const before = old ? { ...old } : null;
    // Saved again with nothing changed: nothing is written, and the history gets no line.
    if (before !== null && JSON.stringify(before) === JSON.stringify(saved)) return old ?? saved;
    if (old) Object.assign(old, saved);
    else this.notes.push(saved);
    // The clinical record: every saved state of it is kept (D-172).
    this.keep(saved.id, old ? "changed" : "created", this.noteLines(saved, before), p.id);
    this.wrote(p.id);
    return old ?? saved;
  }

  /** Keeps a recording with a note (D-208), as the core's `clinic_save_recording`. */
  private saveRecording(r: Extract<ClinicRequest, { op: "save_recording" }>): SessionNote {
    const main = (r.mime.split(";")[0] ?? "").trim().toLowerCase();
    const mime = ({ "audio/x-wav": "audio/wav", "audio/wave": "audio/wav", "audio/mp3": "audio/mpeg", "audio/m4a": "audio/x-m4a" } as Record<string, string>)[main] ?? main;
    need(["audio/webm", "audio/ogg", "audio/mp4", "audio/mpeg", "audio/wav", "audio/x-m4a", "audio/aac"].includes(mime),
      "אפשר לשמור רק קובץ שמע: הקלטה מהתוכנה, או קובץ מהטלפון (m4a,‏ mp3,‏ wav,‏ ogg,‏ webm).");
    need(r.data !== "", "ההקלטה ריקה.");
    need(r.data.length % 4 === 0 && /^[A-Za-z0-9+/]+={0,2}$/.test(r.data), "ההקלטה לא הגיעה בשלמותה. אפשר לנסות שוב.");
    const bytes = (r.data.length / 4) * 3 - (r.data.match(/=*$/)?.[0].length ?? 0);
    need(bytes <= 40 * 1024 * 1024, "ההקלטה גדולה מדי. אפשר לשמור הקלטה של עד 40 מגה-בייט.");
    const p = this.patient(r.patient_id);
    const visitId = blankToNull(r.visit_id);
    if (visitId === null && r.note_id === "") realDate(r.date.trim(), "תאריך");
    const kept = this.notes.reduce((sum, n) => sum + n.recordings.reduce((a, x) => a + x.bytes, 0), 0);
    need(kept + bytes <= 600 * 1024 * 1024, "בכספת כבר שמורות הקלטות בהיקף של כ-600 מגה-בייט, וזה המקסימום, כדי שהגיבוי יישאר קטן מספיק לשחזור. אפשר למחוק הקלטות ישנות שכבר לא צריך.");
    let note: SessionNote | null = r.note_id === "" ? (visitId === null ? null : this.notes.find((n) => n.visit_id === visitId) ?? null) : this.note(r.note_id);
    if (visitId !== null) need(this.visit(visitId).patient_id === p.id, "הפגישה שנבחרה שייכת למטופל אחר.");
    need(note === null || note.patient_id === p.id, "ההקלטה שייכת לסיכום של מטופל אחר.");
    need((note?.recordings.length ?? 0) < 10, "לסיכום אחד אפשר לשמור עד 10 הקלטות.");
    const before = note ? { ...note } : null;
    if (note === null) {
      note = { id: this.newId("note"), patient_id: p.id, visit_id: visitId, date: visitId === null ? r.date.trim() : this.visit(visitId).date, record: "", personal: "", case_input_id: null, recordings: [] };
      this.notes.push(note);
    }
    const id = this.newId("recording");
    this.sounds.set(id, { mime, data: r.data });
    note.recordings = [...note.recordings, { id, mime, seconds: Math.min(r.seconds, 4 * 3600), bytes, date: localToday(this.clock()).date, name: cleanLine(r.name).slice(0, 200) }];
    this.keep(note.id, before ? "changed" : "created", this.noteLines(note, before), p.id);
    this.wrote(p.id);
    return note;
  }

  /** A note that has text is part of the record and stays (she can correct it; every state is
   *  kept). Only one that holds nothing can be removed. */
  private deleteNote(id: string) {
    const n = this.note(id);
    need(n.record.trim() === "" && n.personal.trim() === "" && n.recordings.length === 0, "סיכום פגישה הוא חלק מהרשומה, ולכן לא מוחקים אותו. אפשר לערוך אותו, וכל שינוי נשמר בהיסטוריה.");
    this.keep(n.id, "deleted", this.noteLines(n, n), n.patient_id);
    this.notes = this.notes.filter((x) => x.id !== id);
  }

  private savePayment(input: Payment): Payment {
    const old = input.id === "" ? null : (this.payments.find((x) => x.id === input.id) ?? refuse("התקבול לא נמצא.", "not_found"));
    need(old === null || !old.voided, "תקבול שבוטל אי אפשר לערוך. אפשר לרשום תקבול חדש.");
    // Voiding has its own request, with a reason.
    const saved: Payment = {
      id: old ? old.id : "", patient_id: input.patient_id, date: input.date.trim(), amount: input.amount, method: input.method,
      receipt: cleanLine(input.receipt), payer: cleanLine(input.payer), note: input.note.trim(), voided: false, void_reason: "",
    };
    // The checks, in the core's order (`validate_payment`).
    need(saved.patient_id.trim() !== "", "צריך לבחור מטופל.");
    paymentKnown(saved);
    realDate(saved.date, "תאריך");
    need(whole(saved.amount, 1, MAX_AGOROT), "הסכום צריך להיות גדול מאפס, ועד 100,000 ₪.");
    short(saved.receipt, "מספר קבלה");
    short(saved.payer, "מי שילם");
    long(saved.note, "הערה");
    // Money never moves between cards (D-177): the mistake is voided and written again.
    need(old === null || old.patient_id === saved.patient_id, "אי אפשר להעביר תקבול למטופל אחר. אפשר לבטל את התקבול הזה, לכתוב למה, ולרשום תקבול חדש בכרטיס הנכון.");
    need(this.patients.some((p) => p.id === saved.patient_id), NO_PATIENT);
    // Saved again with nothing changed: nothing is written, and the history gets no line.
    if (old !== null && same(old, saved)) return old;
    if (old) Object.assign(old, saved);
    else this.payments.push(Object.assign(saved, { id: this.newId("payment") }));
    this.keep(saved.id, old ? "changed" : "created", this.paymentLines(saved), saved.patient_id);
    this.wrote(saved.patient_id);
    return old ?? saved;
  }

  private voidPayment(id: string, reason: string): Payment {
    const p = this.payments.find((x) => x.id === id) ?? refuse("התקבול לא נמצא.", "not_found");
    need(!p.voided, "התקבול הזה כבר מבוטל.");
    short(cleanLine(reason), "סיבת הביטול");
    need(cleanLine(reason) !== "", "צריך לכתוב למה הרישום מבוטל.");
    // A void is a write too: a payment that carries a value this version does not know is
    // not written back, so the stored value is never turned into something else (D-178).
    paymentKnown(p);
    p.voided = true;
    p.void_reason = cleanLine(reason);
    this.keep(p.id, "voided", this.paymentLines(p), p.patient_id);
    this.wrote(p.patient_id);
    return p;
  }

  private saveCharge(input: Charge): Charge {
    const old = input.id === "" ? null : (this.charges.find((x) => x.id === input.id) ?? refuse("החיוב לא נמצא.", "not_found"));
    need(old === null || !old.voided, "חיוב שבוטל אי אפשר לערוך. אפשר לרשום חיוב חדש.");
    const saved: Charge = { id: old ? old.id : "", patient_id: input.patient_id, date: input.date.trim(), amount: input.amount, what: cleanLine(input.what), voided: false, void_reason: "" };
    // The checks, in the core's order (`validate_charge`).
    need(saved.patient_id.trim() !== "", "צריך לבחור מטופל.");
    realDate(saved.date, "תאריך");
    need(whole(saved.amount, 1, MAX_AGOROT), "הסכום צריך להיות גדול מאפס, ועד 100,000 ₪.");
    need(saved.what !== "", "צריך לכתוב על מה החיוב.");
    short(saved.what, "על מה החיוב");
    need(old === null || old.patient_id === saved.patient_id, "אי אפשר להעביר חיוב למטופל אחר. אפשר לבטל את החיוב הזה, לכתוב למה, ולרשום חיוב חדש בכרטיס הנכון.");
    need(this.patients.some((p) => p.id === saved.patient_id), NO_PATIENT);
    if (old !== null && same(old, saved)) return old;
    if (old) Object.assign(old, saved);
    else this.charges.push(Object.assign(saved, { id: this.newId("charge") }));
    this.keep(saved.id, old ? "changed" : "created", this.chargeLines(saved), saved.patient_id);
    this.wrote(saved.patient_id);
    return old ?? saved;
  }

  private voidCharge(id: string, reason: string): Charge {
    const c = this.charges.find((x) => x.id === id) ?? refuse("החיוב לא נמצא.", "not_found");
    need(!c.voided, "החיוב הזה כבר מבוטל.");
    short(cleanLine(reason), "סיבת הביטול");
    need(cleanLine(reason) !== "", "צריך לכתוב למה הרישום מבוטל.");
    c.voided = true;
    c.void_reason = cleanLine(reason);
    this.keep(c.id, "voided", this.chargeLines(c), c.patient_id);
    this.wrote(c.patient_id);
    return c;
  }

  private saveTask(input: ClinicTask): ClinicTask {
    const task: ClinicTask = {
      ...input, patient_id: blankToNull(input.patient_id), title: cleanLine(input.title), due: blankToNull(input.due),
      done_on: blankToNull(input.done_on), note: input.note.trim(),
    };
    need(task.title !== "", "צריך לכתוב מה המשימה.");
    short(task.title, "משימה");
    taskKnown(task);
    if (task.due !== null) realDate(task.due, "תאריך יעד");
    if (task.done_on !== null) realDate(task.done_on, "תאריך ביצוע");
    long(task.note, "הערה");
    if (task.patient_id !== null) need(this.patients.some((p) => p.id === task.patient_id), NO_PATIENT);
    const old = task.id === "" ? null : (this.tasks.find((t) => t.id === task.id) ?? refuse("המשימה לא נמצאה. ייתכן שכבר נמחקה.", "not_found"));
    const saved: ClinicTask = { ...task, id: old ? old.id : this.newId("task") };
    if (old) Object.assign(old, saved);
    else this.tasks.push(saved);
    this.wrote(saved.patient_id);
    return old ?? saved;
  }

  /** The open tasks by date (so the overdue come first) and the undated after them; then the
   *  ones that were done, the latest first (as the core's `task_order`). */
  private orderedTasks(): ClinicTask[] {
    const text = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
    return [...this.tasks].sort((a, b) => {
      const open = Number(a.done_on !== null) - Number(b.done_on !== null);
      if (open !== 0) return open;
      const byDate = a.done_on !== null && b.done_on !== null ? text(b.done_on, a.done_on)
        : a.due !== null && b.due !== null ? text(a.due, b.due)
        : a.due !== null ? -1 : b.due !== null ? 1 : 0;
      return byDate || text(a.title, b.title) || text(a.id, b.id);
    });
  }

  private taskViews(today: Today, openOnly: boolean): TaskView[] {
    return this.orderedTasks()
      .filter((t) => !openOnly || t.done_on === null)
      .map((t): TaskView => ({ task: t, patient_name: this.nameOf(t.patient_id), overdue: t.done_on === null && t.due !== null && t.due < today.date }));
  }

  // ---------------------------------------------------------------- settings

  /** The settings as they are stored, checked (the core's `normalize_settings` and `validate_settings`). */
  private checkedSettings(given: ClinicSettings): ClinicSettings {
    const places: string[] = [];
    for (const place of given.places.map(cleanLine)) if (place !== "" && !places.includes(place)) places.push(place);
    const s: ClinicSettings = {
      ...given,
      work_days: [...new Set(given.work_days)].sort((a, b) => a - b),
      places,
      reminder_text: given.reminder_text.trim(),
      payment_text: given.payment_text.trim(),
      note_templates: given.note_templates.map((t) => ({ name: cleanLine(t.name), text: t.text })),
    };
    need(whole(s.day_start, 0, 1439) && whole(s.day_end, 1, 1440) && s.day_start < s.day_end, "שעת הסיום של היום ביומן צריכה להיות אחרי שעת ההתחלה, ועד חצות.");
    need(s.work_days.length > 0, "צריך לבחור לפחות יום עבודה אחד.");
    need(s.work_days.every((d) => whole(d, 0, 6)), "ימי העבודה לא נשמרו כמו שצריך. כדאי לסמן אותם מחדש.");
    need(s.defaults.length === VISIT_KINDS.length && VISIT_KINDS.every((k) => s.defaults.filter((d) => d.kind === k).length === 1), "לכל סוג פגישה צריכה להיות שורה אחת של משך ומחיר.");
    for (const d of s.defaults) {
      need(whole(d.minutes, 5, 720), "משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
      need(whole(d.fee, 0, MAX_AGOROT), "מחיר הפגישה צריך להיות בין 0 ל-100,000 ₪.");
      need(d.kind !== "block" || d.fee === 0, "לזמן שמור אין מחיר.");
    }
    need(s.places.length <= MAX_PLACES, `אפשר לשמור עד ${MAX_PLACES} מקומות.`);
    s.places.forEach((place) => short(place, "מקום"));
    atMost(s.reminder_text, MESSAGE, "נוסח התזכורת");
    atMost(s.payment_text, MESSAGE, "נוסח הודעת התשלום");
    need(s.note_templates.length <= MAX_TEMPLATES, `אפשר לשמור עד ${MAX_TEMPLATES} תבניות לסיכום.`);
    for (const t of s.note_templates) {
      need(t.name !== "", "צריך לתת שם לכל תבנית של סיכום.");
      short(t.name, "שם התבנית");
      atMost(t.text, TEMPLATE, "תבנית לסיכום");
    }
    need(whole(s.report_days, 0, 65_535) && whole(s.debt_days, 0, 65_535) && whole(s.waiting_weeks, 0, 65_535), "מספר הימים או השבועות לא תקין.");
    s.package_name = cleanLine(s.package_name);
    s.starter_tasks = s.starter_tasks.map((t) => ({ ...t, title: cleanLine(t.title) }));
    // The first step is on the day she starts from (D-198): it has no gap.
    s.pathway = s.pathway.map((step, i) => (i === 0 ? { ...step, gap_days: 0 } : step));
    need(whole(s.buffer_minutes, 0, 120), "הזמן הפנוי בין פגישות יכול להיות עד 120 דקות.");
    need(whole(s.package_fee, 0, MAX_AGOROT), "מחיר הפגישה צריך להיות בין 0 ל-100,000 ₪.");
    short(s.package_name, "שם החבילה");
    need(s.package_fee === 0 || s.package_name !== "", "צריך לתת שם לחבילת האבחון, כדי שיופיע בחיוב.");
    need(s.pathway.length <= 12, "אפשר להגדיר עד 12 שלבים במסלול האבחון.");
    for (const step of s.pathway) {
      need(step.kind !== "block" && step.kind !== "unknown", "כל שלב במסלול האבחון צריך להיות פגישה עם המשפחה או עם הילד.");
      need(whole(step.gap_days, 0, 180), "הרווח בין שני שלבים במסלול יכול להיות עד 180 יום.");
    }
    need(s.starter_tasks.length <= 20, "אפשר להגדיר עד 20 מעקבים בערכת הפתיחה.");
    for (const t of s.starter_tasks) {
      need(t.title !== "", "לכל מעקב בערכת הפתיחה צריכה להיות כותרת.");
      need(chars(t.title) <= SHORT, "כותרת של מעקב בערכת הפתיחה ארוכה מדי.");
      need(whole(t.due_days, 0, 365), "מעקב בערכת הפתיחה יכול להיות עד שנה מהיום.");
      need(TASK_KINDS.includes(t.kind), "צריך לבחור סוג לכל מעקב בערכת הפתיחה.");
    }
    return s;
  }

  // ---------------------------------------------------------------- messages

  /** As the core's `clinic_compose` (D-201): the address carries the number or email only. */
  private compose(kind: MessageKind, patientId: string, visitId: string | null, channel: Channel, picked: number | null): Composed {
    const p = this.patient(patientId);
    const phone = channel === "whatsapp" || channel === "sms";
    need(phone || channel === "email", "דרך השליחה לא מוכרת בגרסה הזו.");
    const usable = (c: Contact) => (phone ? c.phone.trim() !== "" : c.email.trim() !== "");
    const contact = picked === null ? p.contacts.find(usable) : p.contacts[picked];
    need(contact !== undefined && usable(contact), phone ? "בכרטיס אין מספר טלפון של איש קשר. אפשר להוסיף אותו בכרטיס." : "בכרטיס אין כתובת מייל של איש קשר. אפשר להוסיף אותה בכרטיס.");
    const c = contact as Contact;
    let link: string;
    if (phone) {
      const digits = c.phone.replace(/\D/g, "");
      const full = c.phone.trim().startsWith("+") ? digits : digits.startsWith("00") ? digits.slice(2) : digits.startsWith("0") ? `972${digits.slice(1)}` : digits;
      need(/^[1-9]\d{7,14}$/.test(full), "למספר הטלפון של איש הקשר אי אפשר לשלוח. כדאי לבדוק אותו בכרטיס.");
      link = channel === "whatsapp" ? `whatsapp://send?phone=${full}` : `sms:+${full}`;
    } else {
      const email = c.email.trim();
      need(/^[A-Za-z0-9._+-]+@[A-Za-z0-9.-]+\.[A-Za-z0-9-]+$/.test(email), "כתובת המייל של איש הקשר לא תקינה. כדאי לבדוק אותה בכרטיס.");
      link = `mailto:${email}`;
    }
    const name = cleanLine(c.name);
    const text = this.message(kind, patientId, visitId, name === "" ? null : name);
    return { text, link, delivery: "on_her_device", to: name === "" ? "איש הקשר" : name };
  }

  private message(kind: MessageKind, patientId: string, visitId: string | null, greet: string | null = null): string {
    const p = this.patient(patientId);
    const v = visitId === null ? null : this.visit(visitId);
    need(v === null || v.patient_id === p.id, "הפגישה שייכת למטופל אחר.");
    const due = this.balance(p.id).due;
    if (kind === "reminder") need(v !== null, "כדי להכין תזכורת צריך לבחור פגישה.");
    else need(due > 0, "אין יתרה לתשלום, ולכן אין מה לשלוח.");
    return fillMessage(kind === "reminder" ? this.settings.reminder_text : this.settings.payment_text, {
      // No name of a parent on the card: "שלום רב".
      "{הורה}": greet ?? p.contacts.map((c) => cleanLine(c.name)).find((name) => name !== "") ?? "רב",
      "{ילד}": p.name,
      "{יום}": v ? dayName(v.date) : "",
      "{תאריך}": v ? formatDate(v.date) : "",
      "{שעה}": v ? formatTime(v.start) : "",
      "{מקום}": v ? v.place : "",
      // What the family still owes; a family that paid ahead owes nothing.
      "{סכום}": plainMoney(Math.max(0, due)),
      "{שמי}": this.host.myName(),
    });
  }

  // ---------------------------------------------------------------- alerts

  /** What needs her, computed on every read (as the core's `clinic_alerts`): the sentences
   *  never carry the patient's name, which the screen shows beside them. */
  private alerts(today: Today): Alert[] {
    const out: Alert[] = [];
    const s = this.settings;
    const add = (kind: Alert["kind"], p: Patient | null, text: string, since: string, ids: { visit?: string; task?: string } = {}) =>
      out.push({ kind, patient_id: p?.id ?? null, patient_name: p?.name ?? null, visit_id: ids.visit ?? null, task_id: ids.task ?? null, text, since });
    const reportWords = (p: Patient) => {
      const c = this.reportOf(p);
      return c === null ? "עדיין לא נפתח תיק לדוח" : `הדוח עדיין לא גמור (${c.approved} מתוך ${c.total} סעיפים)`;
    };
    const weeksWords = (weeks: number) => (weeks === 1 ? "שבוע" : weeks === 2 ? "שבועיים" : `${weeks} שבועות`);
    const noted = new Set(this.notes.map((n) => n.visit_id));

    for (const p of this.patients) {
      const mine = this.visits.filter((v) => v.patient_id === p.id);
      const dates = (kind: VisitKind, status: VisitStatus) => mine.filter((v) => v.kind === kind && v.status === status).map((v) => v.date).sort();

      for (const v of mine) {
        const when = `${formatDate(v.date)} בשעה ${formatTime(v.start)}`;
        if (v.status === "planned" && !ownVisit(v)) {
          if (v.date < today.date || (v.date === today.date && v.start + v.minutes <= today.minute)) {
            add("unmarked", p, `הפגישה מיום ${when} עדיין מסומנת כמתוכננת. אפשר לסמן אם התקיימה.`, v.date, { visit: v.id });
          }
          const ahead = daysBetween(today.date, v.date);
          if (v.kind === "feedback" && ahead >= 0 && ahead <= FEEDBACK_SOON_DAYS && !this.reportFinished(p)) {
            add("feedback_soon", p, `שיחת המשוב נקבעה ליום ${when}, ו${reportWords(p)}.`, v.date, { visit: v.id });
          }
          // Questionnaires and documents still open a week before the feedback (D-199): how many only.
          const missing = this.tasks.filter((t) => t.patient_id === p.id && t.done_on === null && (t.kind === "questionnaire" || t.kind === "document")).length;
          if (v.kind === "feedback" && ahead >= 0 && ahead <= FEEDBACK_SOON_DAYS && missing > 0) {
            const what = missing === 1 ? "שאלון או מסמך אחד עדיין פתוח" : `${missing} שאלונים או מסמכים עדיין פתוחים`;
            add("materials_missing", p, `שיחת המשוב נקבעה ליום ${when}, ו${what} במעקבים.`, v.date, { visit: v.id });
          }
        }
        const age = daysBetween(v.date, today.date);
        if (v.status === "held" && NOTE_KINDS.includes(v.kind) && age >= 0 && age <= NO_NOTE_DAYS && !noted.has(v.id)) {
          add("no_note", p, `עדיין אין סיכום ל${VISIT_KIND_LABEL[v.kind]} מיום ${formatDate(v.date)}.`, v.date, { visit: v.id });
        }
      }

      const due = this.balance(p.id).due;
      const oldest = due > 0 ? this.owedSince(p.id) : null;
      if (oldest !== null && daysBetween(oldest, today.date) >= s.debt_days) {
        add("debt", p, `יתרה פתוחה של ${plainMoney(due)} ₪, מאז ${formatDate(oldest)}.`, oldest);
      }

      if (p.status === "waiting") {
        // A card without the day it joined the list waits since the day it was opened.
        const since = p.waiting_since !== null && isIsoDate(p.waiting_since) ? p.waiting_since : p.created_on;
        const days = daysBetween(since, today.date);
        if (isIsoDate(since) && days >= 7 * s.waiting_weeks) {
          add("waiting", p, days >= 7 ? `ברשימת ההמתנה כבר ${weeksWords(Math.floor(days / 7))}, מאז ${formatDate(since)}.` : `ברשימת ההמתנה מאז ${formatDate(since)}.`, since);
        }
      }

      if (p.status === "active") {
        const lastAssessment = dates("assessment", "held").pop();
        const moreAhead = dates("assessment", "planned").some((date) => date >= today.date);
        const feedbackHeld = dates("feedback", "held").length > 0;
        if (lastAssessment && !this.reportFinished(p) && !moreAhead && !feedbackHeld && addDays(lastAssessment, s.report_days) < today.date) {
          add("report_due", p, `מפגש האבחון האחרון היה ביום ${formatDate(lastAssessment)}, ו${reportWords(p)}.`, lastAssessment);
        }
        const last = mine.map((v) => v.date).filter((date) => date <= today.date).sort().pop();
        const ahead = mine.some((v) => v.status === "planned" && v.date >= today.date);
        const openTask = this.tasks.some((t) => t.patient_id === p.id && t.done_on === null);
        if (last && !ahead && !openTask && !(feedbackHeld && due <= 0) && daysBetween(last, today.date) > NO_NEXT_VISIT_DAYS) {
          add("no_next_visit", p, `הפגישה האחרונה הייתה ביום ${formatDate(last)}, ואין פגישה הבאה ביומן.`, last);
        }
      }
    }

    for (const t of this.tasks) {
      if (t.done_on === null && t.due !== null && t.due < today.date) {
        add("task_overdue", this.patients.find((p) => p.id === t.patient_id) ?? null, `המשימה "${t.title}" הייתה אמורה להיעשות עד ${formatDate(t.due)}.`, t.due, { task: t.id });
      }
    }
    // Oldest first; the rest of the key only makes the order the same on every read.
    const rank: Record<Alert["kind"], number> = { unmarked: 0, no_note: 1, task_overdue: 2, feedback_soon: 3, materials_missing: 4, report_due: 5, debt: 6, no_next_visit: 7, waiting: 8, unreadable: 9 };
    const text = (a: string | null, b: string | null) => ((a ?? "") < (b ?? "") ? -1 : (a ?? "") > (b ?? "") ? 1 : 0);
    return out.sort((a, b) => text(a.since, b.since) || rank[a.kind] - rank[b.kind] || text(a.patient_id, b.patient_id) || text(a.visit_id, b.visit_id) || text(a.task_id, b.task_id));
  }

  // ---------------------------------------------------------------- the front page

  /** The alert of rows that could not be read (D-178; the core's `unreadable_alert`): only on
   *  the front page, always first, about no patient. */
  private unreadableAlert(today: Today): Alert[] {
    const n = this.unreadableRows;
    if (n === 0) return [];
    const text = n === 1
      ? "רישום אחד בקליניקה לא נקרא, ולכן הוא לא מוצג ולא נספר בסיכומים. הוא לא נמחק ולא שונה. כדאי לשחזר מגיבוי, או לפנות לתמיכה."
      : `${n} רישומים בקליניקה לא נקראו, ולכן הם לא מוצגים ולא נספרים בסיכומים. הם לא נמחקו ולא שונו. כדאי לשחזר מגיבוי, או לפנות לתמיכה.`;
    return [{ kind: "unreadable", patient_id: null, patient_name: null, visit_id: null, task_id: null, text, since: today.date }];
  }

  private overview(today: Today): Overview {
    const on = (date: string) => this.visits.filter((v) => v.date === date).sort(byDateTime);
    const todays = on(today.date);
    // A visit with a patient that was not cancelled; time she kept for herself is not one.
    const counts = (v: Visit) => !ownVisit(v) && v.status !== "cancelled";
    // Today has nothing left once every visit of it has ended.
    const left = todays.some((v) => counts(v) && v.start + v.minutes > today.minute);
    const nextDay = left ? null : (this.visits.filter((v) => counts(v) && v.date > today.date).map((v) => v.date).sort()[0] ?? null);
    const week = weekRange(today.date);
    const month = monthOf(today.date);
    return {
      date: today.date,
      visits: todays.map((v) => this.view(v)),
      next_day: nextDay,
      next_visits: nextDay === null ? [] : on(nextDay).map((v) => this.view(v)),
      tasks: this.taskViews(today, true),
      // Rows that were not read come first: every figure here is without them.
      alerts: [...this.unreadableAlert(today), ...this.alerts(today)],
      week_visits: this.visits.filter((v) => counts(v) && v.date >= week.from && v.date <= week.to).length,
      // With what the erased cards received this month (D-179). The total owed is a state
      // of now, not a summary of a period: an erased card is no longer in it.
      month_received: this.payments.filter((p) => !p.voided && monthOf(p.date) === month).reduce((sum, p) => sum + p.amount, 0)
        + (this.tally.get(month)?.received ?? []).reduce((sum, m) => sum + m.amount, 0),
      total_due: this.patients.reduce((sum, p) => sum + Math.max(0, this.balance(p.id).due), 0),
      waiting: this.patients.filter((p) => p.status === "waiting").length,
      active: this.patients.filter((p) => p.status === "active").length,
    };
  }

  // ---------------------------------------------------------------- the diary and income

  private changed(recordId: string): boolean {
    return (this.history.get(recordId)?.length ?? 0) > 1;
  }

  /** The one anonymous line of a month (D-179; the core's `diary_with_erased`); `null` when
   *  nothing of an erased card falls in it. */
  private erasedLine(month: string): DiaryErased | null {
    const erased = this.tally.get(month);
    if (!erased) return null;
    const line: DiaryErased = {
      label: ERASED_LABEL,
      held: erased.held.reduce((sum, k) => sum + k.count, 0),
      cancelled: erased.cancelled,
      no_show: erased.no_show,
      charged: erased.charged,
      received: erased.received.reduce((sum, m) => sum + m.amount, 0),
    };
    const empty = line.held === 0 && line.cancelled === 0 && line.no_show === 0 && line.charged === 0 && line.received === 0;
    return empty ? null : line;
  }

  /** The income diary of a month (as the core's `diary_month`). A voided line is shown with
   *  its amount and counted nowhere. */
  private diary(month: string): DiaryMonth {
    need(/^2[0-2]\d{2}-(0[1-9]|1[0-2])$/.test(month), "צריך לבחור חודש.");
    const { from, to } = monthRange(month);
    const inMonth = (date: string) => date >= from && date <= to;
    const voidedNote = (note: string, voided: boolean, reason: string) => {
      const line = cleanLine(note);
      if (!voided) return line;
      return line === "" ? `בוטל: ${cleanLine(reason)}` : `${line} · בוטל: ${cleanLine(reason)}`;
    };
    const lines: { date: string; order: number; time: number; id: string; line: DiaryLine }[] = [];
    for (const v of this.visits.filter((x) => x.patient_id !== null && inMonth(x.date))) {
      lines.push({ date: v.date, order: 0, time: v.start, id: v.id, line: {
        record_id: v.id, time: v.start, patient_name: this.nameOf(v.patient_id) ?? "", what: VISIT_KIND_LABEL[v.kind], status: v.status,
        charged: visitCharge(v), received: 0, method: null, receipt: "", payer: "", note: v.status_note, voided: false, changed: this.changed(v.id),
      } });
    }
    for (const c of this.charges.filter((x) => inMonth(x.date))) {
      lines.push({ date: c.date, order: 1, time: 0, id: c.id, line: {
        record_id: c.id, time: null, patient_name: this.nameOf(c.patient_id) ?? "", what: c.what, status: null,
        charged: c.amount, received: 0, method: null, receipt: "", payer: "", note: voidedNote("", c.voided, c.void_reason), voided: c.voided, changed: this.changed(c.id),
      } });
    }
    for (const p of this.payments.filter((x) => inMonth(x.date))) {
      lines.push({ date: p.date, order: 2, time: 0, id: p.id, line: {
        record_id: p.id, time: null, patient_name: this.nameOf(p.patient_id) ?? "", what: "תשלום", status: null,
        charged: 0, received: p.amount, method: p.method, receipt: p.receipt, payer: p.payer, note: voidedNote(p.note, p.voided, p.void_reason), voided: p.voided, changed: this.changed(p.id),
      } });
    }
    // By date, then visits by time, then charges, then payments; the id keeps the order the same on every read.
    lines.sort((a, b) => a.date.localeCompare(b.date) || a.order - b.order || a.time - b.time || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    const days: DiaryDay[] = [];
    for (const { date, line } of lines) {
      let day = days[days.length - 1];
      if (!day || day.date !== date) {
        day = { date, lines: [], charged: 0, received: 0 };
        days.push(day);
      }
      day.lines.push(line);
      if (line.voided) continue;
      day.charged += line.charged;
      day.received += line.received;
    }
    const live = this.payments.filter((p) => inMonth(p.date) && !p.voided);
    // What the erased cards counted for in this month is in its totals, its totals by method
    // and its payments with no receipt, and in no day (D-179).
    const erased = this.tally.get(month);
    const line = this.erasedLine(month);
    // A method of a later version is a total of its own, so the methods still add up.
    const methods: PayMethod[] = [...PAY_METHODS, "unknown"];
    const byMethod = methods
      .map((method) => ({
        method,
        amount: live.filter((p) => p.method === method).reduce((sum, p) => sum + p.amount, 0)
          + (erased?.received ?? []).filter((m) => m.method === method).reduce((sum, m) => sum + m.amount, 0),
      }))
      .filter((m) => m.amount > 0);
    return {
      month,
      days,
      charged: days.reduce((sum, d) => sum + d.charged, 0) + (erased?.charged ?? 0),
      received: days.reduce((sum, d) => sum + d.received, 0) + (erased?.received ?? []).reduce((sum, m) => sum + m.amount, 0),
      by_method: byMethod,
      missing_receipts: live.filter((p) => p.receipt === "").length + (erased?.missing_receipts ?? 0),
      erased: line,
    };
  }

  /** The year at a glance (the core's `income_year` and `income_with_erased`): what the
   *  cards that were erased counted for stays in it, so a year reads the same before and
   *  after an erase (D-179). */
  private income(year: number): IncomeYear {
    need(whole(year, 2000, 2200), "צריך לבחור שנה.");
    const sessions = this.visits.filter((v) => v.patient_id !== null && v.date.startsWith(`${year}-`));
    const count = (list: Visit[], status: VisitStatus) => list.filter((v) => v.status === status).length;
    const heldOf = (month: string) => (this.tally.get(month)?.held ?? []).reduce((sum, k) => sum + k.count, 0);
    const months: IncomeMonth[] = Array.from({ length: 12 }, (_, i) => {
      const month = `${year}-${String(i + 1).padStart(2, "0")}`;
      const of = sessions.filter((v) => monthOf(v.date) === month);
      const erased = this.tally.get(month);
      return {
        month,
        held: count(of, "held") + heldOf(month),
        cancelled: count(of, "cancelled") + (erased?.cancelled ?? 0),
        no_show: count(of, "no_show") + (erased?.no_show ?? 0),
        charged: of.reduce((sum, v) => sum + visitCharge(v), 0)
          + this.charges.filter((c) => !c.voided && monthOf(c.date) === month).reduce((sum, c) => sum + c.amount, 0)
          + (erased?.charged ?? 0),
        received: this.payments.filter((p) => !p.voided && monthOf(p.date) === month).reduce((sum, p) => sum + p.amount, 0)
          + (erased?.received ?? []).reduce((sum, m) => sum + m.amount, 0),
      };
    });
    const erasedKinds = months.flatMap((m) => this.tally.get(m.month)?.held ?? []);
    const kinds: VisitKind[] = [...VISIT_KINDS, "unknown"];
    // The screens' own order, then the most frequent first (the sort keeps equal ones in place).
    const byKind = kinds
      .map((kind) => ({
        kind,
        count: (kind === "unknown" ? 0 : sessions.filter((v) => v.kind === kind && v.status === "held").length)
          + erasedKinds.filter((k) => k.kind === kind).reduce((sum, k) => sum + k.count, 0),
      }))
      .filter((k) => k.count > 0)
      .sort((a, b) => b.count - a.count);
    const opened = [...this.erasedCards].filter(([month]) => month.startsWith(`${year}-`)).reduce((sum, [, cards]) => sum + cards, 0);
    return {
      year,
      months,
      charged: months.reduce((sum, m) => sum + m.charged, 0),
      received: months.reduce((sum, m) => sum + m.received, 0),
      new_patients: this.patients.filter((p) => p.created_on.startsWith(`${year}-`)).length + opened,
      by_kind: byKind,
    };
  }

  // ---------------------------------------------------------------- the demo clinic

  /** A working clinic of invented families, dated around today so the calendar is alive. */
  private seed(today: Today) {
    const day = (n: number) => addDays(today.date, n);
    const fees: Partial<Record<VisitKind, number>> = { intake: 45_000, assessment: 60_000, observation: 50_000, feedback: 45_000, therapy: 38_000, guidance: 38_000 };
    this.settings = {
      ...defaultClinicSettings(),
      defaults: defaultClinicSettings().defaults.map((d) => ({ ...d, fee: fees[d.kind] ?? 0 })),
      places: ["הקליניקה", "בגן או בבית הספר", "שיחת וידאו"],
    };
    const minutes = (kind: VisitKind) => this.settings.defaults.find((d) => d.kind === kind)?.minutes ?? 60;

    const family = (name: string, born: number, gender: Patient["gender"], contacts: Contact[], over: Partial<Patient>): Patient => {
      const p: Patient = {
        id: this.newId("patient"), name, birth_date: day(-born), gender, contacts, framework: "", referred_by: "", reason: "", notes: "",
        status: "active", waiting_since: null, urgent: false, session_fee: null, case_id: null, created_on: day(-30), ...over,
      };
      this.patients.push(p);
      return p;
    };
    const parent = (role: Contact["role"], name: string, phone: string): Contact => ({ role, name, phone, email: "" });
    const visit = (p: Patient | null, kind: VisitKind, at: number, start: number, status: VisitStatus, over: Partial<Visit> = {}): Visit => {
      const v: Visit = {
        id: this.newId("visit"), patient_id: p?.id ?? null, kind, title: "", date: day(at), start, minutes: minutes(kind), place: "הקליניקה", status,
        status_note: "", fee: ownTime(kind, p?.id) ? 0 : (p?.session_fee ?? fees[kind] ?? 0), charge_missed: false, series_id: null, note: "", remember: [], ...over,
      };
      this.visits.push(v);
      this.keepVisit(v, "created");
      return v;
    };
    const note = (v: Visit, record: string, personal = "") => {
      const n: SessionNote = { id: this.newId("note"), patient_id: v.patient_id ?? "", visit_id: v.id, date: v.date, record, personal, case_input_id: null, recordings: [] };
      this.notes.push(n);
      this.keep(n.id, "created", this.noteLines(n, null), n.patient_id);
    };
    const paid = (p: Patient, at: number, amount: number, method: PayMethod, receipt: string) => {
      const x: Payment = { id: this.newId("payment"), patient_id: p.id, date: day(at), amount, method, receipt, payer: "", note: "", voided: false, void_reason: "" };
      this.payments.push(x);
      this.keep(x.id, "created", this.paymentLines(x), x.patient_id);
    };
    const task = (p: Patient | null, kind: ClinicTask["kind"], title: string, due: number | null, doneOn: number | null = null) => {
      this.tasks.push({ id: this.newId("task"), patient_id: p?.id ?? null, kind, title, due: due === null ? null : day(due), done_on: doneOn === null ? null : day(doneOn), note: "" });
    };

    // נועם: in the middle of an assessment, the feedback meeting is near and the report is not done.
    const linked = this.host.cases().find((c) => c.childName === "נועם" && !c.inTrash)?.id ?? null;
    const noam = family("נועם", 5 * 365 + 130, "male", [parent("mother", "מיכל", "050-555-01"), parent("father", "אלון", "052-555-02")], {
      framework: "גן חצב", referred_by: "רופאת הילדים", reason: "קושי במעברים ובוויסות בגן. הגננת ביקשה הערכה.", case_id: linked, created_on: day(-42),
    });
    note(visit(noam, "intake", -35, 17 * 60, "held"), "סיבת הפנייה: קושי במעברים בין פעילויות בגן.\n\nרקע התפתחותי: אבני דרך בזמן, לדברי ההורים.\n\nציפיות ההורים: להבין מה עוזר לו בגן.", "להזכיר להורים להביא את דוח הגננת.");
    note(visit(noam, "assessment", -21, 9 * 60, "held"), "כלים שהועברו: מבחן קוגניטיבי, חלק ראשון.\n\nהתנהגות במפגש: שיתף פעולה, נזקק להפסקות קצרות.");
    visit(noam, "assessment", -14, 9 * 60, "held");
    visit(noam, "feedback", 5, 17 * 60, "planned");
    paid(noam, -35, 45_000, "bit", "1041");
    paid(noam, -21, 60_000, "transfer", "1047");
    task(noam, "call", "לחזור לרופאת הילדים", 0);

    // תמר: just started; an assessment session today, an observation tomorrow.
    const tamar = family("תמר", 4 * 365 + 70, "female", [parent("mother", "יעל", "054-555-03")], {
      framework: "גן רימון", referred_by: "הגננת", reason: "קושי להישאר בפעילות אחת לאורך זמן.", created_on: day(-12),
    });
    note(visit(tamar, "intake", -10, 18 * 60, "held"), "סיבת הפנייה: קושי להישאר בפעילות במפגש הבוקר.\n\nמסגרת חינוכית: גן רימון, שנה שנייה.");
    visit(tamar, "assessment", 0, 9 * 60, "planned", { note: "להביא את השאלונים שמילאו ההורים" });
    visit(tamar, "observation", 1, 10 * 60, "planned", { place: "בגן או בבית הספר" });
    paid(tamar, -10, 45_000, "cash", "");
    task(tamar, "questionnaire", "לשלוח שאלון לגננת", 1);
    task(tamar, "document", "לבקש מההורים את סיכום קלינאית התקשורת", -6, -2);

    // איתי: a first meeting with the parents today.
    const itay = family("איתי", 6 * 365 + 20, "male", [parent("father", "דורון", "050-555-04")], {
      framework: "כיתה א'", referred_by: "היועצת", reason: "קשיי קשב בכיתה.", created_on: day(-3),
    });
    visit(itay, "intake", 0, 11 * 60 + 30, "planned");

    // מאיה: a weekly session, her own fee, today's among them.
    const maya = family("מאיה", 7 * 365 + 95, "female", [parent("mother", "הילה", "053-555-05")], {
      framework: "כיתה ב'", reason: "ליווי אחרי אבחון.", session_fee: 35_000, created_on: day(-28),
    });
    const weekly = this.newId("series");
    for (let i = 0; i < 8; i++) {
      const v = visit(maya, "therapy", -21 + 7 * i, 16 * 60, i < 3 ? "held" : "planned", { series_id: weekly });
      if (i < 2) note(v, "המשך עבודה על ויסות. מאיה שיתפה פעולה ונהנתה מהמשחק.");
    }
    paid(maya, -21, 35_000, "paybox", "1049");
    paid(maya, -14, 35_000, "paybox", "1052");

    // יובל: the assessment is behind him; part of the payment is still open.
    const yuval = family("יובל", 5 * 365 + 200, "male", [parent("mother", "ענת", "052-555-07")], {
      framework: "גן דקל", referred_by: "המלצה של משפחה", reason: "עיכוב שפתי.", created_on: day(-90),
    });
    note(visit(yuval, "intake", -80, 17 * 60, "held"), "סיבת הפנייה: עיכוב שפתי.\n\nטיפולים קודמים: קלינאית תקשורת, חצי שנה.");
    note(visit(yuval, "assessment", -70, 9 * 60, "held"), "כלים שהועברו: מבחן קוגניטיבי.\n\nהתנהגות במפגש: נעים ומשתף פעולה.");
    note(visit(yuval, "assessment", -63, 9 * 60, "held"), "כלים שהועברו: מבחן שפה ומשחק חופשי.");
    note(visit(yuval, "feedback", -45, 18 * 60, "held"), "מה נמסר להורים: עיקרי הממצאים וההמלצות.\n\nמה נמסר או נשלח: הדוח המלא.");
    const report: Charge = { id: this.newId("charge"), patient_id: yuval.id, date: day(-45), amount: 120_000, what: "כתיבת דוח אבחון", voided: false, void_reason: "" };
    this.charges.push(report);
    this.keep(report.id, "created", this.chargeLines(report), report.patient_id);
    paid(yuval, -80, 45_000, "transfer", "1012");
    paid(yuval, -63, 120_000, "transfer", "1020");
    task(yuval, "payment", "להזכיר להורים את יתרת התשלום", -3);

    // Two families on the waiting list; one of them for a long time.
    family("עומר", 3 * 365 + 300, "male", [parent("mother", "רונית", "052-555-06")], {
      status: "waiting", waiting_since: day(-56), urgent: true, referred_by: "קלינאית התקשורת", reason: "עיכוב שפתי, והגן מבקש הערכה לפני ועדה.", created_on: day(-56),
    });
    family("שירה", 4 * 365 + 250, "female", [parent("father", "גיל", "050-555-08")], {
      status: "waiting", waiting_since: day(-12), reason: "קושי חברתי בגן.", created_on: day(-12),
    });

    // Her own time.
    visit(null, "block", 0, 13 * 60, "planned", { title: "כתיבת דוחות", minutes: 120, place: "" });
    visit(null, "block", 2, 8 * 60, "planned", { title: "השתלמות", minutes: 180, place: "" });
    task(null, "other", "להזמין טפסי שאלונים", null);
  }
}
