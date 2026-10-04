import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

const identity = (value: string, tag: string) => ({
  id: tag, case_id: "c1", role: "teacher", tag, value, aliases: [], source: "auto", reason: "אחרי 'הגננת'",
});

describe("ImportDialog", () => {
  it("lists what was hidden without asking, and 'להחזיר' takes one item off", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "preview_filter") {
        return {
          tagged: "", original_segments: [{ text: "הגננת רינת סיפרה שהוא משחק עם חן.", mark: null, label: null }],
          tagged_segments: [], auto_hidden: [], hidden: [], checks: { declared_names: 0, patterns: 0, name_suspects: 0, indirect_suspects: 0 },
        };
      }
      if (cmd === "case_detail") return { identities: [identity("רינת", "[גננת_2]")] };
      return null;
    });
    const user = userEvent.setup();
    render(
      <AppContext.Provider value={api()}>
        <ImportDialog caseId="c1" preview={preview()} onClose={vi.fn()} onSaved={vi.fn(async () => undefined)} />
      </AppContext.Provider>,
    );
    expect(screen.queryByText(/תתבקשי להחליט/)).toBeNull();
    const card = screen.getByRole("region", { name: "מה הוסתר אוטומטית" });
    expect(card).toHaveTextContent("הסתרתי אוטומטית 2 פרטים");
    expect(card).toHaveTextContent("לא בטוח");

    await user.click(screen.getByRole("button", { name: "להחזיר את חן" }));
    expect(calls).toContain("restore_auto_hidden");
    expect(await screen.findByText("הסתרתי אוטומטית פרט אחד")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "מה הוסתר אוטומטית" })).toHaveTextContent("רינת");
    expect(screen.getByRole("button", { name: "שמירה בתיק" })).toBeEnabled();
  });
});
