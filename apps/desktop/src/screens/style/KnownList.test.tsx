// "מה התוכנה יודעת עלייך" with the profile editor open: changes not saved are never dropped
// in silence when she opens another row or folds this one (UID-6), and each item's controls
// say which item they are about, with "ביטול" after a delete (UID-14). Every item is invented.
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AppContext, type AppApi } from "../../App";
import type { StyleOverview, StyleProfileView } from "../../ipc/client";
import { status } from "../../test/fixtures";
import { KnownList } from "./KnownList";

const active: StyleProfileView = {
  id: "p0", version: 1, status: "active", created_at: 1_790_000_000, dropped: 0, demo: false,
  profile: {
    reports: 3,
    items: [
      { id: "a", section: null, kind: "rule", text: "גוף נסתר וזמן הווה", enabled: true, origin: "reports", support: 3 },
      { id: "b", section: null, kind: "avoid", text: "בלי מונחים באנגלית", enabled: true, origin: "reports", support: 2 },
      { id: "c", section: null, kind: "phrase", text: "ניכר כי", enabled: true, origin: "reports", support: 2 },
    ],
  },
};

const overview: StyleOverview = {
  sources: [], active, draft: null, versions: [], suggestions: [], enabled: true,
  sections: [{ key: "summary", title: "סיכום" }],
  demo_mode: true,
  learned: {
    from_reports: 0, from_edits: 0, from_finals: 0, finished_reports: 0,
    words_per_sentence: null, sentences_per_paragraph: null, commas_per_sentence: null,
    hedges: null, contrasts: null, first_person: null, fit_recent: null, fit_earlier: null, fit_count: 0, fit_by_month: [],
  },
  interpretations: [], scored_measures: 0, skeletons: [], omissions: [],
};

function setup() {
  mockIPC(() => null);
  const app: AppApi = { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={app}><KnownList o={overview} running={false} onReadScores={vi.fn()} /></AppContext.Provider>
    </QueryClientProvider>,
  );
  return { app, user: userEvent.setup() };
}

const writing = () => screen.getByRole("region", { name: "איך את כותבת" });

describe("what the program knows, with changes not saved", () => {
  it("opening another row asks first, and her change is still there when she goes back to it", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: /איך את כותבת/ }));
    const box = within(writing()).getAllByRole("checkbox", { name: "להשתמש בפריט" })[0] as HTMLElement;
    await user.click(box);
    expect(box).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: /משפטים שחוזרים אצלך/ }));
    // Not closed: the question, and the change kept.
    expect(screen.getByRole("alert")).toHaveTextContent('מה ששינית ב"איך את כותבת" עדיין לא נשמר.');
    expect(within(writing()).getAllByRole("checkbox", { name: "להשתמש בפריט" })[0]).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "חזרה לשמירה" }));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "שמירה כטיוטה" })).toHaveFocus();
    // Folding the same row asks too; "לסגור בלי לשמור" goes on.
    await user.click(screen.getByRole("button", { name: /איך את כותבת/ }));
    await user.click(screen.getByRole("button", { name: "לסגור בלי לשמור" }));
    expect(screen.queryByRole("region", { name: "איך את כותבת" })).toBeNull();
  });

  it("with nothing changed, another row opens at once", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: /איך את כותבת/ }));
    await user.click(screen.getByRole("button", { name: /משפטים שחוזרים אצלך/ }));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("region", { name: "משפטים שחוזרים אצלך" })).toBeInTheDocument();
  });

  it("each item's controls say which item they are about; the edit box has a name; a delete can be undone", async () => {
    const { user, app } = setup();
    await user.click(screen.getByRole("button", { name: /איך את כותבת/ }));
    const row = writing();
    const boxes = within(row).getAllByRole("checkbox", { name: "להשתמש בפריט" });
    expect(boxes[0]).toHaveAccessibleDescription("גוף נסתר וזמן הווה");
    expect(boxes[1]).toHaveAccessibleDescription("בלי מונחים באנגלית");
    const deletes = within(row).getAllByRole("button", { name: "מחיקה" });
    expect(deletes[1]).toHaveAccessibleDescription("בלי מונחים באנגלית");
    await user.click(within(row).getByText("גוף נסתר וזמן הווה"));
    expect(within(row).getByRole("textbox", { name: "נוסח הפריט" })).toHaveFocus();
    await user.tab();
    await user.click(within(row).getAllByRole("button", { name: "מחיקה" })[1] as HTMLElement);
    expect(within(row).queryByText("בלי מונחים באנגלית")).toBeNull();
    expect(app.notify).toHaveBeenCalledWith("הפריט הוסר.", expect.objectContaining({ label: "ביטול" }));
    const undo = (app.notify as ReturnType<typeof vi.fn>).mock.calls.at(-1)?.[1] as { run: () => void };
    await user.click(document.body);
    act(() => undo.run());
    expect(await within(row).findByText("בלי מונחים באנגלית")).toBeInTheDocument();
    // Back in its place: second, after the first.
    const texts = within(row).getAllByRole("checkbox", { name: "להשתמש בפריט" }).map((b) => b.getAttribute("aria-describedby"));
    expect(texts.map((id) => document.getElementById(id ?? "")?.textContent)).toEqual(["גוף נסתר וזמן הווה", "בלי מונחים באנגלית"]);
  });
});
