// "The protection layers" (S4, L6): the ten layers of protection, each with what it does,
// what it protects against and what the program found when it checked just now; then what
// depends on her; then the emergency stop. The texts come from the core, which makes the PDF
// from the same list, so the page and the paper say the same thing.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ipc, type SecurityLayer, type SecurityStatus } from "../ipc/client";
import { useApp } from "../App";
import { ErrorLine } from "./ui";

const STATE_TEXT: Record<string, { chip: string; text: string }> = {
  ok: { chip: "chip chip-ok", text: "פעיל" },
  warn: { chip: "chip chip-warn", text: "לבדוק" },
  info: { chip: "chip chip-sand", text: "לידיעה" },
};

const INFO = { chip: "chip chip-sand", text: "לידיעה" };

function Layer({ l, n }: { l: SecurityLayer; n?: number }) {
  const st = STATE_TEXT[l.state] ?? INFO;
  return (
    <li className="layer">
      <div className="layer-head">
        <span className={st.chip}>{st.text}</span>
        <h4 className="layer-title">{n === undefined ? l.title : `${n}. ${l.title}`}</h4>
      </div>
      <p className="layer-now">{l.now}</p>
      <details>
        <summary>מה זה ומפני מה<span className="visually-hidden">: {l.title}</span></summary>
        <p>{l.what}</p>
        <p>מגינה מפני: {l.against}</p>
      </details>
    </li>
  );
}

export function SecuritySettings() {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["security"], queryFn: () => ipc.securityStatus() });
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  async function stop(stopped: boolean) {
    setError(null);
    try {
      qc.setQueryData<SecurityStatus>(["security"], await ipc.setSendStopped(stopped));
    } catch (e) {
      setError(fail(e as never));
    }
  }

  async function savePdf() {
    setError(null);
    setSaving(true);
    try {
      // The path is isolated, so its slashes do not mix with the Hebrew around it.
      notify(`נשמר: \u2066${await ipc.exportSecurityPdf()}\u2069`);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setSaving(false);
    }
  }

  const s = q.data;
  // The stop is always there, also while the list loads or failed to load (review of
  // stage 5, U7). Sending cannot be turned back on while the gate or the rules fail (U1).
  const stopped = s?.send_stopped ?? false;
  const broken = s?.layers.some((l) => (l.key === "privacy_gate" || l.key === "send_rules") && l.state === "warn") ?? false;
  return (
    <section className="card setting" aria-labelledby="s-sec">
      <h2 id="s-sec">שכבות ההגנה</h2>
      <div className="row">
        <p className="hint">
          {s ? `עד ${s.daily_limit} בקשות ביום, ולא יותר מ-${s.max_request_kb} KB לבקשה.` : "עצירת חירום של כל שליחה ל-AI."}
        </p>
        <button type="button" className={stopped ? "btn" : "btn btn-danger"} disabled={stopped && broken}
          title={stopped && broken ? "השער או כללי השליחה לא עברו את הבדיקה, ולכן אי אפשר להפעיל שליחה." : undefined}
          onClick={() => void stop(!stopped)}>
          {stopped ? "להפעיל שליחה מחדש" : "עצירת חירום: לכבות שליחה"}
        </button>
      </div>
      {stopped && broken && <p className="hint">אי אפשר להפעיל שליחה מחדש: השער או כללי השליחה לא עברו את הבדיקה עכשיו. כדאי לפנות לאור.</p>}
      <p className="hint">שלוש השכבות שכתוב בהן "נבדק עכשיו" נוסו בידי התוכנה ממש עכשיו. בשאר מוצג המצב הנוכחי שלהן.</p>
      {q.isPending && <p className="hint" role="status">בודקת…</p>}
      {q.isError && <ErrorLine error={fail(q.error as never)} retry={() => void qc.invalidateQueries({ queryKey: ["security"] })} />}
      <ol className="sec-list">
        {s?.layers.map((l, i) => <Layer key={l.key} l={l} n={i + 1} />)}
      </ol>
      {s && s.yours.length > 0 && (
        <>
          <h3>מה שתלוי בך</h3>
          <ul className="sec-list">
            {s.yours.map((l) => <Layer key={l.key} l={l} />)}
          </ul>
        </>
      )}
      {s && (
        <div className="row">
          <p className="hint">אותו עמוד כקובץ PDF, להורים או למכון. אין בו שום פרט על ילד.</p>
          <button type="button" className="btn" disabled={saving} onClick={() => void savePdf()}>
            {saving ? "שומרת…" : "לשמור כ-PDF"}
          </button>
        </div>
      )}
      <ErrorLine error={error} />
    </section>
  );
}
