// Browser preview and tests only: an in-memory stand-in for the clinic (docs/CLINIC.md), with
// the same rules the core keeps (crates/dv-domain/src/clinic/logic.rs): validation, series,
// overlaps, charges and balances, alerts, the assessment process, the diary, income, history
// and the messages, and what may be erased (D-174). Every patient, phone
// and text here is invented; nothing is stored and nothing leaves the page.
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
import type { Contact } from "../ipc/generated/Contact";
import type { DiaryDay } from "../ipc/generated/DiaryDay";
import type { DiaryLine } from "../ipc/generated/DiaryLine";
import type { DiaryMonth } from "../ipc/generated/DiaryMonth";
import type { HistoryEntry } from "../ipc/generated/HistoryEntry";
import type { IdentityInput } from "../ipc/generated/IdentityInput";
import type { IncomeMonth } from "../ipc/generated/IncomeMonth";
import type { IncomeYear } from "../ipc/generated/IncomeYear";
import type { InputKind } from "../ipc/generated/InputKind";
import type { MessageKind } from "../ipc/generated/MessageKind";
import type { Overview } from "../ipc/generated/Overview";
import type { Patient } from "../ipc/generated/Patient";
import type { PatientCard } from "../ipc/generated/PatientCard";
import type { PatientRow } from "../ipc/generated/PatientRow";
import type { PayMethod } from "../ipc/generated/PayMethod";
import type { Payment } from "../ipc/generated/Payment";
import type { ProcessStep } from "../ipc/generated/ProcessStep";
import type { Repeat } from "../ipc/generated/Repeat";
import type { SavedVisits } from "../ipc/generated/SavedVisits";
import type { Scope } from "../ipc/generated/Scope";
import type { SessionNote } from "../ipc/generated/SessionNote";
import type { TaskView } from "../ipc/generated/TaskView";
import type { Today } from "../ipc/generated/Today";
import type { UiError } from "../ipc/generated/UiError";
import type { Visit } from "../ipc/generated/Visit";
import type { VisitKind } from "../ipc/generated/VisitKind";
import type { VisitStatus } from "../ipc/generated/VisitStatus";
import type { VisitView } from "../ipc/generated/VisitView";
import {
  addDays, CONTACT_ROLE_LABEL, dayName, daysBetween, formatDate, formatTime, isIsoDate, MAX_AGOROT, moneyNumber, monthOf, monthRange,
  PAY_METHOD_LABEL, PAY_METHODS, today as localToday, VISIT_KIND_LABEL, VISIT_KINDS, VISIT_STATUS_LABEL, weekRange,
} from "../screens/clinic/format";

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
  myName(): string;
}

/** Cases of its own, for a clinic that runs without the demo core (the unit tests). */
export function standInHost(): ClinicHost & { inputs: { id: string; caseId: string; kind: InputKind; title: string; content: string }[]; trash(id: string): void } {
  const cases: CaseInfo[] = [];
  const inputs: { id: string; caseId: string; kind: InputKind; title: string; content: string }[] = [];
  let seq = 0;
  return {
    inputs,
    cases: () => cases.map((c) => ({ ...c })),
    createCase(meta, identities) {
      const id = `case-local-${++seq}`;
      cases.push({ id, code: meta.code, childName: identities.find((i) => i.role === "child")?.value ?? null, approved: 0, total: 16, inTrash: false });
      return id;
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
const realDate = (s: string, field: string) => need(isIsoDate(s), `צריך לבחור תאריך תקין בשדה "${field}".`);
/** One line, single spaces: what a name, a phone or a place is stored as. */
const cleanLine = (s: string) => s.split(/\s+/).filter(Boolean).join(" ");
const blankToNull = (s: string | null) => (s === null || s.trim() === "" ? null : s.trim());
const whole = (n: unknown, min: number, max: number): n is number => typeof n === "number" && Number.isInteger(n) && n >= min && n <= max;
const copy = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

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
  const minutes: Record<VisitKind, number> = { intake: 60, assessment: 90, observation: 60, feedback: 60, therapy: 50, guidance: 50, meeting: 30, other: 50, block: 60 };
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
  };
}

const ymd = (iso: string) => iso.split("-").map(Number) as [number, number, number];
const daysInMonth = (y: number, m: number) => new Date(Date.UTC(y, m, 0)).getUTCDate();

/** Age on `on` of a child born on `birth`; `null` without a birth date, or when it is still ahead. */
export function ageOn(birth: string | null, on: string): Age | null {
  if (!birth || !isIsoDate(birth) || !isIsoDate(on) || birth > on) return null;
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
  private payments: Payment[] = [];
  private charges: Charge[] = [];
  private tasks: ClinicTask[] = [];
  private settings: ClinicSettings = defaultClinicSettings();
  /** Every saved state of a visit, a session note, a charge or a payment, oldest first, by record id. */
  private history = new Map<string, HistoryEntry[]>();
  private seq = 0;
  private seeded = false;
  private readonly host: ClinicHost;
  private readonly demo: boolean;

  /** `demo`: a working clinic of invented families (the preview); `false`: an empty one. */
  constructor(host: ClinicHost = standInHost(), options: { demo?: boolean } = {}) {
    this.host = host;
    const asked = typeof location === "undefined" ? null : new URLSearchParams(location.search).get("clinic");
    // `?clinic=empty` in the address: the clinic of a first day, for the demo page.
    this.demo = options.demo ?? asked !== "empty";
  }

  /** One request, answered as the core answers it. A refusal is thrown as a `UiError`. */
  answer(request: ClinicRequest): ClinicReply {
    // The demo families are written on the first request, when the demo cases already exist.
    if (!this.seeded) {
      this.seeded = true;
      if (this.demo) this.seed(localToday());
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
        return { kind: "history", data: this.history.get(r.record_id) ?? [] };
      case "settings":
        return { kind: "settings", data: this.settings };
      case "save_settings":
        this.settings = this.checkedSettings(r.settings);
        return { kind: "settings", data: this.settings };
      case "message":
        return { kind: "text", data: this.message(r.kind, r.patient_id, r.visit_id) };
      case "case_choices":
        return { kind: "cases", data: this.caseChoices() };
      case "link_case":
        return { kind: "patient", data: this.linkCase(r.patient_id, r.case_id) };
      case "create_case":
        return { kind: "case", data: this.createCase(r.patient_id, this.today(r.today)) };
    }
  }

  // ---------------------------------------------------------------- small things

  private newId(prefix: string): string {
    return `${prefix}-${++this.seq}`;
  }

  private today(t: Today): Today {
    need(isIsoDate(t.date) && whole(t.minute, 0, 1439), "התאריך של היום לא התקבל כמו שצריך. כדאי לסגור את המסך ולפתוח אותו שוב.");
    return t;
  }

  private patient(id: string): Patient {
    return this.patients.find((p) => p.id === id) ?? refuse("המטופל לא נמצא. ייתכן שהכרטיס נמחק.", "not_found");
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

  private keep(recordId: string, action: "created" | "changed" | "voided" | "deleted", lines: string[]) {
    const entries = this.history.get(recordId) ?? [];
    entries.push({ at: Math.floor(Date.now() / 1000), action, lines });
    this.history.set(recordId, entries);
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
      ...(v.kind === "block" ? [] : [v.fee === 0 ? "מחיר: ללא חיוב" : `מחיר: ${plainMoney(v.fee)} ₪`]),
      ...(v.charge_missed ? ["חיוב גם אם לא התקיימה: כן"] : []),
      ...(v.series_id !== null ? ["פגישה חוזרת: כן"] : []),
      ...field("הערה", v.note),
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

  /** The formal record as it was saved. Of the personal note only that there is one: it is
   *  hers alone, and a history she may show someone never carries its words. */
  private noteLines(n: SessionNote): string[] {
    const v = n.visit_id === null ? null : (this.visits.find((x) => x.id === n.visit_id) ?? null);
    return [
      ...field("מטופל", this.nameOf(n.patient_id) ?? ""),
      ...field("תאריך", formatDate(n.date)),
      ...field("פגישה", v ? VISIT_KIND_LABEL[v.kind] : ""),
      n.record === "" ? "רשומה: ריקה" : `רשומה: ${n.record}`,
      ...(n.personal === "" ? [] : ["תרשומת אישית: יש"]),
      ...(n.case_input_id !== null ? ["הועברה לחומרי התיק: כן"] : []),
    ];
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
    }
    p.case_id = caseId;
    return p;
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
    const roles = { mother: "mother", father: "father", guardian: "relative", other: "other" } as const;
    const seen = new Set([p.name]);
    const people: IdentityInput[] = [];
    for (const c of p.contacts) {
      if (c.name === "" || seen.has(c.name)) continue;
      seen.add(c.name);
      people.push({ id: null, role: roles[c.role], value: c.name, aliases: [] });
    }
    p.case_id = this.host.createCase(
      { code, age: ageOn(p.birth_date, today.date), child_gender: p.gender, current_section: null, retention_until: null, consent: null },
      [{ id: null, role: "child", value: p.name, aliases: [] }, ...people],
    );
    return this.caseLink(p) ?? refuse("התיק לא נפתח. אפשר לנסות שוב.");
  }

  private noteToCase(id: string): SessionNote {
    const n = this.note(id);
    const p = this.patient(n.patient_id);
    const c = this.caseOf(p);
    need(c !== null, "למטופל עוד אין תיק אבחון. אפשר לפתוח תיק מהכרטיס, ואז להעביר את הסיכום.");
    need(!c.inTrash, "תיק האבחון נמצא בסל המחזור. אפשר לשחזר אותו ברשימת התיקים, ואז להעביר.");
    need(n.record.trim() !== "", "הרשומה ריקה, ואין מה להעביר לתיק. התרשומת האישית נשארת רק אצלך.");
    const v = n.visit_id === null ? null : (this.visits.find((x) => x.id === n.visit_id) ?? null);
    const kind: InputKind = v?.kind === "intake" ? "intake" : v?.kind === "observation" ? "observation" : "session_note";
    const title = `סיכום ${v ? VISIT_KIND_LABEL[v.kind] : "שיחה"} · ${formatDate(n.date)}`;
    // Only the formal record: the personal note never leaves the clinic.
    if (n.case_input_id === null || !this.host.updateInput(c.id, n.case_input_id, title, n.record)) {
      n.case_input_id = this.host.addInput(c.id, kind, title, n.record);
      this.keep(n.id, "changed", this.noteLines(n));
    }
    return n;
  }

  // ---------------------------------------------------------------- patients

  private savePatient(input: Patient): Patient {
    const name = cleanLine(input.name);
    need(name !== "", "צריך לכתוב את שם הילד או הילדה.");
    atMost(name, NAME, "שם");
    const birth = blankToNull(input.birth_date);
    if (birth !== null) realDate(birth, "תאריך לידה");
    need(input.gender === null || input.gender === "male" || input.gender === "female", "המין שנבחר לא מוכר.");
    // A contact with nothing in it is dropped, not refused: an empty row of the form.
    const contacts = input.contacts
      .map((c): Contact => ({ role: c.role, name: cleanLine(c.name), phone: cleanLine(c.phone), email: c.email.trim() }))
      .filter((c) => c.name !== "" || c.phone !== "" || c.email !== "");
    need(contacts.length <= MAX_CONTACTS, `אפשר לשמור עד ${MAX_CONTACTS} אנשי קשר למטופל.`);
    for (const c of contacts) {
      need(c.role in CONTACT_ROLE_LABEL, "סוג איש הקשר לא מוכר.");
      atMost(c.name, NAME, "שם איש הקשר");
      short(c.phone, "טלפון");
      short(c.email, "מייל");
    }
    const framework = cleanLine(input.framework);
    const referredBy = cleanLine(input.referred_by);
    short(framework, "מסגרת חינוכית");
    short(referredBy, "גורם מפנה");
    long(input.reason.trim(), "סיבת הפנייה");
    long(input.notes.trim(), "הערות");
    need(["waiting", "active", "closed"].includes(input.status), "המצב של המטופל לא מוכר.");
    const waitingSince = blankToNull(input.waiting_since);
    if (waitingSince !== null) realDate(waitingSince, "ממתינים מאז");
    need(input.session_fee === null || whole(input.session_fee, 0, MAX_AGOROT), "מחיר הפגישה צריך להיות בין 0 ל-100,000 ₪.");
    const old = input.id === "" ? null : this.patient(input.id);
    const created = old ? old.created_on : input.created_on.trim();
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
    if (old) Object.assign(old, saved);
    else this.patients.push(saved);
    return old ?? saved;
  }

  /** The day from which a card that holds a record may be erased (D-174): seven years after
   *  the last thing that happened in it, or when the child turns 25, whichever is later. */
  private keptUntil(p: Patient): string {
    const dates = [
      p.created_on,
      ...this.visits.filter((v) => v.patient_id === p.id).map((v) => v.date),
      ...[...this.notes, ...this.payments, ...this.charges].filter((r) => r.patient_id === p.id).map((r) => r.date),
    ].filter(isIsoDate).sort();
    const afterLast = addYears(dates[dates.length - 1] ?? p.created_on, KEEP_YEARS_AFTER_LAST_CHANGE);
    const adult = p.birth_date !== null && isIsoDate(p.birth_date) ? addYears(p.birth_date, KEEP_UNTIL_AGE) : afterLast;
    return adult > afterLast ? adult : afterLast;
  }

  /** Something happened in the card: a visit that was held or charged, a session note, a
   *  charge or a payment (a voided one too: it is part of the income diary). */
  private hasRecord(id: string): boolean {
    return this.visits.some((v) => v.patient_id === id && (v.status === "held" || visitCharge(v) > 0))
      || [...this.notes, ...this.payments, ...this.charges].some((r) => r.patient_id === id);
  }

  private erasePatient(id: string, typed: string) {
    const p = this.patient(id);
    // The request carries no "today"; the core uses its own clock here, the stand-in the page's.
    const today = localToday().date;
    if (this.hasRecord(id)) {
      const until = this.keptUntil(p);
      need(today >= until, `בכרטיס הזה יש פגישות, סיכומים או תשלומים, ורשומה כזו שומרים עד ${formatDate(until)}. עד אז אי אפשר למחוק אותו. אפשר לסגור את הכרטיס: הוא יוצא מהרשימה הפעילה ונשאר שמור בכספת.`);
    }
    need(cleanLine(typed) === p.name, "השם שהוקלד שונה משם המטופל, ולכן שום דבר לא נמחק.");
    const gone = [...this.visits, ...this.notes, ...this.payments, ...this.charges].filter((r) => r.patient_id === id).map((r) => r.id);
    for (const record of gone) this.history.delete(record);
    this.visits = this.visits.filter((v) => v.patient_id !== id);
    this.notes = this.notes.filter((n) => n.patient_id !== id);
    this.payments = this.payments.filter((x) => x.patient_id !== id);
    this.charges = this.charges.filter((x) => x.patient_id !== id);
    this.tasks = this.tasks.filter((t) => t.patient_id !== id);
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

  /** The visit as it is stored, checked in the core's order (`normalize_visit`, `validate_visit`). */
  private checkedVisit(input: Visit): Visit {
    const v: Visit = {
      ...input, patient_id: blankToNull(input.patient_id), title: cleanLine(input.title), date: input.date.trim(), place: cleanLine(input.place),
      status_note: cleanLine(input.status_note), series_id: blankToNull(input.series_id), note: input.note.trim(),
    };
    need(VISIT_KINDS.includes(v.kind), "סוג הפגישה לא מוכר.");
    need(["planned", "held", "cancelled", "no_show"].includes(v.status), "מצב הפגישה לא מוכר.");
    if (v.kind === "block") {
      need(v.patient_id === null, "זמן שמור הוא בלי מטופל. כדי לקבוע פגישה עם מטופל צריך לבחור סוג פגישה אחר.");
      need(v.fee === 0, "לזמן שמור אין מחיר.");
    } else {
      need(v.patient_id !== null && this.patients.some((p) => p.id === v.patient_id), "צריך לבחור מטופל לפגישה.");
    }
    realDate(v.date, "תאריך");
    need(whole(v.minutes, 5, 720), "משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
    need(whole(v.start, 0, 1439) && v.start + v.minutes <= 1440, "הפגישה צריכה להסתיים עד חצות. אפשר לקצר אותה או להקדים את שעת ההתחלה.");
    need(whole(v.fee, 0, MAX_AGOROT), "מחיר הפגישה צריך להיות בין 0 ל-100,000 ₪.");
    short(v.title, "כותרת");
    short(v.place, "מקום");
    short(v.status_note, "מה קרה");
    long(v.note, "הערה");
    // "Charged anyway" has a meaning only for a session that did not take place.
    if (v.status !== "cancelled" && v.status !== "no_show") v.charge_missed = false;
    return v;
  }

  private saved(written: Visit[]): SavedVisits {
    const visits = [...written].sort(byDateTime).map((v) => this.view(v));
    return { visits, overlapping: visits.filter((v) => v.overlaps).length };
  }

  private saveVisit(input: Visit, repeat: Repeat | null, scope: Scope): SavedVisits {
    const v = this.checkedVisit(input);
    need(scope === "one" || scope === "following", "לא ברור על אילו פגישות השינוי חל.");
    // Scheduling a family from the waiting list brings them in.
    const patient = v.patient_id === null ? null : this.patient(v.patient_id);
    if (v.id === "") {
      let dates = [v.date];
      let series: string | null = null;
      if (repeat !== null) {
        need(whole(repeat.every_weeks, 1, 8), "פגישה חוזרת יכולה לחזור כל שבוע ועד כל 8 שבועות.");
        need(whole(repeat.count, 2, 52), "סדרה של פגישות חוזרות היא בין 2 ל-52 פגישות.");
        dates = Array.from({ length: repeat.count }, (_, i) => addDays(v.date, 7 * repeat.every_weeks * i));
        series = this.newId("series");
      }
      // A new visit is always planned: what happened in it is marked afterwards.
      const written = dates.map((date): Visit => ({ ...v, id: this.newId("visit"), date, status: "planned", status_note: "", charge_missed: false, series_id: series }));
      for (const w of written) {
        this.visits.push(w);
        this.keep(w.id, "created", this.visitLines(w));
      }
      if (patient?.status === "waiting") patient.status = "active";
      return this.saved(written);
    }
    need(repeat === null, "אפשר לקבוע חזרה רק כשקובעים פגישה חדשה.");
    const old = this.visit(v.id);
    const written = [old];
    if (scope === "following" && old.series_id !== null) {
      const moved = daysBetween(old.date, v.date);
      for (const o of this.laterOfSeries(old)) {
        // What a series shares; the status and the patient of each visit stay its own.
        Object.assign(o, { kind: v.kind, title: v.title, start: v.start, minutes: v.minutes, place: v.place, fee: v.fee, note: v.note, date: addDays(o.date, moved) });
        this.keep(o.id, "changed", this.visitLines(o));
        written.push(o);
      }
    }
    Object.assign(old, v, { series_id: old.series_id });
    this.keep(old.id, "changed", this.visitLines(old));
    if (patient?.status === "waiting") patient.status = "active";
    return this.saved(written);
  }

  /** The visits of the same series that come after this one and are still planned. */
  private laterOfSeries(v: Visit): Visit[] {
    return this.visits
      .filter((o) => o.series_id === v.series_id && o.id !== v.id && o.status === "planned" && (o.date > v.date || (o.date === v.date && o.start > v.start)))
      .sort(byDateTime);
  }

  private deleteVisit(id: string, scope: Scope) {
    const v = this.visit(id);
    need(v.status === "planned", "אפשר למחוק רק פגישה שעדיין מתוכננת. פגישה שכבר סומנה אפשר לסמן כמבוטלת.");
    const gone = [v];
    if (scope === "following" && v.series_id !== null) {
      gone.push(...this.laterOfSeries(v));
    }
    const ids = new Set(gone.map((g) => g.id));
    for (const g of gone) this.keep(g.id, "deleted", this.visitLines(g));
    this.visits = this.visits.filter((x) => !ids.has(x.id));
    // A note written for a visit that is gone stays, as a note on its own.
    for (const n of this.notes) {
      if (n.visit_id === null || !ids.has(n.visit_id)) continue;
      n.visit_id = null;
      this.keep(n.id, "changed", this.noteLines(n));
    }
  }

  // ---------------------------------------------------------------- notes, payments, charges, tasks

  private saveNote(input: SessionNote): SessionNote {
    need(input.patient_id.trim() !== "", "צריך לבחור מטופל.");
    const p = this.patient(input.patient_id);
    const note = { ...input, visit_id: blankToNull(input.visit_id), date: input.date.trim(), record: input.record.trim(), personal: input.personal.trim() };
    realDate(note.date, "תאריך");
    need(note.record !== "" || note.personal !== "", "הסיכום ריק. צריך לכתוב משהו ברשומה או בתרשומת האישית.");
    atMost(note.record, RECORD, "רשומה");
    atMost(note.personal, RECORD, "תרשומת אישית");
    if (note.visit_id !== null) {
      const v = this.visit(note.visit_id);
      need(v.patient_id === p.id, "הפגישה שייכת למטופל אחר.");
      // One note for a visit: a second one would hide the first.
      need(!this.notes.some((n) => n.visit_id === v.id && n.id !== note.id), "לפגישה הזו כבר יש סיכום. אפשר לפתוח ולערוך אותו.");
    }
    const old = note.id === "" ? null : this.note(note.id);
    need(old === null || old.patient_id === p.id, "אי אפשר להעביר סיכום למטופל אחר.");
    const saved: SessionNote = {
      id: old ? old.id : this.newId("note"),
      patient_id: p.id,
      visit_id: note.visit_id,
      date: note.date,
      record: note.record,
      personal: note.personal,
      // Set only by "להעביר לחומרי התיק".
      case_input_id: old ? old.case_input_id : null,
    };
    if (old) Object.assign(old, saved);
    else this.notes.push(saved);
    // The clinical record: every saved state of it is kept (D-172).
    this.keep(saved.id, old ? "changed" : "created", this.noteLines(saved));
    return old ?? saved;
  }

  /** A note that has text is part of the record and stays (she can correct it; every state is
   *  kept). Only one that holds nothing can be removed. */
  private deleteNote(id: string) {
    const n = this.note(id);
    need(n.record.trim() === "" && n.personal.trim() === "", "סיכום שנכתב בו משהו הוא חלק מהרשומה, ולכן הוא לא נמחק. אפשר לפתוח אותו ולתקן, וכל גרסה נשמרת בהיסטוריה.");
    this.keep(n.id, "deleted", this.noteLines(n));
    this.notes = this.notes.filter((x) => x.id !== id);
  }

  private savePayment(input: Payment): Payment {
    const p = this.patient(input.patient_id);
    realDate(input.date.trim(), "תאריך");
    need(whole(input.amount, 1, MAX_AGOROT), "הסכום צריך להיות גדול מאפס, ועד 100,000 ₪.");
    need(input.method in PAY_METHOD_LABEL, "אמצעי התשלום לא מוכר.");
    short(cleanLine(input.receipt), "מספר קבלה");
    short(cleanLine(input.payer), "מי שילם");
    long(input.note.trim(), "הערה");
    const old = input.id === "" ? null : (this.payments.find((x) => x.id === input.id) ?? refuse("התקבול לא נמצא.", "not_found"));
    need(old === null || !old.voided, "התקבול הזה בוטל, ואי אפשר לשנות אותו. אפשר לרשום תקבול חדש.");
    need(old === null || old.patient_id === p.id, "אי אפשר להעביר תקבול למטופל אחר. אפשר לבטל אותו ולרשום מחדש.");
    const saved: Payment = {
      id: old ? old.id : this.newId("payment"), patient_id: p.id, date: input.date.trim(), amount: input.amount, method: input.method,
      receipt: cleanLine(input.receipt), payer: cleanLine(input.payer), note: input.note.trim(), voided: false, void_reason: "",
    };
    if (old) Object.assign(old, saved);
    else this.payments.push(saved);
    this.keep(saved.id, old ? "changed" : "created", this.paymentLines(saved));
    return old ?? saved;
  }

  private voidPayment(id: string, reason: string): Payment {
    const p = this.payments.find((x) => x.id === id) ?? refuse("התקבול לא נמצא.", "not_found");
    need(!p.voided, "התקבול הזה כבר בוטל.");
    short(cleanLine(reason), "סיבת הביטול");
    need(cleanLine(reason) !== "", "צריך לכתוב למה הרישום מבוטל.");
    p.voided = true;
    p.void_reason = cleanLine(reason);
    this.keep(p.id, "voided", this.paymentLines(p));
    return p;
  }

  private saveCharge(input: Charge): Charge {
    const p = this.patient(input.patient_id);
    realDate(input.date.trim(), "תאריך");
    need(whole(input.amount, 1, MAX_AGOROT), "הסכום צריך להיות גדול מאפס, ועד 100,000 ₪.");
    need(cleanLine(input.what) !== "", "צריך לכתוב על מה החיוב.");
    short(cleanLine(input.what), "על מה החיוב");
    const old = input.id === "" ? null : (this.charges.find((x) => x.id === input.id) ?? refuse("החיוב לא נמצא.", "not_found"));
    need(old === null || !old.voided, "החיוב הזה בוטל, ואי אפשר לשנות אותו. אפשר לרשום חיוב חדש.");
    need(old === null || old.patient_id === p.id, "אי אפשר להעביר חיוב למטופל אחר. אפשר לבטל אותו ולרשום מחדש.");
    const saved: Charge = { id: old ? old.id : this.newId("charge"), patient_id: p.id, date: input.date.trim(), amount: input.amount, what: cleanLine(input.what), voided: false, void_reason: "" };
    if (old) Object.assign(old, saved);
    else this.charges.push(saved);
    this.keep(saved.id, old ? "changed" : "created", this.chargeLines(saved));
    return old ?? saved;
  }

  private voidCharge(id: string, reason: string): Charge {
    const c = this.charges.find((x) => x.id === id) ?? refuse("החיוב לא נמצא.", "not_found");
    need(!c.voided, "החיוב הזה כבר בוטל.");
    short(cleanLine(reason), "סיבת הביטול");
    need(cleanLine(reason) !== "", "צריך לכתוב למה הרישום מבוטל.");
    c.voided = true;
    c.void_reason = cleanLine(reason);
    this.keep(c.id, "voided", this.chargeLines(c));
    return c;
  }

  private saveTask(input: ClinicTask): ClinicTask {
    const task: ClinicTask = {
      ...input, patient_id: blankToNull(input.patient_id), title: cleanLine(input.title), due: blankToNull(input.due),
      done_on: blankToNull(input.done_on), note: input.note.trim(),
    };
    need(task.title !== "", "צריך לכתוב מה המשימה.");
    short(task.title, "משימה");
    if (task.due !== null) realDate(task.due, "תאריך יעד");
    if (task.done_on !== null) realDate(task.done_on, "תאריך ביצוע");
    long(task.note, "הערה");
    need(["call", "questionnaire", "document", "report", "payment", "follow_up", "other"].includes(task.kind), "סוג המשימה לא מוכר.");
    if (task.patient_id !== null) this.patient(task.patient_id);
    const old = task.id === "" ? null : (this.tasks.find((t) => t.id === task.id) ?? refuse("המשימה לא נמצאה. ייתכן שכבר נמחקה.", "not_found"));
    const saved: ClinicTask = { ...task, id: old ? old.id : this.newId("task") };
    if (old) Object.assign(old, saved);
    else this.tasks.push(saved);
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
    return s;
  }

  // ---------------------------------------------------------------- messages

  private message(kind: MessageKind, patientId: string, visitId: string | null): string {
    const p = this.patient(patientId);
    const v = visitId === null ? null : this.visit(visitId);
    need(v === null || v.patient_id === p.id, "הפגישה שייכת למטופל אחר.");
    const due = this.balance(p.id).due;
    if (kind === "reminder") need(v !== null, "כדי להכין תזכורת צריך לבחור פגישה.");
    else need(due > 0, "אין יתרה לתשלום, ולכן אין מה לשלוח.");
    return fillMessage(kind === "reminder" ? this.settings.reminder_text : this.settings.payment_text, {
      // No name of a parent on the card: "שלום רב".
      "{הורה}": p.contacts.map((c) => cleanLine(c.name)).find((name) => name !== "") ?? "רב",
      "{ילד}": p.name,
      "{יום}": v ? dayName(v.date) : "",
      "{תאריך}": v ? formatDate(v.date) : "",
      "{שעה}": v ? formatTime(v.start) : "",
      "{מקום}": v ? v.place : "",
      "{סכום}": plainMoney(kind === "reminder" && v ? v.fee : due),
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
        if (v.status === "planned" && v.kind !== "block") {
          if (v.date < today.date || (v.date === today.date && v.start + v.minutes <= today.minute)) {
            add("unmarked", p, `הפגישה מיום ${when} עדיין מסומנת כמתוכננת. אפשר לסמן אם התקיימה.`, v.date, { visit: v.id });
          }
          const ahead = daysBetween(today.date, v.date);
          if (v.kind === "feedback" && ahead >= 0 && ahead <= FEEDBACK_SOON_DAYS && !this.reportFinished(p)) {
            add("feedback_soon", p, `שיחת המשוב נקבעה ליום ${when}, ו${reportWords(p)}.`, v.date, { visit: v.id });
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
    const rank: Record<Alert["kind"], number> = { unmarked: 0, no_note: 1, task_overdue: 2, feedback_soon: 3, report_due: 4, debt: 5, no_next_visit: 6, waiting: 7 };
    const text = (a: string | null, b: string | null) => ((a ?? "") < (b ?? "") ? -1 : (a ?? "") > (b ?? "") ? 1 : 0);
    return out.sort((a, b) => text(a.since, b.since) || rank[a.kind] - rank[b.kind] || text(a.patient_id, b.patient_id) || text(a.visit_id, b.visit_id) || text(a.task_id, b.task_id));
  }

  // ---------------------------------------------------------------- the front page

  private overview(today: Today): Overview {
    const on = (date: string) => this.visits.filter((v) => v.date === date).sort(byDateTime);
    const todays = on(today.date);
    // A visit with a patient that was not cancelled; time she kept for herself is not one.
    const counts = (v: Visit) => v.kind !== "block" && v.status !== "cancelled";
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
      alerts: this.alerts(today),
      week_visits: this.visits.filter((v) => counts(v) && v.date >= week.from && v.date <= week.to).length,
      month_received: this.payments.filter((p) => !p.voided && monthOf(p.date) === month).reduce((sum, p) => sum + p.amount, 0),
      total_due: this.patients.reduce((sum, p) => sum + Math.max(0, this.balance(p.id).due), 0),
      waiting: this.patients.filter((p) => p.status === "waiting").length,
      active: this.patients.filter((p) => p.status === "active").length,
    };
  }

  // ---------------------------------------------------------------- the diary and income

  private changed(recordId: string): boolean {
    return (this.history.get(recordId)?.length ?? 0) > 1;
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
    const byMethod = PAY_METHODS
      .map((method: PayMethod) => ({ method, amount: live.filter((p) => p.method === method).reduce((sum, p) => sum + p.amount, 0) }))
      .filter((m) => m.amount > 0);
    return {
      month,
      days,
      charged: days.reduce((sum, d) => sum + d.charged, 0),
      received: days.reduce((sum, d) => sum + d.received, 0),
      by_method: byMethod,
      missing_receipts: live.filter((p) => p.receipt === "").length,
    };
  }

  private income(year: number): IncomeYear {
    need(whole(year, 2000, 2200), "צריך לבחור שנה.");
    const sessions = this.visits.filter((v) => v.patient_id !== null && v.date.startsWith(`${year}-`));
    const count = (list: Visit[], status: VisitStatus) => list.filter((v) => v.status === status).length;
    const months: IncomeMonth[] = Array.from({ length: 12 }, (_, i) => {
      const month = `${year}-${String(i + 1).padStart(2, "0")}`;
      const of = sessions.filter((v) => monthOf(v.date) === month);
      return {
        month,
        held: count(of, "held"),
        cancelled: count(of, "cancelled"),
        no_show: count(of, "no_show"),
        charged: of.reduce((sum, v) => sum + visitCharge(v), 0)
          + this.charges.filter((c) => !c.voided && monthOf(c.date) === month).reduce((sum, c) => sum + c.amount, 0),
        received: this.payments.filter((p) => !p.voided && monthOf(p.date) === month).reduce((sum, p) => sum + p.amount, 0),
      };
    });
    const byKind = VISIT_KINDS
      .map((kind) => ({ kind, count: sessions.filter((v) => v.kind === kind && v.status === "held").length }))
      .filter((k) => k.count > 0)
      .sort((a, b) => b.count - a.count);
    return {
      year,
      months,
      charged: months.reduce((sum, m) => sum + m.charged, 0),
      received: months.reduce((sum, m) => sum + m.received, 0),
      new_patients: this.patients.filter((p) => p.created_on.startsWith(`${year}-`)).length,
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
        status_note: "", fee: kind === "block" ? 0 : (p?.session_fee ?? fees[kind] ?? 0), charge_missed: false, series_id: null, note: "", ...over,
      };
      this.visits.push(v);
      this.keep(v.id, "created", this.visitLines(v));
      return v;
    };
    const note = (v: Visit, record: string, personal = "") => {
      const n: SessionNote = { id: this.newId("note"), patient_id: v.patient_id ?? "", visit_id: v.id, date: v.date, record, personal, case_input_id: null };
      this.notes.push(n);
      this.keep(n.id, "created", this.noteLines(n));
    };
    const paid = (p: Patient, at: number, amount: number, method: PayMethod, receipt: string) => {
      const x: Payment = { id: this.newId("payment"), patient_id: p.id, date: day(at), amount, method, receipt, payer: "", note: "", voided: false, void_reason: "" };
      this.payments.push(x);
      this.keep(x.id, "created", this.paymentLines(x));
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
    this.keep(report.id, "created", this.chargeLines(report));
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
