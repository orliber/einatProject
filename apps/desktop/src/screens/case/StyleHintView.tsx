// Next to a proposed paragraph of the AI: how close it is to her writing (ST7), a request to
// bring it closer (ST8), and the phrases she usually changes, with her usual wording (ST9).
// Everything here was worked out on her computer; the text is shown as text.
import { useState } from "react";
import type { StyleHint } from "../../ipc/generated/StyleHint";
import type { StyleMark } from "../../ipc/generated/StyleMark";

const LETTER = /[\p{L}\p{N}]/u;

/** Where `phrase` stands in `text` as whole words: [start, end) pairs. */
export function wholeWordSpans(text: string, phrase: string): [number, number][] {
  const out: [number, number][] = [];
  if (!phrase.trim()) return out;
  let at = text.indexOf(phrase);
  while (at !== -1) {
    const before = at > 0 ? text[at - 1] ?? "" : "";
    const after = text[at + phrase.length] ?? "";
    if (!LETTER.test(before) && !LETTER.test(after)) out.push([at, at + phrase.length]);
    at = text.indexOf(phrase, at + phrase.length);
  }
  return out;
}

/** The paragraph with her usual wording in place of `mark` (or without it, for a deletion). */
export function applyMark(text: string, mark: StyleMark): string {
  const spans = wholeWordSpans(text, mark.from);
  let out = text;
  for (const [s, e] of [...spans].reverse()) out = out.slice(0, s) + mark.to + out.slice(e);
  if (!mark.to) out = out.replace(/ {2,}/g, " ").replace(/ +([,.;:])/g, "$1").replace(/^ +/gm, "");
  return out;
}

/** The paragraph cut into plain pieces and the pieces she usually changes. */
export function markPieces(text: string, marks: StyleMark[]): { text: string; mark?: StyleMark }[] {
  const spans = marks
    .flatMap((m) => wholeWordSpans(text, m.from).map(([s, e]) => ({ s, e, m })))
    .sort((a, b) => a.s - b.s);
  const pieces: { text: string; mark?: StyleMark }[] = [];
  let at = 0;
  for (const { s, e, m } of spans) {
    if (s < at) continue; // overlapping phrases: the first one wins
    if (s > at) pieces.push({ text: text.slice(at, s) });
    pieces.push({ text: text.slice(s, e), mark: m });
    at = e;
  }
  if (at < text.length) pieces.push({ text: text.slice(at) });
  return pieces;
}

/** The paragraph's text with her usual changes marked. */
export function MarkedText({ text, marks }: { text: string; marks: StyleMark[] }) {
  if (marks.length === 0) return <>{text}</>;
  return (
    <>
      {markPieces(text, marks).map((p, i) =>
        p.mark ? (
          <mark key={i} className="habit" title={p.mark.to ? `בדרך כלל את כותבת «${p.mark.to}»` : "בדרך כלל את מוחקת את זה"}>{p.text}</mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}

function level(fit: number): string {
  if (fit >= 75) return "chip chip-ok";
  if (fit >= 50) return "chip chip-sand";
  return "chip chip-warn";
}

export function StyleHintView({ hint, busy, onCloser, onApply }: {
  hint: StyleHint;
  busy: boolean;
  /** Ask the AI to rewrite the paragraph with `hint.ask`. */
  onCloser: (ask: string) => void;
  /** Put her usual wording in place of a marked phrase. */
  onApply: (mark: StyleMark) => void;
}) {
  const [open, setOpen] = useState(false);
  const { fit, details, ask, marks } = hint;
  if (fit === null && marks.length === 0) return null;
  return (
    <div className="style-hint">
      {fit !== null && (
        <div className="row wrap-row style-hint-row">
          <button type="button" className={`${level(fit)} style-fit-chip`} aria-expanded={open} onClick={() => setOpen(!open)}>
            קרוב לסגנון שלך: {fit}
          </button>
          {ask && (
            <button type="button" className="btn btn-small" disabled={busy} onClick={() => onCloser(ask)}>לקרב לסגנון שלי</button>
          )}
        </div>
      )}
      {open && fit !== null && (
        details.length > 0 ? (
          <ul className="small muted style-details">
            {details.map((d) => <li key={d}>{d}</li>)}
          </ul>
        ) : (
          <p className="small muted style-details">הפסקה נשמעת כמו הכתיבה שלך.</p>
        )
      )}
      {marks.length > 0 && (
        <div className="row wrap-row style-habits">
          <span className="small muted">בדרך כלל את משנה:</span>
          {marks.map((m) => (
            <button key={m.from} type="button" className="btn btn-small btn-ghost" disabled={busy} onClick={() => onApply(m)}>
              {m.to ? `«${m.from}» ← «${m.to}»` : `למחוק «${m.from}»`}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
