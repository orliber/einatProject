import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { StyleImportDialog } from "../components/StyleImportDialog";
import type { StyleOverview, StyleProfileView } from "../ipc/client";
import { a11yViolations } from "../test/a11y";
import { prepared, status } from "../test/fixtures";
import { chainLinks, sinceLabel, StyleScreen } from "./StyleScreen";
import { START_STAGES } from "./style/StyleStart";

function api(): AppApi {
  // Every step through the review screen: "show me every request" on (off by default; set here for the tests that read it).
  return { status: status({ review_always: true }), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

function wrap(node: ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api()}>{node}</AppContext.Provider>
    </QueryClientProvider>,
  );
}

const overview = (over: Partial<StyleOverview> = {}): StyleOverview => ({
  sources: [],
  active: null,
  draft: null,
  versions: [],
  suggestions: [],
  enabled: true,
  sections: [{ key: "cognitive", title: "פרופיל קוגניטיבי" }, { key: "summary", title: "סיכום" }],
  demo_mode: true,
  learned: {
    from_reports: 0, from_edits: 0, from_finals: 0, finished_reports: 0,
    words_per_sentence: null, sentences_per_paragraph: null, commas_per_sentence: null,
    hedges: null, contrasts: null, first_person: null, fit_recent: null, fit_earlier: null, fit_count: 0, fit_by_month: [],
  },
  interpretations: [],
  scored_measures: 0,
  skeletons: [],
  omissions: [],
  ...over,
});

const source = { id: "s1", title: "דוח א", format: "docx", added_at: 1_790_000_000, words: 900, headings: ["סיכום"], analyzed: false, analysis_demo: false, analysis_items: 0, analysis_inferences: 0 };

const draft: StyleProfileView = {
  id: "p1", version: 1, status: "draft", created_at: 1_790_000_000, dropped: 1, demo: false,
  profile: {
    reports: 3,
    items: [
      { id: "a", section: null, kind: "rule", text: "גוף נסתר וזמן הווה", enabled: true, origin: "reports", support: 3 },
      { id: "b", section: "cognitive", kind: "template", text: "הציג תפקוד {רמה}", enabled: true, origin: "reports", support: 2 },
    ],
  },
};

const waiting = {
  instrument_name: "WPPSI-IV", measure_name: "קידוד חיות",
  item: { id: "i1", instrument: "wppsi_iv", measure: "animal_coding", case: "ממוצע נמוך", text: "קושי במהירות העבודה, לא בהבנה.", support: 3, approved: false, enabled: true, edited: false },
};

const active: StyleProfileView = {
  ...draft, id: "p0", status: "active",
  profile: {
    reports: 3,
    items: [
      { id: "a", section: null, kind: "rule", text: "גוף נסתר וזמן הווה", enabled: true, origin: "reports", support: 3 },
      { id: "c", section: "summary", kind: "template", text: "לסיכום, {שם} מציג {תחום}", enabled: true, origin: "reports", support: 2 },
    ],
  },
};

const queue = () => screen.getByRole("region", { name: "לאשר אחד-אחד" });
const quote = () => queue().querySelector("blockquote")?.textContent ?? queue().querySelector("textarea")?.value;

describe("StyleScreen (round 46)", () => {
  it("starts with one action: adding a past report, and no numbers", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview() : null));
    wrap(<StyleScreen />);
    expect(await screen.findByRole("button", { name: /הוספת דוח ישן/ })).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "במספרים" })).not.toBeInTheDocument();
    expect(screen.getAllByRole("heading")).toHaveLength(1);
  });

  it("analyzes each report, then builds the profile, each through the review screen", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "style_overview") return overview({ sources: [source] });
      if (cmd === "prepare_style_analysis" || cmd === "prepare_style_profile") return prepared({ approval_id: cmd });
      if (cmd === "send_style_analysis") return { source_id: "s1", kept: 4, dropped: 0, demo: true };
      if (cmd === "send_style_profile") return draft;
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await user.click(await screen.findByRole("button", { name: /ניתוח הדוח ובניית הפרופיל/ }));
    expect(await screen.findByText(/ניתוח סגנון · דוח א/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /שליחה/ }));
    expect(await screen.findByText(/בניית פרופיל הסגנון ·/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /שליחה/ }));
    // The profile is put in use as it came (D-138); items are turned off later, not approved one by one.
    await vi.waitFor(() => expect(calls).toContain("approve_style_draft"));
    const order = calls.filter((c) => c !== "style_overview" && c !== "send_progress");
    expect(order).toEqual(["prepare_style_analysis", "send_style_analysis", "prepare_style_profile", "send_style_profile", "approve_style_draft"]);
  });

  it("three numbers, and a draft is approved one item at a time: the last one puts the profile in use", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      if (cmd === "style_overview") return overview({ sources: [{ ...source, analyzed: true }], draft });
      calls.push([cmd, args]);
      if (cmd === "save_style_draft") return draft;
      if (cmd === "approve_style_draft") return { ...draft, status: "active" };
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    const journey = await screen.findByRole("list", { name: "המסלול" });
    expect(journey).toHaveTextContent("הדוחות שלידוח אחד");
    expect(journey).toHaveTextContent("ניתוחנותח");
    expect(journey).toHaveTextContent("אישור2 מחכים לך");
    expect(within(journey).getByRole("button", { current: "step" })).toHaveTextContent("אישור");
    expect(journey).toHaveTextContent("בשימוש0 דברים");
    expect(queue()).toHaveTextContent("1 מתוך 2");
    expect(queue()).toHaveTextContent("כלל כתיבה");
    expect(quote()).toBe("גוף נסתר וזמן הווה");
    expect(queue()).toHaveTextContent("מופיע ב-3 מתוך 3 הדוחות שלך, בכל הדוח.");
    // Two clicks from the tab: the first item is already on the screen.
    await user.click(within(queue()).getByRole("button", { name: "להשתמש בזה" }));
    await vi.waitFor(() => expect(quote()).toBe("הציג תפקוד {רמה}"));
    expect(calls).toEqual([]);
    expect(queue()).toHaveTextContent("2 מתוך 2");
    await user.click(within(queue()).getByRole("button", { name: "לא צריך" }));
    await vi.waitFor(() => expect(calls.map(([c]) => c)).toEqual(["save_style_draft", "approve_style_draft"]));
    const profile = (calls[0]?.[1] as { profile: StyleProfileView["profile"] }).profile;
    expect(profile.items.find((i) => i.id === "b")?.enabled).toBe(false);
    expect(profile.items.find((i) => i.id === "a")?.enabled).toBe(true);
  });

  it("the rest of the draft goes into use with one click, without touching what she said no to", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      if (cmd === "style_overview") return overview({ sources: [{ ...source, analyzed: true }], draft });
      calls.push(cmd);
      if (cmd === "approve_style_draft") return { ...draft, status: "active" };
      return null;
    });
    // What she said about an earlier draft's items is remembered per draft; this test starts afresh.
    localStorage.removeItem("dv.style.reviewed");
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    expect(queue()).toHaveTextContent("1 מתוך 2");
    await user.click(within(queue()).getByRole("button", { name: "להשתמש בכל 2 שנשארו" }));
    await vi.waitFor(() => expect(calls.filter((c) => c !== "check_update")).toEqual(["approve_style_draft"]));
  });

  it("asks only about what the profile in use does not hold yet", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview({ sources: [{ ...source, analyzed: true }], draft, active }) : null));
    wrap(<StyleScreen />);
    await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    expect(queue()).toHaveTextContent("1 מתוך 1");
    expect(quote()).toBe("הציג תפקוד {רמה}");
  });

  it("the whole draft can still be gone over at once", async () => {
    const saved: unknown[] = [];
    let approved = false;
    mockIPC((cmd, args) => {
      if (cmd === "style_overview") return overview({ sources: [{ ...source, analyzed: true }], draft });
      if (cmd === "save_style_draft") { saved.push(args); return draft; }
      if (cmd === "approve_style_draft") { approved = true; return { ...draft, status: "active" }; }
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await user.click(await screen.findByRole("button", { name: "לעבור על כל הטיוטה בבת אחת" }));
    expect(screen.getByText("טיוטה · עוד לא בשימוש")).toBeInTheDocument();
    expect(screen.getByText(/1 פריטים נפסלו/)).toBeInTheDocument();
    const [, template] = screen.getAllByRole("checkbox", { name: "להשתמש בפריט" });
    if (!template) throw new Error("the template's checkbox");
    await user.click(template);
    await user.click(screen.getByRole("button", { name: "אישור הפרופיל" }));
    await vi.waitFor(() => expect(approved).toBe(true));
    const profile = (saved[0] as { profile: StyleProfileView["profile"] }).profile;
    expect(profile.items.find((i) => i.id === "b")?.enabled).toBe(false);
  });

  it("an interpretation is edited in the queue and used, or kept off when not needed", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      if (cmd === "style_overview") return overview({ sources: [{ ...source, analyzed: true }], active, scored_measures: 4, interpretations: [waiting] });
      calls.push([cmd, args]);
      if (cmd === "update_interpretation") return { ...waiting, item: { ...waiting.item, approved: true } };
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    expect(queue()).toHaveTextContent("איך את מפרשת ציון");
    expect(queue()).toHaveTextContent("WPPSI-IV · קידוד חיות, ממוצע נמוך. כך כתבת ב-3 דוחות.");
    await user.click(within(queue()).getByRole("button", { name: "לערוך" }));
    const box = within(queue()).getByRole("textbox", { name: "הנוסח" });
    await user.clear(box);
    await user.type(box, "מהירות עבודה איטית");
    await user.click(within(queue()).getByRole("button", { name: "שמירה ושימוש" }));
    await vi.waitFor(() => expect(calls).toContainEqual(["update_interpretation", { id: "i1", text: "מהירות עבודה איטית", approved: true, enabled: null }]));
    await user.click(await within(queue()).findByRole("button", { name: "לא צריך" }));
    await vi.waitFor(() => expect(calls).toContainEqual(["update_interpretation", { id: "i1", text: null, approved: true, enabled: false }]));
  });

  it("learns how she interprets scores from the row that holds them, through the review screen", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "style_overview") return overview({ sources: [{ ...source, analyzed: true }], active, scored_measures: 4, interpretations: [waiting] });
      if (cmd === "prepare_interpretations") return prepared({ approval_id: "i" });
      if (cmd === "send_interpretations") return { kept: 1, dropped: 0, demo: true };
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await user.click(await screen.findByRole("button", { name: /איך את מפרשת ציונים/ }));
    const row = screen.getByRole("region", { name: "איך את מפרשת ציונים" });
    expect(within(row).getByRole("group", { name: "WPPSI-IV · קידוד חיות" })).toHaveTextContent("מחכה לאישור");
    expect(within(row).getByText(/4 תת-מבחנים עם תוצאות/)).toBeInTheDocument();
    await user.click(within(row).getByRole("button", { name: "לקרוא שוב את הדוחות" }));
    expect(await screen.findByText(/איך פירשת תוצאות ·/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /שליחה/ }));
    await vi.waitFor(() => expect(calls).toContain("send_interpretations"));
  });

  it("what the program knows opens in place, one row at a time, and edits keep the rest of the profile", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      if (cmd === "style_overview") return overview({ sources: [{ ...source, analyzed: true }], active });
      calls.push([cmd, args]);
      if (cmd === "save_style_draft") return { ...active, status: "draft" };
      if (cmd === "approve_style_draft") return active;
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    expect(await screen.findByRole("heading", { name: "הכול מאושר" })).toBeInTheDocument();
    expect(screen.getByText(/הפרופיל בשימוש, גרסה 1/)).toBeInTheDocument();
    const phrases = screen.getByRole("button", { name: /משפטים שחוזרים אצלך/ });
    expect(phrases).toHaveTextContent("1");
    await user.click(phrases);
    const row = screen.getByRole("region", { name: "משפטים שחוזרים אצלך" });
    expect(within(row).getByText("לסיכום, {שם} מציג {תחום}")).toBeInTheDocument();
    expect(within(row).queryByText("גוף נסתר וזמן הווה")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /איך את כותבת/ }));
    expect(screen.queryByRole("region", { name: "משפטים שחוזרים אצלך" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /משפטים שחוזרים אצלך/ }));
    await user.click(within(screen.getByRole("region", { name: "משפטים שחוזרים אצלך" })).getByRole("checkbox", { name: "להשתמש בפריט" }));
    await user.click(screen.getByRole("button", { name: "אישור הפרופיל" }));
    await vi.waitFor(() => expect(calls.map(([c]) => c)).toEqual(["save_style_draft", "approve_style_draft"]));
    const profile = (calls[0]?.[1] as { profile: StyleProfileView["profile"] }).profile;
    expect(profile.items.find((i) => i.id === "a")?.enabled).toBe(true);
    expect(profile.items.find((i) => i.id === "c")?.enabled).toBe(false);
  });

  it("the profile is used in drafting from one switch that saves at once", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "style_overview") return overview({ sources: [{ ...source, analyzed: true }], active });
      calls.push([cmd, args]);
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await user.click(await screen.findByRole("switch", { name: "להשתמש במה שאישרת בכל ניסוח" }));
    await vi.waitFor(() => expect(calls).toContainEqual(["set_style_enabled", { on: false }]));
  });

  it("her reports are one line, with the list behind a button", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview({ sources: [{ ...source, added_at: Math.floor(Date.now() / 1000) - 8 * 86_400 }] }) : null));
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    const reports = await screen.findByRole("region", { name: "הדוחות שלי" });
    expect(reports).toHaveTextContent("דוח אחד, האחרון נוסף לפני שבוע. אחד עוד לא נותח.");
    expect(within(reports).queryByText("דוח א")).not.toBeInTheDocument();
    await user.click(within(reports).getByRole("button", { name: "לראות את הרשימה" }));
    expect(within(reports).getByText("דוח א")).toBeInTheDocument();
  });
});

describe("sinceLabel", () => {
  const now = Date.UTC(2026, 9, 15);
  const ago = (days: number) => now / 1000 - days * 86_400;
  it("says when in words", () => {
    expect(sinceLabel(ago(0), now)).toBe("היום");
    expect(sinceLabel(ago(1), now)).toBe("אתמול");
    expect(sinceLabel(ago(3), now)).toBe("לפני 3 ימים");
    expect(sinceLabel(ago(9), now)).toBe("לפני שבוע");
    expect(sinceLabel(ago(22), now)).toBe("לפני 3 שבועות");
    expect(sinceLabel(ago(60), now)).toMatch(/^ב-/);
  });
});

describe("what the program measured", () => {
  it("shows her usual figures and how close the drafts were, and can forget the learning", async () => {
    const calls: string[] = [];
    const learned = {
      ...overview().learned,
      from_reports: 40, from_edits: 3, finished_reports: 1, from_finals: 12,
      words_per_sentence: 13.4, sentences_per_paragraph: 3.2, commas_per_sentence: 1.1,
      hedges: 22, contrasts: 9, first_person: 4, fit_recent: 81, fit_earlier: 72, fit_count: 60,
      fit_by_month: [{ month: "2026-09", average: 70, count: 20 }, { month: "2026-10", average: 81, count: 40 }],
    };
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "style_overview") return overview({ sources: [source], learned });
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await user.click(await screen.findByRole("button", { name: /איך את כותבת/ }));
    expect(screen.getByText("בממוצע 13 מילים במשפט")).toBeInTheDocument();
    expect(screen.getByText(/40 פסקאות מהדוחות הישנים, 3 מהתיקונים שלך, דוח אחד שסיימת בתוכנה/)).toBeInTheDocument();
    expect(screen.getByText("81 מתוך 100")).toBeInTheDocument();
    expect(screen.getByText(/לעומת 72/)).toBeInTheDocument();
    // ST12: by month, each bar named in words for a screen reader.
    expect(screen.getByLabelText("ספטמבר 2026: 70 (20 פסקאות)")).toBeInTheDocument();
    expect(screen.getByLabelText("אוקטובר 2026: 81 (40 פסקאות)")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "עוד פעולות על הסגנון" }));
    await user.click(await screen.findByRole("menuitem", { name: /לשכוח את מה שנלמד/ }));
    await user.click(screen.getByRole("button", { name: "לשכוח" }));
    await vi.waitFor(() => expect(calls).toContain("forget_style_learning"));
  });

  it("says nothing before there is anything to measure", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview() : null));
    wrap(<StyleScreen />);
    await screen.findByRole("button", { name: /הוספת דוח ישן/ });
    expect(screen.queryByText("מה התוכנה מדדה בכתיבה שלך")).not.toBeInTheDocument();
  });
});

describe("suggestions from her reports", () => {
  it("asks about how she opens and closes a section, one at a time, and adds it on her yes", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "style_overview") {
        return overview({
          sources: [source],
          suggestions: [
            { id: "shape:opening:summary:x", kind: "opening", from: "לסיכום, נראה כי", to: "", count: 4, confidence: 80, section: "summary" },
            { id: "shape:closing:summary:y", kind: "closing", from: "מומלץ להמשיך", to: "", count: 3, confidence: 60, section: "summary" },
          ],
        });
      }
      calls.push([cmd, args]);
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    expect(queue()).toHaveTextContent("איך את פותחת סעיף");
    expect(quote()).toBe("לסיכום, נראה כי…");
    expect(queue()).toHaveTextContent("מופיע ב-4 מהדוחות שלך, בסעיף סיכום.");
    expect(within(queue()).queryByRole("button", { name: "לערוך" })).not.toBeInTheDocument();
    await user.click(within(queue()).getByRole("button", { name: "להשתמש בזה" }));
    await vi.waitFor(() => expect(calls).toContainEqual(["accept_style_suggestion", { id: "shape:opening:summary:x" }]));
    // The report not read yet waits in the journey's analysis stage, not over the queue (D-100).
    expect(screen.getByRole("list", { name: "המסלול" })).toHaveTextContent("ניתוח הדוח ובניית הפרופיל");
    expect(queue()).not.toHaveTextContent("ניתוח הדוח ובניית הפרופיל");
  });
});

describe("what was learned from her reports (round 34)", () => {
  it("asks about her fixed wording, and shows the order of her sections and what she leaves out", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "style_overview") {
        return overview({
          sources: [source],
          learned: { ...overview().learned, from_reports: 30 },
          suggestions: [
            { id: "formula:x", kind: "formula", from: "מנת המשכל הכללית נמצאה {תוצאה}.", to: "", count: 3, confidence: 100, section: "cognitive" },
          ],
          skeletons: [{ section: "parents_view", title: "תפקוד כיום בבית", band: "גיל שלוש עד שש", reports: 4, topics: ["משחק", "רגשי"] }],
          omissions: ["את לא כותבת אחוזונים ליד התוצאות של תתי-המבחנים, ולכן הם לא נכתבים בדף התוצאות שנשלח."],
        });
      }
      calls.push([cmd, args]);
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    await screen.findByRole("region", { name: "לאשר אחד-אחד" });
    expect(queue()).toHaveTextContent("משפט שחוזר אצלך");
    expect(queue()).toHaveTextContent("מופיע ב-3 מהדוחות שלך, בסעיף פרופיל קוגניטיבי.");
    await user.click(screen.getByRole("button", { name: /סדר הנושאים בכל סעיף/ }));
    expect(screen.getByText("תפקוד כיום בבית (גיל שלוש עד שש):")).toBeInTheDocument();
    expect(screen.getByText(/משחק · רגשי/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /איך את כותבת/ }));
    expect(screen.getByText(/את לא כותבת אחוזונים/)).toBeInTheDocument();
    await user.click(within(queue()).getByRole("button", { name: "להשתמש בזה" }));
    await vi.waitFor(() => expect(calls).toContainEqual(["accept_style_suggestion", { id: "formula:x" }]));
  });
});

describe("StyleImportDialog", () => {
  it("shows hidden details as roles and keeps only the parts she chose", async () => {
    const saved: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "save_style_source") { saved.push(args); return source; }
      return null;
    });
    const onSaved = vi.fn();
    const user = userEvent.setup();
    wrap(
      <StyleImportDialog
        preview={{
          token: "t1", title: "ילד · אבחון", format: "docx", hidden: 7, numbers: 3, warnings: [],
          parts: [
            { index: 0, section: null, heading: "פתיחת הדוח", text: "[ילד] [אדם_1] · [תאריך]", words: 3, included: false },
            { index: 1, section: "summary", heading: "סיכום", text: "לסיכום, [ילד] הוא ילד סקרן ([מספר]).", words: 6, included: true },
          ],
        }}
        sectionTitle={(k) => (k === "summary" ? "סיכום" : "לא זוהה סעיף")}
        onClose={vi.fn()} onSaved={onSaved} />,
    );
    expect(screen.getByText("הוסתרו 7 פרטים מזהים")).toBeInTheDocument();
    expect(screen.queryByText(/\[ילד\]/)).not.toBeInTheDocument();
    expect(screen.getAllByTitle("הוסתר").length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "שמירת הקטעים שסומנו" }));
    await vi.waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(saved[0]).toMatchObject({ token: "t1", included: [1], title: null });
  });
});

describe("chainLinks (D-100)", () => {
  it("keeps a score pattern's colon inside the head's parentheses", () => {
    const text = "בתוצאות (המדד הנמוך ביותר: מהירות עיבוד): נמוך משאר המדדים ← מסקנה: קצב עבודה איטי ← ניסוח: {מדד} נמוך";
    expect(chainLinks(text)).toEqual([
      { label: "בתוצאות (המדד הנמוך ביותר: מהירות עיבוד)", body: "נמוך משאר המדדים" },
      { label: "מסקנה", body: "קצב עבודה איטי" },
      { label: "ניסוח", body: "{מדד} נמוך" },
    ]);
    expect(chainLinks("בלי תווית")).toEqual([{ label: "", body: "בלי תווית" }]);
  });
});

describe("before the first report (D3 / D-112)", () => {
  const path = () => screen.getByRole("list", { name: "ארבעת השלבים" });

  it("draws the four stages in order, each with its number and its words, and starts at the first", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview() : null));
    wrap(<StyleScreen />);
    await screen.findByRole("button", { name: /הוספת דוח ישן/ });
    const steps = within(path()).getAllByRole("listitem");
    expect(steps.map((li) => li.querySelector("strong")?.textContent)).toEqual([
      "שלב 1: הדוח נכנס", "שלב 2: הדוח נקרא", "שלב 3: את מאשרת", "שלב 4: בשימוש",
    ]);
    // Where she stands is said in words and in aria-current, not by colour alone.
    expect(steps[0]).toHaveAttribute("aria-current", "step");
    expect(steps[0]).toHaveTextContent("מתחילים כאן");
    expect(steps.slice(1).every((li) => !li.hasAttribute("aria-current"))).toBe(true);
    // D-138: what is learned is used at once; the path does not promise otherwise.
    expect(steps[2]).not.toHaveTextContent("שום דבר לא נכנס לשימוש בלי האישור שלך");
    expect(steps[2]).toHaveTextContent("לערוך, לכבות או למחוק");
    // The lead names a place that is on the page ("מה התוכנה יודעת עלייך").
    expect(screen.getByText(/מעלים דוח, והשאר קורה לבד/)).toHaveTextContent('ב"מה התוכנה יודעת עלייך"');
    // Still one heading and one action: the path explains, the button starts.
    expect(screen.getAllByRole("heading")).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: /הוספת דוח|להשתמש|לערוך|לא צריך/ })).toHaveLength(1);
  });

  it("shows a small invented example of what is learned, marked as invented, with the name hidden", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview() : null));
    wrap(<StyleScreen />);
    await screen.findByRole("button", { name: /הוספת דוח ישן/ });
    expect(screen.getByText("דוגמה בדויה")).toBeInTheDocument();
    expect(path()).toHaveTextContent("[ילד]");
    expect(path()).toHaveTextContent("עוזב משימה באמצע ← קושי בהתמדה");
    expect(path()).toHaveTextContent("ניכר כי קיים קושי בהתמדה");
    expect(screen.getByText(/מומלץ להוסיף 3 דוחות לפחות/)).toBeInTheDocument();
    expect(START_STAGES).toHaveLength(4);
  });

  it("the button opens the file picker", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview() : null));
    const user = userEvent.setup();
    const { container } = wrap(<StyleScreen />);
    const add = await screen.findByRole("button", { name: /הוספת דוח ישן/ });
    const input = container.querySelector<HTMLInputElement>('input[type="file"]');
    if (!input) throw new Error("no file input");
    const click = vi.spyOn(input, "click").mockImplementation(() => undefined);
    await user.click(add);
    expect(click).toHaveBeenCalledOnce();
  });

  it("passes the accessibility check", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview() : null));
    wrap(<StyleScreen />);
    await screen.findByRole("button", { name: /הוספת דוח ישן/ });
    expect(await a11yViolations(document.body)).toEqual([]);
  });

  it("goes away once a report is in: the journey takes its place", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview({ sources: [source] }) : null));
    wrap(<StyleScreen />);
    expect(await screen.findByRole("list", { name: "המסלול" })).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "ארבעת השלבים" })).not.toBeInTheDocument();
    expect(screen.queryByText("דוגמה בדויה")).not.toBeInTheDocument();
  });
});
