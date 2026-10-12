// Shared pieces: filter text is shown as text, errors offer the next step, dialogs trap the
// keyboard's attention. All names are invented.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AppContext, type AppApi } from "../App";
import { rememberFailure } from "../errors";
import { status } from "../test/fixtures";
import { a11yViolations } from "../test/a11y";
import { mockIPC } from "@tauri-apps/api/mocks";
import { Dialog, ErrorLine, Segments, Skeleton, displayTag } from "./ui";

const PAYLOAD = '<img src=x onerror="window.__xss=1"><script>window.__xss=2</script>';

function api(over: Partial<AppApi> = {}): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined), ...over };
}

afterEach(() => {
  delete (window as unknown as { __xss?: number }).__xss;
});

describe("Segments", () => {
  it("shows markup from a document or the model as characters, never as elements", () => {
    const { container } = render(
      <p>
        <Segments side="original" segments={[
          { text: PAYLOAD, mark: null, label: null },
          { text: "<b>נועה</b>", mark: "replaced", label: "שם הילד/ה" },
          { text: '<a href="javascript:alert(1)">x</a>', mark: "suspect", label: "שם לא מוכר" },
        ]} />
      </p>,
    );
    expect(container.querySelector("img, script, b, a")).toBeNull();
    expect(container.textContent).toContain(PAYLOAD);
    expect(container.textContent).toContain("<b>נועה</b>");
    expect((window as unknown as { __xss?: number }).__xss).toBeUndefined();
  });

  it("the case side marks what will be hidden; the outgoing side shows the role, not the name", () => {
    const segments = [
      { text: "נועה", mark: "replaced" as const, label: "שם הילד/ה" },
      { text: " ציירה עם ", mark: null, label: null },
      { text: "תמר", mark: "suspect" as const, label: "שם לא מוכר" },
    ];
    const original = render(<p data-testid="o"><Segments side="original" segments={segments} /></p>);
    const mark = original.container.querySelector("mark.mark-real");
    expect(mark).toHaveTextContent("נועה");
    expect(mark).toHaveAttribute("title", "יוסתר: שם הילד/ה");
    // A suspect is flagged, not hidden, on both sides.
    expect(original.container.querySelector(".mark-suspect")).toHaveAttribute("title", "שם לא מוכר");
    original.unmount();

    const outgoing = [{ text: "[ילד]", mark: "tag" as const, label: "שם הילד/ה" }, ...segments.slice(1)];
    const out = render(<p><Segments side="outgoing" segments={outgoing} /></p>);
    expect(out.container.querySelector("mark")).toBeNull();
    expect(out.container.querySelector(".mark-role")).toHaveTextContent(/^ילד$/);
    expect(out.container.textContent).not.toContain("נועה");
  });

  it("tags read as roles: brackets are dropped, numbering kept", () => {
    expect(displayTag("[ילד]")).toBe("ילד");
    expect(displayTag("[גננת_2]")).toBe("גננת_2");
  });
});

describe("ErrorLine", () => {
  it("nothing is shown when there is no error", () => {
    const { container } = render(<ErrorLine error={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("a missing consent on a case offers the consent form of that case", async () => {
    const go = vi.fn();
    const message = "לפני שליחה צריך לרשום את הסכמת ההורים.";
    rememberFailure({ code: "consent_missing", message, details: [] });
    render(
      <AppContext.Provider value={api({ go, route: { name: "case", id: "c9", view: "report" } })}>
        <ErrorLine error={message} />
      </AppContext.Provider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(message);
    await userEvent.setup().click(screen.getByRole("button", { name: "רישום ההסכמה" }));
    expect(go).toHaveBeenCalledWith({ name: "case", id: "c9", view: "details" });
  });

  it("a bad key opens the settings; the report for Or is shown in full before it is copied", async () => {
    const go = vi.fn();
    const writeText = vi.fn(async () => undefined);
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    mockIPC((cmd) => {
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.5.1", build_commit: "abc123", fips_active: false, platform: "windows" };
      if (cmd === "recent_crashes") return ["1700000000 · v0.5.1 · thread main · crates/x.rs:3 · (message left out)"];
      throw new Error(cmd);
    });
    const message = "המפתח לא התקבל אצל הספק.";
    rememberFailure({ code: "egress", message, details: ["401 from provider", "request id r-1"] });
    render(
      <AppContext.Provider value={api({ go, route: { name: "case", id: "c-secret", view: "report" } })}>
        <ErrorLine error={message} />
      </AppContext.Provider>,
    );
    await user.click(screen.getByRole("button", { name: "פתיחת ההגדרות" }));
    expect(go).toHaveBeenCalledWith({ name: "settings", pane: "ai" });
    await user.click(screen.getByRole("button", { name: "דוח תקלה לאור" }));
    const box = await screen.findByRole("textbox", { name: "תוכן הדוח" });
    const text = (box as HTMLTextAreaElement).value;
    for (const part of ["0.5.1 (abc123)", "windows", "תיק (לשונית report)", "קוד שגיאה: egress", "401 from provider", "crates/x.rs:3"]) {
      expect(text).toContain(part);
    }
    expect(text).not.toContain("c-secret");
    expect(text).not.toContain(message);
    // She may delete a line first; only what is in the box is copied.
    await user.clear(box);
    await user.type(box, "רק זה");
    await user.click(screen.getByRole("button", { name: "העתקה" }));
    expect(writeText).toHaveBeenCalledWith("רק זה");
    expect(await screen.findByText(/הועתק/)).toBeInTheDocument();
  });

  it("an error with nothing to fix it gets a retry when the caller can retry, else the report for a fault", async () => {
    const retry = vi.fn();
    const { unmount } = render(<ErrorLine error="לא הצלחתי לטעון." retry={retry} />);
    await userEvent.setup().click(screen.getByRole("button", { name: "לנסות שוב" }));
    expect(retry).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "דוח תקלה לאור" })).toBeNull();
    unmount();
    const { unmount: again } = render(<ErrorLine error="אין טקסט לשמור." />);
    expect(screen.queryByRole("button")).toBeNull();
    again();
    rememberFailure({ code: "ai", message: "התשובה לא תקינה.", details: [] });
    render(<ErrorLine error="התשובה לא תקינה." />);
    expect(screen.getByRole("button", { name: "דוח תקלה לאור" })).toBeInTheDocument();
  });

  it("a message that is not the remembered failure never offers that failure's fix", () => {
    rememberFailure({ code: "consent_missing", message: "הודעה אחרת", details: ["x"] });
    render(
      <AppContext.Provider value={api({ route: { name: "case", id: "c1", view: "report" } })}>
        <ErrorLine error="אין טקסט לשמור." />
      </AppContext.Provider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("אין טקסט לשמור.");
    expect(screen.queryByRole("button", { name: "רישום ההסכמה" })).toBeNull();
  });
});

describe("Skeleton", () => {
  it("is announced as loading and draws the given number of bars", () => {
    render(<Skeleton lines={4} label="טוען תיקים" card />);
    const s = screen.getByRole("status", { name: "טוען תיקים" });
    expect(s).toHaveAttribute("aria-busy", "true");
    expect(s.querySelectorAll(".skeleton-line")).toHaveLength(4);
  });
});

describe("Dialog", () => {
  it("takes focus when it opens, is labelled by its title and passes axe", async () => {
    render(<Dialog title="חלון בדיקה" subtitle="הסבר קצר" onClose={vi.fn()}><input aria-label="שדה" /></Dialog>);
    const dialog = screen.getByRole("dialog", { name: "חלון בדיקה" });
    expect(dialog).toHaveFocus();
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(await a11yViolations(document.body)).toEqual([]);
  });

  it("a field marked autoFocus inside keeps the focus, so she can type at once (regression)", async () => {
    const user = userEvent.setup();
    render(<Dialog title="רישום מפגש" onClose={vi.fn()}><textarea aria-label="הטקסט" autoFocus /></Dialog>);
    expect(screen.getByLabelText("הטקסט")).toHaveFocus();
    await user.keyboard("מילה");
    expect(screen.getByLabelText("הטקסט")).toHaveValue("מילה");
  });

  it("Escape and × close a clean dialog", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<Dialog title="d" onClose={onClose}><p>x</p></Dialog>);
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "סגירה" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("a dialog without onClose cannot be dismissed by Escape and has no ×", async () => {
    render(<Dialog title="חובה"><p>x</p></Dialog>);
    await userEvent.setup().keyboard("{Escape}");
    expect(screen.getByRole("dialog", { name: "חובה" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "סגירה" })).toBeNull();
  });

  it("with typing: × asks first, 'continue editing' keeps it open, the footer comes back", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<Dialog title="d" dirty onClose={onClose} footer={<button type="button">שמירה</button>}><p>x</p></Dialog>);
    await user.click(screen.getByRole("button", { name: "סגירה" }));
    const ask = screen.getByRole("alertdialog", { name: "לסגור בלי לשמור" });
    expect(screen.queryByRole("button", { name: "שמירה" })).toBeNull();
    // The safe choice has focus, so Enter does not lose the typing.
    expect(within(ask).getByRole("button", { name: "להמשיך לערוך" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.getByRole("button", { name: "שמירה" })).toBeInTheDocument();
  });
});
