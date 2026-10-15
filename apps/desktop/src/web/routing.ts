// Browser preview only: the same routing rules as crates/dv-domain/src/routing.rs (D-022), so
// the preview behaves like the app. The real app never runs this; the Rust core decides.

const MAX_LINES = 12;

function endsSentence(line: string): boolean {
  const end = line.replace(/["'״”)\]]+$/u, "");
  return /[.!?;׃]$/u.test(end) && !end.endsWith("..");
}

/** Paragraphs: wrapped lines joined, a heading kept with what follows. Never cut by length. */
export function passages(text: string): string[] {
  const out: string[] = [];
  let current: string[] = [];
  const close = () => {
    if (current.length) out.push(current.join("\n"));
    current = [];
  };
  for (const raw of text.split("\n")) {
    const line = raw.replace(/\r$/u, "").trim();
    if (!line) {
      close();
      continue;
    }
    current.push(line);
    if (endsSentence(line) || current.length >= MAX_LINES) close();
  }
  close();
  return out;
}

export interface Suggestion {
  byAi: boolean;
  passageCount: number;
  sections: { section: string; passages: number[] }[];
}

/** What the reading said about a material (round 31), for the text it was read from. */
export interface Reading {
  passageCount: number;
  text: string;
  record?: { kind: string; by: string; setting: "institute" | "community" | "unknown"; findings: string; recommendations: string };
}

export interface Routing {
  suggestion?: Suggestion;
  added: string[];
  removed: string[];
  unplaced?: number;
  reading?: Reading;
}

export function readingOf(r: Routing, count: number): Reading | undefined {
  return r.reading && r.reading.passageCount === count ? r.reading : undefined;
}

export type Feed = "whole" | number[];

function fresh(r: Routing, count: number): Suggestion | undefined {
  return r.suggestion && r.suggestion.passageCount === count ? r.suggestion : undefined;
}

export function base(r: Routing, table: string[], count: number): string[] {
  const s = fresh(r, count);
  return s ? s.sections.filter((x) => x.passages.length > 0).map((x) => x.section) : [...table];
}

export function isSorted(r: Routing, count: number): boolean {
  return fresh(r, count) !== undefined;
}

export function needsSorting(r: Routing, count: number): boolean {
  return count > 0 && ((!isSorted(r, count) && r.unplaced !== count) || !readingOf(r, count));
}

/** The latest "month/year" written in a document, leaving out lines about a birth. */
export function documentDate(text: string): [number, number] | null {
  let best: [number, number] | null = null;
  for (const line of text.split("\n")) {
    if (line.includes("לידה") || line.includes("נולד")) continue;
    for (const m of line.matchAll(/(?<!\d)(?:(\d{1,2})[./-])?(\d{1,2})[./-](\d{4}|\d{2})(?!\d)/g)) {
      const year = (m[3] ?? "").length === 2 ? 2000 + Number(m[3]) : Number(m[3]);
      const month = Number(m[2]);
      if ((m[1] === undefined && (m[3] ?? "").length !== 4) || month < 1 || month > 12 || year < 1990 || year > 2099) continue;
      if (!best || year > best[0] || (year === best[0] && month > best[1])) best = [year, month];
    }
  }
  return best;
}

export function feed(r: Routing, section: string, table: string[], count: number): Feed | null {
  if (r.removed.includes(section)) return null;
  if (r.added.includes(section)) return "whole";
  const s = fresh(r, count);
  if (s) {
    const chosen = Array.from(new Set(s.sections.filter((x) => x.section === section).flatMap((x) => x.passages)))
      .filter((n) => n >= 1 && n <= count)
      .sort((a, b) => a - b);
    return chosen.length ? chosen : null;
  }
  return table.includes(section) ? "whole" : null;
}

export function choose(r: Routing, chosen: string[], table: string[], count: number): Routing {
  const b = base(r, table, count);
  return { ...r, added: chosen.filter((s) => !b.includes(s)), removed: b.filter((s) => !chosen.includes(s)) };
}

/** The chosen passages, with a mark where passages were skipped. */
export function excerpt(all: string[], chosen: number[]): string {
  let out = "";
  let last = 0;
  for (const n of chosen) {
    const p = all[n - 1];
    if (p === undefined) continue;
    if (out) out += n === last + 1 ? "\n\n" : "\n\n(…)\n\n";
    out += p;
    last = n;
  }
  return out;
}

/** Demo-mode sorting by words (crates/dv-ai/src/demo.rs → SECTION_WORDS). */
const SECTION_WORDS: [string, string[]][] = [
  ["referral", ["הפני", "סיבת", "לברר", "פנו לאבחון", "פנו בשל"]],
  ["background", ["הריון", "לידה", "נולד", "אבני דרך", "הלך בגיל", "מילים ראשונות", "אלרגי", "בריאות"]],
  ["parents_view", ["בבית", "לדברי ההורים", "לדברי האם", "לדברי האב", "ההורים מתארים", "ההורים מספרים"]],
  ["kindergarten", ["בגן", "גננת", "הסייעת", "בכיתה", "המורה", "המסגרת", "בחצר", "מפגש בוקר"]],
  ["prior_assessments", ["אבחון קודם", "טיפול", "קלינאית", "ריפוי בעיסוק", "פיזיותרפ", "נוירולוג", "התפתחות הילד"]],
  ["tools", ["WPPSI", "WISC", "ABAS", "ADOS", "CBCL", "ASRS", "CARS", "Bayley", "הועבר"]],
  ["appearance", ["הגיע", "נכנס לחדר", "נפרד", "שיתף פעולה", "במפגש", "בחדר"]],
  ["cognitive", ["ציון", "אחוזון", "הבנה מילולית", "זיכרון", "מהירות עיבוד", "חשיבה", "VCI", "FSIQ", "WPPSI", "WISC"]],
  ["adaptive", ["ABAS", "הסתגלות", "תפקוד יומיומי", "עצמאות"]],
  ["communication", ["שפה", "דיבור", "תקשורת", "משפטים", "קשר עין", "הצבעה", "ADOS", "תחומי עניין", "חזרתי", "הדדי"]],
  ["emotional", ["משחק", "רגש", "תסכול", "חרדה", "פחד", "ויסות", "בובות", "כעס"]],
];

export function sortLocally(texts: string[], keys: string[]): { section: string; passages: number[] }[] {
  return SECTION_WORDS.filter(([key]) => keys.includes(key))
    .map(([section, words]) => ({
      section,
      passages: texts.map((t, i) => (words.some((w) => t.includes(w)) ? i + 1 : 0)).filter((n) => n > 0),
    }))
    .filter((x) => x.passages.length > 0);
}
