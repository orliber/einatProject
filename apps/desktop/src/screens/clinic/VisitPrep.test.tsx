// "לקראת הפגישה": opening a visit that is still ahead shows what the last session left, the
// open follow-ups and the balance; a visit that was held does not; discreet mode shows no
// text. Over the demo core, with invented names; the clock is held at a Sunday morning.
import { configure, screen, within } from "@testing-library/react";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { Visit } from "../../ipc/generated/Visit";
import { a11yViolations } from "../../test/a11y";
import { forgetDiscreet } from "./ClinicScreen";
import { setDiscreet } from "../../discreet";
import { cardOf, openCard } from "./patientsTestKit";
import { lastRecord, prepOf } from "./VisitPrep";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  window.history.replaceState({}, "", "/");
});

/** The row of a visit on the card, by its date. */
const visitOn = async (date: string) => within(await screen.findByRole("region", { name: "פגישות" })).getByText(date).closest("li") as HTMLElement;

async function openPlanned() {
  const opened = await openCard("נועם");
  const card = await cardOf(opened, opened.id);
  const planned = card.visits.find((v) => v.visit.status === "planned");
  if (!planned) throw new Error("the demo child has no planned visit");
  return { ...opened, card, planned: planned.visit };
}

describe("“לקראת הפגישה” in a visit still ahead", () => {
  it("shows the last record, the open follow-ups and the balance", async () => {
    const { user, card, planned } = await openPlanned();
    const last = lastRecord(card, planned);
    expect(last).not.toBeNull();
    const row = await visitOn(planned.date.split("-").reverse().join("."));
    await user.click(within(row).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    const prep = await within(dialog).findByRole("region", { name: "לקראת הפגישה" });
    expect(prep).toHaveTextContent("בפעם הקודמת");
    expect(prep).toHaveTextContent((last?.record ?? "").trim().split("\n")[0] ?? "");
    const open = card.tasks.filter((t) => t.done_on === null);
    for (const t of open.slice(0, 3)) expect(prep).toHaveTextContent(t.title);
    if (card.balance.due > 0) expect(prep).toHaveTextContent("יתרה לתשלום");
    expect(await a11yViolations(dialog)).toEqual([]);
  });

  it("in discreet mode says there is a record, without its words or the follow-ups' words", async () => {
    const { user, card, planned } = await openPlanned();
    setDiscreet(true);
    const row = await visitOn(planned.date.split("-").reverse().join("."));
    await user.click(within(row).getByRole("button", { name: /פתיחת הפגישה/ }));
    const prep = await within(await screen.findByRole("dialog", { name: "פרטי הפגישה" })).findByRole("region", { name: "לקראת הפגישה" });
    expect(prep).toHaveTextContent("הרשומה מוסתרת במצב דיסקרטי.");
    expect(prep).not.toHaveTextContent((lastRecord(card, planned)?.record ?? "").trim().split("\n")[0] ?? "\u0000");
    for (const t of card.tasks.filter((x) => x.done_on === null)) expect(prep).not.toHaveTextContent(t.title);
  });

  it("a visit that was held has no such box", async () => {
    const { user, card } = await openPlanned();
    const held = card.visits.find((v) => v.visit.status === "held");
    if (!held) throw new Error("the demo child has no held visit");
    const row = await visitOn(held.visit.date.split("-").reverse().join("."));
    await user.click(within(row).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    await within(dialog).findByLabelText("סוג הפגישה");
    expect(within(dialog).queryByRole("region", { name: "לקראת הפגישה" })).toBeNull();
  });
});

describe("what “לקראת הפגישה” reads from the card", () => {
  const visit = (over: Partial<Visit>): Visit => ({
    id: "v-next", patient_id: "p", kind: "assessment", title: "", date: "2026-11-20", start: 540, minutes: 90, place: "", status: "planned",
    status_note: "", fee: 0, charge_missed: false, series_id: null, note: "", remember: [], ...over,
  });
  const note = (id: string, date: string, record: string, visitId: string | null = null) =>
    ({ id, patient_id: "p", visit_id: visitId, date, record, personal: "", case_input_id: null, recordings: [] });
  const card = (over: Partial<PatientCard>): PatientCard => ({
    patient: { id: "p", name: "ילד בדוי", birth_date: null, gender: null, contacts: [], framework: "", referred_by: "", reason: "קושי בפרידה בבוקר", notes: "",
      status: "active", waiting_since: null, urgent: false, session_fee: null, case_id: null, created_on: "2026-11-01" },
    age: null, visits: [], notes: [], payments: [], charges: [], tasks: [], balance: { charged: 0, paid: 0, due: 0 }, process: [], case: null, alerts: [], ...over,
  });

  it("a first meeting shows why the family came", () => {
    expect(prepOf(card({}), visit({}))).toMatchObject({ last: null, reason: "קושי בפרידה בבוקר" });
  });

  it("the newest record before the visit, never a later one, an empty one, or the visit's own", () => {
    const c = card({ notes: [note("n4", "2026-11-25", "אחרי"), note("n3", "2026-11-20", "של הפגישה הזו", "v-next"), note("n2", "2026-11-12", "  "), note("n1", "2026-11-05", "מפגש ראשון")] });
    expect(lastRecord(c, visit({}))?.id).toBe("n1");
  });

  it("nothing to say: no box", () => {
    expect(prepOf(card({ patient: { ...card({}).patient, reason: "" } }), visit({}))).toBeNull();
  });

  it("a family that paid ahead is not shown as owing", () => {
    expect(prepOf(card({ patient: { ...card({}).patient, reason: "" }, balance: { charged: 0, paid: 100, due: -100 } }), visit({}))).toBeNull();
  });
});
