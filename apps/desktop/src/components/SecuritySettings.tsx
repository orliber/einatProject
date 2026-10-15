// "Security status" (S4): which protections are on right now, as the program itself checked.
// A missing protection is shown plainly; the emergency stop for sending sits at the bottom.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ipc, type SecurityItem, type SecurityStatus } from "../ipc/client";
import { useApp } from "../App";
import { ErrorLine } from "./ui";

const LABELS: Record<string, (d: string | null) => string> = {
  encrypted: () => "הכספת מוצפנת (AES-256), ומפתח נפרד לכל תיק",
  computer_bound: () => "הסיסמה פותחת את הכספת רק במחשב הזה",
  recovery_kit: (d) => (d === null ? "ערכת השחזור המודפסת עוד לא נבדקה" : d === "0" ? "ערכת השחזור המודפסת נבדקה היום" : `ערכת השחזור המודפסת נבדקה לפני ${d} ימים`),
  fips: () => "מודול ההצפנה",
  disk: () => "הצפנת הדיסק של המחשב (BitLocker / FileVault)",
  cloud_folder: (d) => (d ? `הכספת נמצאת בתיקיית ענן (${d}). כדאי להעביר אותה` : "הכספת לא בתיקיית ענן"),
  log_integrity: (d) => (d ? `יומן הפעילות: ${d}` : "יומן הפעילות שלם ולא שונה"),
  auto_lock: (d) => `נעילה אוטומטית אחרי ${d} דקות, גם כשהחלון תקוע, וכשנועלים את המחשב או הוא נרדם`,
  screen: () => "צילום מסך ושיתוף מסך מציגים חלון ריק",
  send: () => "השליחה ל-AI",
  isolation: () => "קריאת מסמכים בתהליך מבודד",
  memory: () => "מה שהכספת מחזיקה בזיכרון נשאר בזיכרון ולא נכתב לדיסק",
};

const STATE_INFO = { chip: "chip chip-sand", text: "רגיל" };
const STATE_TEXT: Record<string, { chip: string; text: string }> = {
  ok: { chip: "chip chip-ok", text: "פעיל" },
  warn: { chip: "chip chip-warn", text: "לבדוק" },
  info: { chip: "chip chip-sand", text: "רגיל" },
};

function suffix(i: SecurityItem): string {
  if (i.key === "send") return i.state === "ok" ? " פעילה, עם תקרה יומית וגודל מקסימלי" : " כבויה: עצירת חירום";
  if (i.key === "fips") return i.state === "ok" ? " מאושר FIPS 140-3" : " סטנדרטי";
  if (i.key === "recovery_kit" && i.state === "warn") return ". במחשב אחר רק היא פותחת את הכספת: כדאי לבדוק אותה ב\"סיסמה ואבטחה\"";
  if (i.key === "computer_bound" && i.state === "warn") return " (זה יקרה בכניסה הבאה עם הסיסמה)";
  if (i.key === "computer_bound" && i.state === "info") return " (במחשב הזה אין אפשרות כזו)";
  if (i.key === "memory" && i.state === "warn") return " (Windows לא אישר את המקום לזה בהפעלה הזו)";
  if (i.key === "memory" && i.state === "info") return " (במחשב הזה אין אפשרות כזו)";
  if (i.key === "isolation") return i.state === "ok" ? "" : " (במחשב הזה אין בידוד כזה)";
  return "";
}

export function SecuritySettings() {
  const { fail } = useApp();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["security"], queryFn: () => ipc.securityStatus() });
  const [error, setError] = useState<string | null>(null);

  async function stop(stopped: boolean) {
    setError(null);
    try {
      qc.setQueryData<SecurityStatus>(["security"], await ipc.setSendStopped(stopped));
    } catch (e) {
      setError(fail(e as never));
    }
  }

  const s = q.data;
  return (
    <section className="card setting" aria-labelledby="s-sec">
      <h2 id="s-sec">מצב האבטחה</h2>
      <ul className="sec-list">
        {s?.items.map((i) => {
          const st = STATE_TEXT[i.state] ?? STATE_INFO;
          return (
            <li key={i.key}>
              <span className={st.chip}>{st.text}</span> {LABELS[i.key]?.(i.detail) ?? i.key}
              {suffix(i)}
            </li>
          );
        })}
      </ul>
      {s && (
        <div className="row">
          <p className="hint">
            עד {s.daily_limit} בקשות ביום, ולא יותר מ-{s.max_request_kb} KB לבקשה. הכל נשלח רק אחרי אישור המסך "לפני ששולחים".
          </p>
          <button type="button" className={s.send_stopped ? "btn" : "btn btn-danger"} onClick={() => void stop(!s.send_stopped)}>
            {s.send_stopped ? "להפעיל שליחה מחדש" : "עצירת חירום: לכבות שליחה"}
          </button>
        </div>
      )}
      <ErrorLine error={error} />
    </section>
  );
}
