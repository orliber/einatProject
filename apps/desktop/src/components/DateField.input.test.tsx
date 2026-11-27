// The date box as a form uses it: a date typed wrong is not the same as no date.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DateField } from "./DateField";

describe("the date box", () => {
  it("tells a date typed wrong from a date left empty", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<><label htmlFor="d">תאריך</label><DateField id="d" value="2020-02-10" onChange={onChange} notAfterToday /></>);
    const box = screen.getByLabelText("תאריך");
    await user.clear(box);
    expect(onChange).toHaveBeenLastCalledWith("", false);
    await user.type(box, "31.02.2020");
    expect(onChange).toHaveBeenLastCalledWith("", true);
    await user.clear(box);
    await user.type(box, "28.02.2020");
    expect(onChange).toHaveBeenLastCalledWith("2020-02-28", false);
    // A day still to come is wrong where only a past day fits.
    await user.clear(box);
    await user.type(box, "01.01.2999");
    expect(onChange).toHaveBeenLastCalledWith("", true);
  });

  it("shows the problem at once when the form asks (on saving), before she leaves the box", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<><label htmlFor="d">תאריך</label><DateField id="d" value="" onChange={() => undefined} /></>);
    await user.type(screen.getByLabelText("תאריך"), "31.02.2020");
    expect(screen.getByLabelText("תאריך")).toHaveAttribute("aria-invalid", "false");
    rerender(<><label htmlFor="d">תאריך</label><DateField id="d" value="" onChange={() => undefined} checked /></>);
    expect(screen.getByLabelText("תאריך")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("תאריך לא תקין. כותבים יום.חודש.שנה")).toBeInTheDocument();
  });
});
