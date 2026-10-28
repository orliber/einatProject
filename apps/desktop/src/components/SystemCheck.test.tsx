import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { CheckLine, SystemCheck, SystemCheckResult } from "../ipc/client";
import { fakeSystemCheck } from "../web/fakeCore";
import { a11yViolations } from "../test/a11y";
import { CHECKS, SystemCheckSettings, systemCheckSummary } from "./SystemCheck";

const line = (over: Partial<CheckLine> = {}): CheckLine => ({
  key: "docx", label: "קריאת קובץ Word", state: "ok", reason: "קובץ הבדיקה נקרא כמו שצריך.", next_step: null, detail: "docx: ok (docx)", ...over,
});

/** The core's answers: the preview's, except where a test changes one. */
function answer(over: Partial<Record<SystemCheck, SystemCheckResult | Error>> = {}) {
  const calls: SystemCheck[] = [];
  mockIPC((cmd, args) => {
    expect(cmd).toBe("system_check");
    const check = (args as { check: SystemCheck }).check;
    calls.push(check);
    const given = over[check];
    if (given instanceof Error) throw { code: "internal", message: "x", details: [], action: { kind: "report" } };
    return given ?? fakeSystemCheck(check, false);
  });
  return calls;
}

describe("בדיקת מערכת", () => {
  it("runs the five checks one after another and shows each line with its mark and reason", async () => {
    const calls = answer({
      reading: {
        check: "reading",
        lines: [
          line({
            state: "failed", reason: "רכיב הקריאה לא הצליח לעלות (ספרייה שהוא צריך לא נטענה, קוד 0xc0000135). זו לא בעיה בקבצים.",
            next_step: "מתקינים מחדש את כספת האבחון.", detail: "docx: worker stopped: 0xc0000135",
          }),
        ],
      },
    });
    render(<SystemCheckSettings />);
    fireEvent.click(screen.getByRole("button", { name: "בדיקת מערכת" }));
    expect(await screen.findByText("בדיקה אחת לא עברה. לידה כתוב מה לעשות.")).toBeInTheDocument();
    expect(calls).toEqual(CHECKS.map((c) => c.id));
    const failed = screen.getByText(/0xc0000135/).closest("li");
    expect(failed).toHaveClass("failed");
    expect(within(failed as HTMLElement).getByText("✗")).toBeInTheDocument();
    expect(within(failed as HTMLElement).getByText(/מתקינים מחדש/)).toBeInTheDocument();
    expect(within(failed as HTMLElement).getByText(/לא תקין/)).toBeInTheDocument();
    // Every group has its title, and the connection line says nothing was sent.
    for (const c of CHECKS) expect(screen.getByRole("heading", { name: c.label })).toBeInTheDocument();
    expect(screen.getByText(/לחיצת יד מאובטחת בלבד/)).toBeInTheDocument();
    // The result takes the focus, so the keyboard and a screen reader land on it.
    await waitFor(() => expect(screen.getByText(/בדיקה אחת לא עברה/)).toHaveFocus());
    expect(screen.getByRole("button", { name: "לבדוק שוב" })).toBeEnabled();
  });

  it("a check that could not run is one failed line, and the others still run", async () => {
    const calls = answer({ ocr: new Error("refused") });
    render(<SystemCheckSettings />);
    fireEvent.click(screen.getByRole("button", { name: "בדיקת מערכת" }));
    expect(await screen.findByText("הבדיקה עצמה לא הצליחה לרוץ.")).toBeInTheDocument();
    await screen.findByRole("button", { name: "העתקה לאור" });
    expect(calls).toHaveLength(5);
  });

  it("does not start twice while it runs", async () => {
    let release: () => void = () => undefined;
    const calls: string[] = [];
    mockIPC(async (_cmd, args) => {
      const check = (args as { check: SystemCheck }).check;
      calls.push(check);
      if (check === "reading") await new Promise<void>((r) => { release = r; });
      return fakeSystemCheck(check, false);
    });
    render(<SystemCheckSettings />);
    fireEvent.click(screen.getByRole("button", { name: "בדיקת מערכת" }));
    const busy = await screen.findByRole("button", { name: "בודקת…" });
    expect(busy).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("בודקת 1 מתוך 5: קריאת קבצים");
    fireEvent.click(busy);
    release();
    await screen.findByRole("button", { name: "העתקה לאור" });
    expect(calls).toEqual(CHECKS.map((c) => c.id));
  });

  it("copies for Or a plain ASCII summary: no Hebrew, no path, no key", async () => {
    answer({
      connection: {
        check: "connection",
        lines: [line({ key: "connection", state: "failed", next_step: "x", detail: "connection: certificate: unknown issuer" })],
      },
    });
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<SystemCheckSettings />);
    fireEvent.click(screen.getByRole("button", { name: "בדיקת מערכת" }));
    fireEvent.click(await screen.findByRole("button", { name: "העתקה לאור" }));
    await screen.findByText(/הועתק/);
    const text = (writeText.mock.calls[0] as unknown as [string])[0];
    expect(text).toMatch(/^Diagnostic Vault system check\n/);
    expect(text).toContain("connection.connection: failed | connection: certificate: unknown issuer");
    expect(text).toContain("reading.docx: ok | docx: ok (docx)");
    expect(text).toMatch(/^[\x20-\x7E\n]*$/);
  });

  it("the summary drops what follows a path and anything not ASCII", () => {
    const text = systemCheckSummary(
      [{ check: "folders", lines: [line({ key: "export_folder", detail: "export_folder: C:\\Users\\רותם\\Downloads failed" })] }],
      new Date("2026-10-28T10:00:00Z"),
    );
    expect(text).toBe("Diagnostic Vault system check\ntime: 2026-10-28T10:00:00.000Z\nfolders.export_folder: ok | export_folder: C:<path left out>");
  });

  it("the preview answers every check, and says it opens no connection", () => {
    for (const c of CHECKS) {
      const r = fakeSystemCheck(c.id, false);
      expect(r.check).toBe(c.id);
      expect(r.lines.length).toBeGreaterThan(0);
      for (const l of r.lines) expect(l.detail).toMatch(/^[\x20-\x7E]*$/);
    }
    expect(fakeSystemCheck("connection", true).lines[0]?.reason).toMatch(/עצירת השליחה/);
  });

  it("has no accessibility violations with results shown", async () => {
    answer();
    const { container } = render(<SystemCheckSettings />);
    fireEvent.click(screen.getByRole("button", { name: "בדיקת מערכת" }));
    await screen.findByRole("button", { name: "העתקה לאור" });
    expect(await a11yViolations(container)).toEqual([]);
  });
});
