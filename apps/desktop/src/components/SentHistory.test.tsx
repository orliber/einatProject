import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import type { SentItem } from "../ipc/generated/SentItem";
import { status } from "../test/fixtures";
import { placeLabel, SentHistoryDialog } from "./SentHistory";

function Wrap({ children }: { children: ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const api: AppApi = { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
  return (
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api}>{children}</AppContext.Provider>
    </QueryClientProvider>
  );
}

// Fabricated request.
const item = (over: Partial<SentItem> = {}): SentItem => ({
  id: "t1", case_id: "c1", case_code: "TEST-0002", place: "kindergarten", at: 1_760_000_000, ai: "Claude", demo: false,
  sha256: "ab".repeat(32), bytes: 900, hidden: [{ tag: "[הילד]", word: "אלון" }], hidden_match: false, ...over,
});

describe("SentHistoryDialog", () => {
  it("lists every request, opens the exact text, and shows what each tag hid", async () => {
    const asked: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "sent_history") {
        asked.push(args);
        return { items: [item()], next: null };
      }
      if (cmd === "sent_text") {
        return {
          item: item(), hash_ok: true, exact: '{"messages":[]}',
          parts: [{ label: "הוראות קבועות", text: "כללים" }, { label: "בקשה", text: "[הילד] מתקשה במעברים." }],
        };
      }
      return null;
    });
    const user = userEvent.setup();
    render(<Wrap><SentHistoryDialog onClose={vi.fn()} /></Wrap>);
    expect(await screen.findByText("TEST-0002")).toBeInTheDocument();
    expect(screen.getByText("הוסתר פרט אחד")).toBeInTheDocument();
    expect(asked[0]).toEqual({ caseId: null, place: null, query: "", before: null });
    expect(screen.queryByRole("button", { name: "בקשות ישנות יותר" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "הטקסט שיצא" }));
    expect(await screen.findByText("[הילד] מתקשה במעברים.")).toBeInTheDocument();
    expect(screen.getByText("אלון")).toBeInTheDocument();
    expect(screen.getByText(/הטקסט השמור זהה בדיוק למה שיצא/)).toBeInTheDocument();
  });

  it("a search for a hidden word says it did not leave", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "sent_history") return { items: (args as { query: string }).query ? [item({ hidden_match: true })] : [], next: null };
      return null;
    });
    const user = userEvent.setup();
    render(<Wrap><SentHistoryDialog caseId="c1" place="kindergarten" onClose={vi.fn()} /></Wrap>);
    expect(await screen.findByText("עוד לא יצאה שום בקשה.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("חיפוש בבקשות"), "אלון");
    await user.click(screen.getByRole("button", { name: "חיפוש" }));
    expect(await screen.findByText('"אלון" הוסתר בבקשה הזו: לא יצא מהמחשב.')).toBeInTheDocument();
  });

  it("a kept text that no longer matches its hash says so", async () => {
    mockIPC((cmd) => {
      if (cmd === "sent_history") return { items: [item()], next: null };
      if (cmd === "sent_text") return { item: item(), hash_ok: false, exact: "{}", parts: [] };
      return null;
    });
    const user = userEvent.setup();
    render(<Wrap><SentHistoryDialog caseId="c1" onClose={vi.fn()} /></Wrap>);
    await user.click(await screen.findByRole("button", { name: "הטקסט שיצא" }));
    expect(await screen.findByText(/לא תואם את הטביעה/)).toBeInTheDocument();
  });

  it("reads a page at a time, and older requests on asking", async () => {
    const asked: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "sent_history") {
        asked.push(args);
        return (args as { before: unknown }).before
          ? { items: [item({ id: "t2", case_code: "TEST-0003" })], next: null }
          : { items: [item()], next: { at: 1_760_000_000, id: "t1" } };
      }
      return null;
    });
    const user = userEvent.setup();
    render(<Wrap><SentHistoryDialog onClose={vi.fn()} /></Wrap>);
    expect(await screen.findByText("TEST-0002")).toBeInTheDocument();
    expect(screen.queryByText("TEST-0003")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "בקשות ישנות יותר" }));
    expect(await screen.findByText("TEST-0003")).toBeInTheDocument();
    expect(screen.getByText("TEST-0002")).toBeInTheDocument();
    expect(asked[1]).toEqual({ caseId: null, place: null, query: "", before: { at: 1_760_000_000, id: "t1" } });
    expect(screen.queryByRole("button", { name: "בקשות ישנות יותר" })).not.toBeInTheDocument();
  });

  it("a search that read as far as one page reads says where it got to, and goes on on asking", async () => {
    mockIPC((cmd, args) => {
      if (cmd !== "sent_history") return null;
      const a = args as { query: string; before: unknown };
      if (!a.query) return { items: [item()], next: null };
      return a.before ? { items: [item({ id: "t9" })], next: null } : { items: [], next: { at: 1_760_000_000, id: "t5" } };
    });
    const user = userEvent.setup();
    render(<Wrap><SentHistoryDialog caseId="c1" onClose={vi.fn()} /></Wrap>);
    await screen.findByRole("button", { name: "הטקסט שיצא" });
    await user.type(screen.getByLabelText("חיפוש בבקשות"), "מעברים");
    await user.click(screen.getByRole("button", { name: "חיפוש" }));
    expect(await screen.findByText(/"מעברים" לא נמצא בבקשות מאז/)).toBeInTheDocument();
    expect(screen.queryByText(/לא מופיע בשום בקשה/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "להמשיך לחפש בבקשות ישנות יותר" }));
    expect(await screen.findByRole("button", { name: "הטקסט שיצא" })).toBeInTheDocument();
    expect(screen.queryByText(/לא נמצא בבקשות מאז/)).not.toBeInTheDocument();
  });

  it("names what each request was for in her words", () => {
    expect(placeLabel("sorting")).toBe("מיון החומרים לסעיפים");
    expect(placeLabel("consult")).toBe("שאלה מתוך התיק");
    expect(placeLabel("report_check")).toBe("בדיקת הדוח כולו");
    expect(placeLabel("letter_parents")).toBe("מכתב להורים");
    expect(placeLabel("kindergarten")).not.toBe("kindergarten");
  });
});
