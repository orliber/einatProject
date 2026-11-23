import { describe, expect, it } from "vitest";
import { searchForm } from "./searchForm";

describe("a text as a search compares it", () => {
  it("reads a Hebrew final letter as its regular form, and a capital as a small letter", () => {
    expect(searchForm("גן")).toBe("גנ");
    expect(searchForm("גנים")).toBe("גנימ");
    expect(searchForm("גנים")).toContain(searchForm("גן"));
    expect(searchForm("ADOS ץףךםן")).toBe("ados צפכמנ");
  });

  it("keeps one letter for each letter, so a place in it is the place in the text", () => {
    for (const text of ["שיחה עם הגננת", "İstanbul ß", "", "👪 משפחה"]) expect(Array.from(searchForm(text))).toHaveLength(Array.from(text).length);
  });
});
