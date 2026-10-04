// Automatic accessibility check (UX-7): axe-core against WCAG 2.1 A and AA rules.
import axe from "axe-core";

/** Each violation as one readable line; empty when the screen passes. */
export async function a11yViolations(node: Element): Promise<string[]> {
  const result = await axe.run(node, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
    // jsdom does not lay out or paint, so contrast cannot be measured here.
    rules: { "color-contrast": { enabled: false } },
  });
  return result.violations.map(
    (v) => `${v.id}: ${v.help} → ${v.nodes.map((n) => n.target.join(" ")).join(" | ")}`,
  );
}
