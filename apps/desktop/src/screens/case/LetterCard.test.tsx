// The short letter on the finish page (EX-4): which recommendations go in, writing it, approving
// and editing its paragraphs, producing the file, and what she sees when a step fails. The demo
// core has no letters, so a small letter store stands in for it here. Every name is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { STALE_EDIT_KEPT } from "../../i18n/he";
import type { ParagraphView } from "../../ipc/client";
import { clearKept } from "../../kept";
import { a11yViolations } from "../../test/a11y";
import { prepared, status } from "../../test/fixtures";
import { appApi, CaseHost, Host, serve, type Override } from "../../test/host";
import { LetterCard } from "./LetterCard";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

afterEach(() => clearKept());

const para = (id: string, text: string, status: ParagraphView["status"] = "proposed"): ParagraphView => ({
  id, text, status, by_ai: true, sources: [], warnings: [], replaces: null, has_versions: false, style_note: null, style: null,
});

const RECS = ["המלצה בדויה: ליווי של מטפלת רגשית בגן.", "המלצה בדויה: הכנה מראש לכל מעבר בסדר היום."];

/** The demo case with two approved recommendations, and a letter store in place of the core's. */
async function openLetter(over: Override = () => undefined, opts: { reviewAlways?: boolean; recs?: string[] } = {}) {
  const letters: Record<string, ParagraphView[]> = { parents: [], school: [] };
  let pendingAudience = "parents";
  const served = await serve((cmd, args, core) => {
    const out = over(cmd, args, core);
    if (out !== undefined) return out;
    switch (cmd) {
      case "letter":
        return letters[String(args.audience)];
      case "prepare_letter":
        pendingAudience = String(args.audience);
        return prepared({ approval_id: "letter-ok" });
      case "send_section":
        if (args.approvalId !== "letter-ok") return undefined;
        letters[pendingAudience] = [para("l1", `פסקה בדויה ראשונה (${pendingAudience}).`), para("l2", "פסקה בדויה שנייה.")];
        return { paragraphs: [], questions: [], missing: [], contradictions: [], demo: true };
      case "approve_paragraph":
      case "edit_paragraph": {
        const p = Object.values(letters).flat().find((x) => x.id === args.draftId);
        if (!p) return undefined;
        if (cmd === "approve_paragraph") p.status = "approved";
        else p.text = String(args.text);
        return cmd === "edit_paragraph" ? false : null;
      }
      default:
        return undefined;
    }
  });
  for (const text of opts.recs ?? RECS) {
    await served.core.handle("add_own_paragraph", { caseId: served.caseId, sectionKey: "recommendations", text });
  }
  const app = appApi({ status: status({ review_always: opts.reviewAlways ?? false }) });
  const user = userEvent.setup();
  const view = render(
    <Host app={app}>
      <CaseHost caseId={served.caseId}>{(api) => <LetterCard api={api} />}</CaseHost>
    </Host>,
  );
  await screen.findByText("מכתב קצר");
  return { ...served, letters, app, user, view };
}

describe("the short letter", () => {
  it("before writing: who it is for, which recommendations go in, and no file yet; passes the accessibility check", async () => {
    const { view, calls } = await openLetter();
    const to = screen.getByRole("radiogroup", { name: "למי המכתב" });
    expect(within(to).getByRole("radio", { name: "להורים" })).toBeChecked();
    const picks = screen.getByRole("group", { name: "ההמלצות שנכנסות למכתב" });
    expect(within(picks).getAllByRole("checkbox").map((c) => (c as HTMLInputElement).checked)).toEqual([true, true]);
    expect(screen.getByRole("button", { name: "ניסוח המכתב" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "הפקת המכתב" })).toBeNull();
    await waitFor(() => expect(calls.find((c) => c.cmd === "letter")?.args).toMatchObject({ audience: "parents" }));
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("an unmarked recommendation is kept out, and with none marked she is told", async () => {
    const { user, calls } = await openLetter();
    const picks = screen.getByRole("group", { name: "ההמלצות שנכנסות למכתב" });
    const [first, second] = within(picks).getAllByRole("checkbox") as HTMLInputElement[];
    await user.click(first as HTMLInputElement);
    await waitFor(() => expect(first).not.toBeChecked());
    const leftOut = calls.filter((c) => c.cmd === "set_letter_left_out").map((c) => c.args.ids as string[]);
    expect(leftOut[0]).toHaveLength(1);
    // From the keyboard: Space on the second one.
    (second as HTMLInputElement).focus();
    await user.keyboard(" ");
    await waitFor(() => expect(second).not.toBeChecked());
    expect(within(picks).getByText("לא סימנת אף המלצה, אז המכתב ייכתב בלי המלצות.")).toBeInTheDocument();
    // Marked again: back in, and nothing is left out.
    await user.click(first as HTMLInputElement);
    await user.click(second as HTMLInputElement);
    await waitFor(() => expect(calls.filter((c) => c.cmd === "set_letter_left_out").at(-1)?.args.ids).toBeNull());
  });

  it("one recommendation alone needs no choosing", async () => {
    await openLetter(undefined, { recs: [RECS[0] as string] });
    expect(screen.queryByRole("group", { name: "ההמלצות שנכנסות למכתב" })).toBeNull();
  });

  it("writes the letter with her note, then a paragraph is approved and the letter can be produced", async () => {
    const { user, calls } = await openLetter();
    await user.type(screen.getByPlaceholderText("מה עוד חשוב שיהיה במכתב? (לא חובה)"), "בלי מונחים מקצועיים");
    await user.click(screen.getByRole("button", { name: "ניסוח המכתב" }));
    expect(await screen.findByText("פסקה בדויה ראשונה (parents).")).toBeInTheDocument();
    expect(calls.find((c) => c.cmd === "prepare_letter")?.args).toMatchObject({ audience: "parents", note: "בלי מונחים מקצועיים" });
    expect(calls.find((c) => c.cmd === "send_section")?.args).toEqual({ approvalId: "letter-ok" });
    // Nothing approved yet: the file waits.
    const produce = screen.getByRole("button", { name: "הפקת המכתב" });
    expect(produce).toBeDisabled();
    expect(screen.getByRole("button", { name: "ניסוח מחדש" })).toBeEnabled();
    await user.click(screen.getAllByRole("button", { name: "אישור" })[0] as HTMLElement);
    expect(await screen.findByText("✓ מאושר")).toBeInTheDocument();
    expect(produce).toBeEnabled();
    await user.click(produce);
    expect(await screen.findByRole("dialog", { name: "הפקת מכתב Word" })).toBeInTheDocument();
  });

  it("a letter to the school is asked for as such", async () => {
    const { user, calls } = await openLetter();
    await user.click(screen.getByRole("radio", { name: "לצוות החינוכי" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "letter").at(-1)?.args).toMatchObject({ audience: "school" }));
    await user.click(screen.getByRole("button", { name: "ניסוח המכתב" }));
    expect(await screen.findByText("פסקה בדויה ראשונה (school).")).toBeInTheDocument();
    expect(calls.find((c) => c.cmd === "prepare_letter")?.args).toMatchObject({ audience: "school" });
  });

  it("a paragraph is edited in place; Cancel leaves it as it was", async () => {
    const { user, calls, letters, view } = await openLetter();
    letters.parents = [para("l1", "פסקה בדויה לעריכה.")];
    await user.click(screen.getByRole("radio", { name: "לצוות החינוכי" }));
    await user.click(screen.getByRole("radio", { name: "להורים" }));
    await screen.findByText("פסקה בדויה לעריכה.");
    await user.click(screen.getByRole("button", { name: "עריכה" }));
    let box: HTMLElement = screen.getByRole("textbox", { name: "עריכת הפסקה" });
    expect(box).toHaveFocus();
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.type(box, " מילה");
    await user.click(screen.getByRole("button", { name: "ביטול" }));
    expect(screen.getByText("פסקה בדויה לעריכה.")).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === "edit_paragraph")).toBe(false);

    await user.click(screen.getByRole("button", { name: "עריכה" }));
    box = screen.getByRole("textbox", { name: "עריכת הפסקה" });
    await user.clear(box);
    await user.type(box, "פסקה בדויה אחרי עריכה.");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    expect(await screen.findByText("פסקה בדויה אחרי עריכה.")).toBeInTheDocument();
    expect(calls.find((c) => c.cmd === "edit_paragraph")?.args).toMatchObject({ draftId: "l1", text: "פסקה בדויה אחרי עריכה.", openedWith: "פסקה בדויה לעריכה." });
  });

  it("a paragraph changed meanwhile: her text is saved, and she is told the other wording is kept", async () => {
    const { user, letters, app } = await openLetter((cmd) => (cmd === "edit_paragraph" ? true : undefined));
    letters.parents = [para("l1", "פסקה בדויה.")];
    await user.click(screen.getByRole("radio", { name: "לצוות החינוכי" }));
    await user.click(screen.getByRole("radio", { name: "להורים" }));
    await user.click(await screen.findByRole("button", { name: "עריכה" }));
    await user.type(screen.getByRole("textbox", { name: "עריכת הפסקה" }), " נוסף");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith(STALE_EDIT_KEPT));
  });

  it("a letter the check did not let go says why, and nothing is written", async () => {
    const refused = prepared({ approval_id: null, blocked: [{ code: "trap", message: "נשאר בטקסט שם שלא הוסתר.", detail: null }] });
    const { user, calls } = await openLetter((cmd) => (cmd === "prepare_letter" ? refused : undefined));
    await user.click(screen.getByRole("button", { name: "ניסוח המכתב" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("הבקשה לא נשלחה: נשאר בטקסט שם שלא הוסתר.. אפשר לנסות שוב.");
    expect(calls.some((c) => c.cmd === "send_section")).toBe(false);
    expect(screen.getByRole("button", { name: "ניסוח המכתב" })).toBeEnabled();
  });

  it("a failed writing says why in Hebrew, and the button is free again", async () => {
    const { user } = await openLetter((cmd) => {
      if (cmd === "send_section") throw { code: "egress", message: "השליחה לא הצליחה. אפשר לנסות שוב.", details: [], action: { kind: "retry", after_seconds: 0 } };
      return undefined;
    });
    await user.click(screen.getByRole("button", { name: "ניסוח המכתב" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("השליחה לא הצליחה. אפשר לנסות שוב.");
    expect(screen.getByRole("button", { name: "ניסוח המכתב" })).toBeEnabled();
  });

  it("a letter that cannot be read says why", async () => {
    await openLetter((cmd) => {
      if (cmd === "letter") throw { code: "internal", message: "לא הצלחנו לקרוא את המכתב.", details: [] };
      return undefined;
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("לא הצלחנו לקרוא את המכתב.");
  });

  it("with “show me every request” on, the letter waits for her on the review screen", async () => {
    const { user, calls } = await openLetter(undefined, { reviewAlways: true });
    await user.click(screen.getByRole("button", { name: "ניסוח המכתב" }));
    const review = await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" });
    expect(within(review).getByText(/מכתב להורים/)).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === "send_section")).toBe(false);
    await user.click(within(review).getByRole("button", { name: /^שליחה/ }));
    expect(await screen.findByText("פסקה בדויה ראשונה (parents).")).toBeInTheDocument();
  });
});
