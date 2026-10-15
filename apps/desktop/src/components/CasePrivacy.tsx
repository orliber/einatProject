// The case's privacy page (round 27): the names the program hid by itself, for whoever looks
// for them ("להחזיר", another role, "נכון, תמיד להסתיר"), and what left the computer. It takes
// the place of the card that was shown before each send and on import. Plain text only.
import { useState } from "react";
import { useApp } from "../App";
import { roleLabel } from "../i18n/he";
import { ipc } from "../ipc/client";
import type { Identity } from "../ipc/generated/Identity";
import type { Role } from "../ipc/generated/Role";
import { foundRoles } from "./AutoHiddenCard";
import { SentHistoryDialog } from "./SentHistory";
import { ErrorLine, Spinner } from "./ui";

export function CasePrivacyCard(props: {
  caseId: string;
  identities: Identity[];
  /** After a change: the case's names as they are now. */
  onChanged: (fresh: Identity[]) => Promise<void>;
}) {
  const { fail } = useApp();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState(false);
  const found = props.identities.filter((i) => i.source !== "manual");

  async function act(key: string, run: () => Promise<unknown>) {
    setBusy(key);
    setError(null);
    try {
      await run();
      await props.onChanged((await ipc.caseDetail(props.caseId)).identities);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(null);
    }
  }

  const off = busy !== null;
  return (
    <div className="card details-card stack" aria-labelledby="case-privacy">
      <b id="case-privacy">פרטיות התיק</b>
      <button type="button" className="btn align-start" onClick={() => setHistory(true)}>מה יצא מהמחשב בתיק הזה</button>
      {found.length > 0 && (
        <details>
          <summary>{found.length === 1 ? "שם אחד שהתוכנה הסתירה לבד" : `${found.length} שמות שהתוכנה הסתירה לבד`}</summary>
          <ul className="auto-list">
            {found.map((i) => {
              const words = [i.value, ...i.aliases].join(", ");
              return (
                <li key={i.id} className="auto-item">
                  <div className="grow stack" style={{ gap: 2 }}>
                    <b>{words}</b>
                    {i.reason && <span className="small muted">{i.reason}</span>}
                  </div>
                  <label className="auto-role">
                    <span className="visually-hidden">התפקיד של {words}</span>
                    <select className="select auto-select" value={foundRoles.includes(i.role) ? i.role : "other"} disabled={off}
                      onChange={(e) => void act(i.id, () => ipc.changeRole(props.caseId, i.tag, e.target.value as Role))}>
                      {foundRoles.map((x) => <option key={x} value={x}>{roleLabel[x]}</option>)}
                    </select>
                  </label>
                  <button type="button" className="btn btn-small btn-ghost" disabled={off} aria-label={`${words}: נכון, תמיד להסתיר בתיק הזה`}
                    onClick={() => void act(`${i.id}:ok`, () => ipc.confirmAutoHidden(props.caseId, i.value, i.tag))}>
                    {busy === `${i.id}:ok` ? <Spinner /> : "נכון, תמיד להסתיר"}
                  </button>
                  <button type="button" className="btn btn-small" disabled={off} aria-label={`להחזיר את ${words}`}
                    onClick={() => void act(i.id, () => ipc.restoreAutoHidden(props.caseId, i.value, i.tag))}>
                    {busy === i.id ? <Spinner /> : "להחזיר"}
                  </button>
                </li>
              );
            })}
          </ul>
          <p className="small muted">"להחזיר" משאיר את המילה כמו שהיא, בכל התיק.</p>
        </details>
      )}
      <ErrorLine error={error} />
      {history && <SentHistoryDialog caseId={props.caseId} onClose={() => setHistory(false)} />}
    </div>
  );
}
