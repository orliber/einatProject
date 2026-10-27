// The score profile: drawn in code from the numbers on the screen. What is checked is what it
// says (its accessible text, its labels, its axis), not how it looks. The scores are invented.
import { render, screen, within } from "@testing-library/react";
import type { Measure } from "../ipc/generated/Measure";
import { a11yViolations } from "../test/a11y";
import { PROFILE_SCALES, ScoreChart } from "./ScoreChart";

const bands = [
  { min: 40, max: 69, label: "נמוך מאוד", level: 1 },
  { min: 70, max: 84, label: "נמוך", level: 2 },
  { min: 85, max: 115, label: "ממוצע", level: 3 },
  { min: 116, max: 160, label: "גבוה", level: 4 },
];
const measure = (key: string, name_he: string, over: Partial<Measure> = {}): Measure => ({
  key, name_he, abbr: key.toUpperCase(), group: "index", scale: "standard", min: 40, max: 160, bands, aliases: [], ...over,
});
const standard = PROFILE_SCALES.find((s) => s.scale === "standard") ?? { title: "", mean: 100, sd: 15 };

function chart(rows: { measure: Measure; value: number }[]) {
  return render(<ScoreChart title={standard.title} mean={standard.mean} sd={standard.sd} rows={rows} />);
}

describe("the score profile", () => {
  const rows = [
    { measure: measure("vci", "הבנה מילולית"), value: 112 },
    { measure: measure("psi", "מהירות עיבוד"), value: 86 },
  ];

  it("has one accessible text with every score, in the order shown, and passes the accessibility check", async () => {
    const view = chart(rows);
    const img = screen.getByRole("img");
    expect(img).toHaveAccessibleName("פרופיל המדדים: הבנה מילולית 112, מהירות עיבוד 86");
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("names each scale beside its score, and each bar says what it is", () => {
    const view = chart(rows);
    const svg = view.container.querySelector("svg") as SVGSVGElement;
    expect(within(svg as unknown as HTMLElement).getByText("הבנה מילולית")).toBeInTheDocument();
    expect(within(svg as unknown as HTMLElement).getByText("112")).toBeInTheDocument();
    expect([...svg.querySelectorAll("title")].map((t) => t.textContent)).toEqual(["הבנה מילולית: 112", "מהירות עיבוד: 86"]);
  });

  it("the axis shows the mean and one and two standard deviations either side", () => {
    const view = chart(rows);
    const ticks = [...view.container.querySelectorAll("text.chart-tick")].map((t) => t.textContent);
    expect(ticks).toEqual(["70", "85", "100", "115", "130"]);
  });

  it("subtest scores use their own mean and spread, and only ticks inside the scale", () => {
    const scaled = PROFILE_SCALES.find((s) => s.scale === "scaled") ?? { title: "", mean: 10, sd: 3 };
    const sub = measure("similarities", "צורות", { scale: "scaled", min: 1, max: 19, bands: [{ min: 1, max: 19, label: "טווח", level: 3 }] });
    const view = render(<ScoreChart title={scaled.title} mean={scaled.mean} sd={scaled.sd} rows={[{ measure: sub, value: 13 }]} />);
    expect(screen.getByRole("img")).toHaveAccessibleName("פרופיל תת-המבחנים: צורות 13");
    expect([...view.container.querySelectorAll("text.chart-tick")].map((t) => t.textContent)).toEqual(["4", "7", "10", "13", "16"]);
  });

  it("a score at the edge of the scale, or beyond it, still has a bar inside the drawing", () => {
    const view = chart([{ measure: measure("fsiq", "מנת משכל כללית"), value: 40 }, { measure: measure("wmi", "זיכרון עבודה"), value: 200 }]);
    const bars = [...view.container.querySelectorAll("rect.chart-bar")];
    expect(bars).toHaveLength(2);
    for (const bar of bars) {
      expect(Number(bar.getAttribute("width"))).toBeGreaterThanOrEqual(2);
      expect(Number(bar.getAttribute("x"))).toBeGreaterThanOrEqual(0);
    }
    // The text still says the score as it was entered.
    expect(screen.getByRole("img")).toHaveAccessibleName("פרופיל המדדים: מנת משכל כללית 40, זיכרון עבודה 200");
  });

  it("draws nothing without scores", () => {
    const view = chart([]);
    expect(view.container).toBeEmptyDOMElement();
  });
});
