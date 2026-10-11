import { act, fireEvent, render, screen } from "@testing-library/react";
import { Toast } from "./ui";

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
});
