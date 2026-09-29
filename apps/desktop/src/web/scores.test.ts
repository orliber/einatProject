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
  it("reads a half-point CARS score and refuses an impossible one", () => {
    const one = (v: number) => formatSheet({ instrument: "cars_2", module: "", cutoff: null, notes: "", entries: [{ measure: "total", value: v, note: "" }] });
    expect(one(29.5)).toContain("ציון 29.5 – מעט או ללא תסמינים");
    expect(() => one(80)).toThrow(/מחוץ לטווח/);
  });
});
