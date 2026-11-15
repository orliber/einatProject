import { act, fireEvent, render, screen } from "@testing-library/react";
import { Toast } from "./ui";
import { toastOf } from "../App";

describe("Toast", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("goes away by itself", () => {
    const done = vi.fn();
    render(<Toast text="נשמר" onDone={done} />);
    act(() => vi.advanceTimersByTime(3600));
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("runs its action once and closes", () => {
    const done = vi.fn();
    const run = vi.fn();
    render(<Toast text="החומר נמחק" onDone={done} action={{ label: "ביטול", run }} />);
    fireEvent.click(screen.getByRole("button", { name: "ביטול" }));
    expect(run).toHaveBeenCalledTimes(1);
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("stays while the pointer is on it", () => {
    const done = vi.fn();
    render(<Toast text="החומר נמחק" onDone={done} action={{ label: "ביטול", run: () => {} }} />);
    fireEvent.pointerEnter(screen.getByRole("status"));
    act(() => vi.advanceTimersByTime(20000));
    expect(done).not.toHaveBeenCalled();
    fireEvent.pointerLeave(screen.getByRole("status"));
    act(() => vi.advanceTimersByTime(7100));
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("the same words notified again are a new toast with a fresh timer: the second 'נשמר' is not cut short (regression)", () => {
    const done = vi.fn();
    const first = toastOf("פרטי התיק נשמרו.");
    const view = render(<Toast key={first.seq} text={first.text} action={first.action} onDone={done} />);
    act(() => vi.advanceTimersByTime(3000));
    // The app keys the toast by the notice, as `Shell` does: a second save a moment later.
    const second = toastOf("פרטי התיק נשמרו.");
    expect(second.seq).not.toBe(first.seq);
    view.rerender(<Toast key={second.seq} text={second.text} action={second.action} onDone={done} />);
    act(() => vi.advanceTimersByTime(3000));
    expect(done).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(600));
    expect(done).toHaveBeenCalledTimes(1);
  });
});
