import { describe, expect, it } from "vitest";
import { ageFrom, ageWords, parseAge } from "./AgeField";

describe("age at assessment", () => {
  it("counts whole months, like the test manuals", () => {
    expect(ageFrom("2021-05-20", "2026-09-28")).toEqual({ years: 5, months: 4 });
    expect(ageFrom("2021-05-29", "2026-09-28")).toEqual({ years: 5, months: 3 });
    expect(ageFrom("2021-09-28", "2026-09-28")).toEqual({ years: 5, months: 0 });
    expect(ageFrom("2026-10-01", "2026-09-28")).toBeNull();
  });
  it("says the age in words", () => {
    expect(ageWords({ years: 5, months: 4 })).toBe("5 שנים ו-4 חודשים");
    expect(ageWords({ years: 1, months: 1 })).toBe("שנה וחודש");
    expect(ageWords({ years: 2, months: 2 })).toBe("שנתיים וחודשיים");
    expect(ageWords({ years: 4, months: 0 })).toBe("4 שנים");
  });
  it("accepts only possible ages", () => {
    expect(parseAge("5", "4")).toEqual({ years: 5, months: 4 });
    expect(parseAge("5", "")).toEqual({ years: 5, months: 0 });
    expect(parseAge("5", "12")).toBeNull();
    expect(parseAge("", "4")).toBeNull();
  });
});
