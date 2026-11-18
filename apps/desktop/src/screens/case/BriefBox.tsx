// What she asks Claude before a draft (D-140): one box, the same wherever something is sent
// to be worded on the report page. Her words, a style in one click, her recent wishes, and
// for a new section "keep it for this section". Enter sends (an empty box sends as always),
// Shift+Enter is a new line, Esc closes. What is asked goes out only inside the request,
// through the filter and the gate, and is shown on "what leaves the computer".
import { useEffect, useRef, useState } from "react";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";

/** A style in one click: its label, and what it adds to the request. */
export type BriefChip = readonly [label: string, instruction: string];

/** For a first draft of a section. */
export const STYLE_CHIPS: readonly BriefChip[] = [
  ["קצר וממוקד", "כתבי בקצרה ובממוקד."],
  ["מפורט", "כתבי בפירוט, רק ממה שיש בחומרים."],
  ["שפה פשוטה להורים", "כתבי בשפה פשוטה וברורה, שגם הורים יבינו."],
  ["להדגיש חוזקות", "הדגישי את החוזקות, בלי להסתיר את הקשיים."],
  ["זהירות בניסוח", "נסחי בזהירות ובלשון תיאורית, לא שיפוטית."],
];

/** For a draft already on the page ("✦ לשנות"). */
export const CHANGE_CHIPS: readonly BriefChip[] = [
  ["קצר יותר", "קצרי את הטיוטה של הסעיף ושמרי על כל הממצאים העיקריים."],
  ["פחות שיפוטי", "נסחי את הטיוטה של הסעיף בלשון תיאורית ופחות שיפוטית, באותו תוכן ובלי להוסיף עובדות."],
  ["להרחיב", "הרחיבי את הטיוטה של הסעיף, בלי להוסיף עובדות שאינן במקורות."],
  ["לבדוק מול החומרים", "בדקי את הטיוטה מול המקורות: מה חסר, מה סותר, ומה לא מבוסס, ותקני."],
];

/** What was asked, as one request: the chosen styles, then her own words. */
export function composeWish(chips: readonly BriefChip[], picked: readonly string[], typed: string): string {
  const styles = chips.filter(([label]) => picked.includes(label)).map(([, text]) => text);
  const own = typed.trim();
  return [...styles, ...(own ? [own] : [])].join(" ");
}

/** What was in the box as it went: her words, the styles pressed, and "keep it for this section". */
export interface BriefLeft {
  text: string;
  picked: string[];
  keep: boolean;
}

export function BriefBox({ id, title, hint, placeholder, chips, chipsTitle, recent, initial, picked: firstPicked, keep, sendLabel, foot, busy, autoFocus, needs, describedBy, onSend, onCancel, onGone }: {
  id: string;
  title: string;
  hint: string;
  placeholder: string;
  chips: readonly BriefChip[];
  chipsTitle: string;
  /** Her last wishes, offered again in one click. */
  recent?: readonly string[] | undefined;
  /** What the box opens with (a wish kept for the section). */
  initial?: string;
  /** The styles pressed as it opens (as she left them). */
  picked?: readonly string[] | undefined;
  /** "Keep it for this section": shown when given, with its first state. */
  keep?: boolean;
  sendLabel: string;
  foot?: string;
  busy: boolean;
  autoFocus?: boolean;
  /** When an empty box cannot send (a change, her points): what must be there, and the
   *  sentence said when it is not. Without it, an empty box sends (a new section, as always). */
  needs?: { what: "wish" | "typed"; say: string } | undefined;
  /** A line about the box outside it, said with the box (its id). */
  describedBy?: string | undefined;
  /** `wish`: the whole request (styles and words); `typed`: her own words only; `left`: the box
   *  as she sent from it. */
  onSend: (wish: string, typed: string, keep: boolean, left: BriefLeft) => void;
  onCancel: () => void;
  /** The box went, for any reason (folded, its section or the page left, sent, the vault
   *  locked): what was in it then, and whether she changed any of it since it opened. */
  onGone?: (left: BriefLeft, changed: boolean) => void;
}) {
  const box = useRef<LocalTextHandle>(null);
  const frame = useRef<HTMLDivElement>(null);
  const [picked, setPicked] = useState<string[]>(() => [...(firstPicked ?? [])]);
  const [keeping, setKeeping] = useState(keep ?? false);
  /** What it opened with, to tell what she changed. */
  const [opened] = useState(() => ({ text: initial ?? "", picked: [...(firstPicked ?? [])].sort().join("\n"), keep: keep ?? false }));
  /** Said under the box when Enter or the button found nothing to send. */
  const [missing, setMissing] = useState<string | null>(null);

  useEffect(() => {
    if (autoFocus) box.current?.focus();
  }, [autoFocus]);
  // As the box goes: its words from the box itself (the text lives there, F1a), the styles and
  // the checkbox as they are then.
  const latest = useRef({ picked, keeping, onGone });
  useEffect(() => {
    latest.current = { picked, keeping, onGone };
  });
  useEffect(() => {
    const el = frame.current;
    const text = box.current;
    return () => {
      // A trial unmount (StrictMode, in development) leaves the box on the page: it has not gone.
      if (el?.isConnected) return;
      const now = latest.current;
      const left = { text: text?.value() ?? "", picked: now.picked, keep: now.keeping };
      const changed = left.text !== opened.text || [...left.picked].sort().join("\n") !== opened.picked || left.keep !== opened.keep;
      now.onGone?.(left, changed);
    };
  }, [opened]);

  const send = () => {
    if (busy) return;
    const typed = box.current?.value() ?? "";
    const wish = composeWish(chips, picked, typed);
    if (needs && !(needs.what === "typed" ? typed.trim() : wish.trim())) {
      // Nothing to send: say what is missing, where she is typing, instead of doing nothing.
      setMissing(needs.say);
      box.current?.focus();
      return;
    }
    setMissing(null);
    onSend(wish, typed.trim(), keeping, { text: typed, picked: [...picked], keep: keeping });
  };
  const toggle = (label: string) => {
    setMissing(null);
    setPicked((p) => (p.includes(label) ? p.filter((x) => x !== label) : [...p, label]));
  };

  return (
    <div ref={frame} className="brief-box" role="group" aria-labelledby={`${id}-title`}
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onCancel(); } }}>
      <label className="brief-box-label" htmlFor={id}>
        <span id={`${id}-title`} className="brief-box-title">{title}</span>
        <span className="small muted">{hint}</span>
      </label>
      <LocalText ref={box} id={id} className="textarea brief-box-text" rows={2} resize={fitRows} initial={initial ?? ""} placeholder={placeholder}
        aria-describedby={[missing ? `${id}-missing` : "", describedBy ?? ""].filter(Boolean).join(" ") || undefined}
        onEmptyChange={(empty) => { if (!empty) setMissing(null); }}
        onKeyDown={(e) => {
          if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
          e.preventDefault();
          send();
        }} />
      {missing && <span id={`${id}-missing`} className="small warn-text" role="alert">{missing}</span>}
      <div className="brief-box-group">
        <span className="small muted brief-box-sub">{chipsTitle}</span>
        <div className="row wrap-row">
          {chips.map(([label]) => (
            <button key={label} type="button" className={picked.includes(label) ? "pill-btn on" : "pill-btn"}
              aria-pressed={picked.includes(label)} onClick={() => toggle(label)}>{label}</button>
          ))}
        </div>
      </div>
      {recent && recent.length > 0 && (
        <div className="brief-box-group">
          <span className="small muted brief-box-sub">ממה שביקשת קודם</span>
          <div className="row wrap-row">
            {recent.map((r) => (
              <button key={r} type="button" className="pill-btn brief-box-recent" title={r}
                onClick={() => { box.current?.set(r); box.current?.focus(); }}>↺ {r.length > 40 ? `${r.slice(0, 40)}…` : r}</button>
            ))}
          </div>
        </div>
      )}
      <div className="brief-box-foot">
        {keep !== undefined && (
          <label className="brief-box-keep">
            <input type="checkbox" checked={keeping} onChange={(e) => setKeeping(e.target.checked)} />
            לזכור לסעיף הזה גם בדוחות הבאים
          </label>
        )}
        {foot && <span className="small muted">{foot}</span>}
        <span className="grow" />
        <button type="button" className="btn" onClick={onCancel}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy} onClick={send}>{sendLabel}</button>
      </div>
      <span className="small muted">{needs ? "Enter שולח" : "Enter שולח, גם בלי לכתוב כלום"} · Shift+Enter שורה חדשה · ההנחיה עוברת את הסינון יחד עם החומרים.</span>
    </div>
  );
}

/** The box grows to four lines with what she types. */
function fitRows(box: HTMLTextAreaElement) {
  const text = box.value;
  box.rows = Math.min(4, Math.max(2, Math.ceil(text.length / 70) + text.split("\n").length - 1));
}
