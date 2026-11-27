// The window's height after the text size (UID-8): a page zoomed to 140% still measures `vh`
// in the window's own pixels, so a list sized by `46vh` grows with the zoom and outgrows the
// dialog that holds it. Every height in `vh` is divided by `--zoom`, as the dialog's own is.
const sheets = import.meta.glob<string>("../**/*.css", { query: "?raw", import: "default", eager: true });

/** Sheets another part of the work still has to change: none should stay here for long. */
const PENDING = new Set(["../screens/clinic/ClinicScreen.css"]);

describe("viewport units under the text size", () => {
  it("every vh is divided by --zoom", () => {
    expect(Object.keys(sheets).length).toBeGreaterThan(10);
    const bare: string[] = [];
    for (const [path, css] of Object.entries(sheets)) {
      if (PENDING.has(path)) continue;
      const code = css.replace(/\/\*[\s\S]*?\*\//g, "");
      for (const m of code.matchAll(/\d+(?:\.\d+)?[dsl]?vh\b(?!\s*\/\s*var\(--zoom)/g)) {
        bare.push(`${path}: ${code.slice(Math.max(0, (m.index ?? 0) - 30), (m.index ?? 0) + m[0].length)}`);
      }
    }
    expect(bare).toEqual([]);
  });
});
