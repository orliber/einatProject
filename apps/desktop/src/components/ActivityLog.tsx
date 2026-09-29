// The activity log (STANDARDS.md §5.5): who entered, what went to Claude, what was exported or
// erased. Metadata only; a case is named here from this computer's vault, never in the log.
import { useState } from "react";
import { useInfiniteQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type ActivityPage } from "../ipc/client";
import { heDateTime } from "./Backup";
import { Dialog, ErrorLine } from "./ui";
import "./ActivityLog.css";

const KINDS: [string, string][] = [
  ["all", "הכל"],
  ["access", "כניסות"],
  ["send", "שליחה ל-Claude"],
  ["case", "תיקים"],
  ["security", "אבטחה וגיבוי"],
];

function useActivity(enabled: boolean) {
  return useInfiniteQuery({
    queryKey: ["activity"],
    queryFn: ({ pageParam }) => ipc.activity(pageParam),
    initialPageParam: null as number | null,
    getNextPageParam: (last: ActivityPage) => (last.more ? last.last_seq : null),
    enabled,
  });
}

function ActivityDialog({ onClose }: { onClose: () => void }) {
  const { notify, fail } = useApp();
  const qc = useQueryClient();
  const log = useActivity(true);
  const [kind, setKind] = useState("all");
  const [error, setError] = useState<string | null>(null);
  const first = log.data?.pages[0];
  const entries = (log.data?.pages ?? []).flatMap((p) => p.entries).filter((e) => kind === "all" || e.kind === kind);

  async function reviewed() {
    try {
      await ipc.markActivityReviewed();
      await qc.invalidateQueries({ queryKey: ["activity"] });
      notify("נרשם ביומן שעברת עליו.");
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <Dialog wide title="יומן פעולות" onClose={onClose}
      subtitle={first ? (first.intact ? "✓ השרשרת שלמה: אף רשומה לא נמחקה או שונתה." : "בדיקת השרשרת מצאה חריגה: ייתכן שרשומה נמחקה או שונתה.") : "טוען…"}
      footer={<>
        <span className="muted small grow">{first?.reviewed_at ? `נבדק לאחרונה: ${heDateTime(first.reviewed_at)}` : "עוד לא סומן שהיומן נבדק."}</span>
        <button type="button" className="btn" onClick={onClose}>סגירה</button>
        <button type="button" className="btn btn-primary" onClick={() => void reviewed()}>עברתי על היומן</button>
      </>}>
      <div className="stack">
        <div className="row wrap activity-kinds" role="radiogroup" aria-label="סינון">
          {KINDS.map(([k, label]) => (
            <label key={k} className={kind === k ? "pill-btn on" : "pill-btn"}>
              <input type="radio" name="activity-kind" className="visually-hidden" checked={kind === k} onChange={() => setKind(k)} />
              {label}
            </label>
          ))}
        </div>
        {first && !first.intact && <p className="error" role="alert">בדיקת השלמות של היומן מצאה חריגה. כדאי לפנות לאור לפני שממשיכים לעבוד.</p>}
        <ol className="activity-list" aria-label="פעולות, מהחדשה לישנה">
          {entries.map((e) => (
            <li key={e.seq} className={e.warn ? "warn" : undefined}>
              <time className="muted small">{heDateTime(e.ts)}</time>
              <span className="grow">{e.text}</span>
              {e.case && <span className="chip chip-sand activity-case">{e.case}</span>}
            </li>
          ))}
          {log.data && entries.length === 0 && <li className="muted">אין פעולות מהסוג הזה בעמודים שנטענו.</li>}
        </ol>
        {log.hasNextPage && (
          <button type="button" className="btn align-start" disabled={log.isFetchingNextPage} onClick={() => void log.fetchNextPage()}>
            {log.isFetchingNextPage ? "טוען…" : "פעולות קודמות"}
          </button>
        )}
        <ErrorLine error={error ?? (log.error ? fail(log.error as never) : null)} />
      </div>
    </Dialog>
  );
}

/** Settings: the log, and when it was last gone over. */
export function ActivitySettings() {
  const [open, setOpen] = useState(false);
  return (
    <section className="card setting" aria-labelledby="s-activity">
      <h2 id="s-activity">יומן פעולות</h2>
      <p className="muted small">
        כל כניסה, שליחה ל-Claude, ייצוא ומחיקה נרשמים ביומן מוצפן, בלי תוכן ובלי שמות. כל רשומה חתומה יחד עם זו שלפניה,
        כך שמחיקה או שינוי מתגלים. היומן נשמר לפחות שנתיים. מומלץ לעבור עליו פעם בחודש.
      </p>
      <button type="button" className="btn align-start" onClick={() => setOpen(true)}>פתיחת היומן</button>
      {open && <ActivityDialog onClose={() => setOpen(false)} />}
    </section>
  );
}
