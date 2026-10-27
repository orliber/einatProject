import type { InterpretationView, StyleItem, StyleOverview, StyleSuggestion } from "../../ipc/client";

// הסגנון שלי · what waits for her

/** The draft items she already said something about, so leaving the page does not ask again. */
const REVIEWED_KEY = "dv.style.reviewed";
export function loadReviewed(): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(REVIEWED_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}
export function storeReviewed(ids: string[]) {
  try {
    if (ids.length === 0) localStorage.removeItem(REVIEWED_KEY);
    else localStorage.setItem(REVIEWED_KEY, JSON.stringify(ids));
  } catch {
    // Without storage she is only asked again about what she already saw; nothing is used unasked.
  }
}

export type Waiting =
  | { kind: "item"; key: string; item: StyleItem }
  | { kind: "draft"; key: string; items: number }
  | { kind: "suggestion"; key: string; s: StyleSuggestion }
  | { kind: "interpretation"; key: string; view: InterpretationView };

const sameItem = (a: StyleItem, b: StyleItem) => a.kind === b.kind && a.section === b.section && a.text === b.text;

/** Everything that waits for her yes, in one line: the draft's new items, then what was learned
 * from her edits, then how she interprets scores. An item the profile in use already holds is
 * not asked about again. */
export function waitingFor(o: StyleOverview, reviewed: string[]): Waiting[] {
  const out: Waiting[] = [];
  if (o.draft) {
    const known = (o.active?.profile.items ?? []).filter((i) => i.enabled);
    const fresh = o.draft.profile.items.filter((i) => !reviewed.includes(i.id) && !known.some((k) => sameItem(k, i)));
    if (fresh.length > 0) out.push(...fresh.map((item): Waiting => ({ kind: "item", key: `item:${item.id}`, item })));
    else out.push({ kind: "draft", key: `draft:${o.draft.id}`, items: o.draft.profile.items.filter((i) => i.enabled).length });
  }
  out.push(...o.suggestions.map((s): Waiting => ({ kind: "suggestion", key: `sugg:${s.id}`, s })));
  out.push(...o.interpretations.filter((i) => !i.item.approved).map((view): Waiting => ({ kind: "interpretation", key: `int:${view.item.id}`, view })));
  return out;
}
