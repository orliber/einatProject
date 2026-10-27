// The activity log in the settings: what it lists, its filter, the earlier pages, marking it as
// gone over, and what she sees when the chain is broken or a call fails. Every entry is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ActivityEntry } from "../ipc/generated/ActivityEntry";
import type { ActivityPage } from "../ipc/generated/ActivityPage";
import { a11yViolations } from "../test/a11y";
import { appApi, Host, serve, type Override } from "../test/host";
import { ActivitySettings } from "./ActivityLog";

configure({ asyncUtilTimeout: 5000 });

const entry = (seq: number, over: Partial<ActivityEntry> = {}): ActivityEntry => ({
  seq, ts: 1_760_000_000 + seq * 60, event: "unlock", kind: "access", text: `כניסה ${seq}`, warn: false, case: null, ...over,
});

const page = (entries: ActivityEntry[], over: Partial<ActivityPage> = {}): ActivityPage => ({
  entries, more: false, last_seq: entries.at(-1)?.seq ?? null, intact: true, reviewed_at: null, ...over,
});

async function openLog(over?: Override) {
  const served = await serve(over);
  const app = appApi();
  const user = userEvent.setup();
  const view = render(<Host app={app}><ActivitySettings /></Host>);
  await user.click(screen.getByRole("button", { name: "פתיחת היומן" }));
  const dialog = await screen.findByRole("dialog", { name: "יומן פעולות" });
  return { ...served, app, user, view, dialog };
}

const listOf = (dialog: HTMLElement) => within(dialog).getByRole("list", { name: "פעולות, מהחדשה לישנה" });

describe("the activity log", () => {
  it("lists the actions newest first, with the case they were about, and passes the accessibility check", async () => {
    const { dialog, view } = await openLog();
    expect(await within(dialog).findByText("✓ השרשרת שלמה: אף רשומה לא נמחקה או שונתה.")).toBeInTheDocument();
    const rows = within(listOf(dialog)).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("כניסה");
    expect(within(listOf(dialog)).getAllByText("נועם · תיק-1024").length).toBeGreaterThan(0);
    expect(within(dialog).getByText("עוד לא סומן שהיומן נבדק.")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the filter is a group of radio buttons she can reach and change from the keyboard", async () => {
    const { dialog, user } = await openLog();
    await within(dialog).findByText(/השרשרת שלמה/);
    const filter = within(dialog).getByRole("radiogroup", { name: "סינון" });
    const all = within(filter).getByRole("radio", { name: "הכל" });
    expect(all).toBeChecked();
    // Tab reaches the checked radio; the arrow keys move along the group.
    all.focus();
    await user.keyboard("{ArrowRight}");
    const access = within(filter).getByRole("radio", { name: "כניסות" });
    await waitFor(() => expect(access).toBeChecked());
    const texts = within(listOf(dialog)).getAllByRole("listitem").map((li) => li.textContent ?? "");
    expect(texts.length).toBeGreaterThan(0);
    expect(texts.every((t) => /כניסה|נעילה/.test(t))).toBe(true);
    // Sending to the AI: only sends.
    await user.click(within(filter).getByRole("radio", { name: "שליחה ל-AI" }));
    const sends = within(listOf(dialog)).getAllByRole("listitem").map((li) => li.textContent ?? "");
    expect(sends.every((t) => t.includes("נשלח"))).toBe(true);
  });

  it("a kind with nothing in the loaded pages says so", async () => {
    const { dialog, user } = await openLog((cmd) => (cmd === "activity" ? page([entry(2), entry(1)]) : undefined));
    await within(dialog).findByText("כניסה 2");
    await user.click(within(dialog).getByRole("radio", { name: "תיקים" }));
    expect(within(listOf(dialog)).getByText("אין פעולות מהסוג הזה בעמודים שנטענו.")).toBeInTheDocument();
  });

  it("earlier actions load on request, from where the first page ended", async () => {
    const { dialog, user, calls } = await openLog((cmd, args) => {
      if (cmd !== "activity") return undefined;
      return args.before === null ? page([entry(5), entry(4)], { more: true }) : page([entry(3)]);
    });
    await within(dialog).findByText("כניסה 5");
    await user.click(within(dialog).getByRole("button", { name: "פעולות קודמות" }));
    expect(await within(dialog).findByText("כניסה 3")).toBeInTheDocument();
    expect(calls.filter((c) => c.cmd === "activity").map((c) => c.args.before)).toEqual([null, 4]);
    expect(within(dialog).queryByRole("button", { name: "פעולות קודמות" })).toBeNull();
  });

  it("“עברתי על היומן” is written to the log, and the date it was gone over shows", async () => {
    const { dialog, user, app, calls } = await openLog();
    await within(dialog).findByText(/השרשרת שלמה/);
    await user.click(within(dialog).getByRole("button", { name: "עברתי על היומן" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("נרשם ביומן שעברת עליו."));
    expect(calls.some((c) => c.cmd === "mark_activity_reviewed")).toBe(true);
    expect(await within(dialog).findByText(/^נבדק לאחרונה: /)).toBeInTheDocument();
    expect(within(listOf(dialog)).getByText("היומן נבדק")).toBeInTheDocument();
  });

  it("a broken chain is said at the top and as an error to report", async () => {
    const { dialog } = await openLog((cmd) => (cmd === "activity" ? page([entry(1)], { intact: false }) : undefined));
    expect(await within(dialog).findByText("בדיקת השרשרת מצאה חריגה: ייתכן שרשומה נמחקה או שונתה.")).toBeInTheDocument();
    expect(within(dialog).getByRole("alert")).toHaveTextContent("בדיקת השלמות של היומן מצאה חריגה. כדאי לפנות לאור לפני שממשיכים לעבוד.");
  });

  it("a log that cannot be read says why, in Hebrew", async () => {
    const { dialog } = await openLog((cmd) => {
      if (cmd === "activity") throw { code: "internal", message: "לא הצלחנו לקרוא את היומן.", details: [] };
      return undefined;
    });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("לא הצלחנו לקרוא את היומן.");
  });

  it("a failed “עברתי על היומן” says so and is not announced as done", async () => {
    const { dialog, user, app } = await openLog((cmd) => {
      if (cmd === "mark_activity_reviewed") throw { code: "internal", message: "הרישום ביומן לא הצליח.", details: [] };
      return undefined;
    });
    await within(dialog).findByText(/השרשרת שלמה/);
    await user.click(within(dialog).getByRole("button", { name: "עברתי על היומן" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("הרישום ביומן לא הצליח.");
    expect(app.notify).not.toHaveBeenCalled();
  });

  it("Escape closes the log and gives the settings back", async () => {
    const { dialog, user } = await openLog();
    await within(dialog).findByText(/השרשרת שלמה/);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "פתיחת היומן" })).toBeInTheDocument();
  });
});
