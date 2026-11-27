// The waiting list over the demo core: who waits and for how long, the urgent first, the
// actions (a first meeting, urgent, off the list, a new family), a refusal that rolls back,
// the empty list, discreet mode and accessibility. Every name is invented.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import { a11yViolations } from "../../test/a11y";
import { forgetDiscreet } from "./ClinicScreen";
import { openOn, refusing, rowsOf } from "./patientsTestKit";
import { waitedWords, waitingOrder } from "./WaitingTab";

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

const WAITING = { name: "clinic", tab: "waiting" } as const;
const list = () => screen.findByRole("list", { name: /המשפחות שממתינות/ });
/** The families on the list, in the order shown. */
const families = async () => within(await list()).getAllByRole("group").map((g) => g.closest("li") as HTMLElement);
const familyOf = (name: string) => screen.getByRole("group", { name: `פעולות עבור ${name}` }).closest("li") as HTMLElement;
type User = Awaited<ReturnType<typeof openOn>>["user"];
/** What "⋯" of a family offers: the menu is opened, and its items are answered. */
async function more(user: User, name: string): Promise<HTMLElement> {
  await user.click(within(familyOf(name)).getByRole("button", { name: `עוד פעולות עבור ${name}` }));
  return screen.findByRole("menu");
}
/** One of the family's less common actions, from its "⋯". */
async function pick(user: User, name: string, item: string): Promise<void> {
  await user.click(within(await more(user, name)).getByRole("menuitem", { name: item }));
}

describe("the waiting list", () => {
  it("shows who waits, the urgent first, with how long, why and who referred", async () => {
    const { view } = await openOn(WAITING);
    const rows = await families();
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getByRole("button", { name: "עומר" })).toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByRole("button", { name: "שירה" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("2 משפחות ממתינות, אחת מהן דחופה");

    const omer = familyOf("עומר");
    expect(within(omer).getByText("דחוף")).toBeInTheDocument();
    expect(omer).toHaveTextContent("ממתינים 8 שבועות, מאז 20.09.2026");
    // Eight weeks is past the clinic's six: the wait is marked as a long one.
    expect(within(omer).getByText("המתנה ארוכה")).toBeInTheDocument();
    expect(omer).toHaveTextContent("עיכוב שפתי, והגן מבקש הערכה לפני ועדה.");
    expect(omer).toHaveTextContent("הפנה: קלינאית התקשורת");
    expect(omer).toHaveTextContent("אמא: רונית");

    const shira = familyOf("שירה");
    expect(shira).toHaveTextContent("ממתינים יותר משבוע, מאז 03.11.2026");
    expect(within(shira).queryByText("המתנה ארוכה")).toBeNull();
    expect(within(shira).queryByText("דחוף")).toBeNull();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("“לסמן כדחוף” shows at once and is saved; the mark can be taken off", async () => {
    const { user, requests } = await openOn(WAITING);
    await list();
    // In front of each family: the first meeting and the whole pathway; the rest is behind "⋯".
    expect(within(familyOf("שירה")).getAllByRole("button").map((b) => b.textContent)).toEqual(["שירה", "לקבוע פגישת היכרות", "לקבוע את כל המסלול", ""]);
    await pick(user, "שירה", "לסמן כדחוף");
    expect(within(familyOf("שירה")).getByText("דחוף")).toBeInTheDocument();
    await waitFor(() => expect(requests().some((r) => r.op === "save_patient" && r.patient.name === "שירה" && r.patient.urgent)).toBe(true));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("2 משפחות ממתינות, ושתיהן דחופות"));
    // Both urgent: whoever waits longer is still first.
    expect(within((await families())[0] as HTMLElement).getByRole("button", { name: "עומר" })).toBeInTheDocument();

    await pick(user, "עומר", "להסיר את הסימון דחוף");
    await waitFor(() => expect(within(familyOf("עומר")).queryByText("דחוף")).toBeNull());
    // Now שירה alone is urgent, and comes first.
    await waitFor(async () => expect(within((await families())[0] as HTMLElement).getByRole("button", { name: "שירה" })).toBeInTheDocument());
  });

  it("a mark the vault refuses rolls back, and says why", async () => {
    const { user, notes } = await openOn(WAITING, refusing("save_patient", "הכרטיס לא נשמר."));
    await list();
    await pick(user, "שירה", "לסמן כדחוף");
    await waitFor(() => expect(notes).toContain("הכרטיס לא נשמר."));
    await waitFor(() => expect(within(familyOf("שירה")).queryByText("דחוף")).toBeNull());
    // Not urgent after all: the menu offers to mark it again.
    expect(within(await more(user, "שירה")).getByRole("menuitem", { name: "לסמן כדחוף" })).toBeInTheDocument();
  });

  it("“לקבוע פגישת היכרות” opens the visit ready for this family, and saving takes it off the list", async () => {
    const served = await openOn(WAITING);
    const { user, requests } = served;
    await list();
    await user.click(within(familyOf("שירה")).getByRole("button", { name: "לקבוע פגישת היכרות" }));
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    expect(await within(dialog).findByLabelText("סוג הפגישה")).toHaveValue("intake");
    // The family is already chosen: nothing to search for.
    expect(within(dialog).getByRole("group", { name: "מטופל" })).toHaveTextContent("שירה");
    await user.click(within(dialog).getByRole("button", { name: "קביעת הפגישה" }));
    await waitFor(() => expect(requests().some((r) => r.op === "save_visit" && r.visit.kind === "intake")).toBe(true));
    // A visit that overlaps another says so first; either way the dialog is closed in the end.
    const saved = screen.queryByRole("dialog", { name: "הפגישה נשמרה" });
    if (saved) await user.click(within(saved).getByRole("button", { name: "הבנתי" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    await waitFor(() => expect(screen.queryByRole("button", { name: "שירה" })).toBeNull());
    expect(screen.getByRole("status")).toHaveTextContent("משפחה אחת ממתינה, והיא דחופה");
    // The core made the card active.
    expect((await rowsOf(served)).find((r) => r.patient.name === "שירה")?.patient.status).toBe("active");
  });

  it("“להסיר מהרשימה” closes the card only after she confirms, and nothing is erased", async () => {
    const served = await openOn(WAITING);
    const { user, requests, notes } = served;
    await list();
    await pick(user, "עומר", "להסיר מהרשימה");
    const confirm = await screen.findByRole("dialog", { name: "להסיר מרשימת ההמתנה" });
    expect(confirm).toHaveTextContent("המשפחה של עומר תרד מרשימת ההמתנה, והכרטיס ייסגר.");
    expect(confirm).toHaveTextContent("שום דבר לא נמחק");
    expect(requests().some((r) => r.op === "save_patient")).toBe(false);
    await user.click(within(confirm).getByRole("button", { name: "להסיר מהרשימה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_patient")).toMatchObject({ patient: { name: "עומר", status: "closed", urgent: false } });
    expect(requests().some((r) => r.op === "erase_patient")).toBe(false);
    expect(notes).toContain("המשפחה הוסרה מרשימת ההמתנה. הכרטיס נשמר בין הכרטיסים שהסתיימו.");
    await waitFor(() => expect(screen.queryByRole("button", { name: "עומר" })).toBeNull());
    expect(screen.getByRole("status")).toHaveTextContent("משפחה אחת ממתינה");
    expect((await rowsOf(served)).some((r) => r.patient.name === "עומר" && r.patient.status === "closed")).toBe(true);
  });

  it("a removal the vault refuses is said in the dialog, and the family stays", async () => {
    const { user } = await openOn(WAITING, refusing("save_patient", "הכרטיס לא נשמר."));
    await list();
    await pick(user, "עומר", "להסיר מהרשימה");
    const confirm = await screen.findByRole("dialog", { name: "להסיר מרשימת ההמתנה" });
    await user.click(within(confirm).getByRole("button", { name: "להסיר מהרשימה" }));
    expect(await within(confirm).findByText("הכרטיס לא נשמר.")).toBeInTheDocument();
    await user.click(within(confirm).getByRole("button", { name: "ביטול" }));
    expect(familyOf("עומר")).toBeInTheDocument();
  });

  it("“להוסיף לרשימת ההמתנה” opens a new card that starts on the list", async () => {
    const { user, requests, notes } = await openOn(WAITING);
    await list();
    await user.click(screen.getByRole("button", { name: "להוסיף לרשימת ההמתנה" }));
    const dialog = await screen.findByRole("dialog", { name: "מטופל חדש" });
    expect(within(dialog).getByRole("radio", { name: "ברשימת ההמתנה" })).toHaveAttribute("aria-checked", "true");
    await user.type(within(dialog).getByLabelText("שם הילד או הילדה"), "אלה לדוגמה");
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_patient")).toMatchObject({ patient: { name: "אלה לדוגמה", status: "waiting", waiting_since: "2026-11-15" } });
    expect(notes).toContain("המשפחה נוספה לרשימת ההמתנה");
    // She stays on the list, and the new family is on it, waiting from today.
    expect(await screen.findByRole("button", { name: "אלה לדוגמה" })).toBeInTheDocument();
    expect(familyOf("אלה לדוגמה")).toHaveTextContent("ממתינים מהיום");
    expect(screen.getByRole("status")).toHaveTextContent("3 משפחות ממתינות");
  });

  it("the family's name opens its card, and its details can be edited from the list", async () => {
    const { user, went } = await openOn(WAITING);
    await list();
    await pick(user, "שירה", "עריכת הפרטים");
    const dialog = await screen.findByRole("dialog", { name: "פרטי המטופל" });
    expect(within(dialog).getByLabelText("שם הילד או הילדה")).toHaveValue("שירה");
    await user.click(within(dialog).getByRole("button", { name: "ביטול" }));
    await user.click(screen.getByRole("button", { name: "שירה" }));
    expect((went[went.length - 1] as { patientId?: string }).patientId).toMatch(/^patient-/);
  });

  it("an empty list is a friendly sentence with a first step", async () => {
    window.history.replaceState({}, "", "/?clinic=empty");
    const { user, view } = await openOn(WAITING);
    expect(await screen.findByRole("heading", { name: "אף משפחה לא מחכה כרגע" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("אין כרגע משפחות שממתינות");
    expect(screen.queryByRole("list", { name: /המשפחות שממתינות/ })).toBeNull();
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "להוסיף משפחה לרשימה" }));
    expect(await screen.findByRole("dialog", { name: "מטופל חדש" })).toBeInTheDocument();
  });

  it("discreet mode shows initials only, in the list and in the confirmation", async () => {
    const { user } = await openOn(WAITING);
    await list();
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const panel = screen.getByRole("tabpanel");
    for (const name of ["עומר", "שירה", "רונית", "גיל"]) expect(within(panel).queryByText(name, { exact: false }), name).toBeNull();
    expect(panel).toHaveTextContent("אמא: ר.");
    await pick(user, "ע.", "להסיר מהרשימה");
    const confirm = await screen.findByRole("dialog", { name: "להסיר מרשימת ההמתנה" });
    expect(confirm).toHaveTextContent("המשפחה של ע. תרד מרשימת ההמתנה");
    expect(confirm).not.toHaveTextContent("עומר");
  });
});

describe("the words and the order of the waiting list", () => {
  it("says how long a family waits as she would, never rounding a week up", () => {
    expect([0, 1, 2, 5, 7, 12, 14, 20, 56, 60].map(waitedWords)).toEqual([
      "מהיום", "יום אחד", "יומיים", "5 ימים", "שבוע", "יותר משבוע", "שבועיים", "יותר משבועיים", "8 שבועות", "יותר מ-8 שבועות",
    ]);
  });

  it("puts the urgent first, then the longest wait; a card without a date waits since it was opened", () => {
    const sample = (name: string, urgent: boolean, since: string | null, created: string): PatientRow => ({
      patient: {
        id: name, name, birth_date: null, gender: null, contacts: [], framework: "", referred_by: "", reason: "", notes: "", status: "waiting",
        waiting_since: since, urgent, session_fee: null, case_id: null, created_on: created,
      },
      age: null, next_visit: null, last_visit: null, due: 0, open_tasks: 0,
    });
    const rows = [sample("ג", false, "2026-10-01", "2026-10-01"), sample("ב", true, "2026-11-01", "2026-11-01"), sample("א", false, null, "2026-09-01"), sample("ד", true, "2026-10-20", "2026-10-20")];
    expect(rows.sort(waitingOrder).map((r) => r.patient.name)).toEqual(["ד", "ב", "א", "ג"]);
  });
});

describe("how many of the families waiting are urgent", () => {
  it("in words that fit how many wait", async () => {
    const { urgentWords } = await import("./WaitingTab");
    expect(urgentWords(0, 3)).toBe("");
    expect(urgentWords(1, 1)).toBe(", והיא דחופה");
    expect(urgentWords(2, 2)).toBe(", ושתיהן דחופות");
    expect(urgentWords(3, 3)).toBe(", וכולן דחופות");
    expect(urgentWords(1, 3)).toBe(", אחת מהן דחופה");
    expect(urgentWords(2, 3)).toBe(", 2 מהן דחופות");
  });
});
