import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { StyleImportDialog } from "../components/StyleImportDialog";
import type { StyleOverview, StyleProfileView } from "../ipc/client";
import { prepared, status } from "../test/fixtures";
import { StyleScreen } from "./StyleScreen";

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

const source = { id: "s1", title: "דוח א", format: "docx", added_at: 1_790_000_000, words: 900, headings: ["סיכום"], analyzed: false, analysis_demo: false, analysis_items: 0 };

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

describe("StyleScreen", () => {
  it("learns how she interprets scores, through the review screen, and waits for her approval", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "style_overview") return overview({ sources: [{ ...source, analyzed: true }], scored_measures: 4, interpretations: [waiting] });
      if (cmd === "prepare_interpretations") return prepared({ approval_id: "i" });
      if (cmd === "send_interpretations") return { kept: 1, dropped: 0, demo: true };
      if (cmd === "update_interpretation") return { ...waiting, item: { ...waiting.item, approved: true } };
      return null;
    });
    const user = userEvent.setup();
    wrap(<StyleScreen />);
    const group = await screen.findByRole("group", { name: "WPPSI-IV · קידוד חיות" });
    expect(group).toHaveTextContent("מחכה לאישור");
    expect(screen.getByText(/4 תת-מבחנים עם תוצאות/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "לקרוא שוב את הדוחות" }));
    expect(await screen.findByText(/איך פירשת תוצאות ·/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /שליחה/ }));
    await vi.waitFor(() => expect(calls.map(([c]) => c)).toContain("send_interpretations"));
    await user.click(screen.getByRole("button", { name: "אישור" }));
    await vi.waitFor(() => expect(calls).toContainEqual(["update_interpretation", { id: "i1", text: null, approved: true, enabled: null }]));
  });

  it("starts with one action: adding a past report", async () => {
    mockIPC((cmd) => (cmd === "style_overview" ? overview() : null));
    wrap(<StyleScreen />);
    expect(await screen.findByRole("button", { name: /הוספת דוח ישן/ })).toBeInTheDocument();
    expect(screen.getByText(/עוד אין דוחות/)).toBeInTheDocument();
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
    await vi.waitFor(() => expect(calls).toContain("send_style_profile"));
    const order = calls.filter((c) => c !== "style_overview" && c !== "send_progress");
    expect(order).toEqual(["prepare_style_analysis", "send_style_analysis", "prepare_style_profile", "send_style_profile"]);
  });

  it("a draft is edited and approved: only what is checked goes in", async () => {
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
    expect(await screen.findByText("טיוטה · עוד לא בשימוש")).toBeInTheDocument();
    expect(screen.getByText(/1 פריטים נפסלו/)).toBeInTheDocument();
    const [, template] = screen.getAllByRole("checkbox", { name: "להשתמש בפריט" });
    if (!template) throw new Error("the template's checkbox");
    await user.click(template);
    await user.click(screen.getByRole("button", { name: "אישור הפרופיל" }));
    await vi.waitFor(() => expect(approved).toBe(true));
    const profile = (saved[0] as { profile: StyleProfileView["profile"] }).profile;
    expect(profile.items.find((i) => i.id === "b")?.enabled).toBe(false);
    expect(profile.items.find((i) => i.id === "a")?.enabled).toBe(true);
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
    expect(await screen.findByText("בממוצע 13 מילים במשפט")).toBeInTheDocument();
    expect(screen.getByText(/40 פסקאות מהדוחות הישנים, 3 מהתיקונים שלך, דוח אחד שסיימת בתוכנה/)).toBeInTheDocument();
    expect(screen.getByText("81 מתוך 100")).toBeInTheDocument();
    expect(screen.getByText(/לעומת 72/)).toBeInTheDocument();
    // ST12: by month, each bar named in words for a screen reader.
    expect(screen.getByLabelText("ספטמבר 2026: 70 (20 פסקאות)")).toBeInTheDocument();
    expect(screen.getByLabelText("אוקטובר 2026: 81 (40 פסקאות)")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /לשכוח את מה שנלמד/ }));
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
  it("offers how she opens and closes a section, and adds it on her yes", async () => {
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
    expect(await screen.findByText("לסיכום, נראה כי…")).toBeInTheDocument();
    expect(screen.getByText(/כך את פותחת את הסעיף בדרך כלל/)).toBeInTheDocument();
    expect(screen.getByText(/כך את מסיימת את הסעיף בדרך כלל/)).toBeInTheDocument();
    expect(screen.getByText("(ב-4 דוחות · בסעיף סיכום)")).toBeInTheDocument();
    const [first] = screen.getAllByRole("button", { name: "להוסיף לפרופיל" });
    if (first) await user.click(first);
    await vi.waitFor(() => expect(calls).toContainEqual(["accept_style_suggestion", { id: "shape:opening:summary:x" }]));
  });
});

describe("what was learned from her reports (round 34)", () => {
  it("shows her fixed wording to add, the order of her sections and what she leaves out", async () => {
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
    expect(await screen.findByText(/משפט שאת כותבת באותו נוסח בכמה דוחות/)).toBeInTheDocument();
    expect(screen.getByText("(ב-3 דוחות · בסעיף פרופיל קוגניטיבי)")).toBeInTheDocument();
    expect(screen.getByText("תפקוד כיום בבית (גיל שלוש עד שש):")).toBeInTheDocument();
    expect(screen.getByText(/משחק · רגשי/)).toBeInTheDocument();
    expect(screen.getByText(/את לא כותבת אחוזונים/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "להוסיף לפרופיל" }));
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
