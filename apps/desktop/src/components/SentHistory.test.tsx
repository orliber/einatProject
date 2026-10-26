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
        return [item()];
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
    expect(asked[0]).toEqual({ caseId: null, place: null, query: "" });

    await user.click(screen.getByRole("button", { name: "הטקסט שיצא" }));
    expect(await screen.findByText("[הילד] מתקשה במעברים.")).toBeInTheDocument();
    expect(screen.getByText("אלון")).toBeInTheDocument();
    expect(screen.getByText(/הטקסט השמור זהה בדיוק למה שיצא/)).toBeInTheDocument();
  });

  it("a search for a hidden word says it did not leave", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "sent_history") return (args as { query: string }).query ? [item({ hidden_match: true })] : [];
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
      if (cmd === "sent_history") return [item()];
      if (cmd === "sent_text") return { item: item(), hash_ok: false, exact: "{}", parts: [] };
      return null;
    });
    const user = userEvent.setup();
    render(<Wrap><SentHistoryDialog caseId="c1" onClose={vi.fn()} /></Wrap>);
    await user.click(await screen.findByRole("button", { name: "הטקסט שיצא" }));
    expect(await screen.findByText(/לא תואם את הטביעה/)).toBeInTheDocument();
  });

  it("names what each request was for in her words", () => {
    expect(placeLabel("sorting")).toBe("מיון החומרים לסעיפים");
    expect(placeLabel("consult")).toBe("שאלה מתוך התיק");
    expect(placeLabel("report_check")).toBe("בדיקת הדוח כולו");
    expect(placeLabel("letter_parents")).toBe("מכתב להורים");
    expect(placeLabel("kindergarten")).not.toBe("kindergarten");
  });
});
