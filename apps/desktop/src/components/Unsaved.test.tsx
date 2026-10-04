import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { UnsavedNotice, useHoldUnsaved } from "./Unsaved";

describe("an edit open when the vault locks", () => {
  it("goes to the core while she types, and is withdrawn when the editor closes", async () => {
    vi.useFakeTimers();
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "hold_unsaved") calls.push((args as { edit: unknown }).edit);
      return null;
    });
    const { rerender, unmount } = renderHook(({ text }: { text: string | null }) => useHoldUnsaved("c1", "פסקה", text), {
      initialProps: { text: null as string | null },
    });
    // Nothing open, nothing held: no call.
    expect(calls).toEqual([]);
    rerender({ text: "ט" });
    rerender({ text: "טיוטה" });
    await act(async () => { vi.advanceTimersByTime(1000); });
    expect(calls).toEqual([{ case_id: "c1", place: "פסקה", text: "טיוטה" }]);
    rerender({ text: null });
    expect(calls.at(-1)).toBeNull();
    unmount();
    vi.useRealTimers();
  });

  it("is offered back once after entering", async () => {
    mockIPC((cmd) => (cmd === "take_unsaved" ? { case_id: "c1", place: "פסקה בסעיף \"רקע\"", text: "הטקסט שלא נשמר" } : null));
    const go = vi.fn();
    const api: AppApi = { status: status(), go, refresh: vi.fn(), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn() };
    render(<AppContext.Provider value={api}><UnsavedNotice /></AppContext.Provider>);
    expect(await screen.findByDisplayValue("הטקסט שלא נשמר")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "מעבר לתיק" }));
    expect(go).toHaveBeenCalledWith({ name: "case", id: "c1", view: "report" });
    expect(screen.queryByDisplayValue("הטקסט שלא נשמר")).toBeNull();
  });
});
