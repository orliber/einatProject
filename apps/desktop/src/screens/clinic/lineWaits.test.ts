// A line typed for the next visit, as saving a session note reads it. Lines are invented.
import { describe, expect, it } from "vitest";
import type { Visit } from "../../ipc/generated/Visit";
import { lineWaits, REMEMBER_MAX } from "./RememberList";

const visitWith = (n: number) => ({ remember: Array.from({ length: n }, (_, i) => ({ text: `דבר ${i + 1}`, done: false })) }) as unknown as Visit;

describe("a line waiting for the next visit", () => {
  it("waits when there is a next visit and something typed", () => {
    expect(lineWaits(visitWith(2), " לשאול על השינה ")).toBe(true);
    expect(lineWaits(visitWith(2), "   ")).toBe(false);
    expect(lineWaits(null, "לשאול על השינה")).toBe(false);
  });

  it("does not wait when the next visit's list is already full, so the note can still close", () => {
    expect(lineWaits(visitWith(REMEMBER_MAX - 1), "לשאול על השינה")).toBe(true);
    expect(lineWaits(visitWith(REMEMBER_MAX), "לשאול על השינה")).toBe(false);
  });
});
