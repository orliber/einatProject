// The preview's "תמונת מצב" (D-200) keeps the core's rules: what counts as a visit, rates and
// occupancy per thousand, medians of the assessment pathway, debts by age, and referrals with
// the tail folded into "אחרים". Every record here is invented.
import type { Charge } from "../ipc/generated/Charge";
import type { Patient } from "../ipc/generated/Patient";
import type { Payment } from "../ipc/generated/Payment";
import type { UiError } from "../ipc/generated/UiError";
import type { Visit } from "../ipc/generated/Visit";
import { defaultClinicSettings } from "./fakeClinic";
import { fakeInsights, median, perThousand, type InsightsInput } from "./fakeInsights";

const TODAY = "2026-11-15"; // a Sunday

let seq = 0;
const patient = (over: Partial<Patient> = {}): Patient => ({
  id: `p-${++seq}`, name: "ילד בדוי", birth_date: null, gender: null, contacts: [], framework: "", referred_by: "", reason: "", notes: "",
  status: "active", waiting_since: null, urgent: false, session_fee: null, case_id: null, created_on: "2026-01-01", ...over,
});
const visit = (patientId: string | null, date: string, over: Partial<Visit> = {}): Visit => ({
  id: `v-${++seq}`, patient_id: patientId, kind: "assessment", title: "", date, start: 9 * 60, minutes: 60, place: "", status: "held",
  status_note: "", fee: 0, charge_missed: false, series_id: null, note: "", ...over,
});
const charge = (patientId: string, date: string, amount: number, voided = false): Charge => ({
  id: `c-${++seq}`, patient_id: patientId, date, amount, what: "חבילת אבחון", voided, void_reason: "",
});
const payment = (patientId: string, date: string, amount: number, over: Partial<Payment> = {}): Payment => ({
  id: `pay-${++seq}`, patient_id: patientId, date, amount, method: "bit", receipt: "", payer: "", note: "", voided: false, void_reason: "", ...over,
});
const records = (over: Partial<InsightsInput> = {}): InsightsInput => ({
  patients: [], visits: [], charges: [], payments: [],
  settings: { ...defaultClinicSettings(), day_start: 8 * 60, day_end: 14 * 60, work_days: [0, 1, 2, 3, 4] },
  ...over,
});

function refusal(run: () => unknown): UiError {
  try {
    run();
  } catch (e) {
    return e as UiError;
  }
  throw new Error("not refused");
}

describe("the stretch asked for", () => {
  it("refuses what is not a month, a stretch in the wrong order or longer than 24 months, and a day that is not one", () => {
    const empty = records();
    for (const [from, to] of [["2026-13", "2026-11"], ["", "2026-11"], ["2026-1", "2026-11"], ["1999-12", "2000-01"]] as const) {
      expect(refusal(() => fakeInsights(empty, from, to, TODAY))).toMatchObject({ code: "refused", message: "צריך לבחור חודש התחלה וחודש סיום לתמונת המצב." });
    }
    expect(refusal(() => fakeInsights(empty, "2026-11", "2026-10", TODAY)).message).toBe("חודש הסיום צריך להיות אותו חודש או אחרי חודש ההתחלה.");
    expect(refusal(() => fakeInsights(empty, "2024-12", "2026-12", TODAY)).message).toBe("אפשר להציג עד 24 חודשים בבת אחת. כדאי לבחור תקופה קצרה יותר.");
    expect(refusal(() => fakeInsights(empty, "2026-11", "2026-11", "15.11.2026")).message).toBe("התאריך של היום לא התקבל כמו שצריך. כדאי לסגור את המסך ולפתוח אותו שוב.");
    // 24 months are fine, and so is one.
    expect(fakeInsights(empty, "2025-01", "2026-12", TODAY).months).toHaveLength(24);
    expect(fakeInsights(empty, "2026-11", "2026-11", TODAY).months.map((m) => m.month)).toEqual(["2026-11"]);
  });

  it("compares with the stretch of the same length just before", () => {
    const p = patient({ created_on: "2026-07-03" });
    const q = patient({ created_on: "2026-10-01" });
    const input = records({
      patients: [p, q],
      visits: [visit(p.id, "2026-08-10"), visit(p.id, "2026-08-11"), visit(q.id, "2026-10-05")],
      payments: [payment(p.id, "2026-06-30", 45_000), payment(q.id, "2026-10-05", 30_000), payment(q.id, "2026-10-06", 99_000, { voided: true })],
    });
    const out = fakeInsights(input, "2026-09", "2026-11", TODAY);
    expect(out.months.map((m) => m.month)).toEqual(["2026-09", "2026-10", "2026-11"]);
    expect([out.held, out.received, out.new_patients]).toEqual([1, 30_000, 1]);
    expect(out.before).toEqual({ from_month: "2026-06", to_month: "2026-08", held: 2, received: 45_000, new_patients: 1 });
  });
});

describe("visits", () => {
  it("counts patient visits only, and the rates per thousand, rounded to the nearest", () => {
    const p = patient();
    const nov = (date: string, over: Partial<Visit>) => visit(p.id, date, over);
    const input = records({
      patients: [p],
      visits: [
        // Sunday the 1st: three held; Monday the 2nd: one held, one cancelled; Thursday the 5th: one held, one cancelled, one missed.
        nov("2026-11-01", { fee: 45_000 }), nov("2026-11-01", { kind: "intake", fee: 45_000 }), nov("2026-11-01", { kind: "feedback", fee: 45_000 }),
        nov("2026-11-02", {}), nov("2026-11-02", { status: "cancelled", fee: 45_000 }),
        nov("2026-11-05", {}), nov("2026-11-05", { status: "cancelled", fee: 45_000, charge_missed: true }), nov("2026-11-05", { status: "no_show" }),
        // Neither planned visits nor blocks count.
        nov("2026-11-20", { status: "planned", fee: 45_000 }),
        visit(null, "2026-11-03", { kind: "block" }),
      ],
    });
    const out = fakeInsights(input, "2026-11", "2026-11", TODAY);
    expect([out.held, out.cancelled, out.no_show]).toEqual([5, 2, 1]);
    // 2 of 8, 1 of 8 (125.0 and 125 exactly: no rounding needed); 1 of 3 and 2 of 3 below.
    expect([out.cancel_rate, out.no_show_rate]).toEqual([250, 125]);
    expect(perThousand(1, 3)).toBe(333);
    expect(perThousand(2, 3)).toBe(667);
    expect(perThousand(1, 6)).toBe(167);
    expect(perThousand(5, 0)).toBe(0);
    expect(out.by_weekday.map((d) => [d.weekday, d.held, d.cancelled, d.no_show])).toEqual([
      [0, 3, 0, 0], [1, 1, 1, 0], [2, 0, 0, 0], [3, 0, 0, 0], [4, 1, 1, 1], [5, 0, 0, 0], [6, 0, 0, 0],
    ]);
    expect(out.by_kind).toEqual([{ kind: "assessment", count: 3 }, { kind: "intake", count: 1 }, { kind: "feedback", count: 1 }]);
    expect(out.assessments_done).toBe(1);
    // Charged: three held at 450, the missed one charged anyway.
    expect(out.charged).toBe(4 * 45_000);
    // 300 minutes held, of 11 working days (Sunday–Thursday, 1–15 November) of 6 hours.
    expect([out.held_minutes, out.work_minutes]).toEqual([300, 11 * 360]);
    expect(out.occupancy).toBe(76);
  });

  it("has no working minutes for a stretch that has not begun, or without working days", () => {
    expect(fakeInsights(records(), "2026-12", "2026-12", TODAY).work_minutes).toBe(0);
    const none = records({ settings: { ...defaultClinicSettings(), work_days: [] } });
    expect(fakeInsights(none, "2026-11", "2026-11", TODAY)).toMatchObject({ work_minutes: 0, occupancy: 0 });
  });

  it("leaves out the days closed for a holiday only while the holidays are on, as the core does", () => {
    // September 2026: Rosh Hashana's Sunday and Yom Kippur's Monday; 22 days from Sunday to Thursday.
    const on = records();
    const off = records({ settings: { ...on.settings, holidays: false } });
    expect(fakeInsights(on, "2026-09", "2026-09", TODAY).work_minutes).toBe(20 * 360);
    expect(fakeInsights(off, "2026-09", "2026-09", TODAY).work_minutes).toBe(22 * 360);
  });

  it("counts what erased cards counted for, as the yearly summary does", () => {
    const p = patient({ created_on: "2026-03-02" });
    const input = records({
      patients: [p],
      visits: [visit(p.id, "2026-03-10", { kind: "feedback" })],
      tally: new Map([["2026-03", { held: [{ kind: "feedback", count: 1 }, { kind: "intake", count: 2 }], cancelled: 1, no_show: 0, charged: 50_000, received: [{ method: "cash", amount: 20_000 }] }]]),
      erasedCards: new Map([["2026-03", 2]]),
    });
    const out = fakeInsights(input, "2026-03", "2026-03", TODAY);
    expect([out.held, out.cancelled, out.charged, out.received, out.new_patients, out.assessments_done]).toEqual([4, 1, 50_000, 20_000, 3, 2]);
    expect(out.by_kind).toEqual([{ kind: "intake", count: 2 }, { kind: "feedback", count: 2 }]);
    expect(out.by_method).toEqual([{ method: "cash", amount: 20_000 }]);
    // The days of the week know only the cards that still exist.
    expect(out.by_weekday.reduce((sum, d) => sum + d.held, 0)).toBe(1);
  });
});

describe("the assessment pathway", () => {
  it("takes medians (of an even count, the two middle ones' mean, rounded down) and says none when there is none", () => {
    expect(median([])).toBeNull();
    expect(median([7])).toBe(7);
    expect(median([9, 1, 4])).toBe(4);
    expect(median([34, 20])).toBe(27);
    expect(median([15, 20])).toBe(17);

    const a = patient({ waiting_since: "2026-08-01" });
    const b = patient({ waiting_since: "2026-09-01" });
    const c = patient({ status: "waiting", waiting_since: "2026-11-01" });
    const d = patient({ status: "waiting", created_on: "2026-11-10" });
    const input = records({
      patients: [a, b, c, d],
      visits: [
        visit(a.id, "2026-09-01", { kind: "intake" }), visit(a.id, "2026-09-10"), visit(a.id, "2026-09-20"), visit(a.id, "2026-10-05", { kind: "feedback" }),
        // No intake: from the first visit held.
        visit(b.id, "2026-10-01"), visit(b.id, "2026-10-21", { kind: "feedback" }),
      ],
    });
    const out = fakeInsights(input, "2026-09", "2026-11", TODAY);
    expect(out.assessments_done).toBe(2);
    expect(out.intake_to_feedback_days).toBe(27); // 34 and 20
    expect(out.assessment_to_feedback_days).toBe(17); // 15 and 20
    expect(out.waited_days).toBe(30); // 31 and 30
    expect([out.waiting_now, out.waiting_now_days]).toEqual([2, 9]); // 14 and 5 days

    const quiet = fakeInsights(records({ patients: [patient()] }), "2026-11", "2026-11", TODAY);
    expect([quiet.intake_to_feedback_days, quiet.assessment_to_feedback_days, quiet.waited_days, quiet.waiting_now_days]).toEqual([null, null, null, null]);
  });
});

describe("debts by age", () => {
  it("ages each family's debt from its oldest charge not yet covered, in four buckets", () => {
    const owes = (date: string, amount: number, ...paid: number[]) => {
      const p = patient();
      return { p, charges: [charge(p.id, date, amount)], payments: paid.map((x) => payment(p.id, date, x)) };
    };
    const families = [
      owes("2026-10-16", 10_000), // 30 days: the first bucket
      owes("2026-10-15", 20_000), // 31 days
      owes("2026-08-17", 30_000), // 90 days
      owes("2026-08-16", 40_000, 15_000), // 91 days, partly paid: 250 owed
      owes("2026-12-01", 5_000), // dated ahead: 0 days old
      owes("2026-03-01", 50_000, 50_000), // paid: owes nothing
      owes("2026-03-01", 10_000, 30_000), // paid ahead: owes nothing
    ];
    // The oldest is paid first: what is left is the newer charge, so this debt is a fresh one.
    const old = patient();
    const voided = patient();
    const input = records({
      patients: [...families.map((f) => f.p), old, voided],
      charges: [...families.flatMap((f) => f.charges), charge(old.id, "2026-06-01", 30_000), charge(old.id, "2026-11-10", 40_000), charge(voided.id, "2026-01-01", 90_000, true)],
      payments: [...families.flatMap((f) => f.payments), payment(old.id, "2026-07-01", 30_000)],
    });
    const out = fakeInsights(input, "2026-11", "2026-11", TODAY);
    expect(out.debt).toEqual([
      { label: "עד 30 יום", from_days: 0, patients: 3, amount: 10_000 + 5_000 + 40_000 },
      { label: "31–60 יום", from_days: 31, patients: 1, amount: 20_000 },
      { label: "61–90 יום", from_days: 61, patients: 1, amount: 30_000 },
      { label: "מעל 90 יום", from_days: 91, patients: 1, amount: 25_000 },
    ]);
    // A clinic with no debt still has its four rows.
    expect(fakeInsights(records(), "2026-11", "2026-11", TODAY).debt.map((d) => [d.label, d.patients, d.amount])).toEqual([
      ["עד 30 יום", 0, 0], ["31–60 יום", 0, 0], ["61–90 יום", 0, 0], ["מעל 90 יום", 0, 0],
    ]);
  });
});

describe("referrals and income by method", () => {
  it("groups sources without regard to case or spaces, lists the first eight, and folds the rest into 'אחרים'", () => {
    const opened = (referred_by: string, created_on = "2026-11-02") => patient({ referred_by, created_on });
    const input = records({
      patients: [
        opened("גננת", "2026-11-01"), opened("  גננת "), opened("גננת"), opened("גננת  "),
        opened("Dr. Levi", "2026-11-01"), opened("dr.   levi"),
        opened(""), opened("   "),
        ...["מקור א", "מקור ב", "מקור ג", "מקור ד", "מקור ה", "מקור ו", "מקור ז"].map((s) => opened(s)),
        // Opened before the stretch: not a new patient of it.
        opened("גננת", "2026-10-31"),
      ],
    });
    const out = fakeInsights(input, "2026-11", "2026-11", TODAY);
    expect(out.new_patients).toBe(15);
    expect(out.referrals).toEqual([
      { source: "גננת", count: 4 },
      { source: "Dr. Levi", count: 2 },
      { source: "לא צוין", count: 2 },
      { source: "מקור א", count: 1 },
      { source: "מקור ב", count: 1 },
      { source: "מקור ג", count: 1 },
      { source: "מקור ד", count: 1 },
      { source: "מקור ה", count: 1 },
      { source: "אחרים", count: 2 },
    ]);
  });

  it("adds up what was received by method, in the screens' order, without what was voided", () => {
    const p = patient();
    const input = records({
      patients: [p],
      payments: [
        payment(p.id, "2026-11-01", 20_000, { method: "transfer" }), payment(p.id, "2026-11-02", 10_000, { method: "cash" }),
        payment(p.id, "2026-11-03", 5_000, { method: "transfer" }), payment(p.id, "2026-11-04", 70_000, { method: "bit", voided: true }),
        payment(p.id, "2026-10-31", 9_000, { method: "card" }),
      ],
    });
    const out = fakeInsights(input, "2026-11", "2026-11", TODAY);
    expect(out.by_method).toEqual([{ method: "cash", amount: 10_000 }, { method: "transfer", amount: 25_000 }]);
    expect(out.received).toBe(35_000);
  });
});
