// Browser preview and tests only: an in-memory stand-in for the clinic (docs/CLINIC.md), with
// the same rules the core keeps: validation, series, overlaps, charges and balances, alerts,
// the assessment process, the diary, income, history and the messages. Every patient, phone
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
  addDays, CONTACT_ROLE_LABEL, dayName, daysBetween, formatDate, formatDateLong, formatDateShort, formatMoney, formatSpan,
  formatTime, isIsoDate, MAX_AGOROT, moneyNumber, monthOf, monthRange, PAY_METHOD_LABEL, PAY_METHODS, today as localToday,
  VISIT_KIND_LABEL, VISIT_KINDS, VISIT_STATUS_LABEL, visitTitle, weekRange,
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
const short = (s: string, what: string) => need(chars(s) <= SHORT, `${what}: עד ${SHORT} תווים.`);
const long = (s: string, what: string) => need(chars(s) <= LONG, `${what}: עד 4,000 תווים.`);
const whole = (n: unknown, min: number, max: number): n is number => typeof n === "number" && Number.isInteger(n) && n >= min && n <= max;
const copy = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

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

/** Age on `on` of a child born on `birth`; `null` when the dates are out of order. */
function ageOn(birth: string | null, on: string): Age | null {
  if (!birth || !isIsoDate(birth) || !isIsoDate(on)) return null;
  const [by = 0, bm = 0, bd = 0] = birth.split("-").map(Number);
  const [oy = 0, om = 0, od = 0] = on.split("-").map(Number);
  let months = (oy - by) * 12 + (om - bm);
  if (od < bd) months -= 1;
  return months < 0 ? null : { years: Math.floor(months / 12), months: months % 12 };
}

const byDateTime = (a: Visit, b: Visit) => a.date.localeCompare(b.date) || a.start - b.start || a.id.localeCompare(b.id);

/** What a visit costs the family: its fee when it was held, or when it was missed and she
 *  charges anyway; nothing otherwise. A block never costs. */
export function visitCharge(v: Visit): number {
  if (v.kind === "block") return 0;
  if (v.status === "held") return v.fee;
  return (v.status === "cancelled" || v.status === "no_show") && v.charge_missed ? v.fee : 0;
}

/** Two visits overlap when neither is cancelled, they are on the same date and their times meet. */
export function visitsOverlap(a: Visit, b: Visit): boolean {
  if (a.id === b.id || a.status === "cancelled" || b.status === "cancelled" || a.date !== b.date) return false;
  return a.start < b.start + b.minutes && b.start < a.start + a.minutes;
}

/** A message text with its placeholders filled in. */
export function fillMessage(text: string, values: Record<(typeof PLACEHOLDERS)[number], string>): string {
  return PLACEHOLDERS.reduce((out, key) => out.split(key).join(values[key]), text);
}

// ------------------------------------------------------------------ the clinic

export class FakeClinic {
  private patients: Patient[] = [];
  private visits: Visit[] = [];
  private notes: SessionNote[] = [];
  private payments: Payment[] = [];
  private charges: Charge[] = [];
  private tasks: ClinicTask[] = [];
  private settings: ClinicSettings = defaultClinicSettings();
  /** Every saved state of a visit, a charge or a payment, oldest first, by record id. */
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
        need(isIsoDate(r.from) && isIsoDate(r.to), "טווח התאריכים לא תקין.");
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
        this.note(r.id);
        this.notes = this.notes.filter((n) => n.id !== r.id);
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
    need(isIsoDate(t.date) && whole(t.minute, 0, 1439), "התאריך של היום לא תקין.");
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

  private visitLines(v: Visit): string[] {
    const name = this.nameOf(v.patient_id);
    return [
      v.title.trim() ? `${VISIT_KIND_LABEL[v.kind]} · ${v.title.trim()}` : VISIT_KIND_LABEL[v.kind],
      ...(name ? [`מטופל: ${name}`] : []),
      `מועד: ${formatDateLong(v.date)}, ${formatSpan(v.start, v.minutes)}`,
      ...(v.place ? [`מקום: ${v.place}`] : []),
      `מצב: ${VISIT_STATUS_LABEL[v.status]}${v.status_note ? ` (${v.status_note})` : ""}`,
      ...(v.kind === "block" ? [] : [`מחיר: ${formatMoney(v.fee)}${v.charge_missed ? " · מחויבת למרות שלא התקיימה" : ""}`]),
      ...(v.note ? [`הערה: ${v.note}`] : []),
    ];
  }

  private paymentLines(p: Payment): string[] {
    return [
      `תקבול: ${formatMoney(p.amount)} · ${PAY_METHOD_LABEL[p.method]}`,
      `מטופל: ${this.nameOf(p.patient_id) ?? ""}`,
      `תאריך: ${formatDate(p.date)}`,
      ...(p.receipt ? [`מספר קבלה: ${p.receipt}`] : []),
      ...(p.payer ? [`שילם: ${p.payer}`] : []),
      ...(p.note ? [`הערה: ${p.note}`] : []),
      ...(p.voided ? [`בוטל: ${p.void_reason}`] : []),
    ];
  }

  private chargeLines(c: Charge): string[] {
    return [
      `חיוב: ${formatMoney(c.amount)} · ${c.what}`,
      `מטופל: ${this.nameOf(c.patient_id) ?? ""}`,
      `תאריך: ${formatDate(c.date)}`,
      ...(c.voided ? [`בוטל: ${c.void_reason}`] : []),
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

  private reportFinished(p: Patient): boolean {
    const c = this.caseOf(p);
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
    const parent = (role: "mother" | "father"): IdentityInput[] => {
      const name = p.contacts.find((c) => c.role === role && c.name.trim())?.name.trim();
      return name ? [{ id: null, role, value: name, aliases: [] }] : [];
    };
    p.case_id = this.host.createCase(
      { code, age: ageOn(p.birth_date, today.date), child_gender: p.gender, current_section: null, retention_until: null, consent: null },
      [{ id: null, role: "child", value: p.name, aliases: [] }, ...parent("mother"), ...parent("father")],
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
    }
    return n;
  }

  // ---------------------------------------------------------------- patients

  private checkedContact(c: Contact): Contact {
    const out: Contact = { role: c.role, name: c.name.trim(), phone: c.phone.trim(), email: c.email.trim() };
    need(out.role in CONTACT_ROLE_LABEL, "סוג איש הקשר לא מוכר.");
    need(out.name !== "" || out.phone !== "" || out.email !== "", "איש קשר ריק. אפשר למלא שם או טלפון, או להסיר אותו.");
    short(out.name, "שם איש הקשר");
    short(out.phone, "טלפון");
    short(out.email, "דואר אלקטרוני");
    return out;
  }

  private savePatient(input: Patient): Patient {
    const name = input.name.trim();
    need(name !== "", "צריך את שם הילד/ה.");
    need(chars(name) <= 120, "השם ארוך מדי: עד 120 תווים.");
    need(input.birth_date === null || isIsoDate(input.birth_date), "תאריך הלידה לא תקין.");
    need(input.gender === null || input.gender === "male" || input.gender === "female", "המין לא מוכר.");
    need(input.contacts.length <= MAX_CONTACTS, `אפשר לשמור עד ${MAX_CONTACTS} אנשי קשר.`);
    short(input.framework.trim(), "המסגרת החינוכית");
    short(input.referred_by.trim(), "הגורם המפנה");
    long(input.reason, "סיבת הפנייה");
    long(input.notes, "ההערות");
    need(["waiting", "active", "closed"].includes(input.status), "המצב של המטופל לא מוכר.");
    need(input.waiting_since === null || isIsoDate(input.waiting_since), "התאריך של תחילת ההמתנה לא תקין.");
    need(input.session_fee === null || whole(input.session_fee, 0, MAX_AGOROT), "המחיר לפגישה צריך להיות בין 0 ל-100,000 ₪.");
    const old = input.id === "" ? null : this.patient(input.id);
    need(old !== null || isIsoDate(input.created_on), "תאריך פתיחת הכרטיס לא תקין.");
    const created = old ? old.created_on : input.created_on;
    const saved: Patient = {
      id: old ? old.id : this.newId("patient"),
      name,
      birth_date: input.birth_date,
      gender: input.gender,
      contacts: input.contacts.map((c) => this.checkedContact(c)),
      framework: input.framework.trim(),
      referred_by: input.referred_by.trim(),
      reason: input.reason.trim(),
      notes: input.notes.trim(),
      status: input.status,
      // A family on the list has a day it joined: the day the card was opened, unless she said otherwise.
      waiting_since: input.waiting_since ?? (input.status === "waiting" ? created : null),
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

  private erasePatient(id: string, typed: string) {
    const p = this.patient(id);
    need(typed.trim() === p.name, "השם שהוקלד שונה משם המטופל, ולכן שום דבר לא נמחק.");
    const gone = [...this.visits, ...this.payments, ...this.charges].filter((r) => r.patient_id === id).map((r) => r.id);
    for (const record of gone) this.history.delete(record);
    this.visits = this.visits.filter((v) => v.patient_id !== id);
    this.notes = this.notes.filter((n) => n.patient_id !== id);
    this.payments = this.payments.filter((x) => x.patient_id !== id);
    this.charges = this.charges.filter((x) => x.patient_id !== id);
    this.tasks = this.tasks.filter((t) => t.patient_id !== id);
    // The case, if there is one, is not the clinic's to erase.
    this.patients = this.patients.filter((x) => x.id !== id);
  }

  private nextVisit(patientId: string, today: Today): Visit | undefined {
    return this.visits
      .filter((v) => v.patient_id === patientId && v.status === "planned" && (v.date > today.date || (v.date === today.date && v.start + v.minutes > today.minute)))
      .sort(byDateTime)[0];
  }

  private rows(today: Today): PatientRow[] {
    return [...this.patients].sort((a, b) => a.name.localeCompare(b.name, "he")).map((p): PatientRow => {
      const next = this.nextVisit(p.id, today);
      const held = this.visits.filter((v) => v.patient_id === p.id && v.status === "held").sort(byDateTime);
      return {
        patient: p,
        age: ageOn(p.birth_date, today.date),
        next_visit: next ? { date: next.date, start: next.start, kind: next.kind } : null,
        last_visit: held[held.length - 1]?.date ?? null,
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
      tasks: this.tasks.filter((t) => t.patient_id === p.id),
      balance: this.balance(p.id),
      process: this.process(p, today),
      case: this.caseLink(p),
      alerts: this.alerts(today).filter((a) => a.patient_id === p.id),
    };
  }

  /** The assessment, step by step: what is behind her, the step she is at, and what is ahead. */
  private process(p: Patient, today: Today): ProcessStep[] {
    const of = (kind: VisitKind) => this.visits.filter((v) => v.patient_id === p.id && v.kind === kind && v.status !== "cancelled" && v.status !== "no_show").sort(byDateTime);
    const held = (list: Visit[]) => list.filter((v) => v.status === "held");
    const ahead = (list: Visit[]) => list.filter((v) => v.status === "planned" && v.date >= today.date);
    const single = (list: Visit[], none: string): { done: boolean; detail: string } => {
      const last = held(list).pop();
      if (last) return { done: true, detail: `התקיימה ב-${formatDate(last.date)}` };
      const next = ahead(list)[0];
      return { done: false, detail: next ? `נקבעה ל-${formatDate(next.date)}` : none };
    };
    const sessions = of("assessment");
    const observation = of("observation");
    const c = this.caseOf(p);
    const balance = this.balance(p.id);
    const steps: { key: string; label: string; done: boolean; detail: string }[] = [
      { key: "intake", label: "פגישת היכרות", ...single(of("intake"), "עוד לא נקבעה") },
      {
        key: "sessions", label: "מפגשי אבחון",
        done: held(sessions).length > 0 && ahead(sessions).length === 0,
        detail: sessions.length === 0 ? "עוד לא נקבעו מפגשים" : `${held(sessions).length} מתוך ${sessions.length} התקיימו`,
      },
      ...(observation.length > 0 ? [{ key: "observation", label: "תצפית", ...single(observation, "עוד לא נקבעה") }] : []),
      {
        key: "report", label: "הדוח",
        done: this.reportFinished(p),
        detail: c === null ? "עוד לא נפתח תיק אבחון" : c.inTrash ? "תיק האבחון בסל המחזור" : `הדוח: ${c.approved} מתוך ${c.total} סעיפים`,
      },
      { key: "feedback", label: "שיחת משוב", ...single(of("feedback"), "עוד לא נקבעה") },
      {
        key: "payment", label: "תשלום",
        done: balance.charged > 0 && balance.due <= 0,
        detail: balance.charged === 0 ? "עוד לא נרשם חיוב" : balance.due > 0 ? `נותרו לתשלום ${formatMoney(balance.due)}` : "שולם במלואו",
      },
    ];
    const now = steps.findIndex((s) => !s.done);
    return steps.map((s, i) => ({ key: s.key, label: s.label, state: s.done ? "done" : i === now ? "now" : "todo", detail: s.detail }));
  }

  // ---------------------------------------------------------------- visits

  private checkedVisit(input: Visit): Visit {
    const v: Visit = { ...input, title: input.title.trim(), place: input.place.trim(), status_note: input.status_note.trim(), note: input.note.trim() };
    need(VISIT_KINDS.includes(v.kind), "סוג הפגישה לא מוכר.");
    need(["planned", "held", "cancelled", "no_show"].includes(v.status), "מצב הפגישה לא מוכר.");
    need(isIsoDate(v.date), "תאריך הפגישה לא תקין.");
    need(whole(v.minutes, 5, 720), "אורך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
    need(whole(v.start, 0, 1439) && v.start + v.minutes <= 1440, "הפגישה צריכה להתחיל ולהסתיים באותו יום.");
    need(whole(v.fee, 0, MAX_AGOROT), "המחיר צריך להיות בין 0 ל-100,000 ₪.");
    short(v.title, "הכותרת");
    short(v.place, "המקום");
    short(v.status_note, "הסיבה");
    long(v.note, "ההערה");
    if (v.kind === "block") {
      need(v.patient_id === null && v.fee === 0, "חסימת זמן היא בלי מטופל ובלי מחיר.");
    } else {
      need(v.patient_id !== null && this.patients.some((p) => p.id === v.patient_id), "צריך לבחור מטופל לפגישה.");
    }
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
        need(whole(repeat.every_weeks, 1, 8), "פגישה חוזרת: כל שבוע עד כל 8 שבועות.");
        need(whole(repeat.count, 2, 52), "פגישה חוזרת: בין 2 ל-52 פגישות.");
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
    need((old.kind === "block") === (v.kind === "block"), "אי אפשר להפוך פגישה לחסימת זמן או להפך. אפשר למחוק אותה ולקבוע מחדש.");
    const written = [old];
    if (scope === "following" && old.series_id !== null) {
      const moved = daysBetween(old.date, v.date);
      const later = this.visits.filter((o) => o.series_id === old.series_id && o.id !== old.id && o.status === "planned" && o.date > old.date);
      for (const o of later) {
        // What a series shares; the status of each visit stays its own.
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

  private deleteVisit(id: string, scope: Scope) {
    const v = this.visit(id);
    need(v.status === "planned", "אפשר למחוק רק פגישה מתוכננת. פגישה שהתקיימה או בוטלה נשארת ביומן.");
    const gone = [v];
    if (scope === "following" && v.series_id !== null) {
      gone.push(...this.visits.filter((o) => o.series_id === v.series_id && o.id !== v.id && o.status === "planned" && o.date > v.date));
    }
    const ids = new Set(gone.map((g) => g.id));
    for (const g of gone) this.keep(g.id, "deleted", this.visitLines(g));
    this.visits = this.visits.filter((x) => !ids.has(x.id));
    // A note written for a visit that is gone stays, as a note on its own.
    for (const n of this.notes) if (n.visit_id !== null && ids.has(n.visit_id)) n.visit_id = null;
  }

  // ---------------------------------------------------------------- notes, payments, charges, tasks

  private saveNote(input: SessionNote): SessionNote {
    const p = this.patient(input.patient_id);
    need(isIsoDate(input.date), "תאריך הסיכום לא תקין.");
    need(chars(input.record) <= RECORD, "הרשומה ארוכה מדי: עד 20,000 תווים.");
    need(chars(input.personal) <= RECORD, "התרשומת האישית ארוכה מדי: עד 20,000 תווים.");
    need(input.record.trim() !== "" || input.personal.trim() !== "", "הסיכום ריק. אפשר לכתוב רשומה, תרשומת אישית, או שתיהן.");
    if (input.visit_id !== null) {
      const v = this.visit(input.visit_id);
      need(v.patient_id === p.id, "הפגישה שייכת למטופל אחר.");
      need(!this.notes.some((n) => n.visit_id === v.id && n.id !== input.id), "לפגישה הזו כבר יש סיכום. אפשר לפתוח ולערוך אותו.");
    }
    const old = input.id === "" ? null : this.note(input.id);
    need(old === null || old.patient_id === p.id, "אי אפשר להעביר סיכום למטופל אחר.");
    const saved: SessionNote = {
      id: old ? old.id : this.newId("note"),
      patient_id: p.id,
      visit_id: input.visit_id,
      date: input.date,
      record: input.record,
      personal: input.personal,
      // Set only by "להעביר לחומרי התיק".
      case_input_id: old ? old.case_input_id : null,
    };
    if (old) Object.assign(old, saved);
    else this.notes.push(saved);
    return old ?? saved;
  }

  private savePayment(input: Payment): Payment {
    const p = this.patient(input.patient_id);
    need(isIsoDate(input.date), "תאריך התקבול לא תקין.");
    need(whole(input.amount, 1, MAX_AGOROT), "הסכום צריך להיות גדול מאפס, ועד 100,000 ₪.");
    need(input.method in PAY_METHOD_LABEL, "אמצעי התשלום לא מוכר.");
    short(input.receipt.trim(), "מספר הקבלה");
    short(input.payer.trim(), "מי שילם");
    long(input.note, "ההערה");
    const old = input.id === "" ? null : (this.payments.find((x) => x.id === input.id) ?? refuse("התקבול לא נמצא.", "not_found"));
    need(old === null || !old.voided, "התקבול הזה בוטל, ואי אפשר לשנות אותו. אפשר לרשום תקבול חדש.");
    need(old === null || old.patient_id === p.id, "אי אפשר להעביר תקבול למטופל אחר. אפשר לבטל אותו ולרשום מחדש.");
    const saved: Payment = {
      id: old ? old.id : this.newId("payment"), patient_id: p.id, date: input.date, amount: input.amount, method: input.method,
      receipt: input.receipt.trim(), payer: input.payer.trim(), note: input.note.trim(), voided: false, void_reason: "",
    };
    if (old) Object.assign(old, saved);
    else this.payments.push(saved);
    this.keep(saved.id, old ? "changed" : "created", this.paymentLines(saved));
    return old ?? saved;
  }

  private voidPayment(id: string, reason: string): Payment {
    const p = this.payments.find((x) => x.id === id) ?? refuse("התקבול לא נמצא.", "not_found");
    need(!p.voided, "התקבול הזה כבר בוטל.");
    need(reason.trim() !== "", "צריך לכתוב למה התקבול מבוטל. הוא נשאר ביומן עם הסיבה.");
    short(reason.trim(), "סיבת הביטול");
    p.voided = true;
    p.void_reason = reason.trim();
    this.keep(p.id, "voided", this.paymentLines(p));
    return p;
  }

  private saveCharge(input: Charge): Charge {
    const p = this.patient(input.patient_id);
    need(isIsoDate(input.date), "תאריך החיוב לא תקין.");
    need(whole(input.amount, 1, MAX_AGOROT), "הסכום צריך להיות גדול מאפס, ועד 100,000 ₪.");
    need(input.what.trim() !== "", "צריך לכתוב על מה החיוב.");
    short(input.what.trim(), "על מה החיוב");
    const old = input.id === "" ? null : (this.charges.find((x) => x.id === input.id) ?? refuse("החיוב לא נמצא.", "not_found"));
    need(old === null || !old.voided, "החיוב הזה בוטל, ואי אפשר לשנות אותו. אפשר לרשום חיוב חדש.");
    need(old === null || old.patient_id === p.id, "אי אפשר להעביר חיוב למטופל אחר. אפשר לבטל אותו ולרשום מחדש.");
    const saved: Charge = { id: old ? old.id : this.newId("charge"), patient_id: p.id, date: input.date, amount: input.amount, what: input.what.trim(), voided: false, void_reason: "" };
    if (old) Object.assign(old, saved);
    else this.charges.push(saved);
    this.keep(saved.id, old ? "changed" : "created", this.chargeLines(saved));
    return old ?? saved;
  }

  private voidCharge(id: string, reason: string): Charge {
    const c = this.charges.find((x) => x.id === id) ?? refuse("החיוב לא נמצא.", "not_found");
    need(!c.voided, "החיוב הזה כבר בוטל.");
    need(reason.trim() !== "", "צריך לכתוב למה החיוב מבוטל. הוא נשאר ביומן עם הסיבה.");
    short(reason.trim(), "סיבת הביטול");
    c.voided = true;
    c.void_reason = reason.trim();
    this.keep(c.id, "voided", this.chargeLines(c));
    return c;
  }

  private saveTask(input: ClinicTask): ClinicTask {
    need(input.title.trim() !== "", "צריך לכתוב מה המשימה.");
    short(input.title.trim(), "המשימה");
    long(input.note, "ההערה");
    need(input.due === null || isIsoDate(input.due), "התאריך של המשימה לא תקין.");
    need(input.done_on === null || isIsoDate(input.done_on), "התאריך שבו המשימה בוצעה לא תקין.");
    need(["call", "questionnaire", "document", "report", "payment", "follow_up", "other"].includes(input.kind), "סוג המשימה לא מוכר.");
    if (input.patient_id !== null) this.patient(input.patient_id);
    const old = input.id === "" ? null : (this.tasks.find((t) => t.id === input.id) ?? refuse("המשימה לא נמצאה. ייתכן שכבר נמחקה.", "not_found"));
    const saved: ClinicTask = { ...input, id: old ? old.id : this.newId("task"), title: input.title.trim(), note: input.note.trim() };
    if (old) Object.assign(old, saved);
    else this.tasks.push(saved);
    return old ?? saved;
  }

  /** Open tasks: the overdue first, then by date, then the undated. With `openOnly` off, the
   *  done ones follow, the newest first. */
  private taskViews(today: Today, openOnly: boolean): TaskView[] {
    const view = (t: ClinicTask): TaskView => ({ task: t, patient_name: this.nameOf(t.patient_id), overdue: t.done_on === null && t.due !== null && t.due < today.date });
    const open = this.tasks.filter((t) => t.done_on === null).map(view).sort((a, b) =>
      Number(b.overdue) - Number(a.overdue) || (a.task.due ?? "9999").localeCompare(b.task.due ?? "9999"));
    if (openOnly) return open;
    const done = this.tasks.filter((t) => t.done_on !== null).map(view).sort((a, b) => (b.task.done_on ?? "").localeCompare(a.task.done_on ?? ""));
    return [...open, ...done];
  }

  // ---------------------------------------------------------------- settings

  private checkedSettings(s: ClinicSettings): ClinicSettings {
    need(whole(s.day_start, 0, 1439) && whole(s.day_end, 1, 1440) && s.day_start < s.day_end, "שעות היומן: שעת ההתחלה צריכה להיות לפני שעת הסיום.");
    need(s.work_days.every((d) => whole(d, 0, 6)) && new Set(s.work_days).size === s.work_days.length, "ימי העבודה לא תקינים.");
    need(s.defaults.length === VISIT_KINDS.length && VISIT_KINDS.every((k) => s.defaults.filter((d) => d.kind === k).length === 1), "צריך ברירת מחדל אחת לכל סוג פגישה.");
    for (const d of s.defaults) {
      need(whole(d.minutes, 5, 720), `${VISIT_KIND_LABEL[d.kind]}: אורך בין 5 דקות ל-12 שעות.`);
      need(whole(d.fee, 0, MAX_AGOROT), `${VISIT_KIND_LABEL[d.kind]}: מחיר בין 0 ל-100,000 ₪.`);
      need(d.kind !== "block" || d.fee === 0, "לחסימת זמן אין מחיר.");
    }
    const places = s.places.map((p) => p.trim());
    need(places.length <= MAX_PLACES, `אפשר לשמור עד ${MAX_PLACES} מקומות.`);
    need(places.every((p) => p !== ""), "יש מקום בלי שם. אפשר למלא או להסיר אותו.");
    places.forEach((p) => short(p, "שם המקום"));
    need(chars(s.reminder_text) <= MESSAGE, "נוסח התזכורת ארוך מדי: עד 1,000 תווים.");
    need(chars(s.payment_text) <= MESSAGE, "נוסח הודעת התשלום ארוך מדי: עד 1,000 תווים.");
    need(s.note_templates.length <= MAX_TEMPLATES, `אפשר לשמור עד ${MAX_TEMPLATES} תבניות סיכום.`);
    for (const t of s.note_templates) {
      need(t.name.trim() !== "", "יש תבנית סיכום בלי שם.");
      short(t.name.trim(), "שם התבנית");
      need(chars(t.text) <= TEMPLATE, "תבנית סיכום ארוכה מדי: עד 5,000 תווים.");
    }
    need(whole(s.report_days, 1, 365), "ימים עד הדוח: בין 1 ל-365.");
    need(whole(s.debt_days, 1, 365), "ימים עד שחוב מוצג: בין 1 ל-365.");
    need(whole(s.waiting_weeks, 1, 104), "שבועות המתנה: בין 1 ל-104.");
    return { ...s, places, note_templates: s.note_templates.map((t) => ({ name: t.name.trim(), text: t.text })) };
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
      "{הורה}": p.contacts.find((c) => c.name.trim())?.name.trim() ?? "רב",
      "{ילד}": p.name,
      "{יום}": v ? dayName(v.date) : "",
      "{תאריך}": v ? formatDate(v.date) : "",
      "{שעה}": v ? formatTime(v.start) : "",
      "{מקום}": v ? v.place : "",
      "{סכום}": moneyNumber(kind === "reminder" && v ? v.fee : due),
      "{שמי}": this.host.myName(),
    });
  }

  // ---------------------------------------------------------------- alerts

  private alerts(today: Today): Alert[] {
    const out: Alert[] = [];
    const s = this.settings;
    const add = (kind: Alert["kind"], p: Patient | null, text: string, since: string, ids: { visit?: string; task?: string } = {}) =>
      out.push({ kind, patient_id: p?.id ?? null, patient_name: p?.name ?? null, visit_id: ids.visit ?? null, task_id: ids.task ?? null, text, since });
    const who = (id: string | null) => this.patients.find((p) => p.id === id) ?? null;

    for (const v of this.visits) {
      if (v.kind === "block") continue;
      const p = who(v.patient_id);
      const over = v.date < today.date || (v.date === today.date && v.start + v.minutes <= today.minute);
      if (v.status === "planned" && over) {
        add("unmarked", p, `הפגישה מיום ${formatDateShort(v.date)} בשעה ${formatTime(v.start)} (${visitTitle(v)}) עדיין מסומנת כמתוכננת. אפשר לסמן מה קרה בה.`, v.date, { visit: v.id });
      }
      const age = daysBetween(v.date, today.date);
      if (v.status === "held" && NOTE_KINDS.includes(v.kind) && age >= 0 && age <= 60 && !this.notes.some((n) => n.visit_id === v.id)) {
        add("no_note", p, `ל${VISIT_KIND_LABEL[v.kind]} מיום ${formatDateShort(v.date)} עדיין אין סיכום.`, v.date, { visit: v.id });
      }
      const ahead = daysBetween(today.date, v.date);
      if (v.kind === "feedback" && v.status === "planned" && ahead >= 0 && ahead <= 7 && p && !this.reportFinished(p)) {
        add("feedback_soon", p, `שיחת המשוב נקבעה ל${formatDateLong(v.date)}, והדוח עדיין לא גמור.`, v.date, { visit: v.id });
      }
    }

    for (const p of this.patients) {
      const mine = this.visits.filter((v) => v.patient_id === p.id).sort(byDateTime);
      const balance = this.balance(p.id);
      const since = balance.due > 0 ? this.owedSince(p.id) : null;
      if (since !== null && daysBetween(since, today.date) >= s.debt_days) {
        add("debt", p, `יתרה של ${formatMoney(balance.due)} פתוחה מאז ${formatDate(since)}.`, since);
      }
      if (p.status === "waiting" && p.waiting_since !== null) {
        const weeks = Math.floor(daysBetween(p.waiting_since, today.date) / 7);
        if (weeks >= s.waiting_weeks) add("waiting", p, `ברשימת ההמתנה כבר ${weeks} שבועות.`, p.waiting_since);
      }
      if (p.status !== "active") continue;
      const held = (kind: VisitKind) => mine.filter((v) => v.kind === kind && v.status === "held");
      const lastAssessment = held("assessment").pop();
      const moreAhead = mine.some((v) => v.kind === "assessment" && v.status === "planned" && v.date >= today.date);
      if (lastAssessment && !moreAhead && held("feedback").length === 0 && addDays(lastAssessment.date, s.report_days) < today.date && !this.reportFinished(p)) {
        add("report_due", p, `מפגש האבחון האחרון היה ב-${formatDate(lastAssessment.date)}, והדוח עדיין לא גמור.`, lastAssessment.date);
      }
      const past = mine.filter((v) => v.date < today.date);
      const last = past[past.length - 1];
      const planned = mine.some((v) => v.status === "planned" && v.date >= today.date);
      const openTask = this.tasks.some((t) => t.patient_id === p.id && t.done_on === null);
      const closedWell = held("feedback").length > 0 && balance.due <= 0;
      if (last && !planned && !openTask && daysBetween(last.date, today.date) > 14 && !closedWell) {
        add("no_next_visit", p, `הפגישה האחרונה הייתה ב-${formatDate(last.date)}, ואין פגישה או משימה להמשך.`, last.date);
      }
    }

    for (const t of this.tasks) {
      if (t.done_on === null && t.due !== null && t.due < today.date) {
        add("task_overdue", who(t.patient_id), `המשימה "${t.title}" נקבעה ל-${formatDate(t.due)} ועדיין פתוחה.`, t.due, { task: t.id });
      }
    }
    // Oldest first; the sort keeps the order above among alerts of one day.
    return out.sort((a, b) => a.since.localeCompare(b.since));
  }

  // ---------------------------------------------------------------- the front page

  private overview(today: Today): Overview {
    const on = (date: string) => this.visits.filter((v) => v.date === date).sort(byDateTime);
    const todays = on(today.date);
    // Today has nothing left once every visit of it is over or cancelled.
    const left = todays.some((v) => v.status !== "cancelled" && v.start + v.minutes > today.minute);
    const nextDay = left ? null : (this.visits.filter((v) => v.date > today.date && v.status !== "cancelled").sort(byDateTime)[0]?.date ?? null);
    const week = weekRange(today.date);
    const month = monthOf(today.date);
    return {
      date: today.date,
      visits: todays.map((v) => this.view(v)),
      next_day: nextDay,
      next_visits: nextDay === null ? [] : on(nextDay).map((v) => this.view(v)),
      tasks: this.taskViews(today, true),
      alerts: this.alerts(today),
      week_visits: this.visits.filter((v) => v.kind !== "block" && v.status !== "cancelled" && v.date >= week.from && v.date <= week.to).length,
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

  private diary(month: string): DiaryMonth {
    need(/^\d{4}-(0[1-9]|1[0-2])$/.test(month), "החודש לא תקין.");
    const { from, to } = monthRange(month);
    const inMonth = (date: string) => date >= from && date <= to;
    const lines: { date: string; order: number; line: DiaryLine }[] = [];
    for (const v of this.visits.filter((x) => x.patient_id !== null && inMonth(x.date)).sort(byDateTime)) {
      lines.push({ date: v.date, order: 0, line: {
        record_id: v.id, time: v.start, patient_name: this.nameOf(v.patient_id) ?? "", what: visitTitle(v), status: v.status,
        charged: visitCharge(v), received: 0, method: null, receipt: "", payer: "", note: v.status_note, voided: false, changed: this.changed(v.id),
      } });
    }
    for (const c of this.charges.filter((x) => inMonth(x.date))) {
      lines.push({ date: c.date, order: 1, line: {
        record_id: c.id, time: null, patient_name: this.nameOf(c.patient_id) ?? "", what: c.what, status: null,
        charged: c.voided ? 0 : c.amount, received: 0, method: null, receipt: "", payer: "", note: c.voided ? c.void_reason : "", voided: c.voided, changed: this.changed(c.id),
      } });
    }
    for (const p of this.payments.filter((x) => inMonth(x.date))) {
      lines.push({ date: p.date, order: 2, line: {
        record_id: p.id, time: null, patient_name: this.nameOf(p.patient_id) ?? "", what: "תשלום", status: null,
        charged: 0, received: p.voided ? 0 : p.amount, method: p.method, receipt: p.receipt, payer: p.payer, note: p.voided ? p.void_reason : p.note, voided: p.voided, changed: this.changed(p.id),
      } });
    }
    // By date, then visits (already by time), then charges, then payments; the sort is stable.
    lines.sort((a, b) => a.date.localeCompare(b.date) || a.order - b.order);
    const days: DiaryDay[] = [];
    for (const { date, line } of lines) {
      let day = days[days.length - 1];
      if (!day || day.date !== date) {
        day = { date, lines: [], charged: 0, received: 0 };
        days.push(day);
      }
      day.lines.push(line);
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
    need(whole(year, 2000, 2100), "השנה לא תקינה.");
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
      this.notes.push({ id: this.newId("note"), patient_id: v.patient_id ?? "", visit_id: v.id, date: v.date, record, personal, case_input_id: null });
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
