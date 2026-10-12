import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { clearKept, getKept, useKept } from "./kept";

afterEach(() => clearKept());

describe("kept screen state", () => {
  it("is still there when the screen opens again", () => {
    const first = renderHook(() => useKept("t:message", ""));
    act(() => first.result.current[1]("לקצר את הסעיף"));
    first.unmount();
    const again = renderHook(() => useKept("t:message", ""));
    expect(again.result.current[0]).toBe("לקצר את הסעיף");
  });

  it("takes an answer that arrives after the screen closed", () => {
    const screen = renderHook(() => useKept<string[]>("t:turns", []));
    const set = screen.result.current[1];
    screen.unmount();
    set((all) => [...all, "תשובה"]);
    expect(getKept<string[]>("t:turns", [])).toEqual(["תשובה"]);
  });

  it("forgets everything on lock, and an answer after the lock is dropped", () => {
    const screen = renderHook(() => useKept("t:last", ""));
    const set = screen.result.current[1];
    act(() => set("שם הילד"));
    act(() => clearKept());
    expect(screen.result.current[0]).toBe("");
    set("תשובה מאוחרת");
    expect(getKept("t:last", "")).toBe("");
  });
});
