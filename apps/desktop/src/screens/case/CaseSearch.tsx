// U7: search inside the open case, in its materials and its paragraphs. On this computer, in
// what the case screen already holds; nothing is sent. Results are shown as text.
import { regularFinals } from "../../searchForm";
import { useEffect, useMemo, useState } from "react";
import type { CaseDetail } from "../../ipc/client";
import { kindLabel } from "../../i18n/he";
import { Dialog } from "../../components/ui";

export interface Hit {
  where: "material" | "paragraph";
  /** The material's id, or the section's key. */
  target: string;
  title: string;
  /** About 40 characters on each side of the first match. */
  before: string;
  match: string;
  after: string;
  /** Matches in this text. */
  count: number;
}

const AROUND = 40;
/** Niqqud and cantillation, which a search should not have to type. */
const MARKS = /[\u0591-\u05C7]/g;
const MARK = /[\u0591-\u05C7]/;

function fold(s: string): string {
  return regularFinals(s.replace(MARKS, "").toLowerCase());
}

/** A text without niqqud, in lower case and with regular letters for final ones ("גן" finds
 *  "גנים"), with the place of each of its characters in the original. */
interface Folded {
  folded: string;
  /** `null` when nothing was dropped and no character changed length: positions are the same. */
  keep: number[] | null;
}

/** The folded copy of each text, made once: a long prior report is not folded again on every key. */
const foldedTexts = new Map<string, Folded>();
const FOLDED_MAX = 400;

function foldText(text: string): Folded {
  const held = foldedTexts.get(text);
  if (held) return held;
  let made: Folded;
  const lower = text.toLowerCase();
  if (!MARK.test(text) && lower.length === text.length) {
    made = { folded: regularFinals(lower), keep: null };
  } else {
    // Index on a copy without niqqud, then map the position back to the original text.
    const keep: number[] = [];
    const parts: string[] = [];
    for (let i = 0; i < text.length; i++) {
      const c = text[i] ?? "";
      if (MARK.test(c)) continue;
      const l = c.toLowerCase();
      parts.push(regularFinals(l));
      // A letter whose lower case is longer keeps one place per character of it.
      for (let k = 0; k < l.length; k++) keep.push(i);
    }
    made = { folded: parts.join(""), keep };
  }
  if (foldedTexts.size >= FOLDED_MAX) foldedTexts.clear();
  foldedTexts.set(text, made);
  return made;
}

/** Nothing of a case stays here once the vault locks or the search closes. */
export function forgetFoldedTexts() {
  foldedTexts.clear();
}

/** Where `query` appears in `text`, ignoring niqqud and case; the snippet keeps the original. */
function find(text: string, query: string): Omit<Hit, "where" | "target" | "title"> | null {
  const { folded, keep } = foldText(text);
  const q = fold(query.trim());
  if (!q) return null;
  const at = folded.indexOf(q);
  if (at < 0) return null;
  let count = 0;
  for (let i = at; i >= 0; i = folded.indexOf(q, i + q.length)) count++;
  const start = keep ? (keep[at] ?? 0) : at;
  const end = keep ? (keep[at + q.length - 1] ?? start) + 1 : at + q.length;
  const from = Math.max(0, start - AROUND);
  const to = Math.min(text.length, end + AROUND);
  return {
    before: (from > 0 ? "…" : "") + text.slice(from, start),
    match: text.slice(start, end),
    after: text.slice(end, to) + (to < text.length ? "…" : ""),
    count,
  };
}

/** Every material and paragraph of the case that holds `query`: materials first. */
export function searchCase(detail: CaseDetail, query: string): Hit[] {
  if (fold(query.trim()).length < 2) return [];
  const hits: Hit[] = [];
  for (const m of detail.inputs) {
    const f = find(`${m.title}\n${m.content}`, query);
    if (f) hits.push({ where: "material", target: m.id, title: `${kindLabel[m.kind]} · ${m.title}`, ...f });
  }
  for (const s of detail.sections) {
    for (const p of s.paragraphs) {
      const f = find(p.text, query);
      if (f) hits.push({ where: "paragraph", target: s.key, title: s.title, ...f });
    }
  }
  return hits;
}

export function CaseSearch({ detail, onClose, onOpen }: {
  detail: CaseDetail;
  onClose: () => void;
  onOpen: (hit: Hit) => void;
}) {
  const [query, setQuery] = useState("");
  useEffect(() => forgetFoldedTexts, []);
  const hits = useMemo(() => searchCase(detail, query), [detail, query]);
  const materials = hits.filter((h) => h.where === "material");
  const paragraphs = hits.filter((h) => h.where === "paragraph");
  const group = (title: string, list: Hit[]) =>
    list.length > 0 && (
      <section className="search-group" aria-label={title}>
        <h3 className="small muted">{title} ({list.length})</h3>
        <ul className="search-hits">
          {list.map((h, i) => (
            <li key={`${h.target}-${i}`}>
              <button type="button" className="search-hit" onClick={() => onOpen(h)}>
                <span className="search-title">{h.title}{h.count > 1 ? ` · ${h.count} פעמים` : ""}</span>
                <span className="search-snippet serif">{h.before}<mark>{h.match}</mark>{h.after}</span>
              </button>
            </li>
          ))}
        </ul>
      </section>
    );
  return (
    <Dialog help="case-search" title="חיפוש בתיק" subtitle="בחומרים ובפסקאות של התיק הזה. החיפוש נעשה במחשב בלבד." onClose={onClose}>
      <input className="input" type="search" autoFocus placeholder="מילה או ביטוי" aria-label="חיפוש בתיק"
        value={query} onChange={(e) => setQuery(e.target.value)} />
      {fold(query.trim()).length >= 2 && hits.length === 0 && <p className="muted">לא נמצא בתיק.</p>}
      {group("בחומרים", materials)}
      {group("בדוח", paragraphs)}
    </Dialog>
  );
}
