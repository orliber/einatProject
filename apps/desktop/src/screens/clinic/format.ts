// How the clinic shows money, dates and times, and its Hebrew word for every kind and status.
// Pure functions: the screens and the browser-preview stand-in share them.
import type { AlertKind } from "../../ipc/generated/AlertKind";
import type { ContactRole } from "../../ipc/generated/ContactRole";
import type { PatientStatus } from "../../ipc/generated/PatientStatus";
import type { PayMethod } from "../../ipc/generated/PayMethod";
import type { TaskKind } from "../../ipc/generated/TaskKind";
import type { Today } from "../../ipc/generated/Today";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import type { VisitStatus } from "../../ipc/generated/VisitStatus";

// ------------------------------------------------------------------ money

/** The most a single fee, payment or charge can be (the core's limit), in agorot. */
export const MAX_AGOROT = 10_000_000;

/** 45000 → "450", 45050 → "450.50", 1250000 → "12,500": the number alone, without the sign. */
export function moneyNumber(agorot: number): string {
  const abs = Math.abs(Math.trunc(agorot));
  const whole = Math.floor(abs / 100).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const cents = abs % 100;
  return `${agorot < 0 ? "-" : ""}${whole}${cents === 0 ? "" : `.${String(cents).padStart(2, "0")}`}`;
}

/** 45000 → "450 ₪", 45050 → "450.50 ₪". */
export function formatMoney(agorot: number): string {
  return `${moneyNumber(agorot)} ₪`;
}

/** What she typed ("450", "450.5", "1,250", "450 ₪") in agorot; `null` for anything that is not
 *  an amount from 0 up to the core's limit. Never guesses: "12.345" or "4-50" is refused. */
export function parseMoney(text: string): number | null {
  const t = text.replace(/[₪\s‎‏]/g, "");
  const m = /^(\d{1,3}(?:,\d{3})+|\d+)(?:\.(\d{1,2}))?$/.exec(t);
  if (!m) return null;
  const whole = Number((m[1] ?? "").replace(/,/g, ""));
  const cents = Number((m[2] ?? "").padEnd(2, "0"));
  const agorot = whole * 100 + cents;
  return Number.isSafeInteger(agorot) && agorot <= MAX_AGOROT ? agorot : null;
}

/** The text of an amount inside a field she edits: "450", "450.50", and "" for nothing. */
export function moneyInput(agorot: number | null): string {
  return agorot === null ? "" : moneyNumber(agorot).replace(/,/g, "");
}

// ------------------------------------------------------------------ dates

const DAY_NAMES = ["ראשון", "שני", "שלישי", "רביעי", "חמישי", "שישי", "שבת"] as const;
const MONTH_NAMES = ["ינואר", "פברואר", "מרץ", "אפריל", "מאי", "יוני", "יולי", "אוגוסט", "ספטמבר", "אוקטובר", "נובמבר", "דצמבר"] as const;
const DAY_MS = 86_400_000;

const pad = (n: number) => String(n).padStart(2, "0");

/** The parts of a real ISO date; `null` for "2026-02-30" or anything else that is not one. */
function parts(iso: string): { y: number; m: number; d: number } | null {
  const hit = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  if (!hit) return null;
  const y = Number(hit[1]);
  const m = Number(hit[2]);
  const d = Number(hit[3]);
  const date = new Date(Date.UTC(y, m - 1, d));
  return date.getUTCFullYear() === y && date.getUTCMonth() === m - 1 && date.getUTCDate() === d ? { y, m, d } : null;
}

export function isIsoDate(iso: string): boolean {
  return parts(iso) !== null;
}

/** Days since 1970-01-01 (dates are compared and moved as whole days, never as local times,
 *  so a clock change never shifts a visit). `null` when it is not a date. */
export function dayNumber(iso: string): number | null {
  const p = parts(iso);
  return p ? Math.round(Date.UTC(p.y, p.m - 1, p.d) / DAY_MS) : null;
}

export function fromDayNumber(day: number): string {
  const d = new Date(day * DAY_MS);
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
}

/** The date `days` later (or earlier); the same text back when it is not a date. */
export function addDays(iso: string, days: number): string {
  const n = dayNumber(iso);
  return n === null ? iso : fromDayNumber(n + days);
}

/** Days from `from` to `to`; 0 when either is not a date. */
export function daysBetween(from: string, to: string): number {
  const a = dayNumber(from);
  const b = dayNumber(to);
  return a === null || b === null ? 0 : b - a;
}

/** 0 = Sunday … 6 = Saturday; `null` when it is not a date. */
export function weekday(iso: string): number | null {
  const n = dayNumber(iso);
  // 1970-01-01 was a Thursday.
  return n === null ? null : (((n + 4) % 7) + 7) % 7;
}

/** "ראשון", as a message to the parents names the day. */
export function dayName(iso: string): string {
  const w = weekday(iso);
  return w === null ? "" : (DAY_NAMES[w] ?? "");
}

/** "2026-11-15" → "15.11.2026". */
export function formatDate(iso: string): string {
  const p = parts(iso);
  return p ? `${pad(p.d)}.${pad(p.m)}.${p.y}` : "";
}

/** "2026-11-15" → "15.11". */
export function formatDateShort(iso: string): string {
  const p = parts(iso);
  return p ? `${pad(p.d)}.${pad(p.m)}` : "";
}

/** "2026-11-15" → "יום ראשון, 15.11.2026" (Saturday is "שבת", not "יום שבת"). */
export function formatDateLong(iso: string): string {
  const w = weekday(iso);
  if (w === null) return "";
  return `${w === 6 ? "שבת" : `יום ${DAY_NAMES[w] ?? ""}`}, ${formatDate(iso)}`;
}

/** "2026-11" (or a date of that month) → "נובמבר 2026". */
export function formatMonth(month: string): string {
  const hit = /^(\d{4})-(\d{2})/.exec(month);
  const name = hit ? MONTH_NAMES[Number(hit[2]) - 1] : undefined;
  return hit && name ? `${name} ${hit[1]}` : "";
}

/** The month of a date: "2026-11-15" → "2026-11". */
export function monthOf(iso: string): string {
  return iso.slice(0, 7);
}

export interface DateRange {
  from: string;
  to: string;
}

/** The week of a date, Sunday to Saturday. */
export function weekRange(iso: string): DateRange {
  const w = weekday(iso) ?? 0;
  const from = addDays(iso, -w);
  return { from, to: addDays(from, 6) };
}

/** The month of a date (or of a "YYYY-MM"), first day to last. */
export function monthRange(isoOrMonth: string): DateRange {
  const hit = /^(\d{4})-(\d{2})/.exec(isoOrMonth);
  const y = Number(hit?.[1] ?? 1970);
  const m = Number(hit?.[2] ?? 1);
  const last = new Date(Date.UTC(y, m, 0)).getUTCDate();
  return { from: `${y}-${pad(m)}-01`, to: `${y}-${pad(m)}-${pad(last)}` };
}

// ------------------------------------------------------------------ times

/** 570 → "09:30". */
export function formatTime(minutes: number): string {
  const m = Math.max(0, Math.min(1440, Math.trunc(minutes)));
  return `${pad(Math.floor(m / 60))}:${pad(m % 60)}`;
}

/** "9:30", "09:30", "9.30", "930" or "9" → minutes from midnight; `null` for anything else. */
export function parseTime(text: string): number | null {
  const t = text.trim();
  const m = /^(\d{1,2})[:.](\d{2})$/.exec(t) ?? /^(\d{1,2})(\d{2})$/.exec(t) ?? /^(\d{1,2})()$/.exec(t);
  if (!m) return null;
  const h = Number(m[1]);
  const min = m[2] ? Number(m[2]) : 0;
  return h <= 23 && min <= 59 ? h * 60 + min : null;
}

/** "09:30–10:20": a visit's start and end. */
export function formatSpan(start: number, minutes: number): string {
  return `${formatTime(start)}–${formatTime(start + minutes)}`;
}

/** "דקה", "50 דקות", "שעה", "שעה וחצי", "שעתיים", "2:15 שעות". */
export function formatLength(minutes: number): string {
  if (minutes === 1) return "דקה";
  if (minutes < 60) return `${minutes} דקות`;
  if (minutes === 60) return "שעה";
  if (minutes === 90) return "שעה וחצי";
  if (minutes === 120) return "שעתיים";
  return minutes % 60 === 0 ? `${minutes / 60} שעות` : `${Math.floor(minutes / 60)}:${pad(minutes % 60)} שעות`;
}

/** Her local "now", as every clinic request carries it (the core's clock is not hers). */
export function today(now: Date = new Date()): Today {
  return {
    date: `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`,
    minute: now.getHours() * 60 + now.getMinutes(),
  };
}

// ------------------------------------------------------------------ a visit and the time
// When a visit asks for something (review item 3 of 18.11.2026, D-204): one step that fits the
// time is offered on the line, the rest wait in "⋯". A visit under way counts as begun: a
// reminder makes no sense once it has started.

/** The visit has begun (or its day is past). */
export function visitStarted(v: Pick<Visit, "date" | "start">, now: Today): boolean {
  return v.date < now.date || (v.date === now.date && v.start <= now.minute);
}

/** The visit is over: the core's own rule for "פגישה שלא סומנה". */
export function visitEnded(v: Pick<Visit, "date" | "start" | "minutes">, now: Today): boolean {
  return v.date < now.date || (v.date === now.date && v.start + v.minutes <= now.minute);
}

export type VisitStep = "hold" | "note" | "remind" | null;

/** The one step that fits the time: begun and planned → "hold"; held without a note → "note";
 *  still ahead and planned → "remind"; otherwise none. Time she kept for herself has none. */
export function visitStep(v: Visit, hasNote: boolean, now: Today): VisitStep {
  if (ownTime(v.kind) || v.patient_id === null) return null;
  if (v.status === "planned") return visitStarted(v, now) ? "hold" : "remind";
  if (v.status === "held" && !hasNote) return "note";
  return null;
}

/** Where the "now" line goes in a day's list sorted by start: only BETWEEN two entries (the
 *  index of the first one still ahead), else -1. */
export function nowIndex(starts: number[], minute: number): number {
  const i = starts.findIndex((s) => s > minute);
  return i <= 0 ? -1 : i;
}

/** "4 בלוח · 2 אחרי עכשיו": how many visits the day holds, and how many are still ahead when
 *  that is some of them but not all. "" for an empty day. */
export function todayCountWords(total: number, after: number): string {
  if (total <= 0) return "";
  const head = total === 1 ? "פגישה אחת בלוח" : `${total} בלוח`;
  if (after <= 0 || after >= total) return head;
  return `${head} · ${after === 1 ? "אחת" : after} אחרי עכשיו`;
}

/** How soon a visit is, as it is said: "בעוד דקה", "בעוד 48 דקות", "בעוד שעה", "בעוד שעה ודקה",
 *  "בעוד שעה ו-15 דקות", "בעוד שעתיים", "בעוד שעתיים ו-5 דקות", "בעוד 3 שעות", "בעוד 3 שעות
 *  ו-15 דקות". Never "1 דקות" or "2:35 שעות" (a visit's length may read so, a sentence does
 *  not). "עכשיו" for no time at all, which the front page never asks for. */
export function untilWords(minutes: number): string {
  const m = Math.trunc(minutes);
  if (m <= 0) return "עכשיו";
  const hours = Math.floor(m / 60);
  const rest = m % 60;
  const mins = rest === 1 ? "דקה" : `${rest} דקות`;
  if (hours === 0) return `בעוד ${mins}`;
  const head = hours === 1 ? "שעה" : hours === 2 ? "שעתיים" : `${hours} שעות`;
  if (rest === 0) return `בעוד ${head}`;
  return `בעוד ${head} ${rest === 1 ? "ודקה" : `ו-${mins}`}`;
}

// ------------------------------------------------------------------ Hebrew labels
// The same words the core uses (crates/dv-domain/src/clinic/logic.rs: kind_label, status_label,
// method_label, task_kind_label, patient_status_label, contact_role_label): a history line or
// an alert written there reads like the screen beside it.
//
// Every stored kind has an "unknown" member (D-178): a value written by a later version of the
// program. It has a label, so a screen shows it plainly, and it is in none of the lists she
// chooses from (`VISIT_KINDS`, `PAY_METHODS`, …): the core refuses to save a record that
// carries it until she picks a value from the list.

export const VISIT_KIND_LABEL: Record<VisitKind, string> = {
  intake: "פגישת היכרות",
  assessment: "מפגש אבחון",
  observation: "תצפית",
  feedback: "שיחת משוב",
  therapy: "פגישת טיפול",
  guidance: "הדרכת הורים",
  meeting: "פגישה מקצועית",
  other: "פגישה אחרת",
  block: "זמן שמור",
  supervision: "הדרכה",
  unknown: "סוג לא מוכר",
};

/** Her own time (D-207): a block or a supervision. No patient, no fee, not a visit in the counts. */
export function ownTime(kind: VisitKind): boolean {
  return kind === "block" || kind === "supervision";
}

/** The kinds in the order she meets them in an assessment; the block last. */
export const VISIT_KINDS: VisitKind[] = ["intake", "assessment", "observation", "feedback", "therapy", "guidance", "meeting", "other", "block", "supervision"];

export const VISIT_STATUS_LABEL: Record<VisitStatus, string> = {
  planned: "מתוכננת",
  held: "התקיימה",
  cancelled: "בוטלה",
  no_show: "לא הגיעו",
  unknown: "מצב לא מוכר",
};

/** The marks she can give a visit, in the order the dialog offers them. */
export const VISIT_STATUSES: VisitStatus[] = ["planned", "held", "cancelled", "no_show"];

export const PAY_METHOD_LABEL: Record<PayMethod, string> = {
  cash: "מזומן",
  transfer: "העברה בנקאית",
  bit: "ביט",
  paybox: "פייבוקס",
  check: "צ'ק",
  card: "כרטיס אשראי",
  other: "אחר",
  unknown: "אמצעי לא מוכר",
};

/** In the order the core lists them (`PAY_METHODS` in logic.rs). */
export const PAY_METHODS: PayMethod[] = ["cash", "transfer", "bit", "paybox", "check", "card", "other"];

export const TASK_KIND_LABEL: Record<TaskKind, string> = {
  call: "שיחת טלפון",
  questionnaire: "שאלון",
  document: "מסמך",
  report: "דוח",
  payment: "תשלום",
  follow_up: "מעקב",
  other: "אחר",
  unknown: "סוג לא מוכר",
};

export const TASK_KINDS: TaskKind[] = ["call", "questionnaire", "document", "report", "payment", "follow_up", "other"];

export const PATIENT_STATUS_LABEL: Record<PatientStatus, string> = {
  waiting: "ברשימת ההמתנה",
  active: "בתהליך",
  closed: "הסתיים",
  unknown: "מצב לא מוכר",
};

export const PATIENT_STATUSES: PatientStatus[] = ["waiting", "active", "closed"];

export const CONTACT_ROLE_LABEL: Record<ContactRole, string> = {
  mother: "אמא",
  father: "אבא",
  guardian: "אפוטרופוס",
  other: "אחר",
  unknown: "קשר לא מוכר",
};

export const CONTACT_ROLES: ContactRole[] = ["mother", "father", "guardian", "other"];

export const ALERT_KIND_LABEL: Record<AlertKind, string> = {
  unmarked: "פגישה שלא סומנה",
  no_note: "פגישה בלי סיכום",
  debt: "חוב פתוח",
  waiting: "המתנה ארוכה",
  report_due: "דוח מתעכב",
  no_next_visit: "אין פגישה הבאה",
  task_overdue: "משימה שמועדה עבר",
  feedback_soon: "שיחת משוב קרובה",
  materials_missing: "חסרים שאלונים או מסמכים",
  unreadable: "רישום שלא נקרא",
};

/** The options of a select for a stored kind: the values she can choose, and, only while the
 *  record still holds a value this version does not know, that value too (so the select shows
 *  what is stored and never puts another value in its place by itself). */
export function optionsWith<T extends string>(choices: readonly T[], labels: Record<T, string>, current: T): { value: T; label: string }[] {
  const all = choices.includes(current) ? choices : [...choices, current];
  return all.map((value) => ({ value, label: labels[value] }));
}

/** What she is told after "this and the following" left later visits of the series as they
 *  were, because they have a session note or were already marked (D-176); "" when none were. */
export function leftAloneWords(count: number): string {
  if (count <= 0) return "";
  return count === 1
    ? "פגישה אחת בסדרה נשארה כמו שהיא, כי יש לה סיכום או שהיא כבר סומנה"
    : `${count} פגישות בסדרה נשארו כמו שהן, כי יש להן סיכום או שהן כבר סומנו`;
}

/** What the calendar calls a visit: her own words when she gave some, else the kind's name. */
export function visitTitle(visit: { kind: VisitKind; title: string }): string {
  return visit.title.trim() || VISIT_KIND_LABEL[visit.kind];
}

/** "נועם לדוגמה" → "נ. ל.": what discreet mode shows in place of a name, when a family in the
 *  room may see the screen. At most three words are kept. */
export { initials } from "../../discreet";

/** "בוקר טוב", "צהריים טובים", "ערב טוב" or "לילה טוב", by the minute of her day. */
export function greeting(minute: number): string {
  if (minute < 5 * 60) return "לילה טוב";
  if (minute < 12 * 60) return "בוקר טוב";
  if (minute < 17 * 60) return "צהריים טובים";
  if (minute < 22 * 60) return "ערב טוב";
  return "לילה טוב";
}
