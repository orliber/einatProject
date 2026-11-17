// The preview's clinic keeps the rules of docs/CLINIC.md: balances, series, "this and the
// following", overlaps, alerts, history, messages and the link to cases. Every name is invented.
import { clinicApi, dataOf, type ReplyData, type ReplyKind } from "../ipc/clinic";
import type { ClinicRequest } from "../ipc/generated/ClinicRequest";
import type { ClinicTask } from "../ipc/generated/ClinicTask";
import type { Patient } from "../ipc/generated/Patient";
import type { Payment } from "../ipc/generated/Payment";
import type { Today } from "../ipc/generated/Today";
import type { UiError } from "../ipc/generated/UiError";
import type { Visit } from "../ipc/generated/Visit";
import type { VisitKind } from "../ipc/generated/VisitKind";
import { addDays, today as localToday } from "../screens/clinic/format";
import { FakeClinic, fillMessage, standInHost, visitCharge, visitsOverlap } from "./fakeClinic";
import { FakeCore } from "./fakeCore";

const DAY = "2026-11-15"; // a Sunday
const NOW: Today = { date: DAY, minute: 12 * 60 };

function clinic(host = standInHost(), clock?: () => Date) {
  const c = new FakeClinic(host, clock ? { demo: false, clock } : { demo: false });
  const ask = <K extends ReplyKind>(request: ClinicRequest, kind: K): ReplyData<K> => dataOf(request.op, c.answer(request), kind);
  return { ask, host, fake: c };
}
type Ask = ReturnType<typeof clinic>["ask"];

const patient = (over: Partial<Patient> = {}): Patient => ({
  id: "", name: "נועם", birth_date: "2021-06-01", gender: "male", contacts: [{ role: "mother", name: "מיכל", phone: "", email: "" }],
  framework: "", referred_by: "", reason: "", notes: "", status: "active", waiting_since: null, urgent: false, session_fee: null,
  case_id: null, created_on: "2026-09-01", ...over,
});

const visit = (patientId: string | null, over: Partial<Visit> = {}): Visit => ({
  id: "", patient_id: patientId, kind: "assessment", title: "", date: DAY, start: 9 * 60, minutes: 90, place: "הקליניקה", status: "planned",
  status_note: "", fee: 60_000, charge_missed: false, series_id: null, note: "", ...over,
});

const payment = (patientId: string, amount: number, date = DAY): Payment => ({
  id: "", patient_id: patientId, date, amount, method: "bit", receipt: "", payer: "", note: "", voided: false, void_reason: "",
});

const task = (over: Partial<ClinicTask> = {}): ClinicTask => ({ id: "", patient_id: null, kind: "call", title: "להתקשר", due: null, done_on: null, note: "", ...over });

function addPatient(ask: Ask, over: Partial<Patient> = {}): Patient {
  return ask({ op: "save_patient", patient: patient(over) }, "patient");
}

/** A visit as it is after it was scheduled and then marked. */
function addVisit(ask: Ask, patientId: string | null, over: Partial<Visit> = {}): Visit {
  const made = ask({ op: "save_visit", visit: visit(patientId, over), repeat: null, scope: "one" }, "saved").visits[0]?.visit as Visit;
  if (!over.status || over.status === "planned") return made;
  const marked = { ...made, status: over.status, charge_missed: over.charge_missed ?? false, status_note: over.status_note ?? "" };
  return ask({ op: "save_visit", visit: marked, repeat: null, scope: "one" }, "saved").visits[0]?.visit as Visit;
}

function refusal(run: () => unknown): UiError {
  try {
    run();
  } catch (e) {
    return e as UiError;
  }
  throw new Error("expected a refusal");
}

const balanceOf = (ask: Ask, id: string) => ask({ op: "patient_card", id, today: NOW }, "card").balance;
const alertsOf = (ask: Ask, today: Today = NOW) => ask({ op: "overview", today }, "overview").alerts;

describe("the typed client", () => {
  it("an answer of another kind is a fault to report, never a silent cast", () => {
    const wrong = refusal(() => dataOf("overview", { kind: "done" }, "overview"));
    expect(wrong).toMatchObject({ code: "internal", action: { kind: "report" } });
    expect(wrong.message).toMatch(/[א-ת]/);
    // Only fixed words of the program go into the report: the op and the two kinds.
    expect(wrong.details).toEqual(["clinic overview: expected overview, got done"]);
    expect(refusal(() => dataOf("settings", null as never, "settings")).details).toEqual(["clinic settings: expected settings, got nothing"]);
    expect(dataOf("delete_task", { kind: "done" }, "done")).toBeNull();
    expect(dataOf("message", { kind: "text", data: "שלום" }, "text")).toBe("שלום");
  });

  it("has one function for every request the clinic takes", () => {
    expect(Object.keys(clinicApi).sort()).toEqual([
      "calendar", "caseChoices", "createCase", "deleteNote", "deleteTask", "deleteVisit", "diary", "erasePatient", "freeSlots", "history", "holidays",
      "income", "insights", "linkCase", "message", "noteToCase", "overview", "patientCard", "patients", "proposePathway", "saveCharge", "saveNote",
      "savePathway", "savePatient", "savePayment", "saveSettings", "saveTask", "saveVisit", "settings", "starterTasks", "tasks", "voidCharge",
      "voidPayment",
    ]);
  });
});

describe("charges and balances", () => {
  it("a visit costs its fee when held, or when missed and charged anyway", () => {
    const v = visit("p");
    expect(visitCharge(v)).toBe(0);
    expect(visitCharge({ ...v, status: "held" })).toBe(60_000);
    expect(visitCharge({ ...v, status: "cancelled" })).toBe(0);
    expect(visitCharge({ ...v, status: "cancelled", charge_missed: true })).toBe(60_000);
    expect(visitCharge({ ...v, status: "no_show", charge_missed: true })).toBe(60_000);
    expect(visitCharge({ ...v, status: "no_show" })).toBe(0);
  });

  it("due is what was charged less what was paid; voided lines do not count", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    addVisit(ask, p.id, { status: "held", date: "2026-11-01" });
    addVisit(ask, p.id, { status: "no_show", charge_missed: true, date: "2026-11-08", fee: 30_000 });
    addVisit(ask, p.id, { status: "cancelled", date: "2026-11-09" });
    addVisit(ask, p.id, { date: "2026-11-20" });
    const charge = ask({ op: "save_charge", charge: { id: "", patient_id: p.id, date: DAY, amount: 120_000, what: "כתיבת דוח", voided: false, void_reason: "" } }, "charge");
    const first = ask({ op: "save_payment", payment: payment(p.id, 50_000) }, "payment");
    ask({ op: "save_payment", payment: payment(p.id, 20_000) }, "payment");
    expect(balanceOf(ask, p.id)).toEqual({ charged: 210_000, paid: 70_000, due: 140_000 });

    expect(refusal(() => ask({ op: "void_payment", id: first.id, reason: " " }, "payment")).message).toContain("למה");
    expect(ask({ op: "void_payment", id: first.id, reason: "נרשם פעמיים" }, "payment")).toMatchObject({ voided: true, void_reason: "נרשם פעמיים" });
    ask({ op: "void_charge", id: charge.id, reason: "סוכם אחרת" }, "charge");
    expect(balanceOf(ask, p.id)).toEqual({ charged: 90_000, paid: 20_000, due: 70_000 });
    // A voided payment stays, and cannot be edited back to life.
    expect(refusal(() => ask({ op: "save_payment", payment: { ...first, amount: 1 } }, "payment")).code).toBe("refused");
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").payments).toHaveLength(2);
  });

  it("a family that paid ahead has a negative due, and the total owed ignores it", () => {
    const { ask } = clinic();
    const a = addPatient(ask);
    const b = addPatient(ask, { name: "תמר" });
    ask({ op: "save_payment", payment: payment(a.id, 40_000) }, "payment");
    addVisit(ask, b.id, { status: "held" });
    expect(balanceOf(ask, a.id).due).toBe(-40_000);
    const o = ask({ op: "overview", today: NOW }, "overview");
    expect(o.total_due).toBe(60_000);
    expect(o.month_received).toBe(40_000);
  });

  it("refuses an amount that is not money", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    for (const amount of [0, -5, 1.5, 10_000_001]) {
      expect(refusal(() => ask({ op: "save_payment", payment: payment(p.id, amount) }, "payment")).code, String(amount)).toBe("refused");
    }
    expect(refusal(() => ask({ op: "save_visit", visit: visit(p.id, { fee: -1 }), repeat: null, scope: "one" }, "saved")).code).toBe("refused");
  });
});

describe("visits", () => {
  it("a new visit is planned, whatever was sent, and needs a real patient", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const made = ask({ op: "save_visit", visit: visit(p.id, { status: "held", series_id: "forged" }), repeat: null, scope: "one" }, "saved");
    expect(made.visits).toHaveLength(1);
    expect(made.visits[0]).toMatchObject({ patient_name: "נועם", has_note: false, overlaps: false, visit: { status: "planned", series_id: null } });
    expect(made).toMatchObject({ overlapping: 0, left_alone: 0 });
    // A patient that is not there is said as the core says it.
    expect(refusal(() => ask({ op: "save_visit", visit: visit("nobody"), repeat: null, scope: "one" }, "saved"))).toMatchObject({ code: "refused", message: "המטופל שנבחר לא נמצא. ייתכן שהכרטיס נמחק." });
    expect(refusal(() => ask({ op: "save_visit", visit: visit(null), repeat: null, scope: "one" }, "saved")).message).toContain("מטופל");
  });

  it("a block has no patient and no fee", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    expect(refusal(() => ask({ op: "save_visit", visit: visit(p.id, { kind: "block", fee: 0 }), repeat: null, scope: "one" }, "saved")).code).toBe("refused");
    expect(refusal(() => ask({ op: "save_visit", visit: visit(null, { kind: "block" }), repeat: null, scope: "one" }, "saved")).code).toBe("refused");
    const made = ask({ op: "save_visit", visit: visit(null, { kind: "block", fee: 0, title: "כתיבת דוחות" }), repeat: null, scope: "one" }, "saved");
    expect(made.visits[0]?.patient_name).toBeNull();
  });

  it("keeps a visit inside one day and inside the limits", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const bad: Partial<Visit>[] = [{ start: 23 * 60, minutes: 90 }, { minutes: 4 }, { minutes: 721 }, { date: "2026-02-30" }, { start: 1440 }, { title: "א".repeat(201) }];
    for (const over of bad) {
      expect(refusal(() => ask({ op: "save_visit", visit: visit(p.id, over), repeat: null, scope: "one" }, "saved")).code, JSON.stringify(over)).toBe("refused");
    }
    expect(ask({ op: "save_visit", visit: visit(p.id, { start: 23 * 60, minutes: 60 }), repeat: null, scope: "one" }, "saved").visits).toHaveLength(1);
  });

  it("scheduling a family from the waiting list makes them active", () => {
    const { ask } = clinic();
    const p = addPatient(ask, { status: "waiting", waiting_since: "2026-10-01" });
    addVisit(ask, p.id);
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").patient.status).toBe("active");
  });

  it("only a planned visit can be deleted, and a deleted one leaves its history", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const held = addVisit(ask, p.id, { status: "held" });
    const planned = addVisit(ask, p.id, { date: "2026-11-20" });
    expect(refusal(() => ask({ op: "delete_visit", id: held.id, scope: "one" }, "done")).message).toContain("מתוכננת");
    ask({ op: "delete_visit", id: planned.id, scope: "one" }, "done");
    expect(ask({ op: "calendar", from: "2026-11-01", to: "2026-11-30" }, "visits").map((v) => v.visit.id)).toEqual([held.id]);
    expect(ask({ op: "history", record_id: planned.id }, "history").map((h) => h.action)).toEqual(["created", "deleted"]);
    expect(ask({ op: "history", record_id: held.id }, "history").map((h) => h.action)).toEqual(["created", "changed"]);
  });

  it("a visit that has a note is neither deleted nor moved to another patient", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const q = addPatient(ask, { name: "תמר" });
    const made = ask({ op: "save_visit", visit: visit(p.id, { kind: "therapy" }), repeat: { every_weeks: 1, count: 4 }, scope: "one" }, "saved").visits.map((v) => v.visit);
    const [first, second, third] = made as [Visit, Visit, Visit, Visit];
    for (const v of [first, third]) ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: v.id, date: v.date, record: "הכנה לפגישה", personal: "", case_input_id: null } }, "note");
    expect(refusal(() => ask({ op: "delete_visit", id: first.id, scope: "one" }, "done")).message).toContain("יש סיכום");
    expect(refusal(() => ask({ op: "save_visit", visit: { ...first, patient_id: q.id }, repeat: null, scope: "one" }, "saved")).message).toContain("למטופל אחר");
    // "This and the following": the later one that has a record written for it stays.
    ask({ op: "delete_visit", id: second.id, scope: "following" }, "done");
    expect(ask({ op: "calendar", from: "2026-11-01", to: "2026-12-31" }, "visits").map((v) => v.visit.id)).toEqual([first.id, third.id]);
  });

  it("a save that changes nothing leaves no line in the history", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const v = addVisit(ask, p.id);
    ask({ op: "save_visit", visit: v, repeat: null, scope: "one" }, "saved");
    expect(ask({ op: "history", record_id: v.id }, "history").map((h) => h.action)).toEqual(["created"]);
  });

  it("the calendar gives the days asked for, both ends included, by time", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    addVisit(ask, p.id, { date: "2026-11-16", start: 600 });
    addVisit(ask, p.id, { date: "2026-11-15", start: 900 });
    addVisit(ask, p.id, { date: "2026-11-15", start: 480 });
    addVisit(ask, p.id, { date: "2026-11-17" });
    const got = ask({ op: "calendar", from: "2026-11-15", to: "2026-11-16" }, "visits");
    expect(got.map((v) => [v.visit.date, v.visit.start])).toEqual([["2026-11-15", 480], ["2026-11-15", 900], ["2026-11-16", 600]]);
  });
});

describe("the calendar's range", () => {
  it("two real dates in order, at most a hundred days", () => {
    const { ask } = clinic();
    expect(ask({ op: "calendar", from: "2026-11-01", to: "2027-02-08" }, "visits")).toEqual([]);
    for (const [from, to] of [["2026-11-01", "2027-02-09"], ["2026-11-02", "2026-11-01"], ["2026-11-31", "2026-12-01"], ["", ""]] as const) {
      expect(refusal(() => ask({ op: "calendar", from, to }, "visits")).code, `${from}..${to}`).toBe("refused");
    }
  });
});

describe("a repeating series", () => {
  function series(ask: Ask, patientId: string, every = 1, count = 4) {
    return ask({ op: "save_visit", visit: visit(patientId, { kind: "therapy", minutes: 50, fee: 38_000 }), repeat: { every_weeks: every, count }, scope: "one" }, "saved").visits.map((v) => v.visit);
  }

  it("creates the visits weeks apart with one series id", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const made = series(ask, p.id, 2, 3);
    expect(made.map((v) => v.date)).toEqual(["2026-11-15", "2026-11-29", "2026-12-13"]);
    expect(new Set(made.map((v) => v.series_id)).size).toBe(1);
    expect(made[0]?.series_id).not.toBeNull();
    expect(new Set(made.map((v) => v.id)).size).toBe(3);
  });

  it("refuses a repeat out of range, and a repeat on a visit that exists", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    for (const repeat of [{ every_weeks: 0, count: 4 }, { every_weeks: 9, count: 4 }, { every_weeks: 1, count: 1 }, { every_weeks: 1, count: 53 }]) {
      expect(refusal(() => ask({ op: "save_visit", visit: visit(p.id), repeat, scope: "one" }, "saved")).code).toBe("refused");
    }
    const made = addVisit(ask, p.id);
    expect(refusal(() => ask({ op: "save_visit", visit: made, repeat: { every_weeks: 1, count: 2 }, scope: "one" }, "saved")).message).toContain("חדשה");
  });

  it("“only this one” changes one visit", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const made = series(ask, p.id);
    const second = made[1] as Visit;
    const saved = ask({ op: "save_visit", visit: { ...second, start: 17 * 60, date: "2026-11-23" }, repeat: null, scope: "one" }, "saved");
    expect(saved.visits).toHaveLength(1);
    const all = ask({ op: "calendar", from: "2026-11-01", to: "2026-12-31" }, "visits").map((v) => [v.visit.date, v.visit.start]);
    expect(all).toEqual([["2026-11-15", 540], ["2026-11-23", 1020], ["2026-11-29", 540], ["2026-12-06", 540]]);
  });

  it("“this and the following” moves and changes the later planned ones, and leaves the earlier and the marked alone", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const made = series(ask, p.id, 1, 5);
    // The fourth was already cancelled: its status is its own, and it does not move.
    ask({ op: "save_visit", visit: { ...(made[3] as Visit), status: "cancelled", status_note: "חג" }, repeat: null, scope: "one" }, "saved");
    const second = made[1] as Visit;
    const change = { ...second, date: addDays(second.date, 2), start: 16 * 60, minutes: 45, place: "שיחת וידאו", fee: 40_000, note: "להביא משחק", title: "ליווי", status: "held" as const };
    const saved = ask({ op: "save_visit", visit: change, repeat: null, scope: "following" }, "saved");
    expect(saved.visits.map((v) => v.visit.id)).toEqual([made[1]?.id, made[2]?.id, made[4]?.id]);
    // The reply says how many later visits were left exactly as they were (D-176).
    expect(saved.left_alone).toBe(1);

    const all = ask({ op: "calendar", from: "2026-11-01", to: "2026-12-31" }, "visits").map((v) => v.visit);
    const byId = (id: string | undefined) => all.find((v) => v.id === id) as Visit;
    expect(byId(made[0]?.id)).toMatchObject({ date: "2026-11-15", start: 540, fee: 38_000, title: "", status: "planned" });
    expect(byId(made[1]?.id)).toMatchObject({ date: "2026-11-24", start: 960, minutes: 45, place: "שיחת וידאו", fee: 40_000, note: "להביא משחק", title: "ליווי", status: "held" });
    // The same change, moved by the same two days; still planned.
    expect(byId(made[2]?.id)).toMatchObject({ date: "2026-12-01", start: 960, minutes: 45, place: "שיחת וידאו", fee: 40_000, note: "להביא משחק", title: "ליווי", status: "planned" });
    expect(byId(made[3]?.id)).toMatchObject({ date: "2026-12-06", start: 540, status: "cancelled", fee: 38_000 });
    expect(byId(made[4]?.id)).toMatchObject({ date: "2026-12-15", start: 960, status: "planned" });
    expect(new Set(all.map((v) => v.series_id)).size).toBe(1);
  });

  it("“this and the following” on a visit that is not of a series changes only it", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const a = addVisit(ask, p.id);
    addVisit(ask, p.id, { date: "2026-11-22" });
    expect(ask({ op: "save_visit", visit: { ...a, start: 600 }, repeat: null, scope: "following" }, "saved").visits).toHaveLength(1);
  });

  it("deleting “this and the following” removes the later planned ones only", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const made = series(ask, p.id, 1, 5);
    ask({ op: "save_visit", visit: { ...(made[3] as Visit), status: "held" }, repeat: null, scope: "one" }, "saved");
    ask({ op: "delete_visit", id: (made[1] as Visit).id, scope: "following" }, "done");
    const left = ask({ op: "calendar", from: "2026-11-01", to: "2026-12-31" }, "visits").map((v) => v.visit.id);
    expect(left).toEqual([made[0]?.id, made[3]?.id]);
  });

  it("“this and the following” never moves a later visit that has a note, planned or not, and counts it (D-176)", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const made = series(ask, p.id, 1, 5);
    const [, second, third, fourth, fifth] = made as [Visit, Visit, Visit, Visit, Visit];
    // The third is still planned, and a record was already written for it.
    ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: third.id, date: third.date, record: "הכנה למפגש", personal: "", case_input_id: null } }, "note");
    ask({ op: "save_visit", visit: { ...fourth, status: "no_show" }, repeat: null, scope: "one" }, "saved");
    const saved = ask({ op: "save_visit", visit: { ...second, date: addDays(second.date, 1), start: 17 * 60 }, repeat: null, scope: "following" }, "saved");
    expect(saved.left_alone).toBe(2);
    expect(saved.visits.map((v) => v.visit.id)).toEqual([second.id, fifth.id]);
    const all = ask({ op: "calendar", from: "2026-11-01", to: "2026-12-31" }, "visits").map((v) => v.visit);
    expect(all.find((v) => v.id === third.id)).toEqual(third);
    expect(all.find((v) => v.id === fourth.id)).toMatchObject({ date: fourth.date, start: fourth.start, status: "no_show" });
    expect(all.find((v) => v.id === fifth.id)).toMatchObject({ date: addDays(fifth.date, 1), start: 17 * 60 });
    // Nothing was written for the ones left alone: their history has no new line.
    expect(ask({ op: "history", record_id: third.id }, "history")).toHaveLength(1);
    // "Only this one", and a visit that is in no series: nothing is left alone.
    expect(ask({ op: "save_visit", visit: { ...second, date: addDays(second.date, 1), start: 18 * 60 }, repeat: null, scope: "one" }, "saved").left_alone).toBe(0);
    const single = addVisit(ask, p.id, { date: "2026-11-17" });
    expect(ask({ op: "save_visit", visit: { ...single, start: 600 }, repeat: null, scope: "following" }, "saved").left_alone).toBe(0);
  });

  it("a later visit that the change would not alter is not written again", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const made = series(ask, p.id, 1, 3);
    const first = made[0] as Visit;
    // Only the status changes, and that is the visit's own: the later ones stay untouched.
    const saved = ask({ op: "save_visit", visit: { ...first, status: "held" }, repeat: null, scope: "following" }, "saved");
    expect(saved.visits.map((v) => v.visit.id)).toEqual([first.id]);
    expect(ask({ op: "history", record_id: (made[1] as Visit).id }, "history")).toHaveLength(1);
  });

  it("deleting “this and the following” skips a later visit that was ever marked (D-176)", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const made = series(ask, p.id, 1, 4);
    const [first, second, third, fourth] = made as [Visit, Visit, Visit, Visit];
    // The third was cancelled by mistake and put back: it says "planned", and its history remembers.
    const cancelled = ask({ op: "save_visit", visit: { ...third, status: "cancelled" }, repeat: null, scope: "one" }, "saved").visits[0]?.visit as Visit;
    ask({ op: "save_visit", visit: { ...cancelled, status: "planned" }, repeat: null, scope: "one" }, "saved");
    ask({ op: "delete_visit", id: second.id, scope: "following" }, "done");
    expect(ask({ op: "calendar", from: "2026-11-01", to: "2026-12-31" }, "visits").map((v) => v.visit.id)).toEqual([first.id, third.id]);
    expect(ask({ op: "history", record_id: fourth.id }, "history").map((h) => h.action)).toEqual(["created", "deleted"]);
  });
});

describe("a mark stays (D-176)", () => {
  const ONCE = "הפגישה הזו כבר סומנה פעם (התקיימה, בוטלה או לא הגיעו), ולכן היא נשארת ביומן. אם היא לא תתקיים, אפשר לסמן אותה כמבוטלת.";
  const NOTED = "לפגישה הזו יש סיכום, ולכן היא לא יכולה לחזור להיות \"מתוכננת\". אם היא לא התקיימה, אפשר לסמן אותה כמבוטלת ולכתוב מה קרה.";
  const PAID = "על הפגישה הזו כבר נרשם תשלום, ולכן היא לא יכולה לחזור להיות \"מתוכננת\". אם היא סומנה בטעות, אפשר לסמן אותה כמבוטלת בלי חיוב ולכתוב מה קרה, או לבטל קודם את התקבול.";
  const mark = (ask: Ask, v: Visit, over: Partial<Visit>) => ask({ op: "save_visit", visit: { ...v, ...over }, repeat: null, scope: "one" }, "saved").visits[0]?.visit as Visit;

  it("a visit that was ever marked cannot be deleted, even when it says “planned” again", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    for (const status of ["held", "cancelled", "no_show"] as const) {
      const v = addVisit(ask, p.id, { status, date: "2026-11-10" });
      expect(refusal(() => ask({ op: "delete_visit", id: v.id, scope: "one" }, "done")).message).toBe("אפשר למחוק רק פגישה שעדיין מתוכננת. פגישה שכבר סומנה אפשר לסמן כמבוטלת.");
      const back = mark(ask, v, { status: "planned" });
      expect(back.status).toBe("planned");
      expect(refusal(() => ask({ op: "delete_visit", id: v.id, scope: "one" }, "done"))).toMatchObject({ code: "refused", message: ONCE });
    }
    expect(ask({ op: "calendar", from: "2026-11-01", to: "2026-11-30" }, "visits")).toHaveLength(3);
    // One that was never marked goes as before.
    const fresh = addVisit(ask, p.id, { date: "2026-11-20" });
    ask({ op: "delete_visit", id: fresh.id, scope: "one" }, "done");
  });

  it("a mark is taken back only while the visit has no note", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const v = addVisit(ask, p.id, { status: "held", fee: 0 });
    ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: v.id, date: v.date, record: "סיכום", personal: "", case_input_id: null } }, "note");
    expect(refusal(() => mark(ask, v, { status: "planned" }))).toMatchObject({ code: "refused", message: NOTED });
    // Any other change of the mark is always allowed, and kept in the history.
    expect(mark(ask, v, { status: "cancelled", status_note: "נרשם בטעות" }).status).toBe("cancelled");
    expect(mark(ask, v, { status: "no_show" }).status).toBe("no_show");
    expect(mark(ask, v, { status: "held" }).status).toBe("held");
    expect(ask({ op: "history", record_id: v.id }, "history").map((h) => h.action)).toEqual(["created", "changed", "changed", "changed", "changed"]);
  });

  it("a mark is taken back only while what was paid is not more than what is left charged without it", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const first = addVisit(ask, p.id, { status: "held", date: "2026-11-08" });
    const second = addVisit(ask, p.id, { status: "held", date: "2026-11-10" });
    const paid = ask({ op: "save_payment", payment: payment(p.id, 60_000) }, "payment");
    // 120,000 charged, 60,000 paid: without one visit 60,000 is left, and that covers the payment.
    expect(mark(ask, second, { status: "planned" }).status).toBe("planned");
    // Now only the first is charged, and it is what the payment paid for.
    expect(refusal(() => mark(ask, first, { status: "planned" }))).toMatchObject({ code: "refused", message: PAID });
    expect(balanceOf(ask, p.id)).toEqual({ charged: 60_000, paid: 60_000, due: 0 });
    // A missed visit that was charged counts the same; one that charges nothing is never held back.
    const missed = addVisit(ask, p.id, { status: "no_show", charge_missed: true, date: "2026-11-11", fee: 10_000 });
    ask({ op: "save_payment", payment: payment(p.id, 10_000) }, "payment");
    expect(refusal(() => mark(ask, missed, { status: "planned" })).message).toBe(PAID);
    const free = addVisit(ask, p.id, { status: "cancelled", date: "2026-11-12" });
    expect(mark(ask, free, { status: "planned" }).status).toBe("planned");
    // Once the payment is voided nothing leans on the mark any more.
    ask({ op: "void_payment", id: paid.id, reason: "נרשם בכרטיס הלא נכון" }, "payment");
    expect(mark(ask, first, { status: "planned" }).status).toBe("planned");
  });

  it("another family's payments never hold a mark back", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const q = addPatient(ask, { name: "תמר" });
    const v = addVisit(ask, p.id, { status: "held" });
    ask({ op: "save_payment", payment: payment(q.id, 60_000) }, "payment");
    expect(mark(ask, v, { status: "planned" }).status).toBe("planned");
  });
});

describe("a record stays in its card (D-177)", () => {
  it("a payment and a charge never move to another patient", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const q = addPatient(ask, { name: "תמר" });
    const paid = ask({ op: "save_payment", payment: payment(p.id, 45_000) }, "payment");
    const charge = ask({ op: "save_charge", charge: { id: "", patient_id: p.id, date: DAY, amount: 120_000, what: "כתיבת דוח", voided: false, void_reason: "" } }, "charge");
    expect(refusal(() => ask({ op: "save_payment", payment: { ...paid, patient_id: q.id } }, "payment"))).toMatchObject({
      code: "refused", message: "אי אפשר להעביר תקבול למטופל אחר. אפשר לבטל את התקבול הזה, לכתוב למה, ולרשום תקבול חדש בכרטיס הנכון.",
    });
    expect(refusal(() => ask({ op: "save_charge", charge: { ...charge, patient_id: q.id } }, "charge"))).toMatchObject({
      code: "refused", message: "אי אפשר להעביר חיוב למטופל אחר. אפשר לבטל את החיוב הזה, לכתוב למה, ולרשום חיוב חדש בכרטיס הנכון.",
    });
    expect(balanceOf(ask, p.id)).toEqual({ charged: 120_000, paid: 45_000, due: 75_000 });
    expect(balanceOf(ask, q.id)).toEqual({ charged: 0, paid: 0, due: 0 });
    // In its own card it is still corrected, and a save that changes nothing writes nothing.
    expect(ask({ op: "save_payment", payment: { ...paid, receipt: "1071" } }, "payment").receipt).toBe("1071");
    ask({ op: "save_payment", payment: { ...paid, receipt: "1071" } }, "payment");
    ask({ op: "save_charge", charge }, "charge");
    expect(ask({ op: "history", record_id: paid.id }, "history").map((h) => h.action)).toEqual(["created", "changed"]);
    expect(ask({ op: "history", record_id: charge.id }, "history").map((h) => h.action)).toEqual(["created"]);
  });

  it("a visit never moves to another patient and never becomes a block; a block may be given a patient", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const q = addPatient(ask, { name: "תמר" });
    const v = addVisit(ask, p.id);
    expect(refusal(() => ask({ op: "save_visit", visit: { ...v, patient_id: q.id }, repeat: null, scope: "one" }, "saved"))).toMatchObject({
      code: "refused",
      message: "אי אפשר להעביר פגישה למטופל אחר. אם היא נקבעה בכרטיס הלא נכון, אפשר למחוק אותה (או לסמן אותה כמבוטלת, אם היא כבר סומנה) ולקבוע פגישה חדשה בכרטיס הנכון.",
    });
    expect(refusal(() => ask({ op: "save_visit", visit: { ...v, kind: "block", patient_id: null, fee: 0 }, repeat: null, scope: "one" }, "saved"))).toMatchObject({
      code: "refused", message: "אי אפשר להפוך פגישה של מטופל לזמן שמור. אפשר למחוק אותה (או לסמן אותה כמבוטלת, אם היא כבר סומנה) ולשמור זמן חדש ביומן.",
    });
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").visits).toHaveLength(1);
    const block = addVisit(ask, null, { kind: "block", fee: 0, date: "2026-11-17" });
    const given = ask({ op: "save_visit", visit: { ...block, kind: "guidance", patient_id: q.id }, repeat: null, scope: "one" }, "saved").visits[0];
    expect(given).toMatchObject({ patient_name: "תמר", visit: { id: block.id, kind: "guidance" } });
    // A task is a reminder, not a record: it may move.
    const t = ask({ op: "save_task", task: task({ patient_id: p.id }) }, "task");
    expect(ask({ op: "save_task", task: { ...t, patient_id: q.id } }, "task").patient_id).toBe(q.id);
  });

  it("erasing a card removes its own history, with what its removed records left, and nothing of another card", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const q = addPatient(ask, { name: "תמר" });
    const removed = addVisit(ask, p.id, { date: "2026-11-20" });
    ask({ op: "delete_visit", id: removed.id, scope: "one" }, "done");
    const kept = addVisit(ask, q.id, { status: "held" });
    const theirs = ask({ op: "save_payment", payment: payment(q.id, 60_000) }, "payment");
    const gone = addVisit(ask, q.id, { date: "2026-11-21" });
    ask({ op: "delete_visit", id: gone.id, scope: "one" }, "done");
    expect(ask({ op: "history", record_id: removed.id }, "history").map((h) => h.action)).toEqual(["created", "deleted"]);
    ask({ op: "erase_patient", id: p.id, name: "נועם" }, "done");
    expect(ask({ op: "history", record_id: removed.id }, "history")).toEqual([]);
    expect(ask({ op: "history", record_id: kept.id }, "history").map((h) => h.action)).toEqual(["created", "changed"]);
    expect(ask({ op: "history", record_id: theirs.id }, "history").map((h) => h.action)).toEqual(["created"]);
    expect(ask({ op: "history", record_id: gone.id }, "history").map((h) => h.action)).toEqual(["created", "deleted"]);
  });
});

describe("the names of a card are names its case knows (D-177)", () => {
  const META = { code: "תיק-יד", age: null, child_gender: null, current_section: null, retention_until: null, consent: null };
  const people = (host: ReturnType<typeof standInHost>, id: string) => host.identities(id).map((i) => [i.role, i.value, i.aliases]);

  it("linking a card to a hand-made case adds the child's name and every named contact, and unlinking takes nothing back", () => {
    const { ask, host } = clinic();
    const p = addPatient(ask, { name: "נועם לדוגמה", contacts: [
      { role: "mother", name: "מיכל", phone: "", email: "" }, { role: "guardian", name: "סבתא רבקה", phone: "", email: "" },
      { role: "other", name: "", phone: "שלוחה 12", email: "" }, { role: "father", name: "אלון", phone: "", email: "" },
    ] });
    // The case was opened by hand: it knows the child by a first name, and the father under another spelling.
    const made = host.createCase(META, [{ id: null, role: "child", value: "נועם", aliases: [] }, { id: null, role: "father", value: "אלון לדוגמה", aliases: ["אלון"] }]);
    ask({ op: "link_case", patient_id: p.id, case_id: made }, "patient");
    expect(people(host, made)).toEqual([
      ["child", "נועם", ["נועם לדוגמה"]], ["father", "אלון לדוגמה", ["אלון"]], ["mother", "מיכל", []], ["other", "סבתא רבקה", []],
    ]);
    // What the case already had keeps its id (and so its tag).
    const before = host.identities(made);
    ask({ op: "link_case", patient_id: p.id, case_id: null }, "patient");
    expect(host.identities(made)).toEqual(before);
    // Linking again changes nothing: every name is known.
    ask({ op: "link_case", patient_id: p.id, case_id: made }, "patient");
    expect(host.identities(made)).toEqual(before);
  });

  it("a case with no child gets one; a contact added to a linked card later is known at once, and one that left stays", () => {
    const { ask, host } = clinic();
    const p = addPatient(ask);
    const made = host.createCase(META, []);
    ask({ op: "link_case", patient_id: p.id, case_id: made }, "patient");
    expect(people(host, made)).toEqual([["child", "נועם", []], ["mother", "מיכל", []]]);
    const card = ask({ op: "patient_card", id: p.id, today: NOW }, "card").patient;
    ask({ op: "save_patient", patient: { ...card, name: "נועם בדוי", contacts: [{ role: "other", name: "דודה חנה", phone: "", email: "" }] } }, "patient");
    expect(people(host, made)).toEqual([["child", "נועם", ["נועם בדוי"]], ["mother", "מיכל", []], ["other", "דודה חנה", []]]);
    // Also while the case is in the recycle bin: it may come back.
    host.trash(made);
    ask({ op: "save_patient", patient: { ...card, name: "נועם בדוי", contacts: [{ role: "father", name: "אלון", phone: "", email: "" }] } }, "patient");
    expect(people(host, made).map((x) => x[1])).toEqual(["נועם", "מיכל", "דודה חנה", "אלון"]);
  });

  it("before a record goes to the case, a name the case lost is known to it again", () => {
    const { ask, host } = clinic();
    const p = addPatient(ask);
    const v = addVisit(ask, p.id, { status: "held", kind: "intake", date: "2026-11-01" });
    const n = ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: v.id, date: v.date, record: "מיכל סיפרה על הגן", personal: "", case_input_id: null } }, "note");
    const link = ask({ op: "create_case", patient_id: p.id, today: NOW }, "case");
    // She removed the mother from the case's people by hand.
    host.setIdentities(link.id, host.identities(link.id).filter((i) => i.role === "child"));
    ask({ op: "note_to_case", id: n.id }, "note");
    expect(people(host, link.id)).toEqual([["child", "נועם", []], ["mother", "מיכל", []]]);
    expect(host.inputs).toHaveLength(1);
  });

  it("nothing is linked, saved or sent when the names cannot be written", () => {
    const base = standInHost();
    let broken = false;
    const host = { ...base, setIdentities: (id: string, list: Parameters<typeof base.setIdentities>[1]) => { if (broken) throw { code: "internal", message: "הרשימה לא נשמרה.", details: [], action: { kind: "dismiss" } }; base.setIdentities(id, list); } };
    const { ask } = clinic(host);
    const p = addPatient(ask);
    const made = base.createCase(META, []);
    broken = true;
    expect(refusal(() => ask({ op: "link_case", patient_id: p.id, case_id: made }, "patient")).message).toBe("הרשימה לא נשמרה.");
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").patient.case_id).toBeNull();
    broken = false;
    ask({ op: "link_case", patient_id: p.id, case_id: made }, "patient");
    broken = true;
    const card = ask({ op: "patient_card", id: p.id, today: NOW }, "card").patient;
    expect(refusal(() => ask({ op: "save_patient", patient: { ...card, contacts: [{ role: "father", name: "אלון", phone: "", email: "" }] } }, "patient")).message).toBe("הרשימה לא נשמרה.");
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").patient.contacts.map((c) => c.name)).toEqual(["מיכל"]);
    const n = ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: null, date: DAY, record: "שיחה עם אלון", personal: "", case_input_id: null } }, "note");
    base.setIdentities(made, []);
    expect(refusal(() => ask({ op: "note_to_case", id: n.id }, "note")).message).toBe("הרשימה לא נשמרה.");
    expect(base.inputs).toEqual([]);
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").notes[0]?.case_input_id).toBeNull();
  });
});

describe("overlaps", () => {
  it("two visits overlap when their times meet on one date and neither is cancelled", () => {
    const a = visit("p", { id: "a", start: 540, minutes: 60 });
    expect(visitsOverlap(a, visit("q", { id: "b", start: 570, minutes: 60 }))).toBe(true);
    expect(visitsOverlap(a, visit("q", { id: "b", start: 500, minutes: 120 }))).toBe(true);
    // One ends exactly when the other starts: no overlap.
    expect(visitsOverlap(a, visit("q", { id: "b", start: 600, minutes: 60 }))).toBe(false);
    expect(visitsOverlap(a, visit("q", { id: "b", start: 570, date: "2026-11-16" }))).toBe(false);
    expect(visitsOverlap(a, visit("q", { id: "b", start: 570, status: "cancelled" }))).toBe(false);
    expect(visitsOverlap(a, a)).toBe(false);
  });

  it("an overlapping visit is saved anyway, and the reply says so", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const q = addPatient(ask, { name: "תמר" });
    const first = addVisit(ask, p.id, { start: 540, minutes: 90 });
    const saved = ask({ op: "save_visit", visit: visit(q.id, { start: 600, minutes: 60 }), repeat: null, scope: "one" }, "saved");
    expect(saved.overlapping).toBe(1);
    expect(saved.visits[0]?.overlaps).toBe(true);
    const day = ask({ op: "calendar", from: DAY, to: DAY }, "visits");
    expect(day.map((v) => v.overlaps)).toEqual([true, true]);
    // Cancelling one of them clears the other.
    ask({ op: "save_visit", visit: { ...first, status: "cancelled" }, repeat: null, scope: "one" }, "saved");
    expect(ask({ op: "calendar", from: DAY, to: DAY }, "visits").map((v) => v.overlaps)).toEqual([false, false]);
  });

  it("a block overlaps like any visit, and a series counts each clash", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    ask({ op: "save_visit", visit: visit(null, { kind: "block", fee: 0, start: 540, minutes: 120 }), repeat: null, scope: "one" }, "saved");
    addVisit(ask, p.id, { date: "2026-11-29", start: 560, minutes: 30 });
    const saved = ask({ op: "save_visit", visit: visit(p.id, { start: 570, minutes: 50 }), repeat: { every_weeks: 1, count: 3 }, scope: "one" }, "saved");
    expect(saved.visits.map((v) => v.overlaps)).toEqual([true, false, true]);
    expect(saved.overlapping).toBe(2);
  });
});

describe("alerts", () => {
  const kinds = (ask: Ask, today: Today = NOW) => alertsOf(ask, today).map((a) => a.kind);

  it("an empty clinic has nothing to say", () => {
    const { ask } = clinic();
    expect(alertsOf(ask)).toEqual([]);
  });

  it("a planned visit whose end has passed is unmarked; a block never is", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const v = addVisit(ask, p.id, { start: 9 * 60, minutes: 90, date: DAY });
    ask({ op: "save_visit", visit: visit(null, { kind: "block", fee: 0, date: "2026-11-10" }), repeat: null, scope: "one" }, "saved");
    expect(kinds(ask, { date: DAY, minute: 10 * 60 + 29 })).toEqual([]);
    const at = alertsOf(ask, { date: DAY, minute: 10 * 60 + 30 });
    expect(at).toHaveLength(1);
    expect(at[0]).toMatchObject({ kind: "unmarked", patient_id: p.id, patient_name: "נועם", visit_id: v.id, since: DAY });
    // The sentence does not carry the name: the screen shows it as a button of its own.
    expect(at[0]?.text).not.toContain("נועם");
    ask({ op: "save_visit", visit: { ...v, status: "held" }, repeat: null, scope: "one" }, "saved");
    expect(kinds(ask, { date: DAY, minute: 11 * 60 })).toEqual(["no_note"]);
  });

  it("a held session of the last 60 days with no note asks for one; a meeting does not", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const recent = addVisit(ask, p.id, { status: "held", date: "2026-11-10" });
    addVisit(ask, p.id, { status: "held", date: "2026-09-01" });
    addVisit(ask, p.id, { status: "held", date: "2026-11-11", kind: "meeting" });
    addVisit(ask, p.id, { date: "2026-11-20" });
    expect(alertsOf(ask).filter((a) => a.kind === "no_note").map((a) => a.visit_id)).toEqual([recent.id]);
    ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: recent.id, date: recent.date, record: "רשומה", personal: "", case_input_id: null } }, "note");
    expect(kinds(ask)).not.toContain("no_note");
  });

  it("a debt is pointed out once the oldest uncovered charge is old enough", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    addVisit(ask, p.id, { status: "held", date: "2026-10-01", kind: "intake", fee: 45_000 });
    const second = addVisit(ask, p.id, { status: "held", date: "2026-11-05" });
    for (const v of [second]) ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: v.id, date: v.date, record: "רשומה", personal: "", case_input_id: null } }, "note");
    addVisit(ask, p.id, { date: "2026-11-20" });
    const debt = () => alertsOf(ask).filter((a) => a.kind === "debt");
    expect(debt()).toHaveLength(1);
    expect(debt()[0]).toMatchObject({ since: "2026-10-01", patient_id: p.id });
    expect(debt()[0]?.text).toBe("יתרה פתוחה של 1050 ₪, מאז 01.10.2026.");
    // The payment covers the old session; what is left is ten days old.
    ask({ op: "save_payment", payment: payment(p.id, 45_000) }, "payment");
    expect(debt()).toEqual([]);
    expect(balanceOf(ask, p.id).due).toBe(60_000);
  });

  it("a family waiting longer than the setting is pointed out", () => {
    const { ask } = clinic();
    addPatient(ask, { status: "waiting", waiting_since: "2026-10-05", name: "עומר" });
    addPatient(ask, { status: "waiting", waiting_since: "2026-10-04", name: "שירה" });
    // Six whole weeks is 42 days: 2026-10-04 → 2026-11-15.
    const waiting = alertsOf(ask).filter((a) => a.kind === "waiting");
    expect(waiting.map((a) => a.patient_name)).toEqual(["שירה"]);
    expect(waiting[0]?.text).toContain("6");
  });

  it("the report is due once the testing ended and the days she set have passed", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const last = addVisit(ask, p.id, { status: "held", date: "2026-10-24" });
    ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: last.id, date: last.date, record: "רשומה", personal: "", case_input_id: null } }, "note");
    const due = (today: Today) => alertsOf(ask, today).filter((a) => a.kind === "report_due");
    // 21 days after 24.10 is 14.11: due only once that day has passed.
    expect(due({ date: "2026-11-14", minute: 600 })).toEqual([]);
    expect(due(NOW)).toHaveLength(1);
    expect(due(NOW)[0]?.since).toBe("2026-10-24");
    // Another session ahead: the testing is not over.
    const more = addVisit(ask, p.id, { date: "2026-11-20" });
    expect(due(NOW)).toEqual([]);
    ask({ op: "delete_visit", id: more.id, scope: "one" }, "done");
    // The feedback was given: nothing is due.
    addVisit(ask, p.id, { status: "held", kind: "feedback", date: "2026-11-12" });
    expect(due(NOW)).toEqual([]);
  });

  it("a feedback meeting within a week while the report is not finished", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const soon = addVisit(ask, p.id, { kind: "feedback", date: "2026-11-22" });
    addVisit(ask, p.id, { kind: "feedback", date: "2026-11-23" });
    const got = alertsOf(ask).filter((a) => a.kind === "feedback_soon");
    expect(got.map((a) => a.visit_id)).toEqual([soon.id]);
  });

  it("an active patient with nothing ahead, unless a task is open or the work ended well", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const v = addVisit(ask, p.id, { status: "held", date: "2026-10-30", kind: "guidance", fee: 0 });
    ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: v.id, date: v.date, record: "רשומה", personal: "", case_input_id: null } }, "note");
    const none = (today: Today = NOW) => alertsOf(ask, today).filter((a) => a.kind === "no_next_visit");
    // Fourteen days exactly is not yet "more than 14 days".
    expect(none({ date: "2026-11-13", minute: 0 })).toEqual([]);
    expect(none()).toHaveLength(1);
    const t = ask({ op: "save_task", task: task({ patient_id: p.id }) }, "task");
    expect(none()).toEqual([]);
    ask({ op: "save_task", task: { ...t, done_on: DAY } }, "task");
    expect(none()).toHaveLength(1);
    const next = addVisit(ask, p.id, { date: "2026-11-30" });
    expect(none()).toEqual([]);
    ask({ op: "delete_visit", id: next.id, scope: "one" }, "done");
    // A feedback held and nothing owed: the work is done, not forgotten.
    const fb = addVisit(ask, p.id, { status: "held", kind: "feedback", date: "2026-10-31", fee: 0 });
    ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: fb.id, date: fb.date, record: "רשומה", personal: "", case_input_id: null } }, "note");
    expect(none()).toEqual([]);
    // A closed or waiting patient is never asked about.
    ask({ op: "save_patient", patient: { ...p, status: "closed" } }, "patient");
    expect(none()).toEqual([]);
  });

  it("an open task whose date passed, with or without a patient; sorted oldest first", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    ask({ op: "save_task", task: task({ title: "שאלון", due: "2026-11-14", patient_id: p.id }) }, "task");
    ask({ op: "save_task", task: task({ title: "טפסים", due: "2026-11-01" }) }, "task");
    ask({ op: "save_task", task: task({ title: "היום", due: DAY }) }, "task");
    ask({ op: "save_task", task: task({ title: "נעשה", due: "2026-10-01", done_on: "2026-10-02" }) }, "task");
    const got = alertsOf(ask);
    expect(got.map((a) => [a.kind, a.since, a.patient_name])).toEqual([["task_overdue", "2026-11-01", null], ["task_overdue", "2026-11-14", "נועם"]]);
    expect(got[0]?.text).toContain("טפסים");

    const tasks = ask({ op: "overview", today: NOW }, "overview").tasks;
    // Overdue first, then by date; the done one is not on the front page.
    expect(tasks.map((t) => [t.task.title, t.overdue])).toEqual([["טפסים", true], ["שאלון", true], ["היום", false]]);
    expect(ask({ op: "tasks", today: NOW }, "tasks").map((t) => t.task.title)).toEqual(["טפסים", "שאלון", "היום", "נעשה"]);
  });
});

describe("patients", () => {
  it("checks a card before it is saved", () => {
    const { ask } = clinic();
    const bad: Partial<Patient>[] = [
      { name: "  " }, { name: "א".repeat(121) }, { birth_date: "2021-13-01" }, { session_fee: -1 },
      { contacts: Array.from({ length: 7 }, () => ({ role: "other" as const, name: "איש קשר", phone: "", email: "" })) },
      { reason: "א".repeat(4001) }, { created_on: "1999-12-31" }, { waiting_since: "אתמול" },
    ];
    for (const over of bad) expect(refusal(() => addPatient(ask, over)).code, JSON.stringify(over).slice(0, 40)).toBe("refused");
    // Stored on one line with single spaces; an empty row of the contacts form is dropped.
    const saved = addPatient(ask, { name: "  נועם   לדוגמה  ", contacts: [{ role: "mother", name: " מיכל ", phone: "", email: "" }, { role: "other", name: " ", phone: "", email: "" }] });
    expect(saved.name).toBe("נועם לדוגמה");
    expect(saved.contacts).toEqual([{ role: "mother", name: "מיכל", phone: "", email: "" }]);
  });

  it("saving a card never changes its case or the day it was opened", () => {
    const { ask } = clinic();
    const p = addPatient(ask, { case_id: "case-x" });
    expect(p.case_id).toBeNull();
    const again = ask({ op: "save_patient", patient: { ...p, name: "נועם ב", case_id: "case-y", created_on: "2020-01-01" } }, "patient");
    expect(again).toMatchObject({ id: p.id, name: "נועם ב", case_id: null, created_on: "2026-09-01" });
  });

  it("a family put on the waiting list waits from the day the card was opened", () => {
    const { ask } = clinic();
    expect(addPatient(ask, { status: "waiting" }).waiting_since).toBe("2026-09-01");
    expect(addPatient(ask, { status: "waiting", waiting_since: "2026-08-01" }).waiting_since).toBe("2026-08-01");
  });

  it("the list gives age, the next visit, the last held one, what is owed and open tasks", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    addPatient(ask, { name: "אביב", birth_date: null });
    addVisit(ask, p.id, { status: "held", date: "2026-11-01" });
    addVisit(ask, p.id, { date: DAY, start: 9 * 60, minutes: 60 });
    addVisit(ask, p.id, { date: "2026-11-18", kind: "feedback" });
    ask({ op: "save_task", task: task({ patient_id: p.id }) }, "task");
    const rows = ask({ op: "patients", today: NOW }, "patients");
    expect(rows.map((r) => r.patient.name)).toEqual(["אביב", "נועם"]);
    expect(rows[0]).toMatchObject({ age: null, next_visit: null, last_visit: null, due: 0, open_tasks: 0 });
    // Today's visit ended at ten and is not marked yet: it is still the next one, until she marks it.
    expect(rows[1]).toMatchObject({ age: { years: 5, months: 5 }, next_visit: { date: DAY, start: 540, kind: "assessment" }, last_visit: "2026-11-01", due: 60_000, open_tasks: 1 });
  });

  it("a card where nothing happened is erased at once, with the name typed again (D-174)", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const other = addPatient(ask, { name: "תמר" });
    // Scheduled, and a task: nothing happened yet.
    const v = addVisit(ask, p.id, { date: addDays(localToday().date, 3) });
    addVisit(ask, other.id, { date: "2026-11-16" });
    ask({ op: "save_task", task: task({ patient_id: p.id }) }, "task");
    expect(refusal(() => ask({ op: "erase_patient", id: p.id, name: "נועמ" }, "done")).message).toContain("השם שהוקלד לא זהה");
    expect(ask({ op: "patients", today: NOW }, "patients")).toHaveLength(2);
    ask({ op: "erase_patient", id: p.id, name: " נועם " }, "done");
    expect(ask({ op: "patients", today: NOW }, "patients").map((r) => r.patient.name)).toEqual(["תמר"]);
    expect(ask({ op: "calendar", from: "2026-11-01", to: "2026-12-31" }, "visits")).toHaveLength(1);
    expect(ask({ op: "history", record_id: v.id }, "history")).toEqual([]);
    expect(ask({ op: "tasks", today: NOW }, "tasks")).toEqual([]);
    expect(refusal(() => ask({ op: "patient_card", id: p.id, today: NOW }, "card")).code).toBe("not_found");
    // A mistake that was corrected leaves nothing: no anonymous line, and no "new patient".
    expect(ask({ op: "income", year: 2026 }, "income")).toMatchObject({ new_patients: 1, charged: 0, received: 0 });
  });

  it("a card that holds a record is closed, not erased, until its retention period has passed (D-174)", () => {
    const today = localToday().date;
    const happened: ((ask: Ask, id: string) => void)[] = [
      (ask, id) => addVisit(ask, id, { status: "held", date: today }),
      (ask, id) => addVisit(ask, id, { status: "no_show", charge_missed: true, date: today }),
      // A cancellation without a charge is part of the diary too (D-176).
      (ask, id) => addVisit(ask, id, { status: "cancelled", date: today }),
      // And so is a visit that was marked once and put back to "planned".
      (ask, id) => {
        const held = addVisit(ask, id, { status: "held", date: today });
        ask({ op: "save_visit", visit: { ...held, status: "planned" }, repeat: null, scope: "one" }, "saved");
      },
      (ask, id) => ask({ op: "save_note", note: { id: "", patient_id: id, visit_id: null, date: today, record: "שיחת טלפון", personal: "", case_input_id: null } }, "note"),
      (ask, id) => ask({ op: "save_charge", charge: { id: "", patient_id: id, date: today, amount: 100, what: "דוח", voided: false, void_reason: "" } }, "charge"),
      // A voided payment is still a line of the income diary.
      (ask, id) => ask({ op: "void_payment", id: ask({ op: "save_payment", payment: payment(id, 100, today) }, "payment").id, reason: "טעות" }, "payment"),
    ];
    for (const what of happened) {
      const { ask } = clinic();
      const p = addPatient(ask);
      what(ask, p.id);
      const no = refusal(() => ask({ op: "erase_patient", id: p.id, name: "נועם" }, "done"));
      expect(no.code).toBe("refused");
      expect(no.message).toContain("לסגור את הכרטיס");
      expect(ask({ op: "patients", today: NOW }, "patients")).toHaveLength(1);
      // Closing is what the screen offers instead; everything stays.
      expect(ask({ op: "save_patient", patient: { ...p, status: "closed" } }, "patient").status).toBe("closed");
    }
  });

  it("after seven years since the last activity and the child's 25th birthday, the whole card goes", () => {
    // The last activity is also the day a record was last written, so the clock is moved.
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(new Date(2018, 5, 1, 10, 0));
      const { ask } = clinic();
      // Seven years will pass, but the child will not be 25 yet.
      const young = addPatient(ask, { name: "צעיר", birth_date: "2010-01-01", created_on: "2018-05-01" });
      addVisit(ask, young.id, { status: "held", date: "2018-06-01" });
      const old = addPatient(ask, { name: "ותיק", birth_date: "1999-01-01", created_on: "2018-05-01" });
      const v = addVisit(ask, old.id, { status: "held", date: "2018-06-01" });
      const pay = ask({ op: "save_payment", payment: payment(old.id, 60_000, "2018-06-01") }, "payment");
      const touched = addPatient(ask, { name: "קרוב", birth_date: "1999-01-01", created_on: "2018-05-01" });
      const tv = addVisit(ask, touched.id, { status: "held", date: "2018-06-01" });

      // One day short of seven years.
      vi.setSystemTime(new Date(2025, 4, 31, 10, 0));
      expect(refusal(() => ask({ op: "erase_patient", id: old.id, name: "ותיק" }, "done")).message).toContain("החל מיום 01.06.2025");
      // A record corrected now is activity now, whatever date it carries.
      ask({ op: "save_note", note: { id: "", patient_id: touched.id, visit_id: tv.id, date: tv.date, record: "תיקון מאוחר", personal: "", case_input_id: null } }, "note");

      vi.setSystemTime(new Date(2025, 5, 1, 10, 0));
      expect(refusal(() => ask({ op: "erase_patient", id: young.id, name: "צעיר" }, "done")).message).toContain("החל מיום 01.01.2035");
      expect(refusal(() => ask({ op: "erase_patient", id: touched.id, name: "קרוב" }, "done")).message).toContain("החל מיום 31.05.2032");
      // Both passed: everything goes, the history and the money lines too.
      ask({ op: "erase_patient", id: old.id, name: "ותיק" }, "done");
      expect(ask({ op: "patients", today: NOW }, "patients").map((r) => r.patient.name)).toEqual(["צעיר", "קרוב"]);
      expect(ask({ op: "history", record_id: v.id }, "history")).toEqual([]);
      expect(ask({ op: "history", record_id: pay.id }, "history")).toEqual([]);
      expect(ask({ op: "diary", month: "2018-06" }, "diary").days.flatMap((d) => d.lines)).toHaveLength(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("the card walks the assessment step by step", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const steps = () => ask({ op: "patient_card", id: p.id, today: NOW }, "card").process.map((s) => [s.key, s.state]);
    expect(steps()).toEqual([["intake", "now"], ["sessions", "todo"], ["report", "todo"], ["feedback", "todo"], ["payment", "todo"]]);
    addVisit(ask, p.id, { status: "held", kind: "intake", date: "2026-11-01", fee: 45_000 });
    addVisit(ask, p.id, { status: "held", date: "2026-11-08" });
    addVisit(ask, p.id, { date: "2026-11-22" });
    addVisit(ask, p.id, { kind: "observation", date: "2026-11-23" });
    // More than one step can be under way: sessions are running, the observation is set, money is owed.
    expect(steps()).toEqual([["intake", "done"], ["sessions", "now"], ["observation", "now"], ["report", "todo"], ["feedback", "todo"], ["payment", "now"]]);
    const card = ask({ op: "patient_card", id: p.id, today: NOW }, "card");
    expect(card.process.find((s) => s.key === "sessions")?.detail).toBe("1 מתוך 2 התקיימו");
    expect(card.process.find((s) => s.key === "observation")?.detail).toBe("נקבעה ליום 23.11.2026");
    expect(card.process.find((s) => s.key === "report")).toMatchObject({ label: "כתיבת הדוח", detail: "עוד לא נפתח תיק לדוח" });
    expect(card.process.find((s) => s.key === "payment")?.detail).toBe("נותרו 1050 ₪ לתשלום");
    // Newest first.
    expect(card.visits.map((v) => v.visit.date)).toEqual(["2026-11-23", "2026-11-22", "2026-11-08", "2026-11-01"]);
  });
});

describe("notes and the link to a case", () => {
  function withNote(record = "רשומה רשמית", personal = "תרשומת אישית") {
    const made = clinic();
    const p = addPatient(made.ask);
    const v = addVisit(made.ask, p.id, { status: "held", kind: "intake", date: "2026-11-01" });
    const n = made.ask({ op: "save_note", note: { id: "", patient_id: p.id, visit_id: v.id, date: v.date, record, personal, case_input_id: "forged" } }, "note");
    return { ...made, p, v, n };
  }

  it("keeps the record and the personal note apart, one note for a visit", () => {
    const { ask, v, n } = withNote();
    expect(n).toMatchObject({ record: "רשומה רשמית", personal: "תרשומת אישית", case_input_id: null });
    expect(refusal(() => ask({ op: "save_note", note: { ...n, id: "" } }, "note")).message).toContain("כבר יש סיכום");
    expect(refusal(() => ask({ op: "save_note", note: { ...n, record: " ", personal: "" } }, "note")).message).toContain("ריק");
    expect(ask({ op: "calendar", from: v.date, to: v.date }, "visits")[0]?.has_note).toBe(true);
    // Stored trimmed, as the core stores it.
    expect(ask({ op: "save_note", note: { ...n, record: "  רשומה רשמית \n" } }, "note").record).toBe("רשומה רשמית");
  });

  it("a note that has text is never deleted; it is corrected, and every state of it is kept", () => {
    const { ask, p, n } = withNote();
    const no = refusal(() => ask({ op: "delete_note", id: n.id }, "done"));
    expect(no.code).toBe("refused");
    expect(no.message).toContain("לא מוחקים אותו");
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").notes).toHaveLength(1);
    ask({ op: "save_note", note: { ...n, record: "רשומה מתוקנת", personal: "מחשבה שלי בלבד" } }, "note");
    const h = ask({ op: "history", record_id: n.id }, "history");
    expect(h.map((e) => e.action)).toEqual(["created", "changed"]);
    expect(h[0]?.lines).toEqual(["מטופל: נועם", "תאריך: 01.11.2026", "רשומה:", "רשומה רשמית", "תרשומת אישית: נכתבה"]);
    expect(h[1]?.lines).toEqual(["מטופל: נועם", "תאריך: 01.11.2026", "רשומה:", "רשומה מתוקנת", "תרשומת אישית: השתנתה"]);
    // The personal note is hers alone: the history says that it changed, never what it says.
    expect(JSON.stringify(h)).not.toContain("מחשבה שלי בלבד");
    expect(JSON.stringify(h)).not.toContain("\"תרשומת אישית\"");
    // Saved again with nothing changed: no new line. With only the record changed: "בלי שינוי".
    ask({ op: "save_note", note: { ...n, record: "רשומה מתוקנת", personal: "מחשבה שלי בלבד" } }, "note");
    expect(ask({ op: "history", record_id: n.id }, "history")).toHaveLength(2);
    ask({ op: "save_note", note: { ...n, record: "שורה ראשונה\n\nשורה שנייה", personal: "מחשבה שלי בלבד" } }, "note");
    expect(ask({ op: "history", record_id: n.id }, "history")[2]?.lines).toEqual(["מטופל: נועם", "תאריך: 01.11.2026", "רשומה:", "שורה ראשונה", "שורה שנייה", "תרשומת אישית: בלי שינוי"]);
    expect(refusal(() => ask({ op: "delete_note", id: "nothing" }, "done")).code).toBe("not_found");
  });

  it("opens a case with the child's name, age and parents, and a code of today", () => {
    const { ask, host, p } = withNote();
    const link = ask({ op: "create_case", patient_id: p.id, today: NOW }, "case");
    expect(link).toMatchObject({ code: "תיק-2026-1115", approved: 0, in_trash: false });
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").case?.id).toBe(link.id);
    expect(host.cases()[0]).toMatchObject({ childName: "נועם" });
    expect(refusal(() => ask({ op: "create_case", patient_id: p.id, today: NOW }, "case")).message).toContain("כבר יש");
    // The same day again, for another child: the code is not taken twice.
    const q = addPatient(ask, { name: "תמר" });
    expect(ask({ op: "create_case", patient_id: q.id, today: NOW }, "case").code).toBe("תיק-2026-1115-2");
  });

  it("every named contact becomes a person of the case, so the filter masks them later", () => {
    const made = clinic();
    const seen: { role: string; value: string }[] = [];
    const host = { ...made.host, createCase: (meta: Parameters<typeof made.host.createCase>[0], people: Parameters<typeof made.host.createCase>[1]) => { seen.push(...people.map((x) => ({ role: x.role, value: x.value }))); return made.host.createCase(meta, people); } };
    const { ask } = clinic(host);
    const p = addPatient(ask, { contacts: [
      { role: "mother", name: "מיכל", phone: "", email: "" }, { role: "guardian", name: "סבתא רבקה", phone: "", email: "" },
      { role: "other", name: "", phone: "שלוחה 12", email: "" }, { role: "other", name: "דודה חנה", phone: "", email: "" },
    ] });
    ask({ op: "create_case", patient_id: p.id, today: NOW }, "case");
    expect(seen).toEqual([{ role: "child", value: "נועם" }, { role: "mother", value: "מיכל" }, { role: "other", value: "סבתא רבקה" }, { role: "other", value: "דודה חנה" }]);
  });

  it("only the formal record goes into the case, and going again replaces it", () => {
    const { ask, host, p, n } = withNote();
    expect(refusal(() => ask({ op: "note_to_case", id: n.id }, "note")).message).toContain("תיק");
    ask({ op: "create_case", patient_id: p.id, today: NOW }, "case");
    const sent = ask({ op: "note_to_case", id: n.id }, "note");
    expect(sent.case_input_id).not.toBeNull();
    expect(host.inputs).toHaveLength(1);
    expect(host.inputs[0]).toMatchObject({ kind: "intake", title: "סיכום פגישת היכרות · 01.11.2026", content: "רשומה רשמית" });
    expect(JSON.stringify(host.inputs)).not.toContain("תרשומת אישית");

    ask({ op: "save_note", note: { ...sent, record: "רשומה מעודכנת" } }, "note");
    expect(ask({ op: "note_to_case", id: n.id }, "note").case_input_id).toBe(sent.case_input_id);
    expect(host.inputs.map((i) => i.content)).toEqual(["רשומה מעודכנת"]);
  });

  it("refuses an empty record, and a case in the recycle bin", () => {
    const { ask, host, p, n } = withNote("", "רק תרשומת אישית");
    const link = ask({ op: "create_case", patient_id: p.id, today: NOW }, "case");
    expect(refusal(() => ask({ op: "note_to_case", id: n.id }, "note")).message).toContain("ריקה");
    ask({ op: "save_note", note: { ...n, record: "רשומה" } }, "note");
    host.trash(link.id);
    expect(refusal(() => ask({ op: "note_to_case", id: n.id }, "note")).message).toContain("סל המחזור");
    expect(host.inputs).toEqual([]);
  });

  it("a case is linked to one patient at most, and one in the bin is not offered", () => {
    const { ask, host } = clinic();
    const a = addPatient(ask);
    const b = addPatient(ask, { name: "תמר" });
    const meta = { code: "תיק-א", age: null, child_gender: null, current_section: null, retention_until: null, consent: null };
    const one = host.createCase(meta, [{ id: null, role: "child", value: "נועם", aliases: [] }]);
    const two = host.createCase({ ...meta, code: "תיק-ב" }, []);
    expect(ask({ op: "case_choices" }, "cases")).toEqual([{ id: one, label: "נועם · תיק-א" }, { id: two, label: "תיק-ב" }]);
    expect(ask({ op: "link_case", patient_id: a.id, case_id: one }, "patient").case_id).toBe(one);
    expect(ask({ op: "case_choices" }, "cases").map((c) => c.id)).toEqual([two]);
    expect(refusal(() => ask({ op: "link_case", patient_id: b.id, case_id: one }, "patient")).message).toContain("מטופל אחר");
    host.trash(two);
    expect(ask({ op: "case_choices" }, "cases")).toEqual([]);
    expect(refusal(() => ask({ op: "link_case", patient_id: b.id, case_id: two }, "patient")).message).toContain("סל המחזור");
    expect(ask({ op: "link_case", patient_id: a.id, case_id: null }, "patient").case_id).toBeNull();
    expect(ask({ op: "case_choices" }, "cases").map((c) => c.id)).toEqual([one]);
  });
});

describe("the diary, income and history", () => {
  it("the month's diary: visits by time, then charges, then payments; voided shown and not counted", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    addVisit(ask, p.id, { status: "held", date: "2026-11-03", start: 900, kind: "feedback", fee: 45_000 });
    addVisit(ask, p.id, { status: "held", date: "2026-11-03", start: 540 });
    addVisit(ask, p.id, { status: "held", date: "2026-10-31" });
    ask({ op: "save_visit", visit: visit(null, { kind: "block", fee: 0, date: "2026-11-03" }), repeat: null, scope: "one" }, "saved");
    ask({ op: "save_charge", charge: { id: "", patient_id: p.id, date: "2026-11-03", amount: 120_000, what: "כתיבת דוח", voided: false, void_reason: "" } }, "charge");
    const pay = ask({ op: "save_payment", payment: { ...payment(p.id, 60_000, "2026-11-03"), receipt: "1001" } }, "payment");
    const wrong = ask({ op: "save_payment", payment: payment(p.id, 99_000, "2026-11-03") }, "payment");
    ask({ op: "void_payment", id: wrong.id, reason: "הוקלד בטעות" }, "payment");
    ask({ op: "save_payment", payment: { ...payment(p.id, 30_000, "2026-11-10"), method: "cash" } }, "payment");

    const d = ask({ op: "diary", month: "2026-11" }, "diary");
    expect(d.days.map((x) => x.date)).toEqual(["2026-11-03", "2026-11-10"]);
    const first = d.days[0];
    expect(first?.lines.map((l) => l.what)).toEqual(["מפגש אבחון", "שיחת משוב", "כתיבת דוח", "תשלום", "תשלום"]);
    expect(first?.lines.map((l) => l.voided)).toEqual([false, false, false, false, true]);
    expect(first).toMatchObject({ charged: 225_000, received: 60_000 });
    expect(first?.lines[3]).toMatchObject({ record_id: pay.id, received: 60_000, method: "bit", receipt: "1001", changed: false });
    // A voided line is shown with its amount and counted nowhere.
    expect(first?.lines[4]).toMatchObject({ received: 99_000, note: "בוטל: הוקלד בטעות", changed: true });
    expect(d).toMatchObject({ charged: 225_000, received: 90_000, missing_receipts: 1 });
    expect(d.by_method).toEqual([{ method: "cash", amount: 30_000 }, { method: "bit", amount: 60_000 }]);
    expect(refusal(() => ask({ op: "diary", month: "2026-13" }, "diary")).code).toBe("refused");
  });

  it("the year at a glance", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    addPatient(ask, { name: "ישן", created_on: "2025-12-31" });
    addVisit(ask, p.id, { status: "held", date: "2026-03-02" });
    addVisit(ask, p.id, { status: "held", date: "2026-03-09" });
    addVisit(ask, p.id, { status: "held", date: "2026-03-16", kind: "feedback", fee: 45_000 });
    addVisit(ask, p.id, { status: "cancelled", date: "2026-03-20" });
    addVisit(ask, p.id, { status: "no_show", charge_missed: true, date: "2026-04-01", fee: 10_000 });
    addVisit(ask, p.id, { status: "held", date: "2025-12-30" });
    ask({ op: "save_payment", payment: payment(p.id, 50_000, "2026-03-31") }, "payment");
    const y = ask({ op: "income", year: 2026 }, "income");
    expect(y.months).toHaveLength(12);
    expect(y.months[2]).toEqual({ month: "2026-03", held: 3, cancelled: 1, no_show: 0, charged: 165_000, received: 50_000 });
    expect(y.months[3]).toMatchObject({ no_show: 1, charged: 10_000 });
    expect(y).toMatchObject({ charged: 175_000, received: 50_000, new_patients: 1 });
    expect(y.by_kind).toEqual([{ kind: "assessment", count: 2 }, { kind: "feedback", count: 1 }]);
  });

  it("every saved state of a visit, a charge and a payment is kept, in her words", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const v = addVisit(ask, p.id);
    ask({ op: "save_visit", visit: { ...v, start: 600 }, repeat: null, scope: "one" }, "saved");
    ask({ op: "save_visit", visit: { ...v, start: 600, status: "cancelled", status_note: "מחלה" }, repeat: null, scope: "one" }, "saved");
    const h = ask({ op: "history", record_id: v.id }, "history");
    expect(h.map((e) => e.action)).toEqual(["created", "changed", "changed"]);
    expect(h[0]?.lines).toEqual(["מטופל: נועם", "סוג: מפגש אבחון", "תאריך: 15.11.2026", "שעה: 09:00", "משך: 90 דקות", "מקום: הקליניקה", "מצב: מתוכננת", "מחיר: 600 ₪"]);
    expect(h[1]?.lines).toContain("שעה: 10:00");
    expect(h[2]?.lines).toEqual(expect.arrayContaining(["מצב: בוטלה", "מה קרה: מחלה"]));
    const pay = ask({ op: "save_payment", payment: payment(p.id, 45_000) }, "payment");
    ask({ op: "save_payment", payment: { ...pay, amount: 45_050 } }, "payment");
    ask({ op: "void_payment", id: pay.id, reason: "טעות" }, "payment");
    const ph = ask({ op: "history", record_id: pay.id }, "history");
    expect(ph.map((e) => e.action)).toEqual(["created", "changed", "voided"]);
    expect(ph[1]?.lines).toContain("סכום: 450.50 ₪");
    expect(ph[2]?.lines).toEqual(expect.arrayContaining(["בוטל: כן", "סיבת הביטול: טעות"]));
    expect(ask({ op: "history", record_id: "nothing" }, "history")).toEqual([]);
  });
});

describe("a value this version does not know (D-178)", () => {
  const UNKNOWN = (field: string) => `בשדה "${field}" יש ערך שהגרסה הזו של התוכנה לא מכירה. כדי לשמור צריך לבחור ערך מהרשימה, או לעדכן את התוכנה.`;
  const later = <T,>(value: string) => value as unknown as T;

  function planted() {
    const c = clinic();
    c.fake.plant({
      patients: [patient({ id: "p-later", name: "אורי", status: later("on_hold"), contacts: [{ role: later("aunt"), name: "דנה", phone: "", email: "" }] })],
      visits: [
        visit("p-later", { id: "v-kind", kind: later("group"), status: "held", date: "2026-11-03" }),
        visit("p-later", { id: "v-status", status: later("postponed"), date: "2026-11-04" }),
      ],
      payments: [{ ...payment("p-later", 20_000, "2026-11-05"), id: "pay-later", method: later("crypto") }],
      tasks: [task({ id: "t-later", kind: later("fax"), patient_id: "p-later" })],
    });
    return c;
  }

  it("is read as “unknown”, shown, and never crashes a page of figures", () => {
    const { ask } = planted();
    const card = ask({ op: "patient_card", id: "p-later", today: NOW }, "card");
    expect(card.patient.status).toBe("unknown");
    expect(card.patient.contacts[0]?.role).toBe("unknown");
    expect(card.visits.map((v) => [v.visit.kind, v.visit.status])).toEqual([["assessment", "unknown"], ["unknown", "held"]]);
    expect(card.payments[0]?.method).toBe("unknown");
    expect(card.tasks[0]?.kind).toBe("unknown");
    // A status that is not known charges nothing; a kind that is not known charges as any held visit.
    expect(card.balance).toEqual({ charged: 60_000, paid: 20_000, due: 40_000 });
    const d = ask({ op: "diary", month: "2026-11" }, "diary");
    // The method is a total of its own, so the methods still add up to what was received.
    expect(d.by_method).toEqual([{ method: "unknown", amount: 20_000 }]);
    expect(d.received).toBe(20_000);
    expect(ask({ op: "income", year: 2026 }, "income").months[10]).toMatchObject({ held: 1, cancelled: 0, no_show: 0, charged: 60_000, received: 20_000 });
    expect(ask({ op: "overview", today: NOW }, "overview").alerts.every((a) => a.kind !== "unreadable")).toBe(true);
    expect(ask({ op: "patients", today: NOW }, "patients")).toHaveLength(1);
    expect(ask({ op: "calendar", from: "2026-11-01", to: "2026-11-30" }, "visits")).toHaveLength(2);
  });

  it("is never written: each save says which field, in the core's words", () => {
    const { ask } = planted();
    const card = ask({ op: "patient_card", id: "p-later", today: NOW }, "card");
    const says = (run: () => unknown, field: string) => {
      const no = refusal(run);
      expect(no.code).toBe("refused");
      expect(no.message).toBe(UNKNOWN(field));
    };
    const [byStatus, byKind] = card.visits.map((v) => v.visit) as [Visit, Visit];
    says(() => ask({ op: "save_visit", visit: { ...byKind, place: "בגן" }, repeat: null, scope: "one" }, "saved"), "סוג הפגישה");
    says(() => ask({ op: "save_visit", visit: { ...byStatus, place: "בגן" }, repeat: null, scope: "one" }, "saved"), "מצב הפגישה");
    says(() => ask({ op: "save_visit", visit: visit("p-later", { kind: "unknown" }), repeat: null, scope: "one" }, "saved"), "סוג הפגישה");
    says(() => ask({ op: "save_payment", payment: { ...(card.payments[0] as Payment), note: "תיקון" } }, "payment"), "אמצעי התשלום");
    says(() => ask({ op: "void_payment", id: "pay-later", reason: "טעות" }, "payment"), "אמצעי התשלום");
    says(() => ask({ op: "save_task", task: { ...(card.tasks[0] as ClinicTask), title: "אחר" } }, "task"), "סוג המשימה");
    says(() => ask({ op: "save_patient", patient: { ...card.patient, notes: "הערה" } }, "patient"), "מצב הכרטיס");
    says(() => ask({ op: "save_patient", patient: { ...card.patient, status: "active" } }, "patient"), "הקשר לילד");
    // Nothing was written by any of them.
    expect(ask({ op: "patient_card", id: "p-later", today: NOW }, "card")).toEqual(card);

    // Once she picks a value from the list, it is saved.
    const fixed = ask({ op: "save_visit", visit: { ...byKind, kind: "therapy" }, repeat: null, scope: "one" }, "saved").visits[0]?.visit;
    expect(fixed?.kind).toBe("therapy");
    const saved = ask({ op: "save_patient", patient: { ...card.patient, status: "active", contacts: [{ role: "other", name: "דנה", phone: "", email: "" }] } }, "patient");
    expect(saved).toMatchObject({ status: "active", contacts: [{ role: "other" }] });
    expect(ask({ op: "save_payment", payment: { ...(card.payments[0] as Payment), method: "cash" } }, "payment").method).toBe("cash");
    expect(ask({ op: "save_task", task: { ...(card.tasks[0] as ClinicTask), kind: "other" } }, "task").kind).toBe("other");
  });

  it("rows that could not be read are said first on the front page, and only there", () => {
    const { ask, fake } = clinic();
    const p = addPatient(ask);
    addVisit(ask, p.id, { date: "2026-11-10" });
    fake.unreadable(1);
    const one = alertsOf(ask);
    expect(one.map((a) => a.kind)).toEqual(["unreadable", "unmarked"]);
    expect(one[0]).toEqual({
      kind: "unreadable", patient_id: null, patient_name: null, visit_id: null, task_id: null, since: DAY,
      text: "רישום אחד בקליניקה לא נקרא, ולכן הוא לא מוצג ולא נספר בסיכומים. הוא לא נמחק ולא שונה. כדאי לשחזר מגיבוי, או לפנות לתמיכה.",
    });
    expect(ask({ op: "patient_card", id: p.id, today: NOW }, "card").alerts.map((a) => a.kind)).toEqual(["unmarked"]);
    fake.unreadable(3);
    expect(alertsOf(ask)[0]?.text).toBe("3 רישומים בקליניקה לא נקראו, ולכן הם לא מוצגים ולא נספרים בסיכומים. הם לא נמחקו ולא שונו. כדאי לשחזר מגיבוי, או לפנות לתמיכה.");
    fake.unreadable(0);
    expect(alertsOf(ask).map((a) => a.kind)).toEqual(["unmarked"]);
  });
});

describe("what an erased card counted for stays, with no name (D-179)", () => {
  /** A card of 2018 whose retention period has passed by 2026, and the clock that says so. */
  function old() {
    let now = new Date(Date.UTC(2018, 5, 20, 9, 0));
    const c = clinic(standInHost(), () => now);
    const { ask } = c;
    const p = addPatient(ask, { name: "ותיק", birth_date: "1999-01-01", created_on: "2018-05-10" });
    addVisit(ask, p.id, { status: "held", date: "2018-06-01" });
    addVisit(ask, p.id, { status: "held", date: "2018-06-08", kind: "feedback", fee: 45_000 });
    addVisit(ask, p.id, { status: "cancelled", date: "2018-06-10" });
    addVisit(ask, p.id, { status: "no_show", charge_missed: true, date: "2018-07-02", fee: 10_000 });
    // Marked once and put back: it happened, and it counts for nothing.
    const back = addVisit(ask, p.id, { status: "held", date: "2018-07-09" });
    ask({ op: "save_visit", visit: { ...back, status: "planned" }, repeat: null, scope: "one" }, "saved");
    ask({ op: "save_charge", charge: { id: "", patient_id: p.id, date: "2018-06-08", amount: 120_000, what: "כתיבת דוח", voided: false, void_reason: "" } }, "charge");
    const wrong = ask({ op: "save_charge", charge: { id: "", patient_id: p.id, date: "2018-06-09", amount: 7_000, what: "טעות", voided: false, void_reason: "" } }, "charge");
    ask({ op: "void_charge", id: wrong.id, reason: "הוקלד בטעות" }, "charge");
    ask({ op: "save_payment", payment: { ...payment(p.id, 100_000, "2018-06-08"), receipt: "17" } }, "payment");
    ask({ op: "save_payment", payment: { ...payment(p.id, 50_000, "2018-06-30"), method: "cash" } }, "payment");
    const voided = ask({ op: "save_payment", payment: payment(p.id, 9_000, "2018-06-30") }, "payment");
    ask({ op: "void_payment", id: voided.id, reason: "הוקלד בטעות" }, "payment");
    // Another family of the same months, which stays.
    const other = addPatient(ask, { name: "תמר", birth_date: "2012-03-01", created_on: "2018-05-12" });
    addVisit(ask, other.id, { status: "held", date: "2018-06-03" });
    ask({ op: "save_payment", payment: payment(other.id, 60_000, "2018-06-03") }, "payment");
    return { ...c, p, other, at: (date: Date) => { now = date; } };
  }

  it("the year and each month read the same before and after the erase", () => {
    const { ask, p, at } = old();
    const year = ask({ op: "income", year: 2018 }, "income");
    const june = ask({ op: "diary", month: "2018-06" }, "diary");
    const july = ask({ op: "diary", month: "2018-07" }, "diary");
    expect(year.months[5]).toEqual({ month: "2018-06", held: 3, cancelled: 1, no_show: 0, charged: 285_000, received: 210_000 });
    expect(year).toMatchObject({ charged: 295_000, received: 210_000, new_patients: 2 });
    expect(june.erased).toBeNull();

    at(new Date(Date.UTC(2026, 10, 15, 9, 0)));
    ask({ op: "erase_patient", id: p.id, name: "ותיק" }, "done");

    expect(ask({ op: "income", year: 2018 }, "income")).toEqual(year);
    const after = ask({ op: "diary", month: "2018-06" }, "diary");
    expect({ charged: after.charged, received: after.received, by_method: after.by_method, missing_receipts: after.missing_receipts })
      .toEqual({ charged: june.charged, received: june.received, by_method: june.by_method, missing_receipts: june.missing_receipts });
    // The lines themselves are gone: no name, no date, no text. One line of numbers stays.
    expect(after.days.map((d) => d.date)).toEqual(["2018-06-03"]);
    expect(after.days.flatMap((d) => d.lines).map((l) => l.patient_name)).toEqual(["תמר", "תמר"]);
    expect(after.erased).toEqual({ label: "רשומות של כרטיסים שנמחקו", held: 2, cancelled: 1, no_show: 0, charged: 225_000, received: 150_000 });
    expect(JSON.stringify(after)).not.toContain("ותיק");
    expect(JSON.stringify(after)).not.toContain("כתיבת דוח");
    const julyAfter = ask({ op: "diary", month: "2018-07" }, "diary");
    expect(julyAfter).toMatchObject({ days: [], charged: july.charged, received: july.received });
    expect(julyAfter.erased).toEqual({ label: "רשומות של כרטיסים שנמחקו", held: 0, cancelled: 0, no_show: 1, charged: 10_000, received: 0 });
    // A month the card never touched has no such line.
    expect(ask({ op: "diary", month: "2018-08" }, "diary").erased).toBeNull();
  });

  it("two erased cards share the one line, and nothing tells them apart", () => {
    const { ask, p, other, at } = old();
    const year = ask({ op: "income", year: 2018 }, "income");
    at(new Date(Date.UTC(2046, 10, 15, 9, 0)));
    ask({ op: "erase_patient", id: p.id, name: "ותיק" }, "done");
    ask({ op: "erase_patient", id: other.id, name: "תמר" }, "done");
    expect(ask({ op: "income", year: 2018 }, "income")).toEqual(year);
    const june = ask({ op: "diary", month: "2018-06" }, "diary");
    expect(june.days).toEqual([]);
    expect(june.erased).toEqual({ label: "רשומות של כרטיסים שנמחקו", held: 3, cancelled: 1, no_show: 0, charged: 285_000, received: 210_000 });
    expect(june.by_method).toEqual([{ method: "cash", amount: 50_000 }, { method: "bit", amount: 160_000 }]);
    expect(ask({ op: "patients", today: NOW }, "patients")).toEqual([]);
  });

  it("this month's received keeps it; what is owed is a state of now and does not", () => {
    let now = new Date(Date.UTC(2018, 5, 20, 9, 0));
    const { ask } = clinic(standInHost(), () => now);
    const p = addPatient(ask, { name: "ותיק", birth_date: "1999-01-01", created_on: "2018-05-10" });
    addVisit(ask, p.id, { status: "held", date: "2018-06-01" });
    // A payment she wrote long ago with a date of this month by mistake, never corrected.
    ask({ op: "save_payment", payment: payment(p.id, 20_000, "2026-11-02") }, "payment");
    const before = ask({ op: "overview", today: NOW }, "overview");
    expect(before).toMatchObject({ month_received: 20_000, total_due: 40_000 });
    // The retention period is counted from the newest date the card carries.
    now = new Date(Date.UTC(2033, 10, 2, 9, 0));
    ask({ op: "erase_patient", id: p.id, name: "ותיק" }, "done");
    expect(ask({ op: "overview", today: NOW }, "overview")).toMatchObject({ month_received: 20_000, total_due: 0, active: 0 });
  });
});

describe("the stand-in's clock", () => {
  it("a birth date is refused before 1900 and once it has not come yet, in the core's words", () => {
    // 23:30 in UTC: the core's day is the 15th, and her local day may already be the 16th.
    const { ask } = clinic(standInHost(), () => new Date(Date.UTC(2026, 10, 15, 23, 30)));
    const no = (birth: string) => refusal(() => addPatient(ask, { birth_date: birth }));
    expect(no("1899-12-31")).toMatchObject({ code: "refused", message: "צריך לכתוב תאריך לידה תקין, משנת 1900 ואילך." });
    expect(no("2026-02-30").message).toBe("צריך לכתוב תאריך לידה תקין, משנת 1900 ואילך.");
    expect(no("2026-11-17")).toMatchObject({ code: "refused", message: "תאריך הלידה עוד לא הגיע. כדאי לבדוק את השנה שנכתבה." });
    expect(no("2062-06-01").message).toBe("תאריך הלידה עוד לא הגיע. כדאי לבדוק את השנה שנכתבה.");
    // A grown sibling's card, a baby born today, and her own "today" when it is one ahead.
    expect(addPatient(ask, { birth_date: "1900-01-01" }).birth_date).toBe("1900-01-01");
    expect(addPatient(ask, { birth_date: "2026-11-15" }).birth_date).toBe("2026-11-15");
    expect(addPatient(ask, { birth_date: "2026-11-16" }).birth_date).toBe("2026-11-16");
    expect(addPatient(ask, { birth_date: "" }).birth_date).toBeNull();
    // A card that is corrected is checked like a new one.
    const p = addPatient(ask);
    expect(refusal(() => ask({ op: "save_patient", patient: { ...p, birth_date: "2031-01-01" } }, "patient")).message).toContain("עוד לא הגיע");
  });

  it("one clock says the day a card was opened and the day a record was last written", () => {
    let now = new Date(Date.UTC(2019, 0, 31, 23, 59));
    const { ask } = clinic(standInHost(), () => now);
    const p = addPatient(ask, { name: "ותיק", birth_date: "1990-01-01", created_on: "" });
    expect(p.created_on).toBe("2019-01-31");
    addVisit(ask, p.id, { status: "held", date: "2019-01-20" });
    const says = () => refusal(() => ask({ op: "erase_patient", id: p.id, name: "ותיק" }, "done")).message;
    // Seven years from the day the mark was written, by that same clock.
    expect(says()).toContain("החל מיום 31.01.2026");
    now = new Date(Date.UTC(2026, 0, 30, 23, 59));
    expect(says()).toContain("החל מיום 31.01.2026");
    // A minute later the core's day has come, wherever her own clock is.
    now = new Date(Date.UTC(2026, 0, 31, 0, 0));
    ask({ op: "erase_patient", id: p.id, name: "ותיק" }, "done");
    expect(ask({ op: "patients", today: NOW }, "patients")).toEqual([]);
  });
});

describe("messages and settings", () => {
  it("fills every placeholder, each as often as it appears", () => {
    const values = { "{הורה}": "מיכל", "{ילד}": "נועם", "{יום}": "ראשון", "{תאריך}": "15.11.2026", "{שעה}": "09:30", "{מקום}": "הקליניקה", "{סכום}": "450", "{שמי}": "רותם" };
    expect(fillMessage("שלום {הורה} ({הורה}), {ילד} ביום {יום} {תאריך} {שעה} ב{מקום}: {סכום} ₪. {שמי}", values))
      .toBe("שלום מיכל (מיכל), נועם ביום ראשון 15.11.2026 09:30 בהקליניקה: 450 ₪. רותם");
  });

  it("fills in one pass: a value is never read again as a placeholder, and other braces stay", () => {
    const values = { "{הורה}": "{ילד}", "{ילד}": "נועם", "{יום}": "", "{תאריך}": "", "{שעה}": "", "{מקום}": "", "{סכום}": "", "{שמי}": "" };
    expect(fillMessage("שלום {הורה}, {משהו} {{ילד}", values)).toBe("שלום {ילד}, {משהו} {נועם");
  });

  it("a reminder of a visit, and “שלום רב” when no parent is named", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    const v = addVisit(ask, p.id, { date: "2026-11-16", start: 570 });
    expect(ask({ op: "message", kind: "reminder", patient_id: p.id, visit_id: v.id }, "text"))
      .toBe("שלום מיכל, תזכורת לפגישה ביום שני, 16.11.2026, בשעה 09:30. להתראות, רותם בדויה");
    // The strictest default: the child's name is not in a message that leaves by her hand.
    expect(ask({ op: "message", kind: "reminder", patient_id: p.id, visit_id: v.id }, "text")).not.toContain("נועם");
    expect(refusal(() => ask({ op: "message", kind: "reminder", patient_id: p.id, visit_id: null }, "text")).message).toContain("פגישה");
    const q = addPatient(ask, { name: "תמר", contacts: [] });
    const w = addVisit(ask, q.id, { date: "2026-11-17" });
    expect(ask({ op: "message", kind: "reminder", patient_id: q.id, visit_id: w.id }, "text")).toMatch(/^שלום רב,/);
    expect(refusal(() => ask({ op: "message", kind: "reminder", patient_id: q.id, visit_id: v.id }, "text")).code).toBe("refused");
  });

  it("a payment message carries what is owed, and there is none when nothing is", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    expect(refusal(() => ask({ op: "message", kind: "payment", patient_id: p.id, visit_id: null }, "text")).message).toContain("אין יתרה");
    addVisit(ask, p.id, { status: "held", fee: 45_050 });
    expect(ask({ op: "message", kind: "payment", patient_id: p.id, visit_id: null }, "text")).toBe("שלום מיכל, היתרה לתשלום היא 450.50 ₪. תודה רבה, רותם בדויה");
  });

  it("settings start as the core's defaults and are checked on save", () => {
    const { ask } = clinic();
    const s = ask({ op: "settings" }, "settings");
    expect(s).toMatchObject({ day_start: 480, day_end: 1200, work_days: [0, 1, 2, 3, 4], places: ["הקליניקה"], report_days: 21, debt_days: 30, waiting_weeks: 6 });
    const kinds: VisitKind[] = ["intake", "assessment", "observation", "feedback", "therapy", "guidance", "meeting", "other", "block"];
    expect(s.defaults.map((d) => d.kind)).toEqual(kinds);
    expect(s.defaults.every((d) => d.fee === 0)).toBe(true);
    expect(s.note_templates).toHaveLength(4);

    // Stored tidy: places on one line, the empty and the repeated ones dropped; working days in order, once each.
    const saved = ask({ op: "save_settings", settings: { ...s, places: [" הקליניקה ", "שיחת  וידאו", "", "הקליניקה"], work_days: [3, 0, 3], debt_days: 45 } }, "settings");
    expect(saved.places).toEqual(["הקליניקה", "שיחת וידאו"]);
    expect(saved.work_days).toEqual([0, 3]);
    expect(ask({ op: "settings" }, "settings").debt_days).toBe(45);
    const bad = [
      { day_start: 1200, day_end: 480 }, { work_days: [] }, { work_days: [7] }, { defaults: s.defaults.slice(1) },
      { places: Array.from({ length: 13 }, (_, i) => `מקום ${i}`) }, { reminder_text: "א".repeat(1001) },
      { note_templates: Array.from({ length: 21 }, (_, i) => ({ name: `תבנית ${i}`, text: "" })) }, { note_templates: [{ name: " ", text: "" }] },
      { defaults: s.defaults.map((d) => (d.kind === "block" ? { ...d, fee: 100 } : d)) },
    ];
    for (const over of bad) expect(refusal(() => ask({ op: "save_settings", settings: { ...s, ...over } }, "settings")).code, Object.keys(over).join()).toBe("refused");
  });
});

describe("the front page", () => {
  it("today's visits by time; the next day with visits once today has none left", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    addVisit(ask, p.id, { start: 16 * 60, minutes: 50 });
    addVisit(ask, p.id, { start: 9 * 60 });
    addVisit(ask, p.id, { date: "2026-11-18", status: "cancelled" });
    addVisit(ask, p.id, { date: "2026-11-19" });
    addVisit(ask, p.id, { date: "2026-11-22" });
    const noon = ask({ op: "overview", today: NOW }, "overview");
    expect(noon.visits.map((v) => v.visit.start)).toEqual([540, 960]);
    expect(noon).toMatchObject({ date: DAY, next_day: null, next_visits: [], week_visits: 3, waiting: 0, active: 1 });
    const evening = ask({ op: "overview", today: { date: DAY, minute: 16 * 60 + 50 } }, "overview");
    expect(evening.next_day).toBe("2026-11-19");
    expect(evening.next_visits).toHaveLength(1);
  });

  it("hands out copies: changing an answer never changes the clinic", () => {
    const { ask } = clinic();
    const p = addPatient(ask);
    p.name = "שם אחר";
    const rows = ask({ op: "patients", today: NOW }, "patients");
    expect(rows[0]?.patient.name).toBe("נועם");
    (rows[0] as { patient: Patient }).patient.name = "עוד שם";
    expect(ask({ op: "patients", today: NOW }, "patients")[0]?.patient.name).toBe("נועם");
  });
});

describe("the demo clinic", () => {
  it("looks like a working clinic around today", () => {
    const c = new FakeClinic(standInHost());
    const now = localToday();
    const o = dataOf("overview", c.answer({ op: "overview", today: { date: now.date, minute: 8 * 60 } }), "overview");
    expect(o.visits.map((v) => v.patient_name)).toEqual(["תמר", "איתי", null, "מאיה"]);
    expect(o.visits.every((v) => v.visit.status === "planned" && !v.overlaps)).toBe(true);
    expect(o).toMatchObject({ waiting: 2, active: 5, next_day: null });
    expect(o.tasks.length).toBeGreaterThanOrEqual(4);
    expect(o.total_due).toBeGreaterThan(0);
    expect(o.month_received).toBeGreaterThanOrEqual(0);
    const kinds = new Set(o.alerts.map((a) => a.kind));
    for (const k of ["no_note", "debt", "waiting", "task_overdue", "feedback_soon"]) expect(kinds, k).toContain(k);
    // Oldest first.
    expect(o.alerts.map((a) => a.since)).toEqual([...o.alerts.map((a) => a.since)].sort());
    const week = dataOf("calendar", c.answer({ op: "calendar", from: addDays(now.date, -30), to: addDays(now.date, 30) }), "visits");
    expect(week.length).toBeGreaterThan(15);
    const rows = dataOf("patients", c.answer({ op: "patients", today: now }), "patients");
    expect(rows).toHaveLength(7);
    for (const r of rows) dataOf("patient_card", c.answer({ op: "patient_card", id: r.patient.id, today: now }), "card");
  });

  it("keeps clear of anything the commit scan would take for a real phone or mail", () => {
    const c = new FakeClinic(standInHost());
    const rows = dataOf("patients", c.answer({ op: "patients", today: localToday() }), "patients");
    const text = JSON.stringify(rows);
    expect(text).not.toMatch(/(?:\+972[-\s]?|(?:^|[^\d])0)(?:5\d|[2-4]|[89]|7\d)[-\s]?\d{3}[-\s]?\d{4}(?:[^\d]|$)/);
    expect(text).not.toContain("@");
  });

  it("resets with the demo core, answers through its one command, and reaches its cases", async () => {
    const core = new FakeCore();
    await expect(core.handle("clinic", { request: { op: "settings" } })).rejects.toMatchObject({ code: "locked" });
    await core.handle("unlock", { password: "x" });
    const now = localToday();
    const rows = dataOf("patients", (await core.handle("clinic", { request: { op: "patients", today: now } })) as never, "patients");
    const noam = rows.find((r) => r.patient.name === "נועם");
    // The demo patient is linked to the demo case of the same child.
    const cases = (await core.handle("list_cases", {})) as { id: string; child_name: string | null; meta: { code: string } }[];
    expect(noam?.patient.case_id).toBe(cases.find((x) => x.child_name === "נועם")?.id);

    const tamar = rows.find((r) => r.patient.name === "תמר") as (typeof rows)[number];
    const link = dataOf("create_case", (await core.handle("clinic", { request: { op: "create_case", patient_id: tamar.patient.id, today: now } })) as never, "case");
    const after = (await core.handle("list_cases", {})) as { id: string; child_name: string | null; meta: { code: string } }[];
    expect(after.find((x) => x.id === link.id)).toMatchObject({ child_name: "תמר", meta: { code: link.code } });

    // A second core is a second clinic: what was changed in the first is not there.
    const other = new FakeCore();
    await other.handle("unlock", { password: "x" });
    const fresh = dataOf("patients", (await other.handle("clinic", { request: { op: "patients", today: now } })) as never, "patients");
    expect(fresh.find((r) => r.patient.name === "תמר")?.patient.case_id).toBeNull();
  });
});
