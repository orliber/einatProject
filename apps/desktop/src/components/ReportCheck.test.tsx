import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import type { CaseDetail, ParagraphView, ReportCheckResult } from "../ipc/client";
import type { SectionView } from "../ipc/generated/SectionView";
import type { CaseApi } from "../screens/CaseScreen";
import { prepared, status } from "../test/fixtures";
import { ReportCheckItem, ReportCheckSettings } from "./ReportCheck";

function wrap(children: ReactNode, notify = vi.fn()) {
  const api: AppApi = {
    status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify,
    fail: (e) => e.message, lockNow: vi.fn(async () => undefined),
  };
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api}>{children}</AppContext.Provider>
    </QueryClientProvider>,
  );
}

const paragraph = (id: string, status: ParagraphView["status"]): ParagraphView => ({
  id, text: `פסקה ${id}`, status, by_ai: false, sources: [], warnings: [], replaces: null, has_versions: false, style_note: null, style: null,
});
const section = (key: string, title: string, paragraphs: ParagraphView[]) =>
  ({ key, title, part: "חלק", paragraphs }) as unknown as SectionView;

function caseApi(sections: SectionView[], review = vi.fn(async (r: { prepared: { approval_id: string | null }; onSend: (id: string) => Promise<void> }) => {
  await r.onSend(r.prepared.approval_id ?? "");
  return true;
})): CaseApi {
  return {
    caseId: "c1",
    detail: { id: "c1", sections } as unknown as CaseDetail,
    reload: vi.fn(async () => undefined),
    review: review as unknown as CaseApi["review"],
    sort: vi.fn(async () => undefined),
    jobs: [],
    track: vi.fn() as unknown as CaseApi["track"],
    draft: vi.fn(async () => true),
  };
}

describe("the check of the whole report", () => {
  it("is off by default and shows its cost before it is turned on", async () => {
    let on = false;
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "set_report_check") { on = (args as { on: boolean }).on; return null; }
      if (cmd === "report_check_setting") return { on, cents_per_check: 15 };
      return null;
    });
    const notify = vi.fn();
    wrap(<ReportCheckSettings />, notify);
    const sw = await screen.findByRole("switch", { name: /לאפשר בדיקה של הדוח כולו/ });
    await waitFor(() => expect(sw).toBeEnabled());
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    // Asked first, with the cost; nothing is saved until she agrees.
    const ask = await screen.findByRole("group", { name: "לפני ההפעלה" });
    expect(ask).toHaveTextContent("כ-15 סנט");
    expect(calls.some(([c]) => c === "set_report_check")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "ביטול" }));
    expect(screen.queryByRole("group", { name: "לפני ההפעלה" })).not.toBeInTheDocument();
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    fireEvent.click(await screen.findByRole("button", { name: "להפעיל" }));
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "true"));
    expect(calls.find(([c]) => c === "set_report_check")?.[1]).toMatchObject({ on: true });
    expect(notify).toHaveBeenCalledWith("בדיקת הדוח כולו פעילה. היא מופיעה במסך הסיום.");
    // Off goes at once.
    fireEvent.click(sw);
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "false"));
  });

  it("is not in the finish screen while it is off", async () => {
    mockIPC((cmd) => (cmd === "report_check_setting" ? { on: false, cents_per_check: 15 } : null));
    const api = caseApi([section("background", "רקע", [paragraph("p1", "approved")]), section("summary", "סיכום", [paragraph("p2", "approved")])]);
    const { container } = wrap(<ul><ReportCheckItem api={api} onOpen={vi.fn()} /></ul>);
    await new Promise((r) => setTimeout(r, 20));
    expect(container.querySelector("li")).toBeNull();
  });

  it("waits for every draft to be approved", async () => {
    mockIPC((cmd) => (cmd === "report_check_setting" ? { on: true, cents_per_check: 15 } : null));
    const api = caseApi([section("background", "רקע", [paragraph("p1", "approved")]), section("cognitive", "קוגניטיבי", [paragraph("p2", "proposed")])]);
    wrap(<ul><ReportCheckItem api={api} onOpen={vi.fn()} /></ul>);
    expect(await screen.findByText(/עוד מחכות: קוגניטיבי/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "לבדוק את הדוח כולו" })).toBeDisabled();
  });

  it("lists the notes as text, each opening its section", async () => {
    const result: ReportCheckResult = {
      notes: [
        { kind: "summary_only", sections: ["summary"], note: "<b>סקרנות</b> מופיעה רק בסיכום" },
        { kind: "other", sections: [], note: "שורה לא נשלחה" },
      ],
      checked: ["background", "summary"],
      demo: false,
    };
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "report_check_setting") return { on: true, cents_per_check: 15 };
      if (cmd === "prepare_report_check") return prepared({ approval_id: "a1" });
      if (cmd === "send_report_check") return result;
      return null;
    });
    const onOpen = vi.fn();
    const api = caseApi([section("background", "רקע", [paragraph("p1", "approved")]), section("summary", "סיכום", [paragraph("p2", "approved")])]);
    wrap(<ul><ReportCheckItem api={api} onOpen={onOpen} /></ul>);
    fireEvent.click(await screen.findByRole("button", { name: "לבדוק את הדוח כולו" }));
    const jump = await screen.findByRole("button", { name: /מופיע רק בסיכום · סיכום/ });
    // Plain text: the model's words are never markup.
    expect(jump).toHaveTextContent("<b>סקרנות</b> מופיעה רק בסיכום");
    expect(jump.querySelector("b")).toBeNull();
    fireEvent.click(jump);
    expect(onOpen).toHaveBeenCalledWith("summary");
    // A note with no section is shown, without a place to jump to.
    expect(screen.getByText("שורה לא נשלחה").closest("button")).toBeNull();
    expect(calls).toEqual(expect.arrayContaining(["prepare_report_check", "send_report_check"]));
    expect(screen.getByRole("button", { name: "לבדוק שוב" })).toBeEnabled();
  });
});
