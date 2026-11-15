// The patient list over the demo core: what she sees, search, the filter, sorting, pages, the
// way into a card and back, the empty and failing states, discreet mode and accessibility.
// Every name is invented; the clock is held at a Sunday morning.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import type { Patient } from "../../ipc/generated/Patient";
import { a11yViolations } from "../../test/a11y";
import { serve, type Override } from "../../test/host";
import { forgetDiscreet } from "./ClinicScreen";
import { ask, openOn, show, withClipboard } from "./patientsTestKit";

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

const PATIENTS = { name: "clinic", tab: "patients" } as const;
const table = () => screen.findByRole("table", { name: /רשימת המטופלים/ });
/** The names on the list, in the order shown. */
const names = () => screen.getAllByRole("rowheader").map((h) => within(h).getAllByRole("button")[0]?.textContent);
const rowOf = (name: string) => screen.getByRole("button", { name }).closest("tr") as HTMLElement;

describe("the patient list", () => {
  it("shows the families in process by name, with what she needs of each, and passes the accessibility check", async () => {
    const { view } = await openOn(PATIENTS);
    await table();
    expect(names()).toEqual(["איתי", "יובל", "מאיה", "נועם", "תמר"]);
    expect(screen.getByRole("status")).toHaveTextContent("5 מטופלים");
    expect(screen.getByRole("radio", { name: "בתהליך" })).toHaveAttribute("aria-checked", "true");
    // Families that wait have their own tab.
    expect(screen.queryByRole("button", { name: "עומר" })).toBeNull();

    const tamar = rowOf("תמר");
    expect(tamar).toHaveTextContent("אמא: יעל");
    expect(tamar).toHaveTextContent("גן רימון");
    expect(tamar).toHaveTextContent("היום, 09:00");
    expect(tamar).toHaveTextContent("מפגש אבחון");
    expect(tamar).toHaveTextContent("05.11.2026");
    expect(within(tamar).getAllByRole("cell")[4]).toHaveTextContent("1");
    // What is owed is in the warning colour; nothing owed is said quietly.
    const yuval = within(rowOf("יובל")).getAllByRole("cell")[3] as HTMLElement;
    expect(yuval).toHaveTextContent("1,650 ₪");
    expect(yuval.querySelector(".clinic-money-owed")).not.toBeNull();
    expect(within(rowOf("איתי")).getAllByRole("cell")[3]).toHaveTextContent("אין");
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("searches by the child's name, a parent's name or a phone number, after a short pause", async () => {
    const { user, requests } = await openOn(PATIENTS);
    await table();
    const search = screen.getByLabelText("חיפוש");
    await user.type(search, "יעל");
    await waitFor(() => expect(names()).toEqual(["תמר"]));
    expect(screen.getByRole("status")).toHaveTextContent("מטופל אחד");
    await user.clear(search);
    await user.type(search, "052 555 07");
    await waitFor(() => expect(names()).toEqual(["יובל"]));
    await user.clear(search);
    await user.type(search, "מא");
    await waitFor(() => expect(names()).toEqual(["מאיה"]));
    // Searching never asks the vault again: one read of the list serves every key.
    expect(requests().filter((r) => r.op === "patients")).toHaveLength(1);
  });

  it("a search that finds a family outside the filter says so, and offers to show everyone", async () => {
    const { user } = await openOn(PATIENTS);
    await table();
    await user.type(screen.getByLabelText("חיפוש"), "עומר");
    expect(await screen.findByRole("heading", { name: "לא נמצא מטופל כזה" })).toBeInTheDocument();
    expect(screen.getByText(/יש כרטיס אחד שמתאים לחיפוש/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "להציג את כולם" }));
    expect(screen.getByRole("radio", { name: "כולם" })).toHaveAttribute("aria-checked", "true");
    expect(names()).toEqual(["עומר"]);
    expect(rowOf("עומר")).toHaveTextContent("ברשימת ההמתנה");
    expect(rowOf("עומר")).toHaveTextContent("דחוף");

    // Nothing like it anywhere: the way out is to clear the search.
    await user.clear(screen.getByLabelText("חיפוש"));
    await user.type(screen.getByLabelText("חיפוש"), "אין כזה");
    await user.click(await screen.findByRole("button", { name: "לנקות את החיפוש" }));
    await waitFor(() => expect(names()).toHaveLength(7));
  });

  it("filters by where the family is, and an empty filter is a sentence, not a blank", async () => {
    const { user, went } = await openOn(PATIENTS);
    await table();
    await user.click(screen.getByRole("radio", { name: "הסתיים" }));
    expect(screen.getByRole("heading", { name: "אין כרטיסים שהסתיימו" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    await user.click(screen.getByRole("radio", { name: "כולם" }));
    expect(names()).toEqual(["איתי", "יובל", "מאיה", "נועם", "עומר", "שירה", "תמר"]);
    await user.click(screen.getByRole("radio", { name: "בתהליך" }));
    expect(went).toEqual([]);
  });

  it("sorts by a column, and a second press turns it over", async () => {
    const { user } = await openOn(PATIENTS);
    await table();
    const header = (name: string) => screen.getByRole("columnheader", { name: new RegExp(name) });
    expect(header("שם")).toHaveAttribute("aria-sort", "ascending");
    // Amounts start from the largest; those who owe the same stay by name.
    await user.click(within(header("יתרה לתשלום")).getByRole("button"));
    expect(header("יתרה לתשלום")).toHaveAttribute("aria-sort", "descending");
    expect(header("שם")).toHaveAttribute("aria-sort", "none");
    expect(names()).toEqual(["יובל", "נועם", "מאיה", "איתי", "תמר"]);
    await user.click(within(header("יתרה לתשלום")).getByRole("button"));
    expect(header("יתרה לתשלום")).toHaveAttribute("aria-sort", "ascending");
    expect(names()?.[4]).toBe("יובל");
    // The youngest first.
    await user.click(within(header("גיל")).getByRole("button"));
    expect(names()).toEqual(["תמר", "נועם", "יובל", "איתי", "מאיה"]);
    // The next visit: the soonest first, and a family with none last.
    await user.click(within(header("הפגישה הבאה")).getByRole("button"));
    expect(names().slice(0, 2)).toEqual(["תמר", "איתי"]);
    expect(names()?.[4]).toBe("יובל");
    await user.click(within(header("שם")).getByRole("button"));
    await user.click(within(header("שם")).getByRole("button"));
    expect(names()).toEqual(["תמר", "נועם", "מאיה", "יובל", "איתי"]);
  });

  it("a row opens the card through the route, and Back returns to the list", async () => {
    const { user, went } = await openOn(PATIENTS);
    await table();
    await user.click(screen.getByRole("button", { name: "מאיה" }));
    expect(went[went.length - 1]).toMatchObject({ name: "clinic", tab: "patients" });
    expect((went[went.length - 1] as { patientId?: string }).patientId).toMatch(/^patient-/);
    expect(await screen.findByRole("heading", { name: "מאיה" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "מטופלים" })).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("button", { name: "חזרה לרשימת המטופלים" }));
    expect(went[went.length - 1]).toEqual({ name: "clinic", tab: "patients" });
    await table();
    // The rest of the row opens the card too, for the mouse.
    await user.click(within(rowOf("יובל")).getByText("גן דקל", { exact: false }));
    expect(await screen.findByRole("heading", { name: "יובל" })).toBeInTheDocument();
  });

  it("“מטופל חדש” adds a family and opens its card", async () => {
    const { user, went, requests } = await openOn(PATIENTS);
    await table();
    await user.click(screen.getByRole("button", { name: "מטופל חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "מטופל חדש" });
    await user.type(within(dialog).getByLabelText("שם הילד או הילדה"), "אלה לדוגמה");
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));
    await waitFor(() => expect(requests().some((r) => r.op === "save_patient")).toBe(true));
    expect(await screen.findByRole("heading", { name: "אלה לדוגמה" })).toBeInTheDocument();
    expect((went[went.length - 1] as { patientId?: string }).patientId).toMatch(/^patient-/);
  });

  it("a long list comes in pages of 25", async () => {
    const served = await serve(withClipboard);
    const blank: Patient = {
      id: "", name: "", birth_date: null, gender: null, contacts: [], framework: "", referred_by: "", reason: "", notes: "", status: "active",
      waiting_since: null, urgent: false, session_fee: null, case_id: null, created_on: "2026-11-15",
    };
    for (let i = 1; i <= 24; i++) await ask(served, { op: "save_patient", patient: { ...blank, name: `ילד בדוי ${String(i).padStart(2, "0")}` } }, "patient");
    const { user } = show(served, PATIENTS);
    await table();
    expect(screen.getByText("29 מטופלים")).toBeInTheDocument();
    expect(names()).toHaveLength(25);
    const pager = screen.getByRole("navigation", { name: "עמודי רשימת המטופלים" });
    expect(pager).toHaveTextContent("עמוד 1 מתוך 2");
    await user.click(within(pager).getByRole("button", { name: "הבא" }));
    expect(names()).toHaveLength(4);
    // A search starts from the first page again.
    await user.type(screen.getByLabelText("חיפוש"), "ילד בדוי 0");
    await waitFor(() => expect(names()).toHaveLength(9));
    expect(screen.queryByRole("navigation", { name: "עמודי רשימת המטופלים" })).toBeNull();
  });

  it("discreet mode shows initials only, for the child and for the parents", async () => {
    const { user } = await openOn(PATIENTS);
    await table();
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    expect(names()).toEqual(["א.", "י.", "מ.", "נ.", "ת."]);
    const panel = screen.getByRole("tabpanel");
    for (const name of ["תמר", "איתי", "מאיה", "נועם", "יובל", "יעל", "מיכל", "דורון"]) expect(within(panel).queryByText(name, { exact: false }), name).toBeNull();
    expect(panel).toHaveTextContent("אמא: י.");
  });

  it("a new clinic gets a friendly start", async () => {
    window.history.replaceState({}, "", "/?clinic=empty");
    const { user, view } = await openOn(PATIENTS);
    expect(await screen.findByRole("heading", { name: "עוד אין מטופלים בקליניקה" })).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "להוסיף מטופל" }));
    expect(await screen.findByRole("dialog", { name: "מטופל חדש" })).toBeInTheDocument();
  });

  it("shows the shape of the list while it loads, and a failure with a way to try again", async () => {
    let fail = true;
    let release: () => void = () => undefined;
    const wait = new Promise<void>((r) => (release = r));
    const flaky: Override = async (cmd, args) => {
      if (cmd !== "clinic" || (args.request as ClinicRequest).op !== "patients") return undefined;
      await wait;
      if (fail) throw { code: "internal", message: "רשימת המטופלים לא נטענה.", details: [], action: { kind: "retry" } };
      return undefined;
    };
    const { user } = await openOn(PATIENTS, flaky);
    expect(screen.getByRole("status", { name: "טוען את רשימת המטופלים" })).toBeInTheDocument();
    release();
    expect(await screen.findByText("רשימת המטופלים לא נטענה.")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    fail = false;
    await user.click(screen.getByRole("button", { name: /לנסות שוב/ }));
    await table();
    expect(names()).toHaveLength(5);
  });
});
