// The help's pictures: files inside the program (no network), taken from the preview's invented
// cases by `e2e/help-shots.spec.ts` (D-206). A missing picture is no error: the help shows
// its words alone.
const files = import.meta.glob<string>("./shots/*.webp", { eager: true, query: "?url", import: "default" });

export function shotUrl(name: string | undefined): string | undefined {
  return name ? files[`./shots/${name}.webp`] : undefined;
}

/** The names of all pictures in the program (for the tests). */
export function shotNames(): string[] {
  return Object.keys(files).map((k) => k.replace(/^\.\/shots\//, "").replace(/\.webp$/, ""));
}
