// ST2: the closest paragraph of her past reports, shown as text with roles, never sent.
import { render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { Similar, rolesAsWords } from "./Similar";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

describe("Similar", () => {
  it("shows the paragraph with roles in place of tags, as text", async () => {
    mockIPC((cmd, args) => {
      expect(cmd).toBe("similar_paragraph");
      expect(args).toMatchObject({ caseId: "c1", draftId: "d1" });
      return { text: "[ילד] שיחק עם [ילד_גן_2] <b>יפה</b>.", report: "דוח קודם", same_section: false };
    });
    const { container } = render(<AppContext.Provider value={api()}><Similar caseId="c1" draftId="d1" /></AppContext.Provider>);
    expect(await screen.findByText("ילד שיחק עם ילד גן 2 <b>יפה</b>.")).toBeInTheDocument();
    expect(screen.getByText("דוח קודם · מסעיף אחר")).toBeInTheDocument();
    expect(container.querySelector("b")).toBeNull();
  });

  it("says where to add reports when nothing is close", async () => {
    mockIPC(() => null);
    render(<AppContext.Provider value={api()}><Similar caseId="c1" draftId="d1" /></AppContext.Provider>);
    expect(await screen.findByText(/לא נמצאה פסקה דומה/)).toBeInTheDocument();
    expect(rolesAsWords("[מוסד_1]")).toBe("מוסד 1");
  });
});
