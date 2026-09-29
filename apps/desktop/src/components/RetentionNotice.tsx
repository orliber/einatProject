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
      subtitle="התוכנה לא מוחקת לבד. לכל תיק מחליטים: להשאיר עוד, או להעביר לסל המחזור (שם הוא נשמר 30 יום, ומחיקה לצמיתות דורשת סיסמה)."
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
        {items.length === 1 ? "תיק אחד עבר" : `${items.length} תיקים עברו`} את תקופת השמירה. התוכנה לא מוחקת לבד: כדאי לעבור ולהחליט.
      </span>
      <button type="button" className="btn btn-small" onClick={() => setOpen(true)}>לעבור עליהם</button>
      {open && <RetentionDialog items={items} onClose={() => setOpen(false)} />}
    </div>
  );
}
