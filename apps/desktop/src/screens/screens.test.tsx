// Component tests of the main screens over the in-memory demo core (Q-4, UX-7).
// Every name and detail comes from the fake core's invented cases.
import { act, configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../App";
import { FakeCore } from "../web/fakeCore";
import { a11yViolations } from "../test/a11y";
import type { CaseSummary } from "../ipc/generated/CaseSummary";
import type { CaseDetail } from "../ipc/generated/CaseDetail";

// The whole app over the demo core takes a moment to settle in jsdom.
configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

async function openApp(unlock = true, review = true, prep?: (core: FakeCore) => void) {
  const core = new FakeCore();
  prep?.(core);
  if (unlock) await core.handle("unlock", { password: "x" });
  // These tests walk every screen a request passes through, the review screen included, so
  // they run with "show me every request before sending" on (off by default; set here for the tests that read it).
  if (unlock) await core.handle("set_review_always", { on: review });
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
  it("says once what happened since the last opening, and stays closed when she closes it", async () => {
    const { user, view } = await openApp(true, true, (core) => {
      core.sinceLast = "מאז הכניסה הקודמת היו 2 ניסיונות כניסה שנכשלו. אם זו לא את, כדאי להחליף סיסמה.";
    });
    expect(await screen.findByText(/היו 2 ניסיונות כניסה שנכשלו/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "הבנתי" }));
    expect(screen.queryByText(/היו 2 ניסיונות כניסה שנכשלו/)).not.toBeInTheDocument();
    // The cases screen again (as after a visit to a case): still closed.
    view.unmount();
    render(<App />);
    expect(await screen.findByRole("button", { name: /בתיק של נועם$/ })).toBeInTheDocument();
    expect(screen.queryByText(/היו 2 ניסיונות כניסה שנכשלו/)).not.toBeInTheDocument();
  });

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

  it("before the file goes out, an approved paragraph with a note is listed, one click from it", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    mockIPC(async (cmd, args) => {
      const out = await core.handle(cmd, args);
      if (cmd !== "case_detail") return out;
      const d = out as CaseDetail;
      let marked = false;
      return {
        ...d,
        sections: d.sections.map((s) => ({
          ...s,
          paragraphs: s.paragraphs.map((p) => {
            if (marked || p.status !== "approved" || s.key === "signature") return p;
            marked = true;
            return { ...p, warnings: ["כתוב 85, ובטבלה 95"] };
          }),
        })),
      };
    });
    const user = userEvent.setup();
    render(<App />);
    await openCase(user);
    await user.click(screen.getByRole("button", { name: /הוצאת הדוח/ }));
    const side = await screen.findByRole("region", { name: "לפני שהקובץ יוצא" });
    expect(await within(side).findByText("פסקה מאושרת אחת שווה מבט נוסף:")).toBeInTheDocument();
    expect(within(side).getByText(/כתוב 85, ובטבלה 95/)).toBeInTheDocument();
  });

  it("a case whose last send failed says so in the list", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    mockIPC(async (cmd, args) => {
      const out = await core.handle(cmd, args);
      if (cmd !== "list_cases") return out;
      return (out as CaseSummary[]).map((c) => (c.child_name === "נועם" ? { ...c, failed_at: 1_700_000_000 } : c));
    });
    const view = render(<App />);
    const row = (await screen.findByRole("button", { name: /בתיק של נועם$/ })).closest("tr") as HTMLElement;
    expect(within(row).getByText("השליחה נכשלה")).toBeInTheDocument();
    expect(within(row).getByText("לנסות שוב")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the report page shows how close a draft of the AI is to her style, and only for drafts", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    const prepared: Record<string, unknown>[] = [];
    const style = { fit: 82, details: ["משפטים ארוכים מהרגיל אצלך"], ask: "משפטים קצרים יותר", marks: [{ from: "מפגין", to: "מראה" }] };
    mockIPC(async (cmd, args) => {
      if (cmd === "prepare_section") prepared.push(args as Record<string, unknown>);
      const out = await core.handle(cmd, args);
      if (cmd !== "case_detail") return out;
      const d = out as CaseDetail;
      return {
        ...d,
        sections: d.sections.map((s) => s.key !== "kindergarten" ? s : {
          ...s,
          paragraphs: [
            { id: "p-ok", text: "פסקה מאושרת שבה הילד מפגין סקרנות.", status: "approved", by_ai: true, sources: [], warnings: [], replaces: null, has_versions: false, style_note: null, style },
            { id: "p-new", text: "בגן הילד מפגין עניין במשחק.", status: "proposed", by_ai: true, sources: [], warnings: [], replaces: null, has_versions: false, style_note: null, style },
            { id: "p-plain", text: "פסקה בלי ציון.", status: "proposed", by_ai: true, sources: [], warnings: [], replaces: null, has_versions: false, style_note: null, style: null },
          ],
        }),
      };
    });
    const user = userEvent.setup();
    render(<App />);
    await openCase(user);
    const chips = await screen.findAllByRole("button", { name: "קרוב לסגנון שלך: 82" });
    expect(chips).toHaveLength(1);
    // Her usual change is marked in the draft only, as text.
    const marks = Array.from(document.querySelectorAll("mark.habit"));
    expect(marks).toHaveLength(1);
    expect(marks[0]?.closest(".a4-pending")).not.toBeNull();
    await user.click(chips[0] as HTMLElement);
    expect(chips[0]).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("משפטים ארוכים מהרגיל אצלך")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "לקרב לסגנון שלי" }));
    await waitFor(() => expect(prepared).toHaveLength(1));
    const req = prepared[0] as { instruction: string; replaces: string; sectionKey: string };
    expect(req.instruction).toContain("משפטים קצרים יותר");
    expect(req.instruction).toContain("בגן הילד מפגין עניין במשחק.");
    expect(req.replaces).toBe("p-new");
  });

  it("'full page' hides the contents beside the report, and brings it back (round 42)", async () => {
    const { user } = await openApp();
    await openCase(user);
    expect(screen.getByRole("complementary", { name: "תוכן הדוח" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "דף מלא" }));
    expect(screen.queryByRole("complementary", { name: "תוכן הדוח" })).toBeNull();
    expect(screen.getByRole("button", { name: "דף מלא" })).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "דף מלא" }));
    expect(screen.getByRole("complementary", { name: "תוכן הדוח" })).toBeInTheDocument();
  });

  it("material not sorted yet is a clear alert above the writing, with what to do (D-114)", async () => {
    const { user, calls } = await openApp();
    await openCase(user);
    const contents = screen.getByRole("complementary", { name: "תוכן הדוח" });
    const alert = within(contents).getByText(/חומרים עוד לא מוינו לסעיפים|חומר אחד עוד לא מוין לסעיפים/).closest(".contents-alert") as HTMLElement;
    expect(alert).toHaveTextContent("חומר שלא מוין לא נכנס לטיוטות.");
    // Above "write the empty ones", not in small print under it.
    const write = within(contents).getByRole("button", { name: /^✦ לכתוב את/ });
    expect(alert.compareDocumentPosition(write) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await user.click(within(alert).getByRole("button", { name: "למיין את החומרים" }));
    expect(await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" })).toHaveTextContent("מיון החומרים לסעיפים");
    expect(calls).toContain("prepare_sort_parts");
  });

  it("without consent, the alert sends her to record it instead of sorting (D-114)", async () => {
    const { user, core, calls } = await openApp();
    const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
    if (!noam) throw new Error("no case");
    const detail = (await core.handle("case_detail", { caseId: noam.id })) as CaseDetail;
    await core.handle("update_case", { caseId: noam.id, meta: { ...detail.meta, consent: null } });
    // Without consent the case opens on its materials; the contents are on the writing stage.
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    await user.click(within(await screen.findByRole("navigation", { name: "שלבי התיק" })).getByRole("button", { name: /כתיבה/ }));
    const alert = within(screen.getByRole("complementary", { name: "תוכן הדוח" })).getByRole("status");
    expect(within(alert).queryByRole("button", { name: "למיין את החומרים" })).toBeNull();
    expect(alert).toHaveTextContent("קודם רושמים את הסכמת ההורים");
    await user.click(within(alert).getByRole("button", { name: "רישום ההסכמה" }));
    await vi.waitFor(() => expect(screen.queryByRole("complementary", { name: "תוכן הדוח" })).toBeNull());
    expect(calls).not.toContain("prepare_sort_parts");
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

  it("the supervisor's lines are kept in settings and printed under her signature", async () => {
    const { user, core } = await openApp();
    await user.click(await screen.findByRole("button", { name: "הגדרות" }));
    await user.click(await screen.findByRole("button", { name: "הדוח" }));
    const box = await screen.findByLabelText("חתימת המדריכה");
    await user.type(box, "בהדרכת ד\"ר נועה בדויה");
    await user.click(within(box.closest("section") ?? document.body).getByRole("button", { name: "שמירה" }));
    await waitFor(async () => expect(((await core.handle("report_settings", {})) as { supervisor: string[] }).supervisor).toEqual(["בהדרכת ד\"ר נועה בדויה"]));
    await user.click(screen.getByRole("button", { name: "תיקים" }));
    const page = await openCase(user);
    const sign = page.querySelector(".a4-sign");
    expect(sign?.textContent).toMatch(/מומחית.*בהדרכת ד"ר נועה בדויה/);
  });

  it("the report is exported without a password, as Word or as a print-only PDF", async () => {
    const exported: [string, unknown][] = [];
    const { user, core } = await openApp();
    mockIPC((cmd, args) => {
      if (cmd.startsWith("export_")) exported.push([cmd, args]);
      return core.handle(cmd, args);
    });
    await openCase(user);
    await user.click(screen.getByRole("button", { name: /הוצאת הדוח/ }));
    expect(await screen.findByText("נשמר בתיקיית ההורדות, ונפתח בלי סיסמה.")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "הפקת הקובץ" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "הפקת הקובץ" }));
    const dialog = await screen.findByRole("dialog");
    // No password is asked for, and she is told the file opens without one (D-081).
    expect(within(dialog).queryByLabelText(/סיסמה/)).toBeNull();
    expect(within(dialog).getByText("הקובץ נפתח בלי סיסמה: כל מי שיקבל אותו יוכל לקרוא אותו.")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("radio", { name: /PDF להדפסה בלבד/ }));
    await user.click(within(dialog).getByRole("button", { name: "הפקת הדוח" }));
    await waitFor(() => expect(exported).toEqual([["export_pdf", { caseId: expect.any(String), password: null, parents: null }]]));
    expect(await screen.findByRole("dialog", { name: "הדוח מוכן" })).not.toHaveTextContent(/סיסמה/);
  });

  it("the letter is written from the recommendations she marks", async () => {
    const letters: unknown[] = [];
    const { user, core } = await openApp();
    const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם") ?? ({} as never);
    await core.handle("add_own_paragraph", { caseId: noam.id, sectionKey: "recommendations", text: "המלצה על ריפוי בעיסוק." });
    await core.handle("add_own_paragraph", { caseId: noam.id, sectionKey: "recommendations", text: "המלצה על הדרכת הורים." });
    mockIPC((cmd, args) => {
      if (cmd === "set_letter_left_out") letters.push(args);
      return core.handle(cmd, args);
    });
    await openCase(user);
    await user.click(screen.getByRole("button", { name: /הוצאת הדוח/ }));
    const picks = await screen.findByRole("group", { name: "ההמלצות שנכנסות למכתב" });
    const ot = within(picks).getByRole("checkbox", { name: "המלצה על ריפוי בעיסוק." });
    expect(ot).toBeChecked();
    await user.click(ot);
    await waitFor(() => expect(letters).toHaveLength(1));
    expect(letters[0]).toMatchObject({ ids: [expect.any(String)] });
    await waitFor(() => expect(within(picks).getByRole("checkbox", { name: "המלצה על ריפוי בעיסוק." })).not.toBeChecked());
    expect(within(picks).getByRole("checkbox", { name: "המלצה על הדרכת הורים." })).toBeChecked();
    // Marked again: all of them, as before.
    await user.click(within(picks).getByRole("checkbox", { name: "המלצה על ריפוי בעיסוק." }));
    await waitFor(() => expect(letters[1]).toEqual({ caseId: noam.id, ids: null }));
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
    const write = within(page).getAllByRole("button", { name: "✦ לנסח" })[0];
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
    await waitFor(() => expect(calls).toContain("approve_paragraphs"));
    await waitFor(() => expect(screen.queryByRole("button", { name: "✓ לאשר" })).toBeNull());
  });

  it("every clean draft is approved with one press from the contents (D-141)", async () => {
    const { user, calls } = await openApp();
    const page = await openCase(user);
    for (let i = 0; i < 2; i++) {
      const write = within(page).getAllByRole("button", { name: "✦ לנסח" })[0];
      if (!write) throw new Error("no empty section to write");
      await user.click(write);
      const review = await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" });
      await user.click(within(review).getByRole("button", { name: /שליחה/ }));
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "לפני שליחה ל-Claude" })).toBeNull());
      await waitFor(() => expect(calls.filter((c) => c === "send_section").length).toBe(i + 1));
      await screen.findByRole("button", { name: "✓ לאשר" }, { timeout: 6000 });
    }
    await user.click(await screen.findByRole("button", { name: "✓ לאשר את כל הטיוטות (2)" }, { timeout: 6000 }));
    await waitFor(() => expect(calls.filter((c) => c === "approve_paragraphs").length).toBe(1));
    await waitFor(() => expect(screen.queryByRole("button", { name: "✓ לאשר" })).toBeNull());
  });

  it("a second request while the review is open sends the second request, never the first (regression)", async () => {
    const { user, core } = await openApp();
    const page = await openCase(user);
    const prepared: string[] = [];
    const sent: unknown[] = [];
    mockIPC(async (cmd, args) => {
      const out = await core.handle(cmd, args);
      if (cmd === "prepare_section") prepared.push((out as { approval_id: string }).approval_id);
      if (cmd === "send_section") sent.push((args as { approvalId: string }).approvalId);
      return out;
    });
    const writes = within(page).getAllByRole("button", { name: "✦ לנסח" });
    const [first, second] = writes;
    if (!first || !second) throw new Error("two empty sections are needed");
    await user.click(first);
    const review = await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" });
    await user.click(second);
    await waitFor(() => expect(prepared).toHaveLength(2));
    // The review now shows the second request: sending it sends that one.
    await user.click(within(await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" })).getByRole("button", { name: /שליחה/ }));
    await waitFor(() => expect(sent).toHaveLength(1));
    expect(sent[0]).toBe(prepared[1]);
    expect(review).not.toBeInTheDocument();
  });

  it("'back to editing' in the sorting review keeps her on the materials page (regression)", async () => {
    const { user, calls } = await openApp();
    await openCase(user);
    await user.click(within(screen.getByRole("navigation", { name: "שלבי התיק" })).getByRole("button", { name: /חומרים/ }));
    await screen.findByRole("heading", { name: "מה יש בתיק" });
    await user.click(screen.getByRole("button", { name: "לכתיבת הדוח ←" }));
    const review = await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" });
    expect(review).toHaveTextContent("מיון החומרים לסעיפים");
    await user.click(within(review).getByRole("button", { name: "חזרה לעריכה" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "לפני שליחה ל-Claude" })).toBeNull());
    expect(screen.getByRole("heading", { name: "מה יש בתיק" })).toBeInTheDocument();
    expect(screen.queryByRole("article", { name: "הדוח, כמו בקובץ" })).toBeNull();
    expect(calls).not.toContain("send_sort");
    // Nothing is blocked: the same button asks again.
    await user.click(screen.getByRole("button", { name: "לכתיבת הדוח ←" }));
    expect(await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" })).toBeInTheDocument();
  });

  it("a send that failed is sent again from the section, and its draft arrives like the first time (F6)", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: true });
    const calls: string[] = [];
    let sends = 0;
    mockIPC((cmd, args) => {
      calls.push(cmd);
      if (cmd === "send_section" && ++sends === 1) {
        throw { code: "egress", message: "אין חיבור לאינטרנט.", details: [], action: { kind: "retry", after_seconds: 0 } };
      }
      return core.handle(cmd, args);
    });
    const user = userEvent.setup();
    render(<App />);
    const page = await openCase(user);
    const write = within(page).getAllByRole("button", { name: "✦ לנסח" })[0];
    if (!write) throw new Error("no empty section to write");
    await user.click(write);
    const review = await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" });
    await user.click(within(review).getByRole("button", { name: /שליחה/ }));
    const again = await screen.findByRole("button", { name: "לנסות שוב" });
    await user.click(again);
    // The same approved request goes again (nothing prepared again), and the draft shows.
    await screen.findByRole("button", { name: "✓ לאשר" }, { timeout: 6000 });
    expect(sends).toBe(2);
    expect(calls.filter((c) => c === "prepare_section")).toHaveLength(1);
  });

  it("with every request shown first turned off, a cleared request goes at once: no window (round 26)", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: false });
    const calls: string[] = [];
    mockIPC((cmd, args) => {
      calls.push(cmd);
      return core.handle(cmd, args);
    });
    const user = userEvent.setup();
    render(<App />);
    const page = await openCase(user);
    const write = within(page).getAllByRole("button", { name: "✦ לנסח" })[0];
    if (!write) throw new Error("no empty section to write");
    await user.click(write);
    await screen.findByRole("button", { name: "✓ לאשר" }, { timeout: 6000 });
    expect(screen.queryByRole("dialog", { name: /לפני שליחה/ })).toBeNull();
    expect(calls.indexOf("send_section")).toBeGreaterThan(calls.indexOf("prepare_section"));
  });

  it("with it off, a request the gate still refuses is a line with 'לנסות שוב', never a window (round 26)", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: false });
    let prepares = 0;
    mockIPC(async (cmd, args) => {
      const out = await core.handle(cmd, args);
      if (cmd === "prepare_section" && ++prepares === 1) {
        return { ...(out as object), approval_id: null, blocked: [{ code: "internal", message: "שגיאה פנימית", detail: "נועם" }] };
      }
      return out;
    });
    const user = userEvent.setup();
    render(<App />);
    const page = await openCase(user);
    const write = within(page).getAllByRole("button", { name: "✦ לנסח" })[0];
    if (!write) throw new Error("no empty section to write");
    await user.click(write);
    const line = await screen.findByText(/הבקשה לא נשלחה: שגיאה פנימית/);
    expect(screen.queryByRole("dialog", { name: /לפני שליחה/ })).toBeNull();
    // The gate's detail (which may quote a name) is not on the screen.
    expect(line.closest(".job-failed")).not.toHaveTextContent("נועם");
    await user.click(screen.getByRole("button", { name: "לנסות שוב" }));
    await screen.findByRole("button", { name: "✓ לאשר" }, { timeout: 6000 });
    expect(prepares).toBe(2);
  });

  it("at the monthly ceiling, one click raises it and the same request goes (round 26)", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: false });
    const calls: { cmd: string; args: unknown }[] = [];
    let sends = 0;
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "send_section" && ++sends === 1) {
        throw { code: "refused", message: "הגעת לתקרת ההוצאה החודשית שקבעת ($10). אפשר להעלות אותה ל-$20 ולהמשיך.", details: [], action: { kind: "raise_cap", to_usd: 20 } };
      }
      return core.handle(cmd, args);
    });
    const user = userEvent.setup();
    render(<App />);
    const page = await openCase(user);
    const write = within(page).getAllByRole("button", { name: "✦ לנסח" })[0];
    if (!write) throw new Error("no empty section to write");
    await user.click(write);
    await user.click(await screen.findByRole("button", { name: "להעלות ל-$20 ולהמשיך" }));
    await screen.findByRole("button", { name: "✓ לאשר" }, { timeout: 6000 });
    expect(calls.find((c) => c.cmd === "set_monthly_cap")?.args).toEqual({ capUsd: 20 });
    expect(sends).toBe(2);
    expect(calls.filter((c) => c.cmd === "prepare_section")).toHaveLength(1);
  });

  it("a material added is read by itself: its summary, its record, and what is still missing", async () => {
    const { user, calls } = await openApp(true, false);
    await openCase(user);
    await user.click((screen.getAllByRole("button", { name: /חומרים/ })[0] as HTMLElement));
    const list = await screen.findByRole("region", { name: "רשימת החומרים" });
    expect(within(list).getByText(/עוד לא נקרא/)).toBeInTheDocument();
    expect(calls).not.toContain("prepare_sort_parts");
    await user.click(await screen.findByRole("button", { name: "הוספה לאבחונים קודמים" }));
    await user.click(await screen.findByRole("menuitem", { name: "הדבקת טקסט" }));
    await user.type(screen.getByLabelText("כותרת"), "דוח קלינאית");
    await user.type(screen.getByLabelText("הטקסט"), "נבדק ב-3/2024.{Enter}{Enter}נמצא עיכוב קל בשפה.{Enter}{Enter}מומלץ על טיפול בשפה.");
    await user.click(screen.getByRole("button", { name: "שמירה בתיק" }));
    // Sorted and summed up without a window, and the report recorded in the report's order.
    const prior = await screen.findByRole("region", { name: "אבחונים קודמים, בסדר הדוח" }, { timeout: 4000 });
    expect(within(prior).getByText(/3\/2024 · דוח של איש מקצוע/)).toBeInTheDocument();
    expect(within(prior).getByText("נמצא עיכוב קל בשפה.")).toBeInTheDocument();
    expect(within(list).queryByText(/עוד לא נקרא/)).not.toBeInTheDocument();
    expect(calls).toContain("send_sort");
    expect(screen.queryByRole("dialog", { name: /לפני שליחה/ })).not.toBeInTheDocument();
    // The material's window shows the whole summary, as text.
    await user.click(within(prior).getByRole("button", { name: "דוח קלינאית" }));
    const reading = await screen.findByRole("region", { name: "מה כתוב בחומר" });
    expect(within(reading).getByText(/נבדק ב-3\/2024/)).toBeInTheDocument();
  });

  it("writes the whole report in one click, in the background, and checks it against the sources (round 41)", async () => {
    const { user, calls } = await openApp(true, false);
    await openCase(user);
    await user.click(screen.getByRole("button", { name: /עוד פעולות/ }));
    await user.click(await screen.findByRole("menuitem", { name: "כתיבת כל הדוח עם Claude" }));
    // No window: one line says what the program is doing, with "stop".
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(await screen.findByText(/התוכנה עובדת/)).toBeInTheDocument();
    expect(await screen.findByText(/הדוח נכתב ונבדק מול המקורות\. \d+ סעיפים מחכים לאישורך/, {}, { timeout: 20000 })).toBeInTheDocument();
    expect(calls).toContain("send_section");
    expect(calls).toContain("send_verify");
    expect(calls.indexOf("send_verify")).toBeGreaterThan(calls.lastIndexOf("send_section"));
    await user.click(within(screen.getByText(/הדוח נכתב ונבדק/).closest(".run-line") as HTMLElement).getByRole("button", { name: "סגירה" }));
    expect(screen.queryByText(/הדוח נכתב ונבדק/)).toBeNull();
  });

  it("the run goes on after she leaves the case, and the case she comes back to shows its drafts (review of stage 6)", async () => {
    const { user } = await openApp(true, false);
    await openCase(user);
    await user.click(screen.getByRole("button", { name: /עוד פעולות/ }));
    await user.click(await screen.findByRole("menuitem", { name: "כתיבת כל הדוח עם Claude" }));
    expect(await screen.findByText(/התוכנה עובדת/)).toBeInTheDocument();
    // While it writes, nothing on the page sends a second request.
    for (const b of screen.queryAllByRole("button", { name: /^✦ לכתוב את|^מיון החומרים$/ })) expect(b).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "→ תיקים" }));
    await openCase(user);
    await screen.findByText(/הדוח נכתב ונבדק מול המקורות/, {}, { timeout: 20000 });
    // The page is the one she came back to: the new drafts wait for her there (in the contents
    // beside the page since round 48).
    const contents = await screen.findByRole("complementary", { name: "תוכן הדוח" });
    await waitFor(() => expect(contents).toHaveTextContent(/טיוטות מחכות לך|טיוטה אחת מחכה לך/));
  });

  it("with the review screen on, nothing is read by itself", async () => {
    const { user, calls } = await openApp();
    await openCase(user);
    await user.click((screen.getAllByRole("button", { name: /חומרים/ })[0] as HTMLElement));
    await user.click(await screen.findByRole("button", { name: "הוספה לאבחונים קודמים" }));
    await user.click(await screen.findByRole("menuitem", { name: "הדבקת טקסט" }));
    await user.type(screen.getByLabelText("כותרת"), "דוח קלינאית");
    await user.type(screen.getByLabelText("הטקסט"), "נמצא עיכוב קל בשפה.");
    await user.click(screen.getByRole("button", { name: "שמירה בתיק" }));
    await screen.findByText("נשמר בתיק.");
    expect(calls).not.toContain("prepare_sort_parts");
  });

  it("Esc with a changed text asks first; the text is kept until she says to drop it", async () => {
    const { user } = await openApp();
    const page = await openCase(user);
    const original = within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/);
    await user.click(original);
    const box = await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ });
    await user.type(box, " ועוד מילה.");
    await user.keyboard("{Escape}");
    const ask = await screen.findByRole("group", { name: "לבטל את העריכה?" });
    expect((screen.getByRole("textbox", { name: /עריכת פסקה בסעיף/ }) as HTMLTextAreaElement).value).toContain("ועוד מילה.");
    await user.click(within(ask).getByRole("button", { name: "להמשיך לערוך" }));
    expect(screen.queryByRole("group", { name: "לבטל את העריכה?" })).toBeNull();
    expect((screen.getByRole("textbox", { name: /עריכת פסקה בסעיף/ }) as HTMLTextAreaElement).value).toContain("ועוד מילה.");
    // Esc twice: asked, then dropped.
    await user.keyboard("{Escape}{Escape}");
    expect(screen.queryByRole("textbox", { name: /עריכת פסקה בסעיף/ })).toBeNull();
    expect(within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/)).toBeInTheDocument();
    expect(within(page).queryByText(/ועוד מילה/)).toBeNull();
  });

  it("Esc with nothing changed closes the editor at once", async () => {
    const { user } = await openApp();
    const page = await openCase(user);
    await user.click(within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/));
    await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("textbox", { name: /עריכת פסקה בסעיף/ })).toBeNull();
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

describe("text being typed (F1a)", () => {
  it("survives leaving the stage before it was reported, and is in the editor on return", async () => {
    const { user } = await openApp();
    const page = await openCase(user);
    await user.click(within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/));
    const box = await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ });
    // No pause: the box has not reported it yet when she leaves.
    fireEvent.change(box, { target: { value: "נוסח שלא נשמר עדיין" } });
    await user.click(screen.getAllByRole("button", { name: /חומרים/ })[0] as HTMLElement);
    await screen.findByRole("heading", { name: "מה יש בתיק" });
    await user.click(screen.getAllByRole("button", { name: /כתיבה$/ })[0] as HTMLElement);
    const again = await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ });
    expect(again).toHaveValue("נוסח שלא נשמר עדיין");
  });

  it("is held in full before a lock by hand, even typed without a pause", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      return core.handle(cmd, args);
    });
    const user = userEvent.setup();
    render(<App />);
    const page = await openCase(user);
    await user.click(within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/));
    const box = await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ });
    fireEvent.change(box, { target: { value: "נוסח שנכתב רגע לפני הנעילה" } });
    fireEvent.keyDown(window, { key: "l", code: "KeyL", ctrlKey: true });
    await waitFor(() => expect(calls.some((c) => c.cmd === "lock")).toBe(true));
    const lock = calls.findIndex((c) => c.cmd === "lock");
    const held = calls.slice(0, lock).filter((c) => c.cmd === "hold_unsaved").at(-1);
    expect((held?.args as { edits: { text: string }[] }).edits.at(-1)?.text).toBe("נוסח שנכתב רגע לפני הנעילה");
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
    // One section is in focus (round 48): its heading is the one the keyboard reaches.
    const at = headings.findIndex((h) => h.tabIndex === 0);
    expect(at).toBeGreaterThanOrEqual(0);
    act(() => {
      fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown", altKey: true });
    });
    await waitFor(() => expect(document.activeElement).toBe(headings[at + 1]));
    expect(headings[at + 1]).toHaveAttribute("tabindex", "0");
    act(() => {
      fireEvent.keyDown(window, { key: "ArrowUp", code: "ArrowUp", altKey: true });
    });
    await waitFor(() => expect(document.activeElement).toBe(headings[at]));
  });

  it("Alt+↓ does nothing under a dialog or while typing in a field (regression)", async () => {
    const { user } = await openApp();
    await openCase(user);
    const headings = screen.getAllByRole("heading", { level: 3 });
    const at = headings.findIndex((h) => h.tabIndex === 0);
    fireEvent.keyDown(window, { key: "F1", code: "F1" });
    const sheet = await screen.findByRole("dialog", { name: "קיצורי מקלדת" });
    act(() => {
      fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown", altKey: true });
    });
    expect(sheet).toBeInTheDocument();
    expect(headings[at + 1]).toHaveAttribute("tabindex", "-1");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await user.click(within(screen.getByRole("article", { name: "הדוח, כמו בקובץ" })).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/));
    const box = await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ });
    act(() => {
      fireEvent.keyDown(box, { key: "ArrowDown", code: "ArrowDown", altKey: true });
    });
    expect(box).toBeInTheDocument();
    expect(headings[at + 1]).toHaveAttribute("tabindex", "-1");
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
