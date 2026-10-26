import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApp } from "../../App";
import { useAi } from "../../ai";
import { CloseIcon, ErrorLine } from "../../components/ui";
import { ipc, type StyleItem, type StyleKind, type StyleOverview, type StyleProfile, type StyleProfileView } from "../../ipc/client";
import { ItemText } from "./InferenceChain";
import { KIND_HINT, KIND_LABEL, KINDS } from "./labels";

// הסגנון שלי · the profile

/** The profile's items, to check, change, delete or add. `kinds`: only these are shown (a row of
 * "what the program knows"); the rest of the profile is kept as it is when she saves. */
export function ProfileEditor({ view, o, kinds }: { view: StyleProfileView; o: StyleOverview; kinds?: StyleKind[] }) {
  const { fail, notify } = useApp();
  const ai = useAi();
  const qc = useQueryClient();
  const [items, setItems] = useState<StyleItem[]>(view.profile.items);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const isDraft = view.status === "draft";
  const shownKinds = kinds ?? KINDS;

  const groups = useMemo(() => {
    const keys: (string | null)[] = [null, ...o.sections.map((s) => s.key)];
    return keys
      .map((key) => ({ key, title: key ? (o.sections.find((s) => s.key === key)?.title ?? key) : "כל הדוח", items: items.filter((i) => i.section === key && shownKinds.includes(i.kind)) }))
      .filter((g) => g.key === null || g.items.length > 0);
  }, [items, o.sections, shownKinds]);

  const change = (f: (all: StyleItem[]) => StyleItem[]) => {
    setItems(f);
    setDirty(true);
  };
  const profile = (): StyleProfile => ({ items, reports: view.profile.reports });

  async function act(f: () => Promise<unknown>, done: string) {
    setBusy(true);
    setError(null);
    try {
      await f();
      notify(done);
      setDirty(false);
      await qc.invalidateQueries({ queryKey: ["style"] });
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  const enabledCount = items.filter((i) => i.enabled).length;

  return (
    <section id="style-profile" className={isDraft ? "style-profile style-draft" : "style-profile"} aria-label={isDraft ? "טיוטת פרופיל הסגנון" : "פרופיל הסגנון"}>
      {isDraft && (
        <div className="row style-profile-head">
          <span className="chip chip-warn">טיוטה · עוד לא בשימוש</span>
          {view.profile.reports > 0 && <span className="chip chip-sand">נבנה מ-{view.profile.reports} דוחות</span>}
          {view.demo && <span className="chip chip-sand">נבנה במצב הדגמה</span>}
        </div>
      )}
      <p className="hint">
        {isDraft
          ? "רק מה שמסומן ייכנס, ורק אחרי האישור."
          : "כל שינוי משפיע רק אחרי אישור."}
        {isDraft && view.dropped > 0 && ` ${view.dropped} פריטים נפסלו (העתקה או פרט מזהה).`}
      </p>

      {groups.map((g) => (
        <div key={g.key ?? "general"} className="style-group">
          <h3>{g.title}</h3>
          {shownKinds.map((kind) => {
            const list = g.items.filter((i) => i.kind === kind);
            if (list.length === 0) return null;
            return (
              <div key={kind} className="style-kind">
                <span className="style-kind-title">{KIND_LABEL[kind]}{KIND_HINT[kind] && <span className="muted small"> · {KIND_HINT[kind]}</span>}</span>
                <ul className="style-items">
                  {list.map((item) => (
                    <li key={item.id} className={item.enabled ? "style-item" : "style-item style-item-off"}>
                      <input type="checkbox" aria-label="להשתמש בפריט" checked={item.enabled}
                        onChange={() => change((all) => all.map((x) => (x.id === item.id ? { ...x, enabled: !x.enabled } : x)))} />
                      {editing === item.id ? (
                        <textarea className="textarea grow" rows={kind === "example" || kind === "inference" ? 4 : 2} autoFocus defaultValue={item.text}
                          onBlur={(e) => { const text = e.target.value.trim(); setEditing(null); if (text && text !== item.text) change((all) => all.map((x) => (x.id === item.id ? { ...x, text } : x))); }} />
                      ) : (
                        <button type="button" className={kind === "example" ? "style-item-text style-example grow" : "style-item-text grow"} title="עריכה" onClick={() => setEditing(item.id)}>
                          <ItemText item={item} />
                        </button>
                      )}
                      {item.origin === "edits" && <span className="chip chip-sand">מהתיקונים שלך</span>}
                      {item.origin === "manual" && <span className="chip chip-sand">שלך</span>}
                      {item.origin === "reports" && item.support > 1 && <span className="chip">ב-{item.support} דוחות</span>}
                      <button type="button" className="btn icon-btn" aria-label="מחיקה" onClick={() => change((all) => all.filter((x) => x.id !== item.id))}><CloseIcon /></button>
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
          <AddItem kinds={shownKinds} onAdd={(kind, text) => change((all) => [...all, { id: "", section: g.key, kind, text, enabled: true, origin: "manual", support: 0 }])} />
        </div>
      ))}

      <ErrorLine error={error} />
      <div className="row style-profile-foot">
        <span className="muted small grow">{enabledCount} פריטים מסומנים</span>
        {isDraft && (
          <button type="button" className="btn btn-danger-quiet" disabled={busy} onClick={() => void act(() => ipc.discardStyleDraft(), "הטיוטה נמחקה.")}>מחיקת הטיוטה</button>
        )}
        {dirty && (
          <button type="button" className="btn" disabled={busy} onClick={() => void act(() => ipc.saveStyleDraft(profile()), "השינויים נשמרו בטיוטה.")}>שמירה כטיוטה</button>
        )}
        {(isDraft || dirty) && (
          <button type="button" className="btn btn-primary" disabled={busy || enabledCount === 0}
            onClick={() => void act(async () => { if (dirty) await ipc.saveStyleDraft(profile()); await ipc.approveStyleDraft(); }, `הפרופיל אושר. מעכשיו ${ai} כותב בסגנון שלך.`)}>
            אישור הפרופיל
          </button>
        )}
      </div>
    </section>
  );
}

function AddItem({ kinds, onAdd }: { kinds: StyleKind[]; onAdd: (kind: StyleKind, text: string) => void }) {
  const choices = kinds.filter((k) => k !== "example");
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState<StyleKind>(choices[0] ?? "rule");
  const [text, setText] = useState("");
  if (!open) {
    return <button type="button" className="btn btn-ghost btn-small style-add" onClick={() => setOpen(true)}>+ הוספת פריט משלך</button>;
  }
  return (
    <form className="row style-add-form" onSubmit={(e) => { e.preventDefault(); if (text.trim()) { onAdd(kind, text.trim()); setText(""); setOpen(false); } }}>
      <select className="select" aria-label="סוג" value={kind} onChange={(e) => setKind(e.target.value as StyleKind)}>
        {choices.map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
      </select>
      <input className="input grow" autoFocus value={text} onChange={(e) => setText(e.target.value)}
        placeholder={kind === "inference" ? "בתצפית: הילד עוזב משימה באמצע ← מסקנה: קושי בהתמדה ← ניסוח: ניכר קושי ב{תחום} ← המלצה: תיווך בתחילת משימה" : "למשל: פסקת הסיכום פותחת בחוזקות של הילד"} />
      <button type="submit" className="btn btn-small" disabled={!text.trim()}>הוספה</button>
      <button type="button" className="btn btn-small btn-ghost" onClick={() => setOpen(false)}>ביטול</button>
    </form>
  );
}
