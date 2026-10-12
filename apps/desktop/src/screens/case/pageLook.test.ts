// V4: the report page takes her template's font, size, page and margins.
import { pageLook } from "./ReportView";
import type { TemplateView } from "../../ipc/client";

const base: TemplateView = {
  has_marker: true, headers: 1, footers: 0, images: 0, styles_matched: 3, styles_total: 9, size_kb: 20,
  font: null, size_half_points: null, heading_font: null, page_twips: null, margins_twips: null,
};

describe("pageLook", () => {
  it("uses the template's font, size, page and margins", () => {
    const { style, fromTemplate } = pageLook(
      { ...base, font: "Narkisim", size_half_points: 24, heading_font: "David", page_twips: [11906, 16838], margins_twips: [1800, 1300, 1440, 1300] },
      "Arial",
    );
    expect(fromTemplate).toBe(true);
    expect(style.fontFamily).toBe('"Narkisim", var(--font-display)');
    expect(style.fontSize).toBe("12pt");
    expect(style.width).toBe("min(794px, 100%)");
    expect(style.padding).toBe("120px 87px 96px 87px");
    expect((style as Record<string, string>)["--a4-heading-font"]).toBe('"David"');
  });

  it("falls back to the font from Settings, and to nothing", () => {
    expect(pageLook(null, "Arial")).toEqual({ style: { fontFamily: '"Arial", var(--font-display)' }, fromTemplate: false });
    expect(pageLook({ ...base }, null)).toEqual({ style: {}, fromTemplate: false });
  });
});
