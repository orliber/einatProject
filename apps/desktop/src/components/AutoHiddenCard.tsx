import { useState } from "react";
import { useApp } from "../App";
import { roleLabel } from "../i18n/he";
import { ipc } from "../ipc/client";
import type { AutoHidden } from "../ipc/generated/AutoHidden";
import type { Role } from "../ipc/generated/Role";
import { displayTag, ErrorLine, Spinner } from "./ui";
import "./AutoHiddenCard.css";
import { useAi } from "../ai";

/** Who a name the filter kept can be, in the card's role menu. */
export const foundRoles: Role[] = [
  "mother",
  "father",
  "brother",
  "sister",
  "relative",
  "teacher",
  "school_teacher",
  "assistant",
  "doctor",
  "slp",
  "therapist",
  "psychologist",
  "professional",
  "other_child",
  "family",
  "other",
];

/** A name inside a word first, then the weaker signs, then the rest (D-042). */
export function cardOrder(items: AutoHidden[]): AutoHidden[] {
  const rank = (a: AutoHidden) => (a.kind === "declared_in_word" ? 0 : a.uncertain ? 1 : 2);
  return [...items].sort((a, b) => rank(a) - rank(b));
}

/** One row on the card: every form of one name ("נועה", "ונועה", "לנועה") goes out as one tag. */
export type CardRow = { key: string; tokens: string[]; tag: string; role: Role; reason: string; uncertain: boolean; kind: AutoHidden["kind"] };

/** Worth a look: a name inside a word, a close spelling, or a weaker sign. */
export const needsLook = (r: CardRow) => r.kind === "declared_in_word" || r.kind === "similar_spelling" || r.uncertain;

/**
 * The card's rows: the forms of one person (same tag) together, so a name that appears in
 * six documents is one row. A generalized job stays one row per wording. Display only:
 * every item stays hidden exactly as the filter decided.
 */
export function cardRows(items: AutoHidden[]): CardRow[] {
  const rows = new Map<string, CardRow>();
  for (const a of cardOrder(items)) {
    const key = a.kind === "indirect" ? `${a.kind}\u0000${a.token}\u0000${a.tag}` : `${a.kind}\u0000${a.tag}`;
    const row = rows.get(key);
    if (!row) {
      rows.set(key, { key, tokens: [a.token], tag: a.tag, role: a.role, reason: a.reason, uncertain: a.uncertain, kind: a.kind });
    } else {
      if (!row.tokens.includes(a.token)) row.tokens.push(a.token);
      row.uncertain ||= a.uncertain;
    }
  }
  const all = [...rows.values()];
  return [...all.filter(needsLook), ...all.filter((r) => !needsLook(r))];
}

/**
 * "הסתרתי אוטומטית N פרטים": what the filter hid without asking, why, and what goes out
 * instead, with "להחזיר" (keep it as written in this case) and a role menu for kept names.
 * Everything is plain text.
 */
export function AutoHiddenCard(props: {
  items: AutoHidden[];
  /** Null in a question without a case: nothing can be kept or restored there. */
  caseId: string | null;
  /** After a restore or a role change: build the text again. */
  onChanged: () => Promise<void>;
  disabled?: boolean;
}) {
  const { fail } = useApp();
  const ai = useAi();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const rows = cardRows(props.items);
  if (!rows.length) return null;
  const look = rows.filter(needsLook);
  const sure = rows.filter((r) => !needsLook(r));

  async function act(key: string, run: () => Promise<unknown>) {
    setBusy(key);
    setError(null);
    try {
      await run();
      await props.onChanged();
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(null);
    }
  }

  const caseId = props.caseId;
  const off = props.disabled === true || busy !== null;
  const words = (r: CardRow) => r.tokens.join(", ");
  const row = (r: CardRow) => {
    const person = r.kind === "name" || r.kind === "other_case";
    return (
      <li key={r.key} className="auto-item">
        <div className="grow stack" style={{ gap: 2 }}>
          <span className="auto-words">
            <b>{words(r)}</b>
            <span aria-hidden="true"> ← </span>
            <span className="visually-hidden"> יוצא כ: </span>
            <span className="mark-role">{r.tag.startsWith("[") ? displayTag(r.tag) : r.tag}</span>
            {r.uncertain && <span className="chip chip-sand auto-unsure">לא בטוח</span>}
          </span>
          {r.reason && <span className="small muted">{r.reason}</span>}
        </div>
        {caseId && person && (
          <label className="auto-role">
            <span className="visually-hidden">התפקיד של {words(r)}</span>
            <select className="select auto-select" value={foundRoles.includes(r.role) ? r.role : "other"} disabled={off}
              onChange={(e) => void act(r.key, () => ipc.changeRole(caseId, r.tag, e.target.value as Role))}>
              {foundRoles.map((x) => <option key={x} value={x}>{roleLabel[x]}</option>)}
            </select>
          </label>
        )}
        {caseId && (
          <button type="button" className="btn btn-small" disabled={off} aria-label={`להחזיר את ${words(r)}`}
            onClick={() => void act(r.key, async () => {
              // Every form of the word: keeping one and hiding another would make no sense.
              for (const t of r.tokens) await ipc.restoreAutoHidden(caseId, t, r.tag);
            })}>
            {busy === r.key ? <Spinner /> : "להחזיר"}
          </button>
        )}
      </li>
    );
  };
  return (
    <section className="auto-card" aria-label="מה הוסתר אוטומטית">
      <h3 className="auto-title">{rows.length === 1 ? "הסתרתי אוטומטית פרט אחד" : `הסתרתי אוטומטית ${rows.length} פרטים`}</h3>
      <span className="small muted">
        {caseId
          ? `${ai} מקבל את מה שמופיע אחרי החץ. "להחזיר" משאיר את המילה כמו שכתוב, מעכשיו ובכל התיק.`
          : "בשאלה כללית אי אפשר להחזיר מילה שהוסתרה. אם צריך, כדאי לשאול מתוך תיק."}
      </span>
      {look.length > 0 && <ul className="auto-list">{look.map(row)}</ul>}
      {sure.length > 0 && (
        look.length > 0 ? (
          <details className="auto-more">
            <summary>{sure.length === 1 ? "ועוד פרט אחד שהוסתר בביטחון" : `ועוד ${sure.length} פרטים שהוסתרו בביטחון`}</summary>
            <ul className="auto-list">{sure.map(row)}</ul>
          </details>
        ) : (
          <ul className="auto-list">{sure.map(row)}</ul>
        )
      )}
      <ErrorLine error={error} />
    </section>
  );
}
