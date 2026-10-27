// Retention reminders (P-07 ⚖️): cases past their retention date are pointed out, never erased
// on their own. Einat keeps them longer or moves them to the recycle bin.
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type RetentionItem } from "../ipc/client";
import { isoToHe } from "./DateField";
import { Dialog, ErrorLine } from "./ui";
import "./Backup.css";

function RetentionDialog({ items, onClose }: { items: RetentionItem[]; onClose: () => void }) {
  const { go, fail } = useApp();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  async function act(fn: () => Promise<unknown>) {
    setError(null);
    try {
      await fn();
      await Promise.all(["retention", "cases", "trash"].map((k) => qc.invalidateQueries({ queryKey: [k] })));
    } catch (e) {
      setError(fail(e as never));
    }
  }
  return (
    <Dialog title="תיקים שעברו את תקופת השמירה" onClose={onClose}
      subtitle="התוכנה לא מוחקת לבד. לכל תיק: להשאיר, או לסל המחזור (30 יום)."
      footer={<button type="button" className="btn btn-primary" onClick={onClose}>סגירה</button>}>
      <ul className="retention-list">
        {items.map((it) => (
          <li key={it.case_id}>
            <div className="stack grow" style={{ gap: 2 }}>
              <b>{it.label}</b>
              <span className="muted small">
                תקופת השמירה הסתיימה ב-{isoToHe(it.until)}{it.by_default ? " (לפי ברירת המחדל המוצעת)" : ""}
              </span>
            </div>
            <button type="button" className="btn btn-small" onClick={() => go({ name: "case", id: it.case_id, view: "details" })}>פתיחה</button>
            <button type="button" className="btn btn-small" onClick={() => void act(() => ipc.keepCaseLonger(it.case_id, 1))}>להשאיר עוד שנה</button>
            <button type="button" className="btn btn-small btn-danger-quiet" onClick={() => void act(() => ipc.deleteCase(it.case_id))}>לסל המחזור</button>
          </li>
        ))}
      </ul>
      <ErrorLine error={error} />
    </Dialog>
  );
}

export function RetentionNotice() {
  const due = useQuery({ queryKey: ["retention"], queryFn: ipc.retentionDue });
  const [open, setOpen] = useState(false);
  const items = due.data ?? [];
  if (!items.length) return null;
  return (
    <div className="backup-reminder" role="status">
      <span className="grow">
        {items.length === 1 ? "תיק אחד עבר" : `${items.length} תיקים עברו`} את תקופת השמירה. כדאי להחליט מה לעשות.
      </span>
      <button type="button" className="btn btn-small" onClick={() => setOpen(true)}>לעבור עליהם</button>
      {open && <RetentionDialog items={items} onClose={() => setOpen(false)} />}
    </div>
  );
}

/** Reports an older version saved in the app's own folder, when no usual folder could take
 *  them (D-120). Nothing is saved there any more, and nothing there is deleted for her: she is
 *  told while files remain, and opens the folder to move or delete them. */
export function OldExportsNotice() {
  const { fail } = useApp();
  const count = useQuery({ queryKey: ["old-exports"], queryFn: ipc.oldExports });
  const [hidden, setHidden] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const n = count.data ?? 0;
  if (!n || hidden) return null;
  async function open() {
    setError(null);
    try {
      await ipc.openOldExports();
    } catch (e) {
      setError(fail(e as never));
    }
  }
  return (
    <div className="backup-reminder" role="status">
      <span className="grow">
        {n === 1 ? "קובץ אחד שהופק" : `${n} קבצים שהופקו`} בגרסה קודמת {n === 1 ? "נשמר" : "נשמרו"} בתיקייה פנימית של התוכנה, כי תיקיית ההורדות לא הייתה זמינה. אולי יש בהם שמות אמיתיים. כדאי להעביר אותם למקום שלך או למחוק. התוכנה לא מוחקת אותם לבד.
      </span>
      {error && <span className="error small">{error}</span>}
      <button type="button" className="btn btn-small" onClick={() => void open()}>פתיחת התיקייה</button>
      <button type="button" className="link-btn small" onClick={() => setHidden(true)}>לא עכשיו</button>
    </div>
  );
}
