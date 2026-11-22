// Browser preview and tests only: "תמונת מצב" (D-200) over the preview's records, by the rules
// of the core (crates/dv-domain/src/clinic/insights.rs). Counts, sums, rates and medians only:
// no name and no id leaves this function. What erased cards counted for (D-179) is added as
// the core adds it, and the days closed for a holiday are the preview's short list
// (previewHolidays.ts). The refusals are the core's own sentences, thrown as the preview's
// clinic throws them.
import type { Charge } from "../ipc/generated/Charge";
import type { ClinicSettings } from "../ipc/generated/ClinicSettings";
import type { DebtBucket } from "../ipc/generated/DebtBucket";
import type { IncomeMonth } from "../ipc/generated/IncomeMonth";
import type { Insights } from "../ipc/generated/Insights";
import type { KindCount } from "../ipc/generated/KindCount";
import type { MethodTotal } from "../ipc/generated/MethodTotal";
import type { Patient } from "../ipc/generated/Patient";
import type { PayMethod } from "../ipc/generated/PayMethod";
import type { Payment } from "../ipc/generated/Payment";
import type { ReferralCount } from "../ipc/generated/ReferralCount";
import type { UiError } from "../ipc/generated/UiError";
import type { Visit } from "../ipc/generated/Visit";
import type { VisitKind } from "../ipc/generated/VisitKind";
import type { WeekdayCount } from "../ipc/generated/WeekdayCount";
import { dayNumber, fromDayNumber, isIsoDate, monthRange, ownTime, PAY_METHODS, VISIT_KINDS, weekday } from "../screens/clinic/format";
import { previewHolidays } from "./previewHolidays";

/** All the clinic's records, as the preview's clinic holds them. */
export interface InsightsInput {
  patients: Patient[];
  visits: Visit[];
  charges: Charge[];
  payments: Payment[];
  settings: ClinicSettings;
  /** What erased cards counted for, by month `YYYY-MM` (the core's `ErasedTally`). */
  tally?: ReadonlyMap<string, ErasedTallyMonth>;
  /** Erased cards, by the month they were opened (the core's `ErasedCards`). */
  erasedCards?: ReadonlyMap<string, number>;
}

/** One month of the erased cards' tally (the core's `ErasedMonth`): numbers only. */
export interface ErasedTallyMonth {
  held: KindCount[];
  cancelled: number;
  no_show: number;
  charged: number;
  received: { method: PayMethod; amount: number }[];
}

/** The longest stretch the screen may ask for, in months. */
export const MAX_INSIGHT_MONTHS = 24;
/** The referral sources listed one by one; the rest are one row, "אחרים". */
export const REFERRALS_SHOWN = 8;
const REFERRAL_BLANK = "לא צוין";
const REFERRAL_OTHERS = "אחרים";
/** The ages of a debt: the first day of each bucket, and its label. */
const DEBT_BUCKETS: readonly (readonly [number, string])[] = [
  [0, "עד 30 יום"],
  [31, "31–60 יום"],
  [61, "61–90 יום"],
  [91, "מעל 90 יום"],
];
/** What the core's `Agorot` (an i32) holds. */
const AGOROT_MAX = 2_147_483_647;
const U16_MAX = 65_535;

// ------------------------------------------------------------------ refusals

function refuse(message: string): never {
  const e: UiError = { code: "refused", message, details: [], action: { kind: "dismiss" } };
  throw e;
}

/** A sum that does not fit is refused, never wrapped or clamped into a wrong figure. */
function agorot(total: number): number {
  if (total > AGOROT_MAX || total < -AGOROT_MAX - 1) refuse("הסכום הכולל גדול מדי לחישוב. כדאי לבדוק את הסכומים שנרשמו.");
  return total;
}

// ------------------------------------------------------------------ small rules

/** `part` per thousand of `whole`, rounded to the nearest (a half up); 0 when `whole` is 0. */
export function perThousand(part: number, whole: number): number {
  if (whole <= 0) return 0;
  return Math.min(U16_MAX, Math.floor((part * 1000 + Math.floor(whole / 2)) / whole));
}

/** The middle value; for an even number of values, the mean of the two middle ones, rounded down. */
export function median(values: number[]): number | null {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  const upper = sorted[middle];
  if (upper === undefined) return null;
  if (sorted.length % 2 === 1) return upper;
  const lower = sorted[middle - 1] ?? upper;
  return lower + Math.floor((upper - lower) / 2);
}

/** A day of the clinic (2000–2200) as a day number; `null` for anything else. */
function clinicDay(iso: string | null | undefined): number | null {
  if (iso === null || iso === undefined || !isIsoDate(iso) || iso < "2000-01-01" || iso > "2200-12-31") return null;
  return dayNumber(iso);
}

/** Whole days from `from` to `to`; `null` when it would be negative. */
const daysFrom = (from: number, to: number): number | null => (to >= from ? to - from : null);

/** A month as one number, `year × 12 + month − 1`; `null` for anything that is not a month the
 *  clinic may have (2000–2200). */
function monthIndex(month: string): number | null {
  const hit = /^(\d{4})-(\d{2})$/.exec(month);
  if (!hit) return null;
  const y = Number(hit[1]);
  const m = Number(hit[2]);
  return y >= 2000 && y <= 2200 && m >= 1 && m <= 12 ? y * 12 + m - 1 : null;
}

/** The month of a date of the clinic, numbered as `monthIndex` numbers it. */
const dateMonth = (iso: string): number | null => (clinicDay(iso) === null ? null : monthIndex(iso.slice(0, 7)));

const monthText = (index: number) => `${String(Math.floor(index / 12)).padStart(4, "0")}-${String((index % 12) + 1).padStart(2, "0")}`;

/** Whole months, both ends included. */
class Months {
  constructor(readonly first: number, readonly last: number) {}
  holds(index: number | null): boolean {
    return index !== null && index >= this.first && index <= this.last;
  }
  hasDate(iso: string): boolean {
    return this.holds(dateMonth(iso));
  }
  indexes(): number[] {
    return Array.from({ length: this.last - this.first + 1 }, (_, i) => this.first + i);
  }
}

/** A visit of a patient: never a block, never one without a patient. */
const isPatientVisit = (v: Visit) => !ownTime(v.kind) && v.patient_id !== null && v.patient_id.trim() !== "";

/** What a visit costs the family: its fee when held, or when missed and charged anyway. */
function visitCharge(v: Visit): number {
  const charged = v.status === "held" || ((v.status === "cancelled" || v.status === "no_show") && v.charge_missed);
  return charged ? Math.max(0, v.fee) : 0;
}

/** The text of a line, its spaces folded ("  גן  רימון " → "גן רימון"). */
const cleanLine = (s: string) => s.split(/\s+/).filter(Boolean).join(" ");

// ------------------------------------------------------------------ the picture

/** The picture of the months `fromMonth` to `toMonth` (`YYYY-MM`, both included, at most
 *  `MAX_INSIGHT_MONTHS`), on `today` (an ISO date). A refusal is thrown as a `UiError`. */
export function fakeInsights(input: InsightsInput, fromMonth: string, toMonth: string, today: string): Insights {
  const first = monthIndex(fromMonth);
  const last = monthIndex(toMonth);
  if (first === null || last === null) refuse("צריך לבחור חודש התחלה וחודש סיום לתמונת המצב.");
  if (last < first) refuse("חודש הסיום צריך להיות אותו חודש או אחרי חודש ההתחלה.");
  if (last - first >= MAX_INSIGHT_MONTHS) refuse(`אפשר להציג עד ${MAX_INSIGHT_MONTHS} חודשים בבת אחת. כדאי לבחור תקופה קצרה יותר.`);
  const now = clinicDay(today);
  if (now === null) refuse("התאריך של היום לא התקבל כמו שצריך. כדאי לסגור את המסך ולפתוח אותו שוב.");

  const stretch = new Months(first, last);
  const before = new Months(first - (last - first + 1), first - 1);
  const rows = incomeMonths(input, before.first, last);
  const months = stretch.indexes().map((i) => rows.get(i) ?? emptyMonth(i));
  const earlier = before.indexes().map((i) => rows.get(i) ?? emptyMonth(i));
  const sums = sumOf(months);
  const beforeSums = sumOf(earlier);
  const visits = sums.held + sums.cancelled + sums.no_show;

  const live = liveVisits(input.visits, stretch);
  const erased = erasedIn(input, stretch);
  const erasedHeld = erased.flatMap((e) => e.held);
  for (const k of live.kinds) k.count += erasedHeld.filter((e) => e.kind === k.kind).reduce((sum, e) => sum + e.count, 0);
  const seen = heldByPatient(input.visits);
  const feedback = toFeedback(seen, stretch);
  const waiting = waitingNow(input.patients, now);
  const workMinutes = workMinutesOf(input.settings, fromMonth, toMonth, now);
  return {
    from_month: monthText(first),
    to_month: monthText(last),
    held: sums.held,
    cancelled: sums.cancelled,
    no_show: sums.no_show,
    cancel_rate: perThousand(sums.cancelled, visits),
    no_show_rate: perThousand(sums.no_show, visits),
    by_weekday: live.byWeekday,
    by_kind: live.kinds.filter((k) => k.count > 0).sort((a, b) => b.count - a.count),
    held_minutes: live.heldMinutes,
    work_minutes: workMinutes,
    occupancy: perThousand(live.heldMinutes, workMinutes),
    new_patients: opened(input, stretch),
    referrals: referrals(input.patients, stretch),
    assessments_done: live.feedbacks + erasedHeld.filter((e) => e.kind === "feedback").reduce((sum, e) => sum + e.count, 0),
    intake_to_feedback_days: feedback.fromIntake,
    assessment_to_feedback_days: feedback.fromAssessment,
    waited_days: waited(input.patients, seen, stretch),
    waiting_now: waiting.count,
    waiting_now_days: waiting.days,
    charged: agorot(sums.charged),
    received: agorot(sums.received),
    by_method: byMethod(input.payments, erased, stretch),
    debt: debt(input, now),
    months,
    before: {
      from_month: monthText(before.first),
      to_month: monthText(before.last),
      held: beforeSums.held,
      received: agorot(beforeSums.received),
      new_patients: opened(input, before),
    },
  };
}

// ------------------------------------------------------------------ months, as the income year counts them

function emptyMonth(index: number): IncomeMonth {
  return { month: monthText(index), held: 0, cancelled: 0, no_show: 0, charged: 0, received: 0 };
}

/** Every month from `first` to `last` as the income year counts it: patient visits held,
 *  cancelled and missed; what they and the charges not voided charged; the payments not voided. */
function incomeMonths(input: InsightsInput, first: number, last: number): Map<number, IncomeMonth> {
  const wanted = new Months(first, last);
  const rows = new Map<number, IncomeMonth>();
  const rowOf = (iso: string): IncomeMonth | null => {
    const i = dateMonth(iso);
    if (i === null || !wanted.holds(i)) return null;
    let row = rows.get(i);
    if (!row) {
      row = emptyMonth(i);
      rows.set(i, row);
    }
    return row;
  };
  for (const v of input.visits.filter(isPatientVisit)) {
    const row = rowOf(v.date);
    if (!row) continue;
    if (v.status === "held") row.held += 1;
    else if (v.status === "cancelled") row.cancelled += 1;
    else if (v.status === "no_show") row.no_show += 1;
    row.charged += visitCharge(v);
  }
  for (const c of input.charges.filter((x) => !x.voided)) {
    const row = rowOf(c.date);
    if (row) row.charged += c.amount;
  }
  for (const p of input.payments.filter((x) => !x.voided)) {
    const row = rowOf(p.date);
    if (row) row.received += p.amount;
  }
  // What the erased cards counted for, as the income year adds it (D-179).
  for (const [month, e] of input.tally ?? []) {
    const row = rowOf(`${month}-01`);
    if (!row) continue;
    row.held += e.held.reduce((sum, k) => sum + k.count, 0);
    row.cancelled += e.cancelled;
    row.no_show += e.no_show;
    row.charged += e.charged;
    row.received += e.received.reduce((sum, m) => sum + m.amount, 0);
  }
  for (const row of rows.values()) {
    agorot(row.charged);
    agorot(row.received);
  }
  return rows;
}

function sumOf(rows: IncomeMonth[]) {
  return rows.reduce(
    (s, r) => ({ held: s.held + r.held, cancelled: s.cancelled + r.cancelled, no_show: s.no_show + r.no_show, charged: s.charged + r.charged, received: s.received + r.received }),
    { held: 0, cancelled: 0, no_show: 0, charged: 0, received: 0 },
  );
}

// ------------------------------------------------------------------ visits

/** What the cards show of the stretch's visits. */
function liveVisits(visits: Visit[], stretch: Months) {
  const byWeekday: WeekdayCount[] = Array.from({ length: 7 }, (_, d) => ({ weekday: d, held: 0, cancelled: 0, no_show: 0 }));
  // Every kind of the screens, in their order (a kind of a later version has no row).
  const kinds: KindCount[] = VISIT_KINDS.map((kind) => ({ kind, count: 0 }));
  let heldMinutes = 0;
  let feedbacks = 0;
  for (const v of visits) {
    if (!isPatientVisit(v) || !stretch.hasDate(v.date)) continue;
    const day = byWeekday[weekday(v.date) ?? -1];
    if (!day) continue;
    if (v.status === "held") {
      day.held += 1;
      heldMinutes += v.minutes;
      for (const k of kinds) if (k.kind === v.kind) k.count += 1;
      if (v.kind === "feedback") feedbacks += 1;
    } else if (v.status === "cancelled") {
      day.cancelled += 1;
    } else if (v.status === "no_show") {
      day.no_show += 1;
    }
  }
  return { byWeekday, kinds, heldMinutes, feedbacks };
}

/** One held visit of a patient, as the times of the pathway need it. */
interface Seen {
  day: number;
  start: number;
  month: number;
  kind: VisitKind;
}

/** (day, start) of `a` against `b`: negative when earlier. */
const compareWhen = (a: Seen, b: Seen) => a.day - b.day || a.start - b.start;

/** Every patient's held visits (never a block), oldest first. */
function heldByPatient(visits: Visit[]): Map<string, Seen[]> {
  const held = new Map<string, Seen[]>();
  for (const v of visits) {
    if (!isPatientVisit(v) || v.status !== "held" || v.patient_id === null) continue;
    const day = clinicDay(v.date);
    const month = dateMonth(v.date);
    if (day === null || month === null) continue;
    const list = held.get(v.patient_id) ?? [];
    list.push({ day, start: v.start, month, kind: v.kind });
    held.set(v.patient_id, list);
  }
  for (const list of held.values()) list.sort(compareWhen);
  return held;
}

/** The medians of (first intake → feedback, last assessment → feedback), over the patients
 *  whose latest feedback in the stretch was held. */
function toFeedback(seen: Map<string, Seen[]>, stretch: Months) {
  const fromIntake: number[] = [];
  const fromAssessment: number[] = [];
  for (const visits of seen.values()) {
    const feedback = [...visits].reverse().find((s) => s.kind === "feedback" && stretch.holds(s.month));
    if (!feedback) continue;
    const upTo: Seen[] = [];
    for (const s of visits) {
      if (compareWhen(s, feedback) > 0) break;
      upTo.push(s);
    }
    const start = upTo.find((s) => s.kind === "intake") ?? upTo[0];
    const wait = start === undefined ? null : daysFrom(start.day, feedback.day);
    if (wait !== null) fromIntake.push(wait);
    const assessment = [...upTo].reverse().find((s) => s.kind === "assessment" && compareWhen(s, feedback) < 0);
    const after = assessment === undefined ? null : daysFrom(assessment.day, feedback.day);
    if (after !== null) fromAssessment.push(after);
  }
  return { fromIntake: median(fromIntake), fromAssessment: median(fromAssessment) };
}

/** The median days from `waiting_since` to the first held visit, over the patients whose
 *  first held visit was in the stretch. */
function waited(patients: Patient[], seen: Map<string, Seen[]>, stretch: Months): number | null {
  const waits: number[] = [];
  for (const p of patients) {
    const since = clinicDay(p.waiting_since);
    const first = seen.get(p.id)?.[0];
    if (since === null || first === undefined || !stretch.holds(first.month)) continue;
    const wait = daysFrom(since, first.day);
    if (wait !== null) waits.push(wait);
  }
  return median(waits);
}

/** How many are on the waiting list today, and the median days they have waited (with no
 *  day they joined, since the card was opened, as the waiting alert counts it). */
function waitingNow(patients: Patient[], now: number) {
  const waiting = patients.filter((p) => p.status === "waiting");
  const waits: number[] = [];
  for (const p of waiting) {
    const since = clinicDay(p.waiting_since) ?? clinicDay(p.created_on);
    const wait = since === null ? null : daysFrom(since, now);
    if (wait !== null) waits.push(wait);
  }
  return { count: waiting.length, days: median(waits) };
}

/** Her working minutes on her working days (without the days closed for a holiday when she
 *  has the holidays on, as free times count them), from the first day of the stretch to its
 *  last or to today, the earlier. */
function workMinutesOf(settings: ClinicSettings, fromMonth: string, toMonth: string, now: number): number {
  const first = dayNumber(monthRange(fromMonth).from);
  const last = dayNumber(monthRange(toMonth).to);
  if (first === null || last === null) return 0;
  const closed = new Set(settings.holidays ? previewHolidays(monthRange(fromMonth).from, monthRange(toMonth).to).filter((h) => h.closed).map((h) => h.date) : []);
  const perDay = Math.max(0, settings.day_end - settings.day_start);
  let minutes = 0;
  for (let day = first; day <= Math.min(last, now); day += 1) {
    const date = fromDayNumber(day);
    if (settings.work_days.includes(weekday(date) ?? -1) && !closed.has(date)) minutes += perDay;
  }
  return minutes;
}

// ------------------------------------------------------------------ patients

/** Cards opened in `months`. */
/** Cards opened in `months`: those that still exist, and the erased ones (D-179). */
const opened = (input: InsightsInput, months: Months) =>
  input.patients.filter((p) => months.hasDate(p.created_on)).length
  + [...(input.erasedCards ?? [])].filter(([month]) => months.holds(monthIndex(month))).reduce((sum, [, cards]) => sum + cards, 0);

/** The months of the erased cards' tally that fall in `months`. */
const erasedIn = (input: InsightsInput, months: Months): ErasedTallyMonth[] =>
  [...(input.tally ?? [])].filter(([month]) => months.holds(monthIndex(month))).map(([, e]) => e);

/** Where the new patients of the stretch came from: grouped without regard to case or spaces,
 *  shown as the card opened first wrote it; the most first; past the first eight, "אחרים". */
function referrals(patients: Patient[], stretch: Months): ReferralCount[] {
  const fresh = patients
    .filter((p) => stretch.hasDate(p.created_on))
    .sort((a, b) => (a.created_on < b.created_on ? -1 : a.created_on > b.created_on ? 1 : a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  const groups = new Map<string, ReferralCount>();
  for (const p of fresh) {
    const source = cleanLine(p.referred_by) || REFERRAL_BLANK;
    const key = source.toLowerCase();
    const group = groups.get(key);
    if (group) group.count += 1;
    else groups.set(key, { source, count: 1 });
  }
  const rows = [...groups.values()].sort((a, b) => b.count - a.count || (a.source < b.source ? -1 : a.source > b.source ? 1 : 0));
  if (rows.length <= REFERRALS_SHOWN) return rows;
  const others = rows.slice(REFERRALS_SHOWN).reduce((sum, r) => sum + r.count, 0);
  return [...rows.slice(0, REFERRALS_SHOWN), { source: REFERRAL_OTHERS, count: others }];
}

// ------------------------------------------------------------------ money

/** Received in the stretch by method, in the screens' order and a method of a later version
 *  last; methods with nothing left out. */
function byMethod(payments: Payment[], erased: ErasedTallyMonth[], stretch: Months): MethodTotal[] {
  const methods: PayMethod[] = [...PAY_METHODS, "unknown"];
  const totals = new Map<PayMethod, number>(methods.map((m) => [m, 0]));
  const add = (method: PayMethod, amount: number) => {
    if (totals.has(method)) totals.set(method, (totals.get(method) ?? 0) + amount);
  };
  for (const p of payments) {
    if (!p.voided && stretch.hasDate(p.date)) add(p.method, p.amount);
  }
  for (const e of erased) for (const m of e.received) add(m.method, m.amount);
  return methods.map((method) => ({ method, amount: agorot(totals.get(method) ?? 0) })).filter((m) => m.amount > 0);
}

/** Paying the oldest charges first with everything received: the date of the oldest charge
 *  still not fully covered; `null` when nothing is owed. */
function oldestUncovered(visits: Visit[], charges: Charge[], paid: number): string | null {
  const owed = [
    ...visits.map((v) => ({ date: v.date, amount: visitCharge(v) })),
    ...charges.filter((c) => !c.voided).map((c) => ({ date: c.date, amount: c.amount })),
  ].filter((o) => o.amount > 0).sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : 0));
  let money = paid;
  for (const o of owed) {
    if (money < o.amount) return o.date;
    money -= o.amount;
  }
  return null;
}

/** What is owed today by the cards, by the age of each family's oldest uncovered charge
 *  (a charge dated ahead is 0 days old). Four rows, always. */
function debt(input: InsightsInput, now: number): DebtBucket[] {
  const totals = DEBT_BUCKETS.map(() => ({ patients: 0, amount: 0 }));
  for (const p of input.patients) {
    const visits = input.visits.filter((v) => v.patient_id === p.id);
    const charges = input.charges.filter((c) => c.patient_id === p.id);
    const paid = input.payments.filter((x) => x.patient_id === p.id && !x.voided).reduce((sum, x) => sum + x.amount, 0);
    const charged = visits.reduce((sum, v) => sum + visitCharge(v), 0) + charges.filter((c) => !c.voided).reduce((sum, c) => sum + c.amount, 0);
    const due = charged - paid;
    if (due <= 0) continue;
    const since = clinicDay(oldestUncovered(visits, charges, paid));
    const age = since === null ? 0 : Math.max(0, now - since);
    let bucket = 0;
    DEBT_BUCKETS.forEach(([from], i) => {
      if (from <= age) bucket = i;
    });
    const row = totals[bucket];
    if (row) {
      row.patients += 1;
      row.amount += due;
    }
  }
  return DEBT_BUCKETS.map(([from, label], i) => ({ label, from_days: from, patients: totals[i]?.patients ?? 0, amount: agorot(totals[i]?.amount ?? 0) }));
}
