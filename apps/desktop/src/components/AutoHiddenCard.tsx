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
  const items = cardOrder(props.items);
  if (!items.length) return null;

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
  return (
    <section className="auto-card" aria-label="מה הוסתר אוטומטית">
      <h3 className="auto-title">{items.length === 1 ? "הסתרתי אוטומטית פרט אחד" : `הסתרתי אוטומטית ${items.length} פרטים`}</h3>
      <span className="small muted">
        {caseId
          ? `${ai} מקבל את מה שמופיע אחרי החץ. "להחזיר" משאיר את המילה כמו שכתוב, מעכשיו ובכל התיק.`
          : "בשאלה כללית אי אפשר להחזיר מילה שהוסתרה. אם צריך, כדאי לשאול מתוך תיק."}
      </span>
      <ul className="auto-list">
        {items.map((a) => {
          const key = `${a.token}\u0000${a.tag}`;
          const person = a.kind === "name" || a.kind === "other_case";
          return (
            <li key={key} className="auto-item">
              <div className="grow stack" style={{ gap: 2 }}>
                <span className="auto-words">
                  <b>{a.token}</b>
                  <span aria-hidden="true"> ← </span>
                  <span className="visually-hidden"> יוצא כ: </span>
                  <span className="mark-role">{a.tag.startsWith("[") ? displayTag(a.tag) : a.tag}</span>
                  {a.uncertain && <span className="chip chip-sand auto-unsure">לא בטוח</span>}
                </span>
                {a.reason && <span className="small muted">{a.reason}</span>}
              </div>
              {caseId && person && (
                <label className="auto-role">
                  <span className="visually-hidden">התפקיד של {a.token}</span>
                  <select className="select auto-select" value={foundRoles.includes(a.role) ? a.role : "other"} disabled={off}
                    onChange={(e) => void act(key, () => ipc.changeRole(caseId, a.tag, e.target.value as Role))}>
                    {foundRoles.map((r) => <option key={r} value={r}>{roleLabel[r]}</option>)}
                  </select>
                </label>
              )}
              {caseId && (
                <button type="button" className="btn btn-small" disabled={off} aria-label={`להחזיר את ${a.token}`}
                  onClick={() => void act(key, () => ipc.restoreAutoHidden(caseId, a.token, a.tag))}>
                  {busy === key ? <Spinner /> : "להחזיר"}
                </button>
              )}
            </li>
          );
        })}
      </ul>
      <ErrorLine error={error} />
    </section>
  );
}
