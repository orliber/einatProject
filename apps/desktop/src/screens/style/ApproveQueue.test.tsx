// "לאשר אחד-אחד" on its own: the empty queue (nothing read yet, or all approved), one thing at a
// time with the count, editing from the keyboard, a suggestion said yes or no to, a draft with
// nothing new, what she sees when the core refuses, and nothing pressed while a step runs.
// Every item is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { ipc, type StyleOverview, type StyleProfileView, type StyleSuggestion } from "../../ipc/client";
import { a11yViolations } from "../../test/a11y";
import { appApi, Host, serve, type Override } from "../../test/host";
import { ApproveQueue } from "./ApproveQueue";
import { waitingFor } from "./waiting";

configure({ asyncUtilTimeout: 5000 });

const overview = (over: Partial<StyleOverview> = {}): StyleOverview => ({
  sources: [{ id: "s1", title: "דוח בדוי", format: "docx", added_at: 1_790_000_000, words: 900, headings: [], analyzed: true, analysis_demo: false, analysis_items: 2, analysis_inferences: 0 }],
  active: null, draft: null, versions: [], suggestions: [], enabled: true,
  sections: [{ key: "cognitive", title: "פרופיל קוגניטיבי" }],
  demo_mode: true,
  learned: {
    from_reports: 0, from_edits: 0, from_finals: 0, finished_reports: 0,
    words_per_sentence: null, sentences_per_paragraph: null, commas_per_sentence: null,
    hedges: null, contrasts: null, first_person: null, fit_recent: null, fit_earlier: null, fit_count: 0, fit_by_month: [],
  },
  interpretations: [], scored_measures: 0, skeletons: [], omissions: [],
  ...over,
});

const draft: StyleProfileView = {
  id: "p1", version: 2, status: "draft", created_at: 1_790_000_000, dropped: 0, demo: false,
  profile: {
    reports: 3,
    items: [
      { id: "a", section: null, kind: "rule", text: "גוף נסתר וזמן הווה", enabled: true, origin: "reports", support: 3 },
      { id: "b", section: "cognitive", kind: "template", text: "הציג תפקוד {רמה}", enabled: true, origin: "reports", support: 2 },
    ],
  },
};

const suggestion = (over: Partial<StyleSuggestion> = {}): StyleSuggestion => ({
  id: "g1", kind: "replace", from: "מפגין", to: "מראה", count: 4, confidence: 80, section: null, ...over,
});

/** The queue as the style page holds it: the overview from the core, and what she already answered. */
function Page({ running = false, buildDue = false }: { running?: boolean; buildDue?: boolean }) {
  const o = useQuery({ queryKey: ["style"], queryFn: ipc.styleOverview }).data;
  const [reviewed, setReviewed] = useState<string[]>([]);
  if (!o) return null;
  return <ApproveQueue o={o} asks={waitingFor(o, reviewed)} onReviewed={(id) => setReviewed((r) => [...r, id])} running={running} buildDue={buildDue} />;
}

async function openQueue(first: StyleOverview, over: Override = () => undefined, props: { running?: boolean; buildDue?: boolean } = {}) {
  let current = first;
  const served = await serve(async (cmd, args, core) => {
    const out = await over(cmd, args, core);
    if (out !== undefined) return out;
    switch (cmd) {
      case "style_overview":
        return current;
      case "save_style_draft":
        current = { ...current, draft: current.draft && { ...current.draft, profile: args.profile as StyleProfileView["profile"] } };
        return current.draft;
      case "approve_style_draft":
        current = { ...current, active: current.draft && { ...current.draft, status: "active" }, draft: null };
        return current.active;
      case "discard_style_draft":
        current = { ...current, draft: null };
        return null;
      case "accept_style_suggestion":
      case "dismiss_style_suggestion":
        current = { ...current, suggestions: current.suggestions.filter((s) => s.id !== args.id) };
        return null;
      default:
        return undefined;
    }
  });
  const app = appApi();
  const user = userEvent.setup();
  const view = render(<Host app={app}><Page {...props} /></Host>);
  return { ...served, app, user, view };
}

const queue = () => screen.getByRole("region", { name: /לאשר אחד-אחד|הכול מאושר|ללמוד מהדוחות/ });

describe("the approval queue", () => {
  it("before anything is read: where the learning starts, and nothing to press", async () => {
    await openQueue(overview({ sources: [] }), undefined, { buildDue: true });
    expect(await screen.findByRole("heading", { name: "ללמוד מהדוחות" })).toBeInTheDocument();
    expect(queue()).toHaveTextContent("מומלץ");
    expect(within(queue()).queryByRole("button")).toBeNull();
  });

  it("all approved: says which version is in use", async () => {
    await openQueue(overview({ active: { ...draft, id: "p0", status: "active", version: 3 } }));
    expect(await screen.findByRole("heading", { name: "הכול מאושר" })).toBeInTheDocument();
    expect(queue()).toHaveTextContent("הפרופיל בשימוש, גרסה 3.");
  });

  it("one thing at a time, with how far she is; passes the accessibility check", async () => {
    const { view } = await openQueue(overview({ draft }));
    const q = await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    expect(q).toHaveTextContent("1 מתוך 2");
    expect(within(q).getByRole("progressbar", { name: "כמה כבר אישרת" })).toHaveAttribute("aria-valuenow", "0");
    expect(within(q).getByText("גוף נסתר וזמן הווה")).toBeInTheDocument();
    for (const name of ["להשתמש בזה", "לערוך", "לא צריך", "להשתמש בכל 2 שנשארו"]) expect(within(q).getByRole("button", { name })).toBeEnabled();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("editing from the keyboard: Escape leaves the item as it was, Ctrl+Enter saves and uses it", async () => {
    const { user, calls } = await openQueue(overview({ draft }));
    const q = await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    await user.click(within(q).getByRole("button", { name: "לערוך" }));
    let box = within(q).getByRole("textbox", { name: "הנוסח" });
    expect(box).toHaveFocus();
    await user.type(box, " תמיד");
    await user.keyboard("{Escape}");
    expect(within(q).getByText("גוף נסתר וזמן הווה")).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === "save_style_draft")).toBe(false);

    await user.click(within(q).getByRole("button", { name: "לערוך" }));
    box = within(q).getByRole("textbox", { name: "הנוסח" });
    await user.clear(box);
    // Nothing to save while the box is empty.
    expect(within(q).getByRole("button", { name: "שמירה ושימוש" })).toBeDisabled();
    await user.type(box, "גוף שלישי וזמן הווה");
    await user.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(q).toHaveTextContent("2 מתוך 2"));
    const saved = calls.find((c) => c.cmd === "save_style_draft")?.args.profile as StyleProfileView["profile"];
    expect(saved.items.find((i) => i.id === "a")).toMatchObject({ text: "גוף שלישי וזמן הווה", enabled: true });
  });

  it("the last item decided puts the profile in use, and she is told", async () => {
    const { user, app } = await openQueue(overview({ draft: { ...draft, profile: { ...draft.profile, items: [draft.profile.items[0] as StyleProfileView["profile"]["items"][number]] } } }));
    const q = await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    await user.click(within(q).getByRole("button", { name: "להשתמש בזה" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("הפרופיל אושר. מעכשיו Claude כותב בסגנון שלך."));
    expect(await screen.findByRole("heading", { name: "הכול מאושר" })).toBeInTheDocument();
  });

  it("saying no to every item drops the draft, and she is told", async () => {
    const one = { ...draft, profile: { ...draft.profile, items: [draft.profile.items[0] as StyleProfileView["profile"]["items"][number]] } };
    const { user, app, calls } = await openQueue(overview({ draft: one }));
    const q = await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    await user.click(within(q).getByRole("button", { name: "לא צריך" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("לא נשאר פריט לשימוש, והטיוטה נמחקה."));
    expect(calls.map((c) => c.cmd)).toContain("discard_style_draft");
    expect(calls.map((c) => c.cmd)).not.toContain("approve_style_draft");
  });

  it("a draft with nothing new asks once, for the whole version", async () => {
    const { user, calls } = await openQueue(overview({ draft, active: { ...draft, id: "p0", status: "active", version: 1 } }));
    const q = await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    expect(q).toHaveTextContent("הפרופיל המעודכן");
    expect(q).toHaveTextContent("2 פריטים, כולם כבר אושרו בגרסה הקודמת.");
    expect(within(q).queryByRole("button", { name: "לערוך" })).toBeNull();
    await user.click(within(q).getByRole("button", { name: "להשתמש בזה" }));
    await waitFor(() => expect(calls.map((c) => c.cmd)).toContain("approve_style_draft"));
  });

  it("a word she always changes: said in her terms, added on her yes, gone on her no", async () => {
    const { user, calls, app } = await openQueue(overview({ suggestions: [suggestion(), suggestion({ id: "g2", kind: "opening", from: "בפגישה הראשונה", count: 1 })] }));
    const q = await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    expect(q).toHaveTextContent("מילה שאת מחליפה");
    expect(q).toHaveTextContent("במקום מפגין את כותבת מראה");
    expect(q).toHaveTextContent("שינית את זה 4 פעמים.");
    // A suggestion has no wording of its own to edit.
    expect(within(q).queryByRole("button", { name: "לערוך" })).toBeNull();
    await user.click(within(q).getByRole("button", { name: "להשתמש בזה" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("נוסף לפרופיל."));
    expect(calls.find((c) => c.cmd === "accept_style_suggestion")?.args).toEqual({ id: "g1" });
    await waitFor(() => expect(q).toHaveTextContent("איך את פותחת סעיף"));
    expect(q).toHaveTextContent("בפגישה הראשונה…");
    expect(q).toHaveTextContent("מופיע ב-1 מתוך 1 הדוחות שלך.");
    await user.click(within(q).getByRole("button", { name: "לא צריך" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "dismiss_style_suggestion")?.args).toEqual({ id: "g2" }));
  });

  it("a refusal from the core says why, in Hebrew, and the item stays", async () => {
    const { user } = await openQueue(overview({ suggestions: [suggestion()] }), (cmd) => {
      if (cmd === "accept_style_suggestion") throw { code: "internal", message: "הפריט לא נשמר בפרופיל.", details: [] };
      return undefined;
    });
    const q = await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    await user.click(within(q).getByRole("button", { name: "להשתמש בזה" }));
    expect(await within(q).findByRole("alert")).toHaveTextContent("הפריט לא נשמר בפרופיל.");
    expect(q).toHaveTextContent("מילה שאת מחליפה");
    expect(within(q).getByRole("button", { name: "להשתמש בזה" })).toBeEnabled();
  });

  it("while the analysis runs nothing can be decided", async () => {
    await openQueue(overview({ draft }), undefined, { running: true });
    const q = await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    for (const name of ["להשתמש בזה", "לערוך", "לא צריך", "להשתמש בכל 2 שנשארו"]) expect(within(q).getByRole("button", { name })).toBeDisabled();
  });
});
