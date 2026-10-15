// Settings → privacy, the first thing on the page (round 50, D-087): what left the computer
// this month, in one sentence, and the door to the full list. Nothing here is new data: the
// requests are the ones "מה יצא מהמחשב" keeps, counted.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { SentHistoryDialog } from "./SentHistory";

/** Requests since the first of this month, by the computer's calendar. */
export function thisMonth(ats: number[], now = Date.now()): number {
  const d = new Date(now);
  const start = new Date(d.getFullYear(), d.getMonth(), 1).getTime() / 1000;
  return ats.filter((at) => at >= start).length;
}

export function PrivacyStatus() {
  const sent = useQuery({ queryKey: ["sent", null, null, ""], queryFn: () => ipc.sentHistory(null, null, "") });
  const security = useQuery({ queryKey: ["security"], queryFn: () => ipc.securityStatus() });
  const [open, setOpen] = useState(false);
  const n = sent.data ? thisMonth(sent.data.filter((i) => !i.demo).map((i) => i.at)) : null;
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
