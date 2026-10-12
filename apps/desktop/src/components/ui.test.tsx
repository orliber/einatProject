// Shared pieces: filter text is shown as text, errors offer the next step, dialogs trap the
// keyboard's attention. All names are invented.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AppContext, type AppApi } from "../App";
import { rememberFailure } from "../errors";
import { status } from "../test/fixtures";
import { a11yViolations } from "../test/a11y";
import { Dialog, ErrorLine, Segments, displayTag } from "./ui";

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

  it("a bad key opens the settings; technical details can be copied for Or", async () => {
    const go = vi.fn();
    const writeText = vi.fn(async () => undefined);
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const message = "המפתח לא התקבל אצל הספק.";
    rememberFailure({ code: "egress", message, details: ["401 from provider", "request id r-1"] });
    render(
      <AppContext.Provider value={api({ go, route: { name: "cases" } })}>
        <ErrorLine error={message} />
      </AppContext.Provider>,
    );
    await user.click(screen.getByRole("button", { name: "פתיחת ההגדרות" }));
    expect(go).toHaveBeenCalledWith({ name: "settings" });
    await user.click(screen.getByRole("button", { name: "העתקת פרטים לאור" }));
    expect(writeText).toHaveBeenCalledWith("401 from provider\nrequest id r-1");
    expect(await screen.findByRole("button", { name: "הועתק" })).toBeInTheDocument();
  });

  it("a message that is not the remembered failure gets no actions", () => {
    rememberFailure({ code: "consent_missing", message: "הודעה אחרת", details: ["x"] });
    render(
      <AppContext.Provider value={api({ route: { name: "case", id: "c1", view: "report" } })}>
        <ErrorLine error="אין טקסט לשמור." />
      </AppContext.Provider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("אין טקסט לשמור.");
    expect(screen.queryByRole("button")).toBeNull();
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
