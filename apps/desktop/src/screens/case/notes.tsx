// D-161: no note gets lost, and the page stays clean. Everything there is to say about a
// paragraph (a number unlike the score table, a name in the answer, what the check against
// the sources found) is one folded line under it that opens on a click; what the answer said
// about the whole section (its questions, what is missing) is one folded line too, shown only
// for the section in focus. Nothing is drawn when there is nothing to say. Plain text only.
import type { CaseDetail } from "../../ipc/client";
import type { NoteKind } from "../../ipc/generated/NoteKind";
import type { SectionNotes } from "../../ipc/generated/SectionNotes";
import { WarnIcon } from "../../components/ui";

type Paragraph = CaseDetail["sections"][number]["paragraphs"][number];

/** A note to look at: its kind when the core gave one. */
export interface HeldNote {
  kind: NoteKind | null;
  text: string;
}

/** The kinds that keep a small visible mark on the folded line: the paragraph says something
 *  the case does not. */
const BLOCKING: readonly NoteKind[] = ["score_mismatch", "name_in_answer", "verify_contradicted"];

/** The mark's words, by kind. */
const MARK: Partial<Record<NoteKind, string>> = {
  score_mismatch: "מספר שלא תואם את טבלת הציונים",
  name_in_answer: "שם בתשובה",
  verify_contradicted: "סותר את החומרים",
};

/** Said before a line of the check against the sources, so it reads by itself. */
const LEAD: Partial<Record<NoteKind, string>> = {
  verify_contradicted: "בבדיקה מול המקורות, סתירה: ",
  verify_unsupported: "בבדיקה מול המקורות, בלי בסיס: ",
  verify_partial: "בבדיקה מול המקורות, בסיס חלקי: ",
};

/** What holds a paragraph for her eye: every note but the proofreading. A warning the core
 *  sent without a kind is still a note. */
export function heldNotes(p: Pick<Paragraph, "notes" | "warnings">): HeldNote[] {
  const notes: HeldNote[] = (p.notes ?? []).filter((n) => n.kind !== "polish");
  for (const w of p.warnings ?? []) if (!notes.some((n) => n.text === w)) notes.push({ kind: null, text: w });
  return notes;
}

/** Nothing to look at: "approve all" may take it. */
export function isClean(p: Pick<Paragraph, "notes" | "warnings">): boolean {
  return heldNotes(p).length === 0;
}

/** A note as one plain line. */
export function noteLine(n: HeldNote): string {
  return `${(n.kind && LEAD[n.kind]) ?? ""}${n.text}`;
}

/** Under a paragraph: one quiet line with the count, opening to the notes. */
export function ParagraphNotes({ paragraph }: { paragraph: Pick<Paragraph, "notes" | "warnings"> }) {
  const notes = heldNotes(paragraph);
  if (notes.length === 0) return null;
  const marks = Array.from(new Set(notes.flatMap((n) => (n.kind && BLOCKING.includes(n.kind) ? [MARK[n.kind] ?? ""] : []))));
  const count = notes.length === 1 ? "הערה אחת לבדיקה" : `${notes.length} הערות לבדיקה`;
  return (
    <details className={marks.length ? "note-fold small is-blocking" : "note-fold small"} aria-label="הערות לבדיקה">
      <summary>
        {marks.length > 0 && <WarnIcon />}
        <span>{count}{marks.length > 0 ? ` · ${marks.join(" · ")}` : ""}</span>
      </summary>
      <ul>{notes.map((n, i) => <li key={i}>{noteLine(n)}</li>)}</ul>
    </details>
  );
}

const count = (n: number, one: string, many: (n: number) => string) => (n === 1 ? one : many(n));

/** The section's line: what the last answer said beside its paragraphs, in a few words. */
export function sectionNotesLine(notes: SectionNotes | undefined, ai: string): string | null {
  if (!notes) return null;
  const parts = [
    notes.questions.length ? `${ai} שאל ${count(notes.questions.length, "שאלה אחת", (n) => `${n} שאלות`)}` : "",
    notes.missing.length ? `חסר מידע ${count(notes.missing.length, "בנושא אחד", (n) => `ב-${n} נושאים`)}` : "",
    notes.contradictions.length ? count(notes.contradictions.length, "סתירה אחת בין החומרים", (n) => `${n} סתירות בין החומרים`) : "",
    notes.unanswered.length ? count(notes.unanswered.length, "שורה אחת בהנחיה בלי פסקה", (n) => `${n} שורות בהנחיה בלי פסקה`) : "",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : null;
}

/** For the section in focus: one folded line, opening to the lists. */
export function SectionNotesFold({ notes, ai }: { notes: SectionNotes | undefined; ai: string }) {
  const line = sectionNotesLine(notes, ai);
  if (!notes || !line) return null;
  const groups: [string, string[]][] = [
    [`${ai} שאל`, notes.questions],
    ["חסר בחומרים", notes.missing],
    ["סתירות בין החומרים", notes.contradictions],
    ["שורות בהנחיה שאין עליהן פסקה", notes.unanswered],
  ];
  return (
    <details className="note-fold note-section small" aria-label="מה עלה בניסוח הסעיף">
      <summary><span>{line}</span></summary>
      <div className="note-groups">
        {groups.filter(([, lines]) => lines.length > 0).map(([title, lines]) => (
          <div key={title}>
            <b>{title}</b>
            <ul>{lines.map((l, i) => <li key={i}>{l}</li>)}</ul>
          </div>
        ))}
        <span className="muted">אפשר לענות ב"✦ לשנות", להוסיף חומר, או להשאיר כך.</span>
      </div>
    </details>
  );
}
