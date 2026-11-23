// A scroll that glides is movement too: with "reduce motion" on in the system (WCAG 2.3.3),
// the page jumps to its place instead.
export function scrollGlide(): ScrollBehavior {
  const reduce = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  return reduce ? "auto" : "smooth";
}
