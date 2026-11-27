// A payment recorded from a visit's row on the front page: mark "התקיימה", then "לרשום תקבול"
// under "⋯", with the visit's fee already in the box. Over the demo core; names are invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import { a11yViolations } from "../../test/a11y";
import { appApi, Host, serve, type Override } from "../../test/host";
import { ClinicScreen, forgetDiscreet } from "./ClinicScreen";
import type { Payment } from "../../ipc/generated/Payment";
import { lastMethod } from "./PaymentDialog";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  // The afternoon: the morning's visits are over.
  vi.setSystemTime(new Date(2026, 10, 15, 15, 12));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
});

const withClipboard: Override = (cmd) => (cmd === "copy_secret" ? 60 : undefined);

async function openToday(over: Override = withClipboard) {
  const served = await serve(over);
  const user = userEvent.setup();
  const notes: string[] = [];
  render(<Host app={appApi({ route: { name: "clinic" }, notify: (text: string) => void notes.push(text) })}><ClinicScreen /></Host>);
  const today = await screen.findByRole("region", { name: "הפגישות של היום" });
  const requests = () => served.calls.filter((c) => c.cmd === "clinic").map((c) => c.args.request as ClinicRequest);
  return { ...served, user, notes, today, requests };
}

describe("“לרשום תקבול” from a visit's row", () => {
  it("is offered once the visit was held, opens with the visit's fee, and records the payment on the family's card", async () => {
    const { user, today, requests, notes } = await openToday();
    const row = within(today).getAllByRole("listitem").find((li) => within(li).queryByRole("button", { name: "התקיימה" })) as HTMLElement;
    expect(row).toBeDefined();
    const more = within(row).getByRole("button", { name: /^עוד פעולות לפגישה/ });
    // Still planned: nothing to receive for it yet.
    await user.click(more);
    expect(within(await screen.findByRole("menu")).queryByRole("menuitem", { name: "לרשום תקבול" })).toBeNull();
    await user.keyboard("{Escape}");

    await user.click(within(row).getByRole("button", { name: "התקיימה" }));
    await within(row).findByRole("button", { name: "לכתוב סיכום" });
    const held = requests().find((r) => r.op === "save_visit");
    if (held?.op !== "save_visit") throw new Error("the visit was not saved");
    expect(held.visit.fee).toBeGreaterThan(0);

    await waitFor(() => expect(requests().filter((r) => r.op === "overview").length).toBeGreaterThan(1));
    await user.click(within(row).getByRole("button", { name: /^עוד פעולות לפגישה/ }));
    await user.click(await screen.findByRole("menuitem", { name: "לרשום תקבול" }));
    const dialog = await screen.findByRole("dialog", { name: "תקבול חדש" });
    const amount = within(dialog).getByLabelText("סכום");
    expect(amount).toHaveValue(String(held.visit.fee / 100));
    // Away from the card, the dialog says whose payment it is.
    expect(dialog).toHaveTextContent(/תקבול מהמשפחה של \S+, על .+ מיום 15\.11\.2026\./);
    expect(await a11yViolations(dialog)).toEqual([]);

    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "תקבול חדש" })).toBeNull());
    const paid = requests().find((r) => r.op === "save_payment");
    if (paid?.op !== "save_payment") throw new Error("no payment was saved");
    expect(paid.payment).toMatchObject({ patient_id: held.visit.patient_id, amount: held.visit.fee, date: "2026-11-15", voided: false });
    expect(notes).toContain("התקבול נרשם");
  });

  it("once the family has paid, the box says so, and a sum above the balance asks before it is recorded", async () => {
    const { user, today, requests, notes } = await openToday();
    const row = within(today).getAllByRole("listitem").find((li) => within(li).queryByRole("button", { name: "התקיימה" })) as HTMLElement;
    await user.click(within(row).getByRole("button", { name: "התקיימה" }));
    await within(row).findByRole("button", { name: "לכתוב סיכום" });
    await waitFor(() => expect(requests().filter((r) => r.op === "overview").length).toBeGreaterThan(1));
    const openPayment = async () => {
      await user.click(within(row).getByRole("button", { name: /^עוד פעולות לפגישה/ }));
      await user.click(await screen.findByRole("menuitem", { name: "לרשום תקבול" }));
      return screen.findByRole("dialog", { name: "תקבול חדש" });
    };
    // Pay what the family owes, whatever it is.
    let dialog = await openPayment();
    const card = requests().find((r) => r.op === "patient_card");
    expect(card).toBeDefined();
    const amount = within(dialog).getByLabelText("סכום");
    const hint = within(dialog).queryByText(/^היתרה של המשפחה: /)?.textContent;
    if (hint) {
      await user.clear(amount);
      await user.type(amount, hint.replace(/[^\d.]/g, "").replace(/\.$/, ""));
    }
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "תקבול חדש" })).toBeNull());
    expect(notes).toContain("התקבול נרשם");
    const saved = () => requests().filter((r) => r.op === "save_payment").length;
    expect(saved()).toBe(1);

    // Again, with a zero too many: the box said nothing is owed, and she is asked first.
    dialog = await openPayment();
    expect(dialog).toHaveTextContent("למשפחה אין יתרה פתוחה: הכול כבר שולם.");
    await user.clear(within(dialog).getByLabelText("סכום"));
    await user.type(within(dialog).getByLabelText("סכום"), "6000");
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    const ask = await screen.findByRole("dialog", { name: "הסכום גבוה מהיתרה" });
    expect(ask).toHaveTextContent("למשפחה אין יתרה פתוחה. אחרי הרישום תהיה למשפחה זכות של");
    await user.click(within(ask).getByRole("button", { name: "ביטול" }));
    expect(saved()).toBe(1);
    expect(within(dialog).getByLabelText("סכום")).toHaveValue("6000");

    // Recorded after all: the family is now ahead, and a further sum adds to what it has.
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    await user.click(within(await screen.findByRole("dialog", { name: "הסכום גבוה מהיתרה" })).getByRole("button", { name: "לרשום 6,000 ₪" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "תקבול חדש" })).toBeNull());
    expect(saved()).toBe(2);
    dialog = await openPayment();
    expect(await within(dialog).findByText("למשפחה כבר יש זכות של 6,000 ₪.")).toBeInTheDocument();
    await user.clear(within(dialog).getByLabelText("סכום"));
    await user.type(within(dialog).getByLabelText("סכום"), "100");
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    expect(await screen.findByRole("dialog", { name: "הסכום גבוה מהיתרה" }))
      .toHaveTextContent("למשפחה כבר יש זכות של 6,000 ₪. אחרי הרישום תהיה למשפחה זכות של 6,100 ₪.");
  });

  it("a refusal of the vault stays in the dialog, and nothing is claimed", async () => {
    const refuse: Override = (cmd, args) => {
      if (cmd === "copy_secret") return 60;
      if (cmd === "clinic" && (args.request as ClinicRequest).op === "save_payment") throw { code: "refused", message: "התקבול לא נרשם: בדיקה.", details: [], action: { kind: "dismiss" } };
      return undefined;
    };
    const { user, today, notes } = await openToday(refuse);
    const row = within(today).getAllByRole("listitem").find((li) => within(li).queryByRole("button", { name: "התקיימה" })) as HTMLElement;
    await user.click(within(row).getByRole("button", { name: "התקיימה" }));
    await within(row).findByRole("button", { name: "לכתוב סיכום" });
    await user.click(within(row).getByRole("button", { name: /^עוד פעולות לפגישה/ }));
    await user.click(await screen.findByRole("menuitem", { name: "לרשום תקבול" }));
    const dialog = await screen.findByRole("dialog", { name: "תקבול חדש" });
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    expect(await within(dialog).findByText("התקבול לא נרשם: בדיקה.")).toBeInTheDocument();
    expect(notes).not.toContain("התקבול נרשם");
  });
});

describe("the method the payment box opens on", () => {
  const pay = (date: string, method: Payment["method"], voided = false): Payment =>
    ({ id: date, patient_id: "p", date, amount: 100, method, receipt: "", payer: "", note: "", voided, void_reason: "" });

  it("is how the family paid last time, and a bank transfer when nothing was paid yet", () => {
    expect(lastMethod([])).toBe("transfer");
    expect(lastMethod([pay("2026-10-01", "cash"), pay("2026-11-02", "bit"), pay("2026-09-01", "transfer")])).toBe("bit");
  });

  it("does not learn from a payment that was voided", () => {
    expect(lastMethod([pay("2026-10-01", "cash"), pay("2026-11-02", "bit", true)])).toBe("cash");
  });
});
