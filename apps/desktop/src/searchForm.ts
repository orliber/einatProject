// A text as a search compares it, the same as the core's `search_form` (dv-domain, clinic
// rules): lower case, and a Hebrew final letter as its regular form, so that "גן" also finds
// "גנים". One letter stays one letter, so a place in the folded text is the place in the text.
const FINALS: Record<string, string> = { "ך": "כ", "ם": "מ", "ן": "נ", "ף": "פ", "ץ": "צ" };

export function searchLetter(c: string): string {
  const regular = FINALS[c];
  if (regular !== undefined) return regular;
  const lower = c.toLowerCase();
  // A letter whose lower case is more than one letter keeps its place as it is.
  return Array.from(lower).length === 1 ? lower : c;
}

export function searchForm(text: string): string {
  return Array.from(text, searchLetter).join("");
}

/** Only the final letters, for a text that was already put in lower case. */
export function regularFinals(text: string): string {
  return text.replace(/[ךםןףץ]/g, (c) => FINALS[c] ?? c);
}
