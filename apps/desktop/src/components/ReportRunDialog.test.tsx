// "כתיבת כל הדוח": what is still in the queue can be stopped, and a section that failed is
// tried again on its own, with a fresh request. All names are invented.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import type { CaseDetail } from "../ipc/client";
import type { CaseApi } from "../screens/CaseScreen";
import { prepared, status } from "../test/fixtures";
import { ReportRunDialog } from "./ReportRunDialog";

const KEYS = ["a", "b", "c", "d"];

function app(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

function caseApi(): CaseApi {
  const detail = { routing: [], sections: KEYS.map((k) => ({ key: k, title: `סעיף ${k}` })) } as unknown as CaseDetail;
  return {
    caseId: "c1", detail, jobs: [],
    reload: vi.fn(async () => undefined),
    review: vi.fn(async () => true),
    sort: vi.fn(async () => undefined),
    track: (_job, run) => run(),
    draft: vi.fn(async () => true),
  };
}

describe("ReportRunDialog", () => {
  it("stops what was not sent yet, and tries a failed section again on its own", async () => {
    const sent: string[] = [];
    const waiting: (() => void)[] = [];
    let fresh = 0;
    mockIPC((cmd, args) => {
      const a = args as { approvalId?: string; sectionKey?: string };
      if (cmd === "prepare_full_draft") return KEYS.map((k) => [k, prepared({ approval_id: `ap-${k}` })]);
      if (cmd === "prepare_section") return prepared({ approval_id: `fresh-${++fresh}` });
      if (cmd === "send_section") {
        sent.push(a.approvalId ?? "");
        const fails = a.approvalId === "ap-b";
        return new Promise((done, failed) => waiting.push(() => (fails ? failed({ code: "egress", message: "אין חיבור לאינטרנט.", details: [] }) : done({ draft_ids: [], demo: false }))));
      }
      return null;
    });
    const user = userEvent.setup();
    render(<AppContext.Provider value={app()}><ReportRunDialog api={caseApi()} onClose={vi.fn()} /></AppContext.Provider>);
    await user.click(await screen.findByRole("button", { name: "שליחה וכתיבת 4 סעיפים" }));
    await vi.waitFor(() => expect(sent).toHaveLength(3));
    await user.click(screen.getByRole("button", { name: "לעצור את מה שעוד לא נשלח" }));
    for (const w of waiting.splice(0)) w();
    await vi.waitFor(() => expect(screen.queryByRole("button", { name: /עוצר/ })).toBeNull());
    expect(sent).toEqual(["ap-a", "ap-b", "ap-c"]);
    expect(screen.getAllByText("מוכן לשליחה")).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "לנסות שוב" }));
    await vi.waitFor(() => expect(sent).toContain("fresh-1"));
    for (const w of waiting.splice(0)) w();
  });
});
