import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Segmented, Switch } from "./Switch";

describe("Switch and Segmented", () => {
  it("a switch is a switch, named by its label, and flips on click and on Space", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Switch id="s1" label="להראות לי כל בקשה" hint="הסבר" checked={false} onChange={onChange} />);
    const sw = screen.getByRole("switch", { name: "להראות לי כל בקשה" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    await user.click(sw);
    expect(onChange).toHaveBeenLastCalledWith(true);
    sw.focus();
    await user.keyboard(" ");
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it("a segmented choice is a radio group with one pressed option", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Segmented label="נעילה אוטומטית אחרי" value={10} options={[5, 10, 15, 30].map((m) => ({ value: m, label: String(m) }))} onChange={onChange} />);
    const group = screen.getByRole("radiogroup", { name: "נעילה אוטומטית אחרי" });
    expect(group).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "10" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: "30" }));
    expect(onChange).toHaveBeenLastCalledWith(30);
  });
});
