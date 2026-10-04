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
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
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

describe("StyleScreen", () => {
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
    expect(screen.getByText(/1 פריטים שחזרו מ-Claude לא נשמרו/)).toBeInTheDocument();
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
