// The money tab over the demo core: the income diary of a month (days, lines, voided and
// changed lines, totals, payments with no receipt number, the notice about the tax
// authority), the open balances with the payment message, and the year at a glance.
// Every name is invented; the clock is held at a Sunday morning.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { Alert } from "../../ipc/generated/Alert";
import type { DiaryLine } from "../../ipc/generated/DiaryLine";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import { a11yViolations } from "../../test/a11y";
import type { Override, Served } from "../../test/host";
import { forgetDiscreet } from "./ClinicScreen";
import { formatMoney } from "./format";
import { barPercent, daysLackingReceipts, forgetMoneyView, lacksReceipt, missingReceipts, openBalances, shiftMonth } from "./MoneyTab";
import { ask, holding, idOf, NOW, openOn, refusing } from "./officeTestKit";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  forgetMoneyView();
  window.history.replaceState({}, "", "/");
});

const MONEY = { name: "clinic", tab: "money" } as const;
const NOTICE = "היומן שומר כל שינוי שנעשה בפגישות, בחיובים ובתקבולים. התוכנה אינה רשומה ברשות המסים כתוכנה לניהול 'יומן רופא', ואת הקבלות מפיקים בתוכנת החשבוניות. כאן רושמים את מספר הקבלה.";
const day = (caption: string) => screen.findByRole("table", { name: caption });
const totals = () => screen.getByRole("region", { name: "סיכום נובמבר 2026" });
const balances = () => screen.getByRole("region", { name: "יתרות פתוחות" });
const owing = () => within(balances()).findByRole("list", { name: "משפחות עם יתרה לתשלום" });
/** The row of a child on a day's table. */
const rowOf = (table: HTMLElement, name: string, what: string) =>
  within(table).getAllByRole("row").find((r) => within(r).queryByRole("rowheader", { name }) !== null && r.textContent?.includes(what)) as HTMLElement;

const line = (over: Partial<DiaryLine>): DiaryLine => ({
  record_id: "x", time: null, patient_name: "", what: "תשלום", status: null, charged: 0, received: 0, method: null, receipt: "", payer: "", note: "", voided: false, changed: false, ...over,
});

describe("the pure pieces", () => {
  it("months move across a year end, and stop at the years the clinic keeps", () => {
    expect(shiftMonth("2026-11", 1)).toBe("2026-12");
    expect(shiftMonth("2026-12", 1)).toBe("2027-01");
    expect(shiftMonth("2026-01", -1)).toBe("2025-12");
    expect(shiftMonth("2000-01", -1)).toBeNull();
    expect(shiftMonth("2200-12", 1)).toBeNull();
  });

  it("only a payment that counts can lack a receipt number", () => {
    expect(lacksReceipt(line({ received: 100 }))).toBe(true);
    expect(lacksReceipt(line({ received: 100, receipt: "17" }))).toBe(false);
    expect(lacksReceipt(line({ received: 100, voided: true }))).toBe(false);
    expect(lacksReceipt(line({ charged: 100 }))).toBe(false);
    const days = [
      { date: "2026-11-01", charged: 0, received: 100, lines: [line({ record_id: "a", received: 100, receipt: "17" })] },
      { date: "2026-11-02", charged: 50, received: 100, lines: [line({ record_id: "b", charged: 50 }), line({ record_id: "c", received: 100 })] },
    ];
    expect(daysLackingReceipts(days).map((d) => [d.date, d.lines.map((l) => l.record_id)])).toEqual([["2026-11-02", ["c"]]]);
    // The count is of the lines the days show (an erased card's payments have none).
    expect(missingReceipts(days)).toBe(1);
    expect(missingReceipts([])).toBe(0);
  });

  it("the debts the clinic points out come first, the oldest first; then the rest by size", () => {
    const row = (id: string, due: number) => ({ patient: { id, name: id }, due }) as unknown as PatientRow;
    const debt = (id: string, since: string) => ({ kind: "debt", patient_id: id, since }) as unknown as Alert;
    const out = openBalances([row("a", 100), row("b", 900), row("c", 0), row("d", 300), row("e", -50), row("f", 200)], [debt("d", "2026-09-01"), debt("a", "2026-08-01")]);
    expect(out.map((b) => [b.row.patient.id, b.since])).toEqual([["a", "2026-08-01"], ["d", "2026-09-01"], ["b", null], ["f", null]]);
  });

  it("a bar is a share of the largest month, never a sliver that cannot be seen", () => {
    expect(barPercent(0, 1000)).toBe(0);
    expect(barPercent(1000, 1000)).toBe(100);
    expect(barPercent(250, 1000)).toBe(25);
    expect(barPercent(1, 1000)).toBe(1);
    expect(barPercent(5, 0)).toBe(0);
  });
});

describe("the income diary", () => {
  it("shows the month by days, each line with its details, and the day's totals", async () => {
    const { view } = await openOn(MONEY);
    expect(screen.getByRole("radio", { name: "יומן הכנסות" })).toHaveAttribute("aria-checked", "true");
    expect(await screen.findByRole("heading", { name: "נובמבר 2026" })).toBeInTheDocument();
    const fifth = await day("יום חמישי, 05.11.2026");
    expect(within(fifth).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["שעה", "ילד או ילדה", "מה", "מצב", "חיוב", "התקבל", "אמצעי", "קבלה", "מי שילם", "הערה"]);
    // The session, then the payment of that day.
    const session = rowOf(fifth, "תמר", "פגישת היכרות");
    expect(session).toHaveTextContent("18:00");
    expect(within(session).getByText("התקיימה")).toBeInTheDocument();
    expect(session).toHaveTextContent("450 ₪");
    const paid = rowOf(fifth, "תמר", "תשלום");
    expect(paid).toHaveTextContent("מזומן");
    expect(within(paid).getByText("בלי מספר קבלה")).toBeInTheDocument();
    // The day came to 450 charged and 450 received.
    const foot = within(fifth).getByRole("rowheader", { name: "סך היום" }).closest("tr") as HTMLElement;
    expect(within(foot).getAllByRole("cell").slice(0, 2).map((c) => c.textContent)).toEqual(["450 ₪", "450 ₪"]);
    // A visit that is only planned is in the diary, and charges nothing yet.
    const today = await day("יום ראשון, 15.11.2026");
    expect(within(rowOf(today, "איתי", "פגישת היכרות")).getByText("מתוכננת")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("says, always, that the program is not registered with the tax authority", async () => {
    const { user } = await openOn(MONEY);
    expect(screen.getAllByRole("note").some((n) => n.textContent === NOTICE)).toBe(true);
    await day("יום חמישי, 05.11.2026");
    // In an empty month too.
    await user.click(screen.getByRole("button", { name: "החודש הבא" }));
    await user.click(screen.getByRole("button", { name: "החודש הבא" }));
    expect(await screen.findByRole("heading", { name: "ינואר 2027" })).toBeInTheDocument();
    expect(screen.getAllByRole("note").some((n) => n.textContent === NOTICE)).toBe(true);
    // And nothing here offers to take the diary out of the vault.
    expect(screen.queryByRole("button", { name: /ייצוא|קובץ|הדפסה/ })).toBeNull();
  });

  it("the month's figures: charged, received, by method, as the vault counts them", async () => {
    const served = await openOn(MONEY);
    await day("יום חמישי, 05.11.2026");
    const diary = await ask(served, { op: "diary", month: "2026-11" }, "diary");
    expect(diary.received).toBe(80_000);
    expect(within(totals()).getByText("חויב בחודש").closest("div")).toHaveTextContent(formatMoney(diary.charged));
    expect(within(totals()).getByText("התקבל בחודש").closest("div")).toHaveTextContent("800 ₪");
    const methods = within(totals()).getByRole("list", { name: "התקבל לפי אמצעי תשלום" });
    expect(within(methods).getAllByRole("listitem").map((m) => m.textContent)).toEqual(["מזומן: 450 ₪", "פייבוקס: 350 ₪"]);
  });

  it("moves between months, and back to this one", async () => {
    const { user, requests } = await openOn(MONEY);
    await day("יום חמישי, 05.11.2026");
    expect(screen.queryByRole("button", { name: "לחודש הנוכחי" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "החודש הקודם" }));
    expect(await screen.findByRole("heading", { name: "אוקטובר 2026" })).toBeInTheDocument();
    expect(await day("יום ראשון, 25.10.2026")).toBeInTheDocument();
    expect(requests().some((r) => r.op === "diary" && r.month === "2026-10")).toBe(true);
    await user.click(screen.getByRole("button", { name: "החודש הבא" }));
    await user.click(screen.getByRole("button", { name: "החודש הבא" }));
    expect(await screen.findByRole("heading", { name: "דצמבר 2026" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "לחודש הנוכחי" }));
    expect(await screen.findByRole("heading", { name: "נובמבר 2026" })).toBeInTheDocument();
  });

  it("a month with nothing in it says so", async () => {
    const { user } = await openOn(MONEY);
    await day("יום חמישי, 05.11.2026");
    for (let i = 0; i < 3; i++) await user.click(screen.getByRole("button", { name: "החודש הבא" }));
    expect(await screen.findByRole("heading", { name: "אין רישומים בחודש פברואר 2027" })).toBeInTheDocument();
    expect(screen.getByText("עוד לא התקבל תשלום בחודש הזה.")).toBeInTheDocument();
  });

  it("counts the payments with no receipt number, and shows only them on request", async () => {
    const { user } = await openOn(MONEY);
    await day("יום חמישי, 05.11.2026");
    expect(within(totals()).getByText("תקבול אחד בלי מספר קבלה")).toBeInTheDocument();
    const only = within(totals()).getByRole("button", { name: "להציג רק אותם" });
    expect(only).toHaveAttribute("aria-pressed", "false");
    await user.click(only);
    expect(only).toHaveAttribute("aria-pressed", "true");
    const shown = screen.getByRole("region", { name: "תקבולים בלי מספר קבלה" });
    expect(within(shown).getAllByRole("table")).toHaveLength(1);
    const table = within(shown).getByRole("table", { name: "יום חמישי, 05.11.2026" });
    // Only the payment: not the session of that day, and no day total that would mislead.
    expect(within(table).getAllByRole("row")).toHaveLength(2);
    expect(within(table).queryByRole("rowheader", { name: "סך היום" })).toBeNull();
    expect(within(shown).getByRole("status")).toHaveTextContent("מוצגים רק תקבולים בלי מספר קבלה.");
    await user.click(within(shown).getByRole("button", { name: "להציג את כל היומן" }));
    expect(await day("יום ראשון, 01.11.2026")).toBeInTheDocument();
  });

  it("a voided payment stays, struck through with its reason, is counted nowhere, and its history opens from “שונה”", async () => {
    const prepare = async (served: Served) => {
      const tamar = await idOf(served, "תמר");
      const card = await ask(served, { op: "patient_card", id: tamar, today: NOW }, "card");
      await ask(served, { op: "void_payment", id: card.payments[0]?.id ?? "", reason: "נרשם פעמיים" }, "payment");
    };
    const { user, view } = await openOn(MONEY, undefined, prepare);
    const fifth = await day("יום חמישי, 05.11.2026");
    const paid = rowOf(fifth, "תמר", "תשלום");
    expect(paid).toHaveClass("clinic-diary-void");
    expect(within(paid).getByText("בוטל")).toBeInTheDocument();
    expect(paid).toHaveTextContent("בוטל: נרשם פעמיים");
    // A voided payment does not lack a receipt, and the day and the month do not count it.
    expect(within(paid).queryByText("בלי מספר קבלה")).toBeNull();
    const foot = within(fifth).getByRole("rowheader", { name: "סך היום" }).closest("tr") as HTMLElement;
    expect(within(foot).getAllByRole("cell").slice(0, 2).map((c) => c.textContent)).toEqual(["450 ₪", "0 ₪"]);
    expect(within(totals()).getByText("התקבל בחודש").closest("div")).toHaveTextContent("350 ₪");
    expect(within(totals()).getByText("לכל התקבולים של החודש יש מספר קבלה.")).toBeInTheDocument();
    // The session of that day was never changed: no mark on it.
    expect(within(rowOf(fifth, "תמר", "פגישת היכרות")).queryByRole("button", { name: /^שונה/ })).toBeNull();

    await user.click(within(paid).getByRole("button", { name: "שונה. היסטוריית השינויים של תשלום, תמר" }));
    const dialog = await screen.findByRole("dialog", { name: "היסטוריית שינויים" });
    expect(dialog).toHaveTextContent("תשלום · תמר");
    expect(await within(dialog).findByText("נוצר")).toBeInTheDocument();
    expect(within(dialog).getAllByRole("listitem")[0]).toHaveTextContent("בוטל");
    expect(dialog).toHaveTextContent("סיבת הביטול: נרשם פעמיים");
    expect(await a11yViolations(view.baseElement)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "סגירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("a name on a line opens the card, when one card has that name", async () => {
    const { user, went } = await openOn(MONEY);
    const fifth = await day("יום חמישי, 05.11.2026");
    await user.click(await within(rowOf(fifth, "תמר", "תשלום")).findByRole("button", { name: "תמר" }));
    expect(went[went.length - 1]).toMatchObject({ name: "clinic", tab: "patients" });
    expect((went[went.length - 1] as { patientId?: string }).patientId).toMatch(/^patient-/);
  });

  it("a busy month comes in pages of days", async () => {
    const prepare = async (served: Served) => {
      const tamar = await idOf(served, "תמר");
      for (let d = 16; d <= 28; d++) {
        await ask(served, { op: "save_charge", charge: { id: "", patient_id: tamar, date: `2026-11-${d}`, amount: 1_000, what: "צילום מסמכים", voided: false, void_reason: "" } }, "charge");
      }
    };
    const { user } = await openOn(MONEY, undefined, prepare);
    await day("יום ראשון, 01.11.2026");
    expect(screen.getAllByRole("table")).toHaveLength(10);
    const pager = screen.getByRole("navigation", { name: "עמודים של ימי החודש" });
    await user.click(within(pager).getByRole("button", { name: "הבא" }));
    expect(await day("שבת, 28.11.2026")).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "יום ראשון, 01.11.2026" })).toBeNull();
  });

  it("shows the shape of the diary while it loads, and a diary that could not be read says so", async () => {
    const { over, release } = holding("diary");
    const first = await openOn(MONEY, over);
    expect(await screen.findByRole("status", { name: "טוען את יומן ההכנסות" })).toBeInTheDocument();
    expect(screen.getAllByRole("note").some((n) => n.textContent === NOTICE)).toBe(true);
    release();
    await day("יום חמישי, 05.11.2026");
    expect(screen.queryByRole("status", { name: "טוען את יומן ההכנסות" })).toBeNull();
    first.view.unmount();

    await openOn(MONEY, refusing("diary", "היומן לא נקרא מהכספת."));
    expect((await screen.findAllByRole("alert")).some((a) => a.textContent?.includes("היומן לא נקרא מהכספת."))).toBe(true);
  });
});

describe("open balances", () => {
  it("lists who owes, how much and since when, the old debt first", async () => {
    const served = await openOn(MONEY);
    const list = await owing();
    const rows = (await ask(served, { op: "patients", today: NOW }, "patients")).filter((r) => r.due > 0);
    const families = within(list).getAllByRole("listitem");
    expect(families).toHaveLength(rows.length);
    // יובל owes since the report was charged, long enough for the clinic to point it out.
    const first = families[0] as HTMLElement;
    expect(within(first).getByRole("button", { name: "יובל" })).toBeInTheDocument();
    expect(first).toHaveTextContent(formatMoney(rows.find((r) => r.patient.name === "יובל")?.due ?? 0));
    expect(first).toHaveTextContent(/מאז \d\d\.\d\d\.2026/);
    // A fresher debt has no date to point at: it is from the last 30 days (the clinic's threshold).
    const noam = within(list).getByRole("group", { name: "פעולות עבור נועם" }).closest("li") as HTMLElement;
    expect(noam).toHaveTextContent("מ-30 הימים האחרונים");
    const total = rows.reduce((sum, r) => sum + r.due, 0);
    expect(balances()).toHaveTextContent(`${rows.length} משפחות`);
    expect(balances()).toHaveTextContent(formatMoney(total));
    // A family that owes nothing is not here.
    expect(within(list).queryByRole("button", { name: "תמר" })).toBeNull();
  });

  it("“הודעת תשלום” fills the message and copies it through the protected clipboard only", async () => {
    const { user, calls, notes } = await openOn(MONEY);
    const list = await owing();
    await user.click(within(within(list).getByRole("group", { name: "פעולות עבור יובל" })).getByRole("button", { name: "הודעת תשלום" }));
    await waitFor(() => expect(notes).toHaveLength(1));
    expect(notes[0]).toBe("הודעת התשלום הועתקה ללוח המוגן, ואפשר להדביק אותה בהודעה. יימחק מהלוח בעוד דקה.");
    const copied = calls.filter((c) => c.cmd === "copy_secret");
    expect(copied).toHaveLength(1);
    expect(String(copied[0]?.args.text)).toMatch(/^שלום ענת, היתרה לתשלום היא [\d.]+ ₪\. תודה רבה, /);
    // The toast never carries the message, and the default text leaves the child's name out.
    expect(String(copied[0]?.args.text)).not.toContain("יובל");
  });

  it("a message that could not be made, or copied, is said, and nothing is claimed", async () => {
    const broken: Override = (cmd) => {
      if (cmd === "copy_secret") throw { code: "internal", message: "ההעתקה לא הצליחה.", details: [], action: { kind: "dismiss" } };
      return undefined;
    };
    const first = await openOn(MONEY, broken);
    await first.user.click(within(within(await owing()).getByRole("group", { name: "פעולות עבור יובל" })).getByRole("button", { name: "הודעת תשלום" }));
    await waitFor(() => expect(first.notes).toEqual(["ההעתקה לא הצליחה."]));
    first.view.unmount();

    const second = await openOn(MONEY, refusing("message", "אין יתרה לתשלום, ולכן אין מה לשלוח."));
    await second.user.click(within(within(await owing()).getByRole("group", { name: "פעולות עבור יובל" })).getByRole("button", { name: "הודעת תשלום" }));
    await waitFor(() => expect(second.notes).toEqual(["אין יתרה לתשלום, ולכן אין מה לשלוח."]));
    expect(second.calls.some((c) => c.cmd === "copy_secret")).toBe(false);
  });

  it("“לכרטיס” opens the family's card", async () => {
    const served = await openOn(MONEY);
    const list = await owing();
    await served.user.click(within(within(list).getByRole("group", { name: "פעולות עבור יובל" })).getByRole("button", { name: "לכרטיס" }));
    expect(served.went[served.went.length - 1]).toEqual({ name: "clinic", tab: "patients", patientId: await idOf(served, "יובל") });
  });

  it("a clinic where nobody owes says so, and an empty month is an empty month", async () => {
    window.history.replaceState({}, "", "/?clinic=empty");
    const { view } = await openOn(MONEY);
    expect(await screen.findByRole("heading", { name: "אין רישומים בחודש נובמבר 2026" })).toBeInTheDocument();
    expect(await within(balances()).findByText("אין יתרות פתוחות. כל מה שחויב שולם.")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });
});

describe("the year at a glance", () => {
  const openYear = async (over?: Override) => {
    const served = await openOn(MONEY, over);
    await served.user.click(screen.getByRole("radio", { name: "סיכום שנתי" }));
    return served;
  };

  it("shows twelve months as a table with the year's totals, as the vault counts them", async () => {
    const served = await openYear();
    const table = await screen.findByRole("table", { name: "סיכום 2026 לפי חודשים" });
    const income = await ask(served, { op: "income", year: 2026 }, "income");
    expect(within(table).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["חודש", "התקיימו", "בוטלו", "לא הגיעו", "חויב", "התקבל"]);
    const months = within(table).getAllByRole("rowheader").map((h) => h.textContent);
    expect(months).toEqual(["ינואר", "פברואר", "מרץ", "אפריל", "מאי", "יוני", "יולי", "אוגוסט", "ספטמבר", "אוקטובר", "נובמבר", "דצמבר", "סך השנה"]);
    const cells = (name: string) => within(within(table).getByRole("rowheader", { name }).closest("tr") as HTMLElement).getAllByRole("cell").map((c) => c.textContent);
    const november = income.months[10];
    expect(cells("נובמבר")).toEqual([String(november?.held), String(november?.cancelled), String(november?.no_show), formatMoney(november?.charged ?? 0), formatMoney(november?.received ?? 0)]);
    const held = income.months.reduce((sum, m) => sum + m.held, 0);
    expect(held).toBeGreaterThan(5);
    expect(cells("סך השנה")).toEqual([String(held), "0", "0", formatMoney(income.charged), formatMoney(income.received)]);
    expect(await a11yViolations(served.view.container)).toEqual([]);
  });

  it("draws a plain bar for each month, sized by the largest, hidden from a screen reader", async () => {
    const served = await openYear();
    const table = await screen.findByRole("table", { name: "סיכום 2026 לפי חודשים" });
    const income = await ask(served, { op: "income", year: 2026 }, "income");
    const most = Math.max(...income.months.map((m) => m.received));
    const bars = Array.from(table.querySelectorAll<HTMLElement>(".clinic-bar"));
    expect(bars).toHaveLength(12);
    expect(bars.map((b) => b.style.width)).toEqual(income.months.map((m) => `${barPercent(m.received, most)}%`));
    expect(bars.some((b) => b.style.width === "100%")).toBe(true);
    expect(bars.every((b) => b.closest("[aria-hidden='true']") !== null)).toBe(true);
    // No picture, no canvas: plain elements, and the table is the data.
    expect(table.querySelector("svg, canvas, img")).toBeNull();
    expect(screen.getByText("הפס שליד כל חודש מראה כמה התקבל בו, ביחס לחודש שבו התקבל הכי הרבה.")).toBeInTheDocument();
  });

  it("counts the new patients of the year and the visits held by kind", async () => {
    const served = await openYear();
    await screen.findByRole("table", { name: "סיכום 2026 לפי חודשים" });
    const income = await ask(served, { op: "income", year: 2026 }, "income");
    const kinds = within(screen.getByRole("region", { name: "פגישות שהתקיימו, לפי סוג" })).getAllByRole("listitem").map((k) => k.textContent);
    expect(kinds).toEqual(income.by_kind.map((k) => `${{ intake: "פגישת היכרות", assessment: "מפגש אבחון", feedback: "שיחת משוב", therapy: "פגישת טיפול" }[k.kind as "intake"]}${k.count}`));
    expect(kinds.length).toBeGreaterThan(2);
    expect(screen.getByRole("region", { name: "מטופלים חדשים בשנה" })).toHaveTextContent(String(income.new_patients));
    expect(income.new_patients).toBe(7);
  });

  it("moves between years; a year with nothing says so; the view is kept while the program is open", async () => {
    const { user, requests, view } = await openYear();
    await screen.findByRole("table", { name: "סיכום 2026 לפי חודשים" });
    await user.click(screen.getByRole("button", { name: "השנה הקודמת" }));
    expect(await screen.findByRole("heading", { name: "אין רישומים בשנת 2025" })).toBeInTheDocument();
    expect(requests().some((r) => r.op === "income" && r.year === 2025)).toBe(true);
    await user.click(screen.getByRole("button", { name: "לשנה הנוכחית" }));
    expect(await screen.findByRole("table", { name: "סיכום 2026 לפי חודשים" })).toBeInTheDocument();
    // The notice is above both views; the year has no export either.
    expect(screen.getAllByRole("note").some((n) => n.textContent === NOTICE)).toBe(true);
    expect(screen.queryByRole("button", { name: /ייצוא|קובץ|הדפסה/ })).toBeNull();
    view.unmount();
    await openOn(MONEY);
    expect(screen.getByRole("radio", { name: "סיכום שנתי" })).toHaveAttribute("aria-checked", "true");
  });

  it("shows the shape of the year while it loads, and a year that could not be read says so", async () => {
    const { over, release } = holding("income");
    const first = await openYear(over);
    expect(await screen.findByRole("status", { name: "טוען את סיכום השנה" })).toBeInTheDocument();
    release();
    expect(await screen.findByRole("table", { name: "סיכום 2026 לפי חודשים" })).toBeInTheDocument();
    first.view.unmount();
    forgetMoneyView();

    await openYear(refusing("income", "הסיכום לא נקרא מהכספת."));
    expect(await screen.findByRole("alert")).toHaveTextContent("הסיכום לא נקרא מהכספת.");
  });
});

describe("discreet mode", () => {
  it("shows initials in the diary, in the balances and in the history's heading", async () => {
    const prepare = async (served: Served) => {
      const maya = await idOf(served, "מאיה");
      await ask(served, { op: "save_payment", payment: { id: "", patient_id: maya, date: "2026-11-08", amount: 35_000, method: "transfer", receipt: "1060", payer: "סבתא רבקה", note: "", voided: false, void_reason: "" } }, "payment");
    };
    const { user } = await openOn(MONEY, undefined, prepare);
    const eighth = await day("יום ראשון, 08.11.2026");
    expect(rowOf(eighth, "מאיה", "תשלום")).toHaveTextContent("סבתא רבקה");
    await owing();
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const panel = screen.getByRole("tabpanel");
    for (const name of ["תמר", "איתי", "מאיה", "נועם", "יובל", "סבתא רבקה"]) expect(within(panel).queryByText(name), name).toBeNull();
    expect(panel).not.toHaveTextContent("רבקה");
    expect(within(eighth).getAllByRole("rowheader", { name: "מ." }).length).toBeGreaterThan(0);
    expect(eighth).toHaveTextContent("ס. ר.");
    expect(within(balances()).getByRole("group", { name: "פעולות עבור י." })).toBeInTheDocument();
  });

  it("leaves her notes off the diary's lines, and a line's history shows only what was done and when", async () => {
    const prepare = async (served: Served) => {
      const maya = await idOf(served, "מאיה");
      await ask(served, { op: "save_payment", payment: { id: "", patient_id: maya, date: "2026-11-08", amount: 35_000, method: "transfer", receipt: "1060", payer: "", note: "שילמו בשתי העברות", voided: false, void_reason: "" } }, "payment");
      const card = await ask(served, { op: "patient_card", id: maya, today: NOW }, "card");
      await ask(served, { op: "void_payment", id: card.payments.find((p) => p.date === "2026-11-08")?.id ?? "", reason: "נרשם בכרטיס הלא נכון" }, "payment");
    };
    const { user } = await openOn(MONEY, undefined, prepare);
    const eighth = await day("יום ראשון, 08.11.2026");
    expect(rowOf(eighth, "מאיה", "תשלום")).toHaveTextContent("שילמו בשתי העברות");
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const paid = rowOf(eighth, "מ.", "תשלום");
    expect(paid).not.toHaveTextContent("שילמו בשתי העברות");
    await user.click(within(paid).getByRole("button", { name: "שונה. היסטוריית השינויים של תשלום, מ." }));
    const dialog = await screen.findByRole("dialog", { name: "היסטוריית שינויים" });
    expect(await within(dialog).findByText("נוצר")).toBeInTheDocument();
    expect(dialog).toHaveTextContent("פרטי השינויים מוסתרים במצב דיסקרטי.");
    for (const text of ["מאיה", "נרשם בכרטיס הלא נכון", "שילמו בשתי העברות", "סיבת הביטול"]) expect(dialog, text).not.toHaveTextContent(text);
  });
});
