// The finish page's notes, as cards: what each kind is called in plain words, and the sentence
// of the paragraph a note is about, with the words in question marked. The sentence is found
// by plain string search and drawn from text nodes only: a note or a paragraph is never markup.
import type { NoteKind } from "../../ipc/generated/NoteKind";

/** A note's title, by kind: what she would call it. */
export const NOTE_TITLE: Record<NoteKind, string> = {
  score_mismatch: "מספר שלא תואם את טבלת הציונים",
  band_word: "תיאור רמה שלא תואם את הציון",
  percentile: "אחוזון שלא תואם את הציון",
  cutoff: "ציון מול ערך הסף",
  age: "גיל שלא תואם את פרטי התיק",
  gender: "לשון זכר או נקבה",
  grade: "כיתה או מסגרת שלא תואמות את פרטי התיק",
  recommendation_basis: "המלצה בלי ממצא שמסביר אותה",
  dsm_severity: "רמת חומרה לפי DSM",
  term: "מונח מקצועי",
  interpretation: "פירוש של תוצאה",
  number_not_in_sources: "מספר שלא מופיע בחומרים",
  name_in_answer: "שם שנשאר בטקסט",
  past_report_overlap: "משפט שדומה לדוח קודם",
  no_source: "פסקה שלא נשענת על חומר מהתיק",
  verify_contradicted: "משפט שסותר את החומרים",
  verify_unsupported: "משפט בלי בסיס בחומרים",
  verify_partial: "משפט שרק חלקו נשען על החומרים",
  polish: "ניסוח",
};

/** A note the core sent without a kind. */
export const NOTE_TITLE_PLAIN = "הערה";

/** The title of a note. */
export function noteTitle(kind: NoteKind | null): string {
  return kind ? NOTE_TITLE[kind] : NOTE_TITLE_PLAIN;
}

/** The note's own words, without the "to check" the core ends some lines with: the list
 *  already says so. */
export function noteWhy(text: string): string {
  return text.replace(/\s+[–-]\s*לבדוק\s*$/, "").trim();
}

const QUOTED = /"([^"\n]+)"|«([^»\n]+)»|״([^״\n]+)״|“([^”\n]+)”/g;
const NUMBER = /\d+(?:[.,]\d+)?/g;
const WORDY = /[\p{L}\p{N}]/u;

/** What a note points at, in the order worth trying: the words it quotes, then its numbers. */
export function needlesOf(note: string): string[] {
  const out: string[] = [];
  for (const m of note.matchAll(QUOTED)) {
    const q = (m[1] ?? m[2] ?? m[3] ?? m[4] ?? "").trim();
    if (WORDY.test(q) && !out.includes(q)) out.push(q);
  }
  for (const m of note.matchAll(NUMBER)) if (!out.includes(m[0])) out.push(m[0]);
  return out;
}

const DIGIT = /\d/;

/** Where the needle is in the text; a number only as a whole number, never inside another. */
function locate(text: string, needle: string): [number, number] | null {
  let from = 0;
  for (;;) {
    const at = text.indexOf(needle, from);
    if (at < 0) return null;
    const end = at + needle.length;
    const numeric = DIGIT.test(needle[0] ?? "") && DIGIT.test(needle[needle.length - 1] ?? "");
    const inside = numeric && (DIGIT.test(text[at - 1] ?? "") || DIGIT.test(text[end] ?? ""));
    if (!inside) return [at, end];
    from = at + 1;
  }
}

export interface Excerpt {
  before: string;
  mark: string;
  after: string;
}

const HEAD = 140;
const LONG = 190;
const END = /[.!?\n]/;

/** The sentence around the first needle found in the paragraph; with nothing found, the
 *  paragraph's beginning and no mark. */
export function excerptOf(text: string, needles: string[]): Excerpt {
  let hit: [number, number] | null = null;
  for (const n of needles) {
    hit = n ? locate(text, n) : null;
    if (hit) break;
  }
  if (!hit) return { before: text.length > HEAD ? `${text.slice(0, HEAD).trimEnd()}…` : text, mark: "", after: "" };
  const [at, end] = hit;
  // A sentence ends at a mark followed by a space or by the end ("5.4" is not an end).
  let start = 0;
  for (let i = at - 1; i >= 0; i--) {
    if (END.test(text[i] ?? "") && (text[i] === "\n" || /\s/.test(text[i + 1] ?? ""))) {
      start = i + 1;
      break;
    }
  }
  let stop = text.length;
  for (let i = end; i < text.length; i++) {
    if (END.test(text[i] ?? "") && (text[i] === "\n" || i + 1 === text.length || /\s/.test(text[i + 1] ?? ""))) {
      stop = text[i] === "\n" ? i : i + 1;
      break;
    }
  }
  let head = "";
  let tail = "";
  if (stop - start > LONG) {
    const s = Math.max(start, at - 70);
    const e = Math.min(stop, end + 90);
    if (s > start) head = "…";
    if (e < stop) tail = "…";
    start = s;
    stop = e;
  }
  return {
    before: head + text.slice(start, at).trimStart(),
    mark: text.slice(at, end),
    after: text.slice(end, stop).trimEnd() + tail,
  };
}

/** The sentence, with the words in question marked. Text nodes only. */
export function Quote({ text, needles }: { text: string; needles: string[] }) {
  const x = excerptOf(text, needles);
  return (
    <>
      {x.before}
      {x.mark && <mark className="a4-mark">{x.mark}</mark>}
      {x.after}
    </>
  );
}
