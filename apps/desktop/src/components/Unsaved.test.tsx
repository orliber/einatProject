import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { forgetHold, holdNow, UnsavedNotice, useHoldUnsaved } from "./Unsaved";

describe("an edit open when the vault locks", () => {
  it("goes to the core while she types, and is withdrawn when the editor closes", async () => {
    vi.useFakeTimers();
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "hold_unsaved") calls.push((args as { edits: unknown }).edits);
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
    expect(calls).toEqual([[{ case_id: "c1", place: "פסקה", view: "report", text: "טיוטה" }]]);
    rerender({ text: null });
    await act(async () => { vi.advanceTimersByTime(500); });
    expect(calls.at(-1)).toEqual([]);
    unmount();
    vi.useRealTimers();
  });

  it("is held as it is in the box this moment when a lock comes, and kept when the screen goes", async () => {
    vi.useFakeTimers();
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "hold_unsaved") calls.push((args as { edits: unknown }).edits);
      return null;
    });
    let live: ((t: string) => void) | null = null;
    const { unmount } = renderHook(() => { live = useHoldUnsaved("c1", "פסקה", ""); });
    // Every key, no pause: nothing goes to the core while she types…
    for (const t of ["ט", "טי", "טיו", "טיוטה"]) {
      act(() => live?.(t));
      vi.advanceTimersByTime(50);
    }
    expect(calls).toEqual([]);
    // …and a lock by hand sends exactly what is in the box.
    await act(async () => { await holdNow(); });
    expect(calls).toEqual([[{ case_id: "c1", place: "פסקה", view: "report", text: "טיוטה" }]]);
    // Leaving the screen with the editor open keeps it held.
    unmount();
    await act(async () => { vi.advanceTimersByTime(500); });
    expect(calls).toHaveLength(1);
    forgetHold();
    vi.useRealTimers();
  });

  it("two cases' editors are both held, and closing one keeps the other", async () => {
    vi.useFakeTimers();
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "hold_unsaved") calls.push((args as { edits: unknown }).edits);
      return null;
    });
    // Case A's editor: typed, then she went to another case with it open.
    const a = renderHook(() => useHoldUnsaved("case-a", "פסקה משלי", "טקסט של תיק א בדוי"));
    await act(async () => { vi.advanceTimersByTime(500); });
    a.unmount();
    // Case B's editor: one letter, then Esc.
    const b = renderHook(({ text }: { text: string | null }) => useHoldUnsaved("case-b", "פסקה משלי", text), {
      initialProps: { text: "ב" as string | null },
    });
    await act(async () => { vi.advanceTimersByTime(500); });
    b.rerender({ text: null });
    await act(async () => { await holdNow(); });
    expect(calls.at(-1)).toEqual([{ case_id: "case-a", place: "פסקה משלי", view: "report", text: "טקסט של תיק א בדוי" }]);
    b.unmount();
    forgetHold();
    vi.useRealTimers();
  });

  it("the same editor shown again is one text: cancelling it lets go of it", async () => {
    vi.useFakeTimers();
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "hold_unsaved") calls.push((args as { edits: unknown }).edits);
      return null;
    });
    const first = renderHook(() => useHoldUnsaved("c1", "פסקה", "טיוטה"));
    await act(async () => { vi.advanceTimersByTime(500); });
    first.unmount();
    // Back on the stage: the kept editor opens again, then she cancels it.
    const again = renderHook(({ text }: { text: string | null }) => useHoldUnsaved("c1", "פסקה", text), {
      initialProps: { text: "טיוטה" as string | null },
    });
    await act(async () => { vi.advanceTimersByTime(500); });
    again.rerender({ text: null });
    await act(async () => { await holdNow(); });
    expect(calls.at(-1)).toEqual([]);
    again.unmount();
    forgetHold();
    vi.useRealTimers();
  });

  it("two texts kept at the lock are offered one after the other", async () => {
    mockIPC((cmd) => (cmd === "take_unsaved"
      ? [
          { case_id: "c1", place: "פסקה בתיק א", view: "report", text: "טקסט בדוי א" },
          { case_id: "c2", place: "רישום מפגש בתיק ב", view: "materials", text: "טקסט בדוי ב" },
        ]
      : null));
    const api: AppApi = { status: status(), go: vi.fn(), refresh: vi.fn(), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn() };
    render(<AppContext.Provider value={api}><UnsavedNotice /></AppContext.Provider>);
    expect(await screen.findByDisplayValue("טקסט בדוי א")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "הבא (עוד 1)" }));
    expect(screen.getByDisplayValue("טקסט בדוי ב")).toBeInTheDocument();
    const closers = screen.getAllByRole("button", { name: "סגירה" });
    fireEvent.click(closers[closers.length - 1] as HTMLElement);
    expect(screen.queryByDisplayValue("טקסט בדוי ב")).toBeNull();
  });

  it("is offered back once after entering", async () => {
    mockIPC((cmd) => (cmd === "take_unsaved" ? [{ case_id: "c1", place: "פסקה בסעיף \"רקע\"", text: "הטקסט שלא נשמר" }] : null));
    const go = vi.fn();
    const api: AppApi = { status: status(), go, refresh: vi.fn(), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn() };
    render(<AppContext.Provider value={api}><UnsavedNotice /></AppContext.Provider>);
    expect(await screen.findByDisplayValue("הטקסט שלא נשמר")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "מעבר לתיק" }));
    expect(go).toHaveBeenCalledWith({ name: "case", id: "c1", view: "report" });
    expect(screen.queryByDisplayValue("הטקסט שלא נשמר")).toBeNull();
  });

  it("returns to the materials for a session note, and to the conversation for a question", async () => {
    for (const [view, route] of [
      ["materials", { name: "case", id: "c1", view: "materials" }],
      ["consult", { name: "consult", caseId: "c1" }],
    ] as const) {
      mockIPC((cmd) => (cmd === "take_unsaved" ? [{ case_id: "c1", place: "רישום מפגש", view, text: `טקסט ${view}` }] : null));
      const go = vi.fn();
      const api: AppApi = { status: status(), go, refresh: vi.fn(), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn() };
      const { unmount } = render(<AppContext.Provider value={api}><UnsavedNotice /></AppContext.Provider>);
      await screen.findByDisplayValue(`טקסט ${view}`);
      fireEvent.click(screen.getByRole("button", { name: "מעבר לתיק" }));
      expect(go).toHaveBeenCalledWith(route);
      unmount();
    }
  });
});
