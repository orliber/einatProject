// "The layers of protection" (S4, round 50, D-087): what the program itself checked when the
// vault opened, as a grid of small cards, one per layer; the ones that depend on her are
// marked so. The emergency stop for sending sits below, as before.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ipc, type SecurityItem, type SecurityStatus } from "../ipc/client";
import { useApp } from "../App";
import { daysAgo, heDateTime, useBackupStatus } from "./Backup";
import { ErrorLine } from "./ui";

export type Layer = {
  key: string;
  /** ok: on and verified · warn: she should look · info: a fact, neither. */
  state: "ok" | "warn" | "info";
  name: string;
  /** One line of what was found. */
  line: string;
  /** When this is checked ("בכל פתיחה", "תמיד", a time). */
  when: string;
  /** Depends on her, not on the program. */
  you?: boolean;
  /** Where to fix it, when it is hers to fix. */
  action?: { label: string; pane: "backup" | "security" };
};

/** The card for each item the core reports. */
export function layerOf(i: SecurityItem, s: SecurityStatus): Layer | null {
  const ok = i.state === "ok";
  const state = i.state === "warn" ? "warn" : i.state === "info" ? "info" : "ok";
  switch (i.key) {
    case "encrypted": return { key: i.key, state, name: "הכספת מוצפנת", line: "AES-256, מפתח נפרד לכל תיק", when: "בכל פתיחה" };
    case "fips": return { key: i.key, state, name: "מודול ההצפנה", line: ok ? "מאושר FIPS 140-3" : "סטנדרטי", when: "בכל פתיחה" };
    case "disk": return { key: i.key, state, you: true, name: "הדיסק מוצפן (BitLocker / FileVault)", line: ok ? "פעיל. אם ייכבה, התוכנה תגיד לך בכניסה" : "לא פעיל או לא ידוע. מדליקים בהגדרות המחשב", when: "נבדק עכשיו" };
    case "cloud_folder": return { key: i.key, state, name: "הכספת לא בתיקיית ענן", line: i.detail ? `נמצאת בתיקיית ${i.detail}. כדאי להעביר אותה` : "לא OneDrive, לא Google Drive", when: "בכל פתיחה" };
    case "log_integrity": return { key: i.key, state, name: "יומן פעילות שלם", line: i.detail ?? "כל פתיחת תיק וכל שליחה נרשמות, ולא נמחקות", when: "תמיד" };
    case "auto_lock": return { key: i.key, state, name: `נעילה אחרי ${i.detail ?? "?"} דקות`, line: "גם כשהחלון תקוע, וכשנועלים את המחשב או הוא נרדם", when: "תמיד" };
    case "screen": return { key: i.key, state, name: "צילום מסך מציג חלון ריק", line: "גם בשיתוף מסך", when: "תמיד" };
    case "send": return { key: i.key, state, name: "שליחה ל-AI", line: ok ? `פעילה, עד ${s.daily_limit} בקשות ביום ו-${s.max_request_kb} KB לבקשה` : "כבויה: עצירת חירום", when: "בכל שליחה" };
    case "isolation": return { key: i.key, state, name: "קריאת מסמכים בתהליך מבודד", line: ok ? "קובץ פגום לא מגיע לכספת" : "במחשב הזה אין בידוד כזה", when: "בכל קובץ" };
    default: return { key: i.key, state, name: i.key, line: i.detail ?? "", when: "" };
  }
}

const COUNT = ["אפס", "אחת", "שתיים", "שלוש", "ארבע", "חמש", "שש", "שבע", "שמונה", "תשע", "עשר"];
const count = (n: number) => COUNT[n] ?? String(n);

export function SecuritySettings() {
  const { fail, go } = useApp();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["security"], queryFn: () => ipc.securityStatus() });
  const backup = useBackupStatus();
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
  const layers: Layer[] = s ? s.items.map((i) => layerOf(i, s)).filter((l): l is Layer => l !== null) : [];
  if (backup.data) {
    const b = backup.data;
    layers.push({
      key: "backup", you: true, state: b.due ? "warn" : "ok", name: "גיבוי מהשבוע האחרון",
      line: b.last_at ? `האחרון: ${heDateTime(b.last_at)}${b.last_check_at ? ` · תרגול שחזור: ${heDateTime(b.last_check_at)}` : ""}` : "עוד אין גיבוי",
      when: b.days_since === null ? "" : daysAgo(b.days_since), action: { label: "לגבות", pane: "backup" },
    });
  }
  const bad = layers.filter((l) => l.state === "warn");
  const yours = layers.filter((l) => l.you).length;
  const title = !layers.length ? "בודק את שכבות ההגנה…"
    : bad.length === 0 ? `${count(layers.length)} שכבות הגנה, כולן תקינות`
      : `${layers.length - bad.length} מתוך ${layers.length} תקינות: ${bad.map((l) => l.name).join(", ")}`;

  return (
    <section className="card setting" aria-labelledby="s-sec">
      <div className="layers-head">
        <div>
          <h2 id="s-sec">{title}</h2>
          <span className="hint">נבדקו עכשיו, בפתיחת הכספת. {count(yours)} תלויות בך.</span>
        </div>
        <button type="button" className="btn btn-small" onClick={() => { void qc.invalidateQueries({ queryKey: ["security"] }); void qc.invalidateQueries({ queryKey: ["backup"] }); }}>לבדוק שוב</button>
      </div>
      <ul className="layers">
        {layers.map((l) => (
          <li key={l.key} className={["layer", l.state, l.you ? "you" : ""].filter(Boolean).join(" ")}>
            <span className="layer-mark" aria-hidden="true">{l.you ? "את" : l.state === "warn" ? "!" : "✓"}</span>
            <span className="visually-hidden">{l.state === "warn" ? "לבדוק: " : l.state === "ok" ? "תקין: " : ""}</span>
            <div className="layer-text">
              <strong>{l.name}</strong>
              <span className="small muted">{l.line}</span>
            </div>
            {l.action ? (
              <button type="button" className="link-btn small layer-when" onClick={() => go({ name: "settings", pane: l.action?.pane ?? "security" })}>{l.action.label}</button>
            ) : (
              <span className="small muted layer-when">{l.when}</span>
            )}
          </li>
        ))}
      </ul>
      <details className="layers-more">
        <summary>מה קורה אם שכבה נכשלת</summary>
        <p className="hint">שכבה של התוכנה שנכשלת עוצרת את השליחה מיד, ושום בקשה לא יוצאת עד שאור בודק. שכבה שתלויה בך מופיעה כאן עם "לבדוק", ובכניסה הבאה גם בהודעה אחת עם כפתור למקום שבו מתקנים.</p>
      </details>
      {s && (
        <div className="row">
          <p className="hint grow">עצירת חירום מכבה כל שליחה ל-AI עד שמדליקים אותה מחדש כאן.</p>
          <button type="button" className={s.send_stopped ? "btn" : "btn btn-danger"} onClick={() => void stop(!s.send_stopped)}>
            {s.send_stopped ? "להפעיל שליחה מחדש" : "עצירת חירום: לכבות שליחה"}
          </button>
        </div>
      )}
      <ErrorLine error={error} />
    </section>
  );
}
