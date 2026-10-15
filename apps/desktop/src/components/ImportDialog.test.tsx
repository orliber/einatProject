import { render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import type { ImportPreview } from "../ipc/client";
import { status } from "../test/fixtures";
import { ImportDialog } from "./ImportDialog";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

// Fabricated document.
const preview = (): ImportPreview => ({
  file_name: "שיחה.docx",
  format: "docx",
  pages: 1,
  title: "שיחה עם הגננת",
  suggested_kind: "kindergarten",
  body: "הגננת רינת סיפרה שהוא משחק עם חן.",
  preview: [
    { text: "הגננת ", mark: null, label: null },
    { text: "רינת", mark: "replaced", label: "שם אדם" },
    { text: " סיפרה שהוא משחק עם ", mark: null, label: null },
    { text: "חן", mark: "replaced", label: "שם אדם" },
    { text: ".", mark: null, label: null },
  ],
  suspects: [],
  auto_hidden: [
    { token: "רינת", tag: "[גננת_2]", role: "teacher", reason: "אחרי 'הגננת'", uncertain: false, kind: "name" },
    { token: "חן", tag: "[ילד_אחר_1]", role: "other_child", reason: "אחרי 'משחק עם'", uncertain: true, kind: "name" },
  ],
  hidden: ["שם אדם"],
  left_out: [],
  name_suggestions: [],
  warnings: [],
});

describe("ImportDialog", () => {
  it("shows the text with what is hidden marked, and no card: the names are on the case's privacy page", () => {
    mockIPC(() => null);
    render(
      <AppContext.Provider value={api()}>
        <ImportDialog caseId="c1" preview={preview()} onClose={vi.fn()} onSaved={vi.fn(async () => undefined)} />
      </AppContext.Provider>,
    );
    expect(screen.queryByRole("region", { name: "מה הוסתר אוטומטית" })).toBeNull();
    expect(screen.queryByText(/הסתרתי אוטומטית/)).toBeNull();
    const text = screen.getByRole("region", { name: "הטקסט שיישמר בתיק" });
    expect(text).toHaveTextContent("הגננת רינת סיפרה שהוא משחק עם חן.");
    expect(screen.getByRole("button", { name: "שמירה בתיק" })).toBeEnabled();
  });
});
