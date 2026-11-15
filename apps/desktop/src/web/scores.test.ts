import { describe, expect, it } from "vitest";
import { formatSheet } from "./scores";

// The same sheet as crates/dv-domain/src/scores.rs `sheet_text_with_ranges_gap_note_and_cutoff`.
describe("browser preview score text matches the core", () => {
  it("writes ranges, percentiles and the gap note like the Rust formatter", () => {
    const text = formatSheet({
      instrument: "wppsi_iv", module: "", cutoff: null, notes: "שיתף פעולה לאורך כל ההעברה",
      entries: [
        { measure: "vci", value: 112, note: "" },
        { measure: "psi", value: 88, note: "עבד לאט ובדייקנות" },
        { measure: "matrix", value: 13, note: "" },
      ],
    });
    expect(text).toContain("- הבנה מילולית (VCI): ציון 112, אחוזון 79 – ממוצע גבוה");
    expect(text).toContain("- מהירות עיבוד (PSI): ציון 88, אחוזון 21 – ממוצע נמוך. עבד לאט ובדייקנות");
    expect(text).toContain("- מטריצות (MR): ציון 13, אחוזון 84 – ממוצע גבוה");
    expect(text).toContain("פער של 24 נקודות");
  });
  it("gives relative places, elevated scales and an order without totals, like the core (D-165)", () => {
    const wppsi = formatSheet({
      instrument: "wppsi_iv", module: "", cutoff: null, notes: "",
      entries: [
        { measure: "block_design", value: 6, note: "" },
        { measure: "matrix", value: 13, note: "" },
        { measure: "information", value: 10, note: "" },
        { measure: "fsiq", value: 98, note: "" },
      ],
    });
    expect(wppsi).toContain("חוזקות וחולשות יחסיות, ביחס לממוצע האישי בתת-מבחנים שבטבלה (הפרש של 3 נקודות ומעלה), חושבו בתוכנה. חוזקה יחסית: מטריצות (MR) – ציון 13. חולשה יחסית: קוביות (BD) – ציון 6\n");
    const brief = formatSheet({
      instrument: "brief_p", module: "", cutoff: null, notes: "",
      entries: [
        { measure: "inhibit", value: 66, note: "" },
        { measure: "shift", value: 50, note: "" },
        { measure: "isci", value: 70, note: "" },
        { measure: "fi", value: 55, note: "" },
        { measure: "gec", value: 66, note: "" },
      ],
    });
    expect(brief).toContain("סולמות בטווח חריג לפי הטבלה: עכבה (INH) – ציון 66, מוגבר באופן מובהק קלינית; שליטה עצמית מעכבת (ISCI) – ציון 70, מוגבר באופן מובהק קלינית; מדד ניהולי כללי (GEC) – ציון 66, מוגבר באופן מובהק קלינית\n");
    expect(brief).toContain("מדדים מהגבוה לנמוך: שליטה עצמית מעכבת (ISCI) – ציון 70; מדד גמישות (FI) – ציון 55\n");
    const ados = formatSheet({
      instrument: "ados_2", module: "1", cutoff: 11, notes: "",
      entries: [
        { measure: "social_affect", value: 10, note: "" },
        { measure: "rrb", value: 4, note: "" },
        { measure: "comparison", value: 7, note: "" },
      ],
    });
    expect(ados).not.toContain("מהגבוה לנמוך");
  });
  it("reads a half-point CARS score and refuses an impossible one", () => {
    const one = (v: number) => formatSheet({ instrument: "cars_2", module: "", cutoff: null, notes: "", entries: [{ measure: "total", value: v, note: "" }] });
    expect(one(29.5)).toContain("ציון 29.5 – מעט או ללא תסמינים");
    expect(() => one(80)).toThrow(/מחוץ לטווח/);
  });
  it("refuses a score that is not a number, like the core does", () => {
    const one = (v: number) => formatSheet({ instrument: "cars_2", module: "", cutoff: null, notes: "", entries: [{ measure: "total", value: v, note: "" }] });
    expect(() => one(Number.NaN)).toThrow(/מחוץ לטווח/);
    expect(() => one(Number.POSITIVE_INFINITY)).toThrow(/מחוץ לטווח/);
  });
});
