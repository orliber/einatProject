// The case screen over the demo core: deleting the sample case refreshes the lists, one sorting
// at a time (UIC-2), "approve all" asks once, and the keys say where they work.
// Every name and detail comes from the fake core's invented cases.
import { act, configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../App";
import { FakeCore } from "../web/fakeCore";
import { confirmLine } from "./CaseScreen";
import { rememberSendStopped } from "../sendStopped";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 60_000 });

type Hook = (cmd: string, args: unknown) => Promise<unknown> | undefined;

async function openApp(prep?: (core: FakeCore) => Promise<void>, hook?: Hook) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("set_review_always", { on: false });
  await prep?.(core);
  const calls: string[] = [];
  mockIPC(async (cmd, args) => {
    calls.push(cmd);
    const held = hook?.(cmd, args);
    if (held) await held;
    return core.handle(cmd, args);
  });
  const user = userEvent.setup();
  render(<App />);
  return { core, calls, user };
}

describe("the case screen", () => {
  it("the case's own bar says when sending to the AI is stopped, as the top bar does", async () => {
    const { user } = await openApp();
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    const bar = screen.getByRole("navigation", { name: "שלבי התיק" }).closest("header") as HTMLElement;
    expect(within(bar).queryByRole("button", { name: "השליחה ל-AI כבויה" })).toBeNull();
    act(() => rememberSendStopped(true));
    try {
      await user.click(within(bar).getByRole("button", { name: "השליחה ל-AI כבויה" }));
      expect(await screen.findByRole("heading", { name: /סיסמה ואבטחה/ })).toBeInTheDocument();
    } finally {
      act(() => rememberSendStopped(false));
    }
  });

  it("deleting the sample case takes it off the list at once, and the recycle bin has it", async () => {
    const { user } = await openApp(async (core) => {
      await core.handle("create_sample_case", {});
    });
    await user.click(await screen.findByRole("button", { name: /בתיק של תמר$/ }));
    await user.click(await screen.findByRole("button", { name: "למחוק את תיק הדוגמה" }));
    // Back on the list, well within the lists' 5 seconds of freshness: the sample is gone.
    await screen.findByRole("button", { name: /בתיק של נועם$/ });
    await waitFor(() => expect(screen.queryByRole("button", { name: /בתיק של תמר$/ })).toBeNull());
    expect(screen.queryByText("תמר")).toBeNull();
    await user.click(screen.getByRole("button", { name: /סל המחזור/ }));
    expect(await screen.findByText("תמר")).toBeInTheDocument();
  });

  it("'לכתיבת הדוח ←' while the materials are being read waits for that reading: nothing is sent twice (UIC-2)", async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((r) => { release = r; });
    let holding = false;
    const { user, calls } = await openApp(undefined, (cmd) => (cmd === "send_sort" && holding ? gate : undefined));
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    await user.click(within(screen.getByRole("navigation", { name: "שלבי התיק" })).getByRole("button", { name: /חומרים/ }));
    await screen.findByRole("region", { name: "רשימת החומרים" });
    holding = true;
    await user.click(await screen.findByRole("button", { name: "הוספה לאבחונים קודמים" }));
    await user.click(await screen.findByRole("menuitem", { name: "הדבקת טקסט" }));
    await user.type(screen.getByLabelText("כותרת"), "דוח בדוי");
    await user.type(screen.getByLabelText("הטקסט"), "נמצא עיכוב קל בשפה.");
    await user.click(screen.getByRole("button", { name: "שמירה בתיק" }));
    // The reading by itself has started, and its answers are held on their way.
    await waitFor(() => expect(calls).toContain("send_sort"));
    await user.click(screen.getByRole("button", { name: "לכתיבת הדוח ←" }));
    await user.click(screen.getByRole("button", { name: "לכתיבת הדוח ←" }));
    release();
    await screen.findByRole("article", { name: "הדוח, כמו בקובץ" }, { timeout: 10_000 });
    expect(calls.filter((c) => c === "prepare_sort_parts")).toHaveLength(1);
    // One request per material not read yet (the demo case's 4, and the one just added), once.
    await waitFor(() => expect(calls.filter((c) => c === "send_sort")).toHaveLength(5));
    expect(calls.filter((c) => c === "prepare_sort_parts")).toHaveLength(1);
  });

  it("'לאשר את כל הטיוטות' asks once, saying how many paragraphs; “ביטול” approves nothing", async () => {
    const { user, calls } = await openApp();
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    await user.click(screen.getByRole("button", { name: /עוד פעולות/ }));
    await user.click(await screen.findByRole("menuitem", { name: "כתיבת כל הדוח עם Claude" }));
    const contents = screen.getByRole("complementary", { name: "תוכן הדוח" });
    const all = await within(contents).findByRole("button", { name: /^✓ לאשר את כל הטיוטות \(\d+\)$/ }, { timeout: 30_000 });
    await user.click(all);
    const ask = within(contents).getByRole("group", { name: "לאשר את כל הטיוטות?" });
    expect(ask).toHaveTextContent(/פסקאות ב-\d+ סעיפים יאושרו וייכנסו לדוח\./);
    expect(calls).not.toContain("approve_clean_paragraphs");
    await user.click(within(ask).getByRole("button", { name: "ביטול" }));
    expect(calls).not.toContain("approve_clean_paragraphs");
    await user.click(within(contents).getByRole("button", { name: /^✓ לאשר את כל הטיוטות/ }));
    await user.click(within(within(contents).getByRole("group", { name: "לאשר את כל הטיוטות?" })).getByRole("button", { name: "כן, לאשר" }));
    await waitFor(() => expect(calls).toContain("approve_clean_paragraphs"));
  });

  it("the keys under the contents say where each works: the section's heading, or a paragraph", async () => {
    const { user } = await openApp();
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const contents = await screen.findByRole("complementary", { name: "תוכן הדוח" });
    const keys = contents.querySelector(".contents-keys") as HTMLElement;
    expect(keys.textContent).toBe("על כותרת הסעיף:Enter לאשר ולעבור הלאהCtrl+Enter לשנותעל פסקה:Enter לערוךCtrl+Enter לאשר");
  });
});

describe("what 'approve all' says before it approves", () => {
  it("counts paragraphs and sections, and what stays for her eyes", () => {
    expect(confirmLine(5, 3, 0)).toBe("5 פסקאות ב-3 סעיפים יאושרו וייכנסו לדוח.");
    expect(confirmLine(2, 2, 1)).toBe("2 פסקאות ב-2 סעיפים יאושרו וייכנסו לדוח. פסקה אחת עם הערה נשארת לבדיקה שלך.");
    expect(confirmLine(4, 2, 3)).toBe("4 פסקאות ב-2 סעיפים יאושרו וייכנסו לדוח. 3 פסקאות עם הערה נשארות לבדיקה שלך.");
  });
});
