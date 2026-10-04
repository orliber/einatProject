// Component tests of the main screens over the in-memory demo core (Q-4, UX-7).
// Every name and detail comes from the fake core's invented cases.
import { act, configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../App";
import { FakeCore } from "../web/fakeCore";
import { a11yViolations } from "../test/a11y";

// The whole app over the demo core takes a moment to settle in jsdom.
configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

async function openApp(unlock = true) {
  const core = new FakeCore();
  if (unlock) await core.handle("unlock", { password: "x" });
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    calls.push(cmd);
    return core.handle(cmd, args);
  });
  const user = userEvent.setup();
  const view = render(<App />);
  return { core, calls, user, view };
}

async function openCase(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
  return screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
}

describe("main screens", () => {
  it("the lock screen passes the accessibility check", async () => {
    const { view } = await openApp(false);
    expect(await screen.findByLabelText("סיסמה")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the cases screen passes the accessibility check", async () => {
    const { view } = await openApp();
    expect(await screen.findByRole("button", { name: /בתיק של נועם$/ })).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the report page passes the accessibility check", async () => {
    const { user, view } = await openApp();
    await openCase(user);
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("settings and consultation pass the accessibility check", async () => {
    const { user, view } = await openApp();
    await user.click(await screen.findByRole("button", { name: "הגדרות" }));
    await screen.findByRole("heading", { level: 1 });
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "התייעצות" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "התייעצות" })).toHaveAttribute("aria-current", "page"));
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the page is Hebrew and right-to-left", async () => {
    await openApp();
    await screen.findByRole("button", { name: /בתיק של נועם$/ });
    // index.html sets these for the real window; the test checks the screens do not fight them.
    expect(document.querySelector("[dir='ltr'] input, [dir='ltr'] textarea")).toBeNull();
  });
});

describe("writing the report", () => {
  it("a section is drafted only after the 'what leaves the computer' screen, then approved", async () => {
    const { user, calls } = await openApp();
    const page = await openCase(user);
    const write = within(page).getAllByRole("button", { name: "✦ לכתוב עם Claude" })[0];
    if (!write) throw new Error("no empty section to write");
    await user.click(write);
    const review = await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" });
    expect(calls).toContain("prepare_section");
    expect(calls).not.toContain("send_section");
    // Names are shown as they will leave: as roles, not as names.
    expect(within(review).queryAllByText("נועם").every((el) => el.closest("mark"))).toBe(true);
    await user.click(within(review).getByRole("button", { name: /שליחה/ }));
    await waitFor(() => expect(calls).toContain("send_section"));
    const approve = await screen.findByRole("button", { name: "✓ לאשר" }, { timeout: 6000 });
    await user.click(approve);
    await waitFor(() => expect(calls).toContain("approve_paragraph"));
    await waitFor(() => expect(screen.queryByRole("button", { name: "✓ לאשר" })).toBeNull());
  });

  it("an edited paragraph keeps its earlier wording, and it can be brought back", async () => {
    const { user } = await openApp();
    const page = await openCase(user);
    const original = within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/);
    await user.click(original);
    const box = await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ });
    await user.clear(box);
    await user.type(box, "נוסח חדש של ההפניה.");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    expect(await within(page).findByText("נוסח חדש של ההפניה.")).toBeInTheDocument();

    await user.click(within(page).getByRole("button", { name: "גרסאות קודמות של הפסקה" }));
    const versions = await screen.findByRole("dialog", { name: "גרסאות קודמות של הפסקה" });
    expect(within(versions).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/)).toBeInTheDocument();
    expect(await a11yViolations(versions)).toEqual([]);
    await user.click(within(versions).getByRole("button", { name: "להחזיר את הניסוח הזה" }));
    expect(await within(page).findByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/)).toBeInTheDocument();
    expect(within(page).queryByText("נוסח חדש של ההפניה.")).toBeNull();
  });

  it("the export screen lists what goes into the file", async () => {
    const { user, view } = await openApp();
    await openCase(user);
    await user.click(screen.getByRole("button", { name: /הוצאת הדוח/ }));
    expect((await screen.findAllByText(/סיסמה/)).length).toBeGreaterThan(0);
    expect(await a11yViolations(view.container)).toEqual([]);
  });
});

describe("keyboard", () => {
  it("F1 opens the shortcuts sheet, and Escape closes it", async () => {
    await openApp();
    await screen.findByRole("button", { name: /בתיק של נועם$/ });
    fireEvent.keyDown(window, { key: "F1", code: "F1" });
    const sheet = await screen.findByRole("dialog", { name: "קיצורי מקלדת" });
    expect(within(sheet).getByText("Ctrl+L")).toBeInTheDocument();
    expect(await a11yViolations(sheet)).toEqual([]);
    fireEvent.keyDown(window, { key: "Escape", code: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "קיצורי מקלדת" })).toBeNull());
  });

  it("Ctrl+L locks with the Hebrew layout on (the key types ך)", async () => {
    const { calls } = await openApp();
    await screen.findByRole("button", { name: /בתיק של נועם$/ });
    fireEvent.keyDown(window, { key: "ך", code: "KeyL", ctrlKey: true });
    await waitFor(() => expect(calls).toContain("lock"));
  });

  it("Alt+↓ moves to the next section, and focus goes to its heading", async () => {
    const { user } = await openApp();
    await openCase(user);
    const headings = screen.getAllByRole("heading", { level: 3 });
    act(() => {
      fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown", altKey: true });
    });
    await waitFor(() => expect(document.activeElement).toBe(headings[0]));
    act(() => {
      fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown", altKey: true });
    });
    await waitFor(() => expect(document.activeElement).toBe(headings[1]));
  });

  it("Ctrl+K on a paragraph opens 'change with AI' for it", async () => {
    const { user } = await openApp();
    const page = await openCase(user);
    const para = within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/);
    para.focus();
    fireEvent.keyDown(para, { key: "k", code: "KeyK", ctrlKey: true });
    expect(await screen.findByRole("dialog", { name: "מה לשנות בפסקה" })).toBeInTheDocument();
  });
});
