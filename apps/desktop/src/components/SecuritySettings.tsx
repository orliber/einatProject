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
        <strong>{n === undefined ? l.title : `${n}. ${l.title}`}</strong>
      </div>
      <p className="layer-now">{l.now}</p>
      <details>
        <summary>מה זה ומפני מה</summary>
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
      notify(`נשמר: ${await ipc.exportSecurityPdf()}`);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setSaving(false);
    }
  }

  const s = q.data;
  return (
    <section className="card setting" aria-labelledby="s-sec">
      <h2 id="s-sec">שכבות ההגנה</h2>
      <p className="hint">כל שכבה נבדקה עכשיו בידי התוכנה, לא רק לפי ההגדרות.</p>
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
          <p className="hint">
            עד {s.daily_limit} בקשות ביום, ולא יותר מ-{s.max_request_kb} KB לבקשה.
          </p>
          <button type="button" className={s.send_stopped ? "btn" : "btn btn-danger"} onClick={() => void stop(!s.send_stopped)}>
            {s.send_stopped ? "להפעיל שליחה מחדש" : "עצירת חירום: לכבות שליחה"}
          </button>
        </div>
      )}
      {s && (
        <div className="row">
          <p className="hint">אותו עמוד כקובץ PDF, להורים או למכון. אין בו שום פרט על ילד.</p>
          <button type="button" className="btn" disabled={saving} onClick={() => void savePdf()}>
            לשמור כ-PDF
          </button>
        </div>
      )}
      <ErrorLine error={error} />
    </section>
  );
}
