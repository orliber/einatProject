// Settings → privacy, the first thing on the page (round 50, D-087): what left the computer
// this month, in one sentence, and the door to the full list. Nothing here is new data: the
// requests are the ones "מה יצא מהמחשב" keeps, counted.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { SentHistoryDialog } from "./SentHistory";

/** The first of this month, by the computer's calendar, in seconds. */
export function monthStart(now = Date.now()): number {
  const d = new Date(now);
  return new Date(d.getFullYear(), d.getMonth(), 1).getTime() / 1000;
}

export function PrivacyStatus() {
  // Counted in the vault, not by reading the whole history (D-123).
  const since = monthStart();
  const sent = useQuery({ queryKey: ["sent-count", since], queryFn: () => ipc.sentCount(since) });
  const security = useQuery({ queryKey: ["security"], queryFn: () => ipc.securityStatus() });
  const [open, setOpen] = useState(false);
  const n = sent.data ?? null;
  const layersOk = security.data ? security.data.items.every((i) => i.state !== "warn") : null;
  const count = n === null ? "" : n === 0 ? "שום בקשה לא יצאה החודש." : n === 1 ? "בקשה אחת יצאה החודש, אחרי הסתרה." : `${n} בקשות יצאו החודש, כולן אחרי הסתרה.`;
  const layers = layersOk === null ? "" : layersOk ? " שכבות ההגנה נבדקו בפתיחת הכספת, כולן תקינות." : " אחת משכבות ההגנה מבקשת מבט (הגדרות ← סיסמה ואבטחה).";
  return (
    <section className="card setting privacy-status" aria-labelledby="s-privacy-status">
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="privacy-shield">
        <path d="M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z" /><path d="m9 12 2 2 4-4" />
      </svg>
      <div className="privacy-status-text">
        <h2 id="s-privacy-status">שום שם לא יצא מהמחשב.</h2>
        <span className="hint">{count}{layers}</span>
      </div>
      <button type="button" className="btn" onClick={() => setOpen(true)}>מה יצא מהמחשב</button>
      {open && <SentHistoryDialog onClose={() => setOpen(false)} />}
    </section>
  );
}
