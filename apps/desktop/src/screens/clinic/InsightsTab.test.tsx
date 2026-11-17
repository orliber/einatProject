// "תמונת מצב" (D-200) over the demo clinic, with the core's answer to "insights" given by the
// test: the four tiles and their change, the ready stretches and the months they ask for, the
// months as charts and as a table, medians with nothing to count, debts by age, the last
// picture kept while another is read, discreet mode, and a read that failed. Every figure is
// invented; the clock is held at a Sunday morning (15.11.2026).
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import type { IncomeMonth } from "../../ipc/generated/IncomeMonth";
import type { Insights } from "../../ipc/generated/Insights";
import { a11yViolations } from "../../test/a11y";
import type { Override } from "../../test/host";
import { forgetDiscreet } from "./ClinicScreen";
import { forgetInsightsPeriod, niceTop, percentText, periodName, presetRange, withFrom, withTo } from "./InsightsTab";
import { openOn } from "./officeTestKit";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  forgetInsightsPeriod();
  window.history.replaceState({}, "", "/");
});

const INSIGHTS = { name: "clinic", tab: "insights" } as const;

/** "2026-09" … "2026-11", both included. */
function monthsOf(from: string, to: string): string[] {
  const at = (m: string) => Number(m.slice(0, 4)) * 12 + Number(m.slice(5, 7)) - 1;
  return Array.from({ length: at(to) - at(from) + 1 }, (_, i) => {
    const n = at(from) + i;
    return `${Math.floor(n / 12)}-${String((n % 12) + 1).padStart(2, "0")}`;
  });
}

/** The core's picture of a stretch: the same invented clinic for any months asked. */
function picture(from: string, to: string, over: Partial<Insights> = {}): Insights {
  const months: IncomeMonth[] = monthsOf(from, to).map((month, i) => ({
    month, held: 12 + i * 3, cancelled: 2, no_show: 1, charged: 700_000, received: 600_000 + i * 25_000,
  }));
  return {
    from_month: from, to_month: to,
    held: 42, cancelled: 5, no_show: 3, cancel_rate: 100, no_show_rate: 60,
    by_weekday: [
      { weekday: 0, held: 20, cancelled: 2, no_show: 0 }, { weekday: 1, held: 10, cancelled: 1, no_show: 1 },
      { weekday: 2, held: 0, cancelled: 0, no_show: 0 }, { weekday: 3, held: 0, cancelled: 0, no_show: 0 },
      { weekday: 4, held: 12, cancelled: 2, no_show: 2 }, { weekday: 5, held: 0, cancelled: 0, no_show: 0 },
      { weekday: 6, held: 0, cancelled: 0, no_show: 0 },
    ],
    by_kind: [{ kind: "assessment", count: 20 }, { kind: "intake", count: 8 }, { kind: "therapy", count: 8 }, { kind: "feedback", count: 6 }],
    held_minutes: 2490, work_minutes: 3960, occupancy: 629,
    new_patients: 7,
    referrals: [{ source: "גננת", count: 3 }, { source: "רופאת ילדים", count: 2 }, { source: "לא צוין", count: 2 }],
    assessments_done: 6, intake_to_feedback_days: 45, assessment_to_feedback_days: 12, waited_days: null,
    waiting_now: 4, waiting_now_days: 38,
    charged: 2_100_000, received: 1_850_000,
    by_method: [{ method: "cash", amount: 250_000 }, { method: "transfer", amount: 1_200_000 }, { method: "bit", amount: 400_000 }],
    debt: [
      { label: "עד 30 יום", from_days: 0, patients: 2, amount: 90_000 },
      { label: "31–60 יום", from_days: 31, patients: 1, amount: 45_000 },
      { label: "61–90 יום", from_days: 61, patients: 0, amount: 0 },
      { label: "מעל 90 יום", from_days: 91, patients: 1, amount: 60_000 },
    ],
    months,
    before: { from_month: "2026-06", to_month: "2026-08", held: 30, received: 2_000_000, new_patients: 7 },
    ...over,
  };
}

/** The core's answer to "insights", by the months asked; `hold` keeps an answer back until it resolves. */
function answering(make: (from: string, to: string) => Insights = picture, hold?: () => Promise<void> | null): Override {
  return async (cmd, args) => {
    const r = args.request as ClinicRequest | undefined;
    if (cmd !== "clinic" || r?.op !== "insights") return undefined;
    const wait = hold?.();
    if (wait) await wait;
    return { kind: "insights", data: make(r.from_month, r.to_month) };
  };
}

const tiles = () => screen.findByRole("list", { name: "התקופה במספרים" });
const tile = (all: HTMLElement, label: string) => within(all).getAllByRole("listitem").find((li) => li.firstChild?.textContent === label) as HTMLElement;
const asked = (requests: () => ClinicRequest[]) =>
  requests().flatMap((r) => (r.op === "insights" ? [[r.from_month, r.to_month]] : []));

describe("the pure pieces", () => {
  it("the ready stretches end with this month", () => {
    expect(presetRange("month", "2026-11")).toEqual({ from: "2026-11", to: "2026-11" });
    expect(presetRange("quarter", "2026-11")).toEqual({ from: "2026-09", to: "2026-11" });
    expect(presetRange("quarter", "2026-01")).toEqual({ from: "2025-11", to: "2026-01" });
    expect(presetRange("year", "2026-11")).toEqual({ from: "2026-01", to: "2026-11" });
    expect(presetRange("twelve", "2026-11")).toEqual({ from: "2025-12", to: "2026-11" });
    // Never before the clinic's first year.
    expect(presetRange("twelve", "2000-03")).toEqual({ from: "2000-01", to: "2000-03" });
  });

  it("picking one end moves the other only as far as it must", () => {
    expect(withFrom({ from: "2026-01", to: "2026-11" }, "2026-03")).toEqual({ from: "2026-03", to: "2026-11" });
    expect(withFrom({ from: "2026-01", to: "2026-03" }, "2026-05")).toEqual({ from: "2026-05", to: "2026-05" });
    expect(withFrom({ from: "2026-01", to: "2026-11" }, "2023-01")).toEqual({ from: "2023-01", to: "2024-12" });
    expect(withTo({ from: "2026-03", to: "2026-11" }, "2026-01")).toEqual({ from: "2026-01", to: "2026-01" });
    expect(withTo({ from: "2023-01", to: "2023-02" }, "2026-11")).toEqual({ from: "2024-12", to: "2026-11" });
  });

  it("names a stretch, a share and an axis's top in her words", () => {
    expect(periodName("2026-11", "2026-11")).toBe("נובמבר 2026");
    expect(periodName("2026-09", "2026-11")).toBe("ספטמבר–נובמבר 2026");
    expect(periodName("2025-12", "2026-11")).toBe("דצמבר 2025–נובמבר 2026");
    expect(percentText(83)).toBe("8.3%");
    expect(percentText(250)).toBe("25%");
    expect(percentText(0)).toBe("0%");
    expect([niceTop(0), niceTop(1), niceTop(7), niceTop(34), niceTop(1_234_500)]).toEqual([0, 1, 8, 40, 1_500_000]);
  });
});

describe("the picture", () => {
  it("shows the four tiles with the change from the stretch before, and passes the accessibility check", async () => {
    const { view, requests } = await openOn(INSIGHTS, answering());
    expect(screen.getByRole("tab", { name: "תמונת מצב" })).toHaveAttribute("aria-selected", "true");
    const all = await tiles();
    expect(asked(requests)).toEqual([["2026-09", "2026-11"]]);
    expect(screen.getByRole("heading", { name: "תמונת מצב" })).toBeInTheDocument();
    expect(screen.getByText("ספטמבר–נובמבר 2026")).toBeInTheDocument();

    const held = tile(all, "פגישות שהתקיימו");
    expect(held).toHaveTextContent("42");
    expect(held).toHaveTextContent("עלייה של 12 לעומת יוני–אוגוסט 2026");
    const received = tile(all, "התקבל");
    expect(received).toHaveTextContent("18,500 ₪");
    expect(received).toHaveTextContent("ירידה של 1,500 ₪ לעומת יוני–אוגוסט 2026");
    expect(tile(all, "מטופלים חדשים")).toHaveTextContent("ללא שינוי לעומת יוני–אוגוסט 2026");
    const occupancy = tile(all, "תפוסה");
    expect(occupancy).toHaveTextContent("62.9%");
    expect(occupancy).toHaveTextContent("41.5 שעות פגישה מתוך 66 שעות עבודה");
    // November is not over yet.
    expect(screen.getByText("נובמבר 2026 עוד לא הסתיים, ולכן ההשוואה לתקופה שלפני עוד לא שלמה.")).toBeInTheDocument();

    // Cancellations: the two rates, and the weekdays that had visits.
    const missed = screen.getByRole("region", { name: "ביטולים ואי-הגעות" });
    expect(missed).toHaveTextContent("מתוך 50 פגישות");
    expect(missed).toHaveTextContent("בוטלו10%");
    expect(missed).toHaveTextContent("לא הגיעו6%");
    const days = within(missed).getByRole("list", { name: "לפי יום בשבוע" });
    expect(within(days).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "ראשון20 התקיימו · 2 בוטלו או לא הגיעו (9.1%)",
      "שני10 התקיימו · 2 בוטלו או לא הגיעו (16.7%)",
      "חמישי12 התקיימו · 4 בוטלו או לא הגיעו (25%)",
    ]);

    // Where they come from, by kind and by method: rows of a name and a figure, the most first.
    const sources = screen.getByRole("list", { name: "מקורות ההפניה" });
    expect(within(sources).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["גננת3 (42.9%)", "רופאת ילדים2 (28.6%)", "לא צוין2 (28.6%)"]);
    expect(within(screen.getByRole("list", { name: "פגישות שהתקיימו לפי סוג" })).getAllByRole("listitem").map((li) => li.textContent))
      .toEqual(["מפגש אבחון20", "פגישת היכרות8", "פגישת טיפול8", "שיחת משוב6"]);
    expect(within(screen.getByRole("list", { name: "התקבל לפי אמצעי תשלום" })).getAllByRole("listitem").map((li) => li.textContent))
      .toEqual(["העברה בנקאית12,000 ₪", "ביט4,000 ₪", "מזומן2,500 ₪"]);

    expect(screen.getByText("מספרים בלבד, בלי שמות. הכל מחושב במחשב הזה ולא נשלח לשום מקום.")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("asks for the months of each ready stretch, and of the months she picks", async () => {
    const { user, requests } = await openOn(INSIGHTS, answering());
    await tiles();
    expect(screen.getByRole("radio", { name: "3 החודשים האחרונים" })).toHaveAttribute("aria-checked", "true");
    const last = () => asked(requests).at(-1);

    await user.click(screen.getByRole("radio", { name: "החודש" }));
    await waitFor(() => expect(last()).toEqual(["2026-11", "2026-11"]));
    expect(screen.getByRole("radio", { name: "החודש" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: "מתחילת השנה" }));
    await waitFor(() => expect(last()).toEqual(["2026-01", "2026-11"]));
    await user.click(screen.getByRole("radio", { name: "12 החודשים האחרונים" }));
    await waitFor(() => expect(last()).toEqual(["2025-12", "2026-11"]));
    expect(screen.getByLabelText("מחודש")).toHaveValue("2025-12");
    expect(screen.getByLabelText("עד חודש")).toHaveValue("2026-11");

    // From a month to a month: no ready stretch is chosen any more.
    await user.selectOptions(screen.getByLabelText("מחודש"), "2026-03");
    await waitFor(() => expect(last()).toEqual(["2026-03", "2026-11"]));
    expect(screen.getAllByRole("radio").filter((r) => r.getAttribute("aria-checked") === "true")).toEqual([]);
    expect(screen.getByText("מרץ–נובמבר 2026")).toBeInTheDocument();
    // An end before the start brings the start with it.
    await user.selectOptions(screen.getByLabelText("עד חודש"), "2026-01");
    await waitFor(() => expect(last()).toEqual(["2026-01", "2026-01"]));
    // Never more than 24 months.
    await user.selectOptions(screen.getByLabelText("מחודש"), "2023-01");
    await waitFor(() => expect(last()).toEqual(["2023-01", "2024-12"]));
    expect(screen.getByText("ינואר 2023–דצמבר 2024")).toBeInTheDocument();
  });

  it("keeps the stretch she chose after a look at another tab", async () => {
    const { user, requests } = await openOn(INSIGHTS, answering());
    await tiles();
    await user.click(screen.getByRole("radio", { name: "מתחילת השנה" }));
    await waitFor(() => expect(asked(requests).at(-1)).toEqual(["2026-01", "2026-11"]));
    await user.click(screen.getByRole("tab", { name: "כספים" }));
    await user.click(screen.getByRole("tab", { name: "תמונת מצב" }));
    expect(await screen.findByRole("radio", { name: "מתחילת השנה" })).toHaveAttribute("aria-checked", "true");
  });

  it("draws the months as two charts, and shows them as a table on request", async () => {
    const { user } = await openOn(INSIGHTS, answering());
    await tiles();
    const months = screen.getByRole("region", { name: "לפי חודשים" });
    // One measure a chart, its title naming it; the figures are in the table for a screen reader.
    expect([...months.querySelectorAll("figcaption")].map((c) => c.textContent)).toEqual(["פגישות שהתקיימו", "התקבל"]);
    const table = within(months).getByRole("table", { name: "ספטמבר–נובמבר 2026 לפי חודשים" });
    expect(table.closest(".visually-hidden")).not.toBeNull();
    expect(within(table).getAllByRole("rowheader").map((h) => h.textContent)).toEqual(["ספטמבר 2026", "אוקטובר 2026", "נובמבר 2026", "סך התקופה"]);
    const october = within(table).getByRole("rowheader", { name: "אוקטובר 2026" }).closest("tr") as HTMLElement;
    expect(within(october).getAllByRole("cell").map((c) => c.textContent)).toEqual(["15", "2", "1", "7,000 ₪", "6,250 ₪"]);

    await user.click(within(months).getByRole("button", { name: "הצגה כטבלה" }));
    expect(within(months).getByRole("button", { name: "הצגה כטבלה" })).toHaveAttribute("aria-pressed", "true");
    expect(months.querySelector("figure")).toBeNull();
    expect(within(months).getByRole("table").closest(".visually-hidden")).toBeNull();
  });

  it("says plainly when a median has nothing to count yet", async () => {
    await openOn(INSIGHTS, answering((from, to) => picture(from, to, {
      assessments_done: 0, intake_to_feedback_days: null, assessment_to_feedback_days: null, waited_days: null, waiting_now: 0, waiting_now_days: null,
    })));
    await tiles();
    const pathway = screen.getByRole("region", { name: "מסלול האבחון" });
    expect(within(pathway).getAllByText("אין עדיין נתונים")).toHaveLength(3);
    expect(pathway).toHaveTextContent("אין משפחות שממתינות");
  });

  it("gives days in her words", async () => {
    await openOn(INSIGHTS, answering((from, to) => picture(from, to, { intake_to_feedback_days: 45, assessment_to_feedback_days: 1, waited_days: 2 })));
    await tiles();
    const pathway = screen.getByRole("region", { name: "מסלול האבחון" });
    const fact = (label: string) => within(pathway).getByText(label).closest("div") as HTMLElement;
    expect(fact("מפגישת ההיכרות עד שיחת המשוב")).toHaveTextContent("45 ימים (כ-6 שבועות)");
    expect(fact("מהמפגש האחרון של האבחון עד שיחת המשוב")).toHaveTextContent("יום אחד");
    expect(fact("המתנה עד הפגישה הראשונה")).toHaveTextContent("יומיים");
    expect(fact("ברשימת ההמתנה היום")).toHaveTextContent("4 משפחות · ממתינות 38 ימים (כ-5 שבועות)");
    expect(fact("אבחונים שהסתיימו")).toHaveTextContent("6");
  });

  it("lists the debts in four rows of age, the oldest marked when it holds a debt", async () => {
    await openOn(INSIGHTS, answering());
    await tiles();
    const debts = screen.getByRole("region", { name: "חובות לפי ותק" });
    const rows = within(within(debts).getByRole("list", { name: "חובות לפי ותק" })).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent)).toEqual(["עד 30 יום2 משפחות · 900 ₪", "31–60 יוםמשפחה אחת · 450 ₪", "61–90 יוםאין", "מעל 90 יוםמשפחה אחת · 600 ₪"]);
    expect(rows.map((r) => r.classList.contains("insights-rank-warn"))).toEqual([false, false, false, true]);
    expect(debts).toHaveTextContent("סך הכל: 4 משפחות, 1,950 ₪");
    expect(debts).toHaveTextContent("נכון להיום");
  });

  it("marks no row when the oldest bucket is empty, and says when nothing is owed", async () => {
    const quietOld = (from: string, to: string) => picture(from, to, {
      debt: picture(from, to).debt.map((d) => (d.from_days === 91 ? { ...d, patients: 0, amount: 0 } : d)),
    });
    const first = await openOn(INSIGHTS, answering(quietOld));
    await tiles();
    expect(screen.getByRole("region", { name: "חובות לפי ותק" }).querySelector(".insights-rank-warn")).toBeNull();
    first.view.unmount();

    const paid = (from: string, to: string) => picture(from, to, { debt: picture(from, to).debt.map((d) => ({ ...d, patients: 0, amount: 0 })) });
    await openOn(INSIGHTS, answering(paid));
    await tiles();
    expect(screen.getByRole("region", { name: "חובות לפי ותק" })).toHaveTextContent("אין חובות פתוחים היום. כל מה שחויב שולם.");
  });

  it("says when there is nothing yet to show, with the stretch still to pick", async () => {
    const empty = (from: string, to: string) => picture(from, to, {
      held: 0, cancelled: 0, no_show: 0, cancel_rate: 0, no_show_rate: 0, charged: 0, received: 0, new_patients: 0, referrals: [], waiting_now: 0,
      waiting_now_days: null, debt: picture(from, to).debt.map((d) => ({ ...d, patients: 0, amount: 0 })),
      before: { from_month: "2026-06", to_month: "2026-08", held: 0, received: 0, new_patients: 0 },
    });
    await openOn(INSIGHTS, answering(empty));
    expect(await screen.findByRole("heading", { name: "אין עדיין נתונים לתקופה הזו" })).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "התקופה במספרים" })).toBeNull();
    expect(screen.getByRole("radio", { name: "החודש" })).toBeInTheDocument();
  });

  it("hides where families were referred from in discreet mode", async () => {
    const { user } = await openOn(INSIGHTS, answering());
    await tiles();
    expect(screen.getByRole("list", { name: "מקורות ההפניה" })).toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const referrals = screen.getByRole("region", { name: "מאיפה מגיעים" });
    expect(referrals).toHaveTextContent("מקורות ההפניה מוסתרים במצב דיסקרטי.");
    expect(referrals).not.toHaveTextContent("גננת");
    expect(referrals).toHaveTextContent("7 מטופלים חדשים");
  });
});

describe("reading", () => {
  it("shows a quiet frame while the first picture is read, and keeps the last one while another is", async () => {
    let gate: { wait: Promise<void>; open: () => void } | null = null;
    const hold = () => {
      let open: () => void = () => undefined;
      const wait = new Promise<void>((r) => (open = r));
      gate = { wait, open };
    };
    hold();
    const { user } = await openOn(INSIGHTS, answering(picture, () => gate?.wait ?? null));
    expect(await screen.findByRole("status", { name: "טוען את תמונת המצב" })).toBeInTheDocument();
    (gate as { open: () => void } | null)?.open();
    await tiles();

    hold();
    await user.click(screen.getByRole("radio", { name: "החודש" }));
    // The last picture stays, quieter and marked busy, until the new one comes.
    const body = (await tiles()).closest(".insights-body") as HTMLElement;
    await waitFor(() => expect(body).toHaveAttribute("aria-busy", "true"));
    expect(screen.queryByRole("status", { name: "טוען את תמונת המצב" })).toBeNull();
    (gate as { open: () => void } | null)?.open();
    await waitFor(() => expect(body).not.toHaveAttribute("aria-busy"));
    expect(within(screen.getByRole("region", { name: "לפי חודשים" })).getByRole("table", { name: "נובמבר 2026 לפי חודשים" })).toBeInTheDocument();
  });

  it("says when the picture could not be read, and reads it again", async () => {
    let fail = true;
    const over: Override = async (cmd, args, core) => {
      const r = args.request as ClinicRequest | undefined;
      if (cmd === "clinic" && r?.op === "insights" && fail) {
        fail = false;
        throw { code: "refused", message: "תמונת המצב לא נקראה מהכספת.", details: [], action: { kind: "dismiss" } };
      }
      return answering()(cmd, args, core);
    };
    const { user } = await openOn(INSIGHTS, over);
    expect((await screen.findAllByRole("alert")).some((a) => a.textContent?.includes("תמונת המצב לא נקראה מהכספת."))).toBe(true);
    await user.click(screen.getByRole("button", { name: "לנסות שוב" }));
    expect(await tiles()).toBeInTheDocument();
    expect(screen.queryByText("תמונת המצב לא נקראה מהכספת.")).toBeNull();
  });
});
