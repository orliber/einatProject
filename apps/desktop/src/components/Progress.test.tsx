import { act, render, screen } from "@testing-library/react";
import { byPace, estimateMs, learned, leftText, recordDuration, sizeBucket, timed, WaitLine } from "./Progress";

describe("wait estimates", () => {
  beforeEach(() => localStorage.clear());

  it("starts from a first guess, said as one, and learns from this computer", async () => {
    expect(learned("export", "word")).toBe(false);
    expect(estimateMs("export", "word")).toBe(6000);
    expect(leftText(1000, 6000, false)).toBe("בדרך כלל כ-5 שניות");
    recordDuration("export", "word", 20_000);
    // The clock stands still around this wait: 0 ms is not kept, so a slow runner that
    // measures 1 ms cannot add a third value and move the median.
    const now = vi.spyOn(Date, "now").mockReturnValue(1_790_000_000_000);
    await timed("export", Promise.resolve("ok"), "word");
    now.mockRestore();
    recordDuration("export", "word", 22_000);
    expect(learned("export", "word")).toBe(true);
    expect(estimateMs("export", "word")).toBe(22_000);
    expect(leftText(5000, 20_000, true)).toBe("נותרו כ-15 שניות");
  });

  it("keeps nothing from a wait that failed", async () => {
    await expect(timed("backup-check", Promise.reject(new Error("x")))).rejects.toThrow();
    expect(learned("backup-check")).toBe(false);
  });

  it("never claims to know once past the usual time", () => {
    expect(leftText(9000, 6000, true)).toBe("לוקח קצת יותר מהרגיל, עוד מעט");
    expect(leftText(9000, 6000, false)).toBe("עדיין עובדת, בפעם הראשונה זה לוקח יותר");
  });

  it("says a download's time by its own pace", () => {
    expect(byPace(0, 1000, 10, 100)).toBe("10%");
    expect(byPace(0, 10_000, 50, 100)).toBe("50% · נותרו כ-10 שניות");
    expect(sizeBucket(100_000)).toBe("small");
    expect(sizeBucket(10_000_000)).toBe("large");
  });
});

describe("WaitLine", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("shows the bar only after a moment, with the time so far and what is left", () => {
    render(<WaitLine task="check" label="בודקת את הדוח" />);
    expect(screen.queryByText(/0:0/)).toBeNull();
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByText("בודקת את הדוח")).toBeInTheDocument();
    expect(screen.getByText(/0:01 · בדרך כלל כ-5 שניות/)).toBeInTheDocument();
  });
});
