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

function clinic(host = standInHost()) {
  const c = new FakeClinic(host, { demo: false });
  const ask = <K extends ReplyKind>(request: ClinicRequest, kind: K): ReplyData<K> => dataOf(request.op, c.answer(request), kind);
  return { ask, host };
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
      "calendar", "caseChoices", "createCase", "deleteNote", "deleteTask", "deleteVisit", "diary", "erasePatient", "history", "income", "linkCase",
      "message", "noteToCase", "overview", "patientCard", "patients", "saveCharge", "saveNote", "savePatient", "savePayment", "saveSettings",
      "saveTask", "saveVisit", "settings", "tasks", "voidCharge", "voidPayment",
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
    const made = ask({ op: "save_visit", visit: visit(p.id, { status: "held", charge_missed: true }), repeat: null, scope: "one" }, "saved");
    expect(made.visits).toHaveLength(1);
    expect(made.visits[0]).toMatchObject({ patient_name: "נועם", has_note: false, overlaps: false, visit: { status: "planned", charge_missed: false, series_id: null } });
    expect(made.overlapping).toBe(0);
    expect(refusal(() => ask({ op: "save_visit", visit: visit("nobody"), repeat: null, scope: "one" }, "saved")).message).toContain("מטופל");
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
      { reason: "א".repeat(4001) }, { created_on: "" }, { waiting_since: "אתמול" },
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
    // Scheduled, cancelled without a charge, and a task: nothing happened yet.
    const v = addVisit(ask, p.id, { date: addDays(localToday().date, 3) });
    addVisit(ask, p.id, { status: "cancelled", date: "2026-11-02" });
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
  });

  it("a card that holds a record is closed, not erased, until its retention period has passed (D-174)", () => {
    const today = localToday().date;
    const happened: ((ask: Ask, id: string) => void)[] = [
      (ask, id) => addVisit(ask, id, { status: "held", date: today }),
      (ask, id) => addVisit(ask, id, { status: "no_show", charge_missed: true, date: today }),
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
