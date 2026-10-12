import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { SampleGuide, sampleStep } from "./SampleGuide";

describe("SampleGuide", () => {
  it("explains the page she is on, a report section counting as the report", () => {
    expect(sampleStep("materials")).toBe("materials");
    expect(sampleStep("background")).toBe("report");
  });

  it("leads on, throws the case away, and hides", () => {
    const onNext = vi.fn();
    const onDelete = vi.fn();
    render(<SampleGuide view="materials" busy={false} onNext={onNext} onDelete={onDelete} />);
    fireEvent.click(screen.getByRole("button", { name: "לכתיבת הדוח" }));
    expect(onNext).toHaveBeenCalledWith("materials");
    fireEvent.click(screen.getByRole("button", { name: "למחוק את תיק הדוגמה" }));
    expect(onDelete).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "להסתיר את ההסבר" }));
    expect(screen.queryByRole("region", { name: "תיק לדוגמה" })).toBeNull();
  });
});
