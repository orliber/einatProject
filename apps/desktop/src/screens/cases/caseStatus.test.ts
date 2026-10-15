import { describe, expect, it } from "vitest";
import type { CaseSummary } from "../../ipc/client";
import { caseStatus, TOTAL_SECTIONS } from "./caseStatus";

// All invented.
function summary(over: Partial<CaseSummary> & { approved?: number; consent?: boolean }): CaseSummary {
  const approved = Array.from({ length: over.approved ?? 0 }, (_, i) => `s${i}`);
  return {
    id: "c1", child_name: "ילד בדוי", created_at: 0, updated_at: 0, folder_id: null, deleted_at: null, failed_at: null,
    approved_sections: approved,
    meta: {
      code: "תיק-1", age: { years: 5, months: 4 }, child_gender: "male", current_section: null, retention_until: null,
      consent: over.consent === false ? null : { given_on: "2026-09-01", form_version: "v1", given_by: "ההורים" },
    },
    ...over,
  } as CaseSummary;
}

describe("caseStatus: one word, one line, three steps", () => {
  it("no materials and no sections: collecting materials", () => {
    const s = caseStatus(summary({}));
    expect(s.word).toBe("אוספים חומרים");
    expect(s.steps).toEqual(["now", "empty", "empty"]);
    expect(s.view).toBe("materials");
  });

  it("some sections approved: writing, with the count", () => {
    const s = caseStatus(summary({ approved: 2 }));
    expect(s.word).toBe("בכתיבה");
    expect(s.detail).toBe(`2 מתוך ${TOTAL_SECTIONS} סעיפים אושרו`);
    expect(s.steps).toEqual(["done", "now", "empty"]);
    expect(s.view).toBe("report");
  });

  it("every section approved: ready to export", () => {
    const s = caseStatus(summary({ approved: TOTAL_SECTIONS }));
    expect(s.word).toBe("מוכן להוצאה");
    expect(s.steps).toEqual(["done", "done", "now"]);
  });

  it("a missing consent comes before everything else", () => {
    const s = caseStatus(summary({ approved: 3, consent: false }));
    expect(s.word).toBe("חסרה הסכמה");
    expect(s.attention).toBe(true);
  });

  it("a failed send is said first, and the case opens where it was", () => {
    const s = caseStatus(summary({ approved: 3, failed_at: 1 }));
    expect(s.word).toBe("השליחה נכשלה");
    expect(s.attention).toBe(true);
    expect(s.view).toBe("report");
  });

  it("no word appears twice in a row: the word, the detail and the button", () => {
    const all = [summary({}), summary({ approved: 2 }), summary({ approved: TOTAL_SECTIONS }), summary({ approved: 3, consent: false }), summary({ approved: 3, failed_at: 1 })];
    for (const s of all.map(caseStatus)) {
      const words = `${s.word} ${s.detail} ${s.button}`.split(/\s+/).filter((w) => w.length > 3);
      expect(new Set(words).size).toBe(words.length);
    }
  });
});
