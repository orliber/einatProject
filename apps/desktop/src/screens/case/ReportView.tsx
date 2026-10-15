import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type ReactNode, type RefObject } from "react";
import { useApp } from "../../App";
import { ipc, type CaseDetail, type ReportSettings, type TemplateView } from "../../ipc/client";
import { kindLabel } from "../../i18n/he";
import { ageWords } from "../../components/AgeField";
import { ErrorLine, SparkIcon, WarnIcon } from "../../components/ui";
import { Why } from "../../components/Why";
import { ProgressLine } from "../../components/Progress";
import { useHoldUnsaved } from "../../components/Unsaved";
import { setKept, useKept } from "../../kept";
import { guideOpenKey } from "../../components/SectionGuide";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { useRotatingPlaceholder } from "../../components/Rotating";
import { DRAFT_HINTS, PARAGRAPH_HINTS } from "../../i18n/suggestions";
import { ParagraphVersions } from "../../components/ParagraphVersions";
import { isCombo, KEYS } from "../../shortcuts";
import { ActionsMenu, type MenuItem } from "../../components/Menu";
import { SentHistoryDialog } from "../../components/SentHistory";
import { DRAFT_FROM_APPROVED, DRAFT_FROM_SOURCES } from "./instructions";
import type { CaseApi } from "../CaseScreen";
import "./ReportView.css";
import { MarkedText, StyleHintView, applyMark } from "./StyleHintView";
import { useAi } from "../../ai";

type Section = CaseDetail["sections"][number];

/**
 * The report as the Word file will look, and the place to work on it: an A4 page with the
 * title, the child's details and every section in order. On the page: write a section with
 * Claude, approve a draft, edit any paragraph, add one of her own. Only approved paragraphs
 * go into the file; a draft waiting for approval is marked, an empty section is a gap to fill.
 */
export function ReportView({ api, current, focus, onPick }: {
  api: CaseApi;
  onWriteAll: () => void;
  /** The section in focus (round 48): one at a time, with its buttons; the rest are the page. */
  current: string | null;
  /** A section chosen in the contents, by Alt+↓ or by Enter: it comes into view. */
  focus?: { key: string; at: number; move: boolean } | null;
  /** Move the focus; `move` brings the section into view and puts the keyboard on it. */
  onPick: (key: string, move?: boolean) => void;
}) {
  const { detail } = api;
  const [settings, setSettings] = useState<ReportSettings | null>(null);

  useEffect(() => {
    ipc.reportSettings().then(setSettings).catch(() => setSettings(null));
  }, []);
  // V4: the page in her template's fonts, size and margins, when she has one.
  const [template, setTemplate] = useState<TemplateView | null>(null);
  useEffect(() => {
    ipc.reportTemplate().then(setTemplate).catch(() => setTemplate(null));
  }, []);
  const look = pageLook(template, settings?.font ?? null);

  // A section chosen in the contents, the next-step line or by Enter comes into view, briefly
  // marked, and the keyboard follows the eye: focus lands on its heading (UX-5, UX-7).
  useEffect(() => {
    if (!focus?.key || !focus.move) return;
    const el = document.getElementById(`sec-${focus.key}`);
    if (!el) return;
    el.scrollIntoView({ block: "start", behavior: "smooth" });
    el.classList.add("a4-flash");
    el.querySelector<HTMLElement>(".a4-heading")?.focus({ preventScroll: true });
    const t = setTimeout(() => el.classList.remove("a4-flash"), 1600);
    return () => clearTimeout(t);
  }, [focus]);

  // Arriving at the page with a draft waiting: the keyboard is on it, so Enter approves it.
  const arrived = useRef(false);
  useEffect(() => {
    if (arrived.current || focus?.move || !current) return;
    arrived.current = true;
    const s = detail.sections.find((x) => x.key === current);
    if (!s?.paragraphs.some((p) => p.status === "proposed")) return;
    const el = document.getElementById(`sec-${current}`);
    el?.scrollIntoView({ block: "nearest" });
    el?.querySelector<HTMLElement>(".a4-heading")?.focus({ preventScroll: true });
  }, [current, detail.sections, focus]);

  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const childLabel = detail.meta.child_gender === "female" ? "שם הילדה" : detail.meta.child_gender === "male" ? "שם הילד" : "שם הילד/ה";
  const parts = Array.from(new Set(detail.sections.map((s) => s.part)));
  const content = detail.sections.filter((s) => s.key !== "signature");
  const signature = detail.sections.find((s) => s.key === "signature");
  const signatureLines = signature?.paragraphs.filter((p) => p.status === "approved").flatMap((p) => p.text.split("\n")) ?? [];
  const supervisor = settings?.supervisor ?? [];
  const sign = [...(signatureLines.length ? signatureLines : settings?.signature ?? []), ...(supervisor.length ? ["", ...supervisor] : [])];

  return (
    <div className="view">
      <div className="report-desk">
        <article className="a4" style={look.style} aria-label="הדוח, כמו בקובץ">
          <header className="a4-header">{settings?.confidentiality}</header>
          <h1 className="a4-title">{settings?.title ?? "דוח אבחון פסיכולוגי"}</h1>
          <dl className="a4-info">
            {child && <><dt>{childLabel}</dt><dd>{child}</dd></>}
            {detail.meta.age && <><dt>גיל בזמן האבחון</dt><dd>{ageWords(detail.meta.age)}</dd></>}
            <dt>תאריך הדוח</dt><dd>{new Date().toLocaleDateString("he-IL")}</dd>
          </dl>
          {parts.map((part) => (
            <section key={part}>
              <h2 className="a4-part">{part}</h2>
              {content.filter((s) => s.part === part).map((s) => (
                <A4Section key={s.key} api={api} section={s} focused={current === s.key}
                  onPick={() => { if (current !== s.key) onPick(s.key); }}
                  onNext={() => { const n = nextWaiting(content, s.key); if (n) onPick(n, true); }} />
              ))}
            </section>
          ))}
          {sign.length > 0 && (
            <footer className="a4-sign">
              {sign.map((l, i) => (l ? <p key={i}>{l}</p> : <br key={i} />))}
            </footer>
          )}
        </article>
      </div>
    </div>
  );
}

const hasDraft = (s: Section) => s.paragraphs.some((p) => p.status === "proposed");

/** The section the focus starts on: the first draft waiting, else the first section to write. */
export function firstFocus(sections: Section[]): string | null {
  const content = sections.filter((s) => s.key !== "signature");
  const waiting = content.find(hasDraft);
  if (waiting) return waiting.key;
  const toWrite = content.find((s) => s.paragraphs.length === 0 && s.mode !== "self" && (s.source_count > 0 || !s.sortable));
  return (toWrite ?? content[0])?.key ?? null;
}

/** After `key`, the next section with a draft waiting (round 48): Enter approves and moves on to
 *  it, never to an empty one. From the end it goes round to the top; `null` when none is left. */
export function nextWaiting(sections: Section[], key: string): string | null {
  const at = sections.findIndex((s) => s.key === key);
  const after = [...sections.slice(at + 1), ...sections.slice(0, Math.max(at, 0))];
  return after.find(hasDraft)?.key ?? null;
}

/** Twentieths of a point to screen pixels (96 per inch, 1440 twentieths per inch). */
const TWIPS_PER_PX = 15;

/**
 * The page's look: her template's font, text size, page width and margins where it says them,
 * else the font chosen in Settings, else the app's own. Font names arrive checked by the core
 * (letters, digits, spaces); they are quoted, and only ever set as a style value.
 */
export function pageLook(template: TemplateView | null, settingsFont: string | null): { style: CSSProperties; fromTemplate: boolean } {
  const style: CSSProperties & Record<string, string> = {};
  const font = template?.font ?? settingsFont;
  if (font) style.fontFamily = `"${font.replace(/"/g, "")}", var(--font-display)`;
  if (template?.size_half_points) style.fontSize = `${template.size_half_points / 2}pt`;
  if (template?.heading_font) style["--a4-heading-font"] = `"${template.heading_font.replace(/"/g, "")}"`;
  if (template?.page_twips) {
    style.width = `min(${Math.round(template.page_twips[0] / TWIPS_PER_PX)}px, 100%)`;
    style.minHeight = `${Math.round(template.page_twips[1] / TWIPS_PER_PX)}px`;
  }
  if (template?.margins_twips) {
    const [t, r, b, l] = template.margins_twips.map((m) => `${Math.round(m / TWIPS_PER_PX)}px`);
    style.padding = `${t} ${r} ${b} ${l}`;
  }
  const fromTemplate = Boolean(template && (template.font || template.size_half_points || template.margins_twips || template.page_twips));
  return { style, fromTemplate };
}

/** "✦ לשנות" on a draft (round 48): four quick changes, and the change in her own words. */
const CHANGES: [string, string][] = [
  ["קצר יותר", "קצרי את הטיוטה של הסעיף ושמרי על כל הממצאים העיקריים."],
  ["פחות שיפוטי", "נסחי את הטיוטה של הסעיף בלשון תיאורית ופחות שיפוטית, באותו תוכן ובלי להוסיף עובדות."],
  ["להרחיב", "הרחיבי את הטיוטה של הסעיף, בלי להוסיף עובדות שאינן במקורות."],
  ["לבדוק מול החומרים", "בדקי את הטיוטה מול המקורות: מה חסר, מה סותר, ומה לא מבוסס, ותקני."],
];

const PARA_CHANGES: [string, string][] = [
  ["לקצר", "קצרי את הפסקה הבאה ושמרי על כל הממצאים שבה."],
  ["לנסח אחרת", "נסחי מחדש את הפסקה הבאה, באותו תוכן ובלי להוסיף עובדות."],
  ["להרחיב", "הרחיבי את הפסקה הבאה, רק ממה שיש במקורות."],
  ["לפשט להורים", "נסחי את הפסקה הבאה בשפה פשוטה וברורה להורים, באותו תוכן."],
];

const DERIVED = ["summary", "diagnoses", "recommendations", "dsm"];

/** The request for a first draft, with what she wrote in "מה חשוב לך שייכתב כאן?" (round 48).
 *  It goes the way every request of hers goes: prepared, through the gate, then sent. */
export function draftRequest(sectionKey: string, brief: string): string | undefined {
  const wish = brief.trim();
  if (!wish) return undefined;
  const base = DERIVED.includes(sectionKey) ? DRAFT_FROM_APPROVED : DRAFT_FROM_SOURCES;
  return `${base} מה שחשוב לי שייכתב בסעיף: ${wish}`;
}

/** "או במילים שלך" inside an open card. Its changing hint is a timer, so it lives here, in
 *  the card, and not in the section: fifteen closed cards run no timers. Enter sends. */
function WishBox({ boxRef, hints, wish, typing, onText, onEmptyChange, onSend }: {
  boxRef: RefObject<LocalTextHandle | null>;
  hints: readonly string[];
  wish: string;
  typing: boolean;
  onText: (text: string) => void;
  onEmptyChange: (empty: boolean) => void;
  onSend: () => void;
}) {
  const hint = useRotatingPlaceholder(hints, typing);
  return (
    <label className="stack small muted" style={{ gap: 4 }}>
      או במילים שלך
      <LocalText ref={boxRef} className="textarea" rows={2} initial={wish} placeholder={hint}
        onText={onText} onEmptyChange={onEmptyChange}
        onKeyDown={(e) => {
          if (e.key !== "Enter" || e.shiftKey) return;
          e.preventDefault();
          onSend();
        }} />
    </label>
  );
}

/** A small card next to what opened it; Escape or a click outside closes it. */
function Popover({ label, onClose, children }: { label: string; onClose: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  }, [onClose]);
  // Focus moves in once, when it opens (not on every keystroke inside it).
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("button, input, textarea")?.focus();
    const away = (e: MouseEvent) => {
      // The button that opened it (in the same anchor) toggles it itself.
      const anchor = ref.current?.parentElement ?? ref.current;
      if (anchor && !anchor.contains(e.target as Node)) close.current();
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, []);
  return (
    <div ref={ref} className="a4-pop" role="dialog" aria-label={label}
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      {children}
    </div>
  );
}

/** "✦ לשנות" opened under the draft, on the page: the keyboard moves into it, Esc closes it. */
function ChangePanel({ onClose, children }: { onClose: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("button")?.focus();
  }, []);
  return (
    <div ref={ref} className="a4-change" role="group" aria-label="מה לשנות בטיוטה"
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      {children}
    </div>
  );
}

/**
 * One section on the page (round 48). In focus it has its buttons, in one of four states: before
 * the draft ("מה חשוב לך שייכתב כאן?" and "✦ לנסח"), a draft waiting ("✓ לאשר · ✦ לשנות · לערוך
 * · ⋯"), approved (the text, as in the file), or her own section. Out of focus it is the page: an
 * approved section as it will print, a draft in grey "מחכה לתורה", an empty one a single line.
 */
function A4Section({ api, section: s, focused, onPick, onNext }: {
  api: CaseApi;
  section: Section;
  focused: boolean;
  onPick: () => void;
  /** After a draft is approved: the next draft waiting comes into focus. */
  onNext: () => void;
}) {
  const { go, fail } = useApp();
  const ai = useAi();
  const { caseId, reload, detail } = api;
  /** A paragraph being edited in place, or a new one at `at`: "first", "end" or after a paragraph's id.
   *  The text being typed lives in the box (`editBox`); `text` is its debounced copy, for the
   *  vault lock, and `empty` follows the box only when it turns empty or back (F1a). */
  // Kept while the vault is open: leaving for another stage and coming back finds the editor
  // open with what she typed (F1a, "ובעזיבה").
  const [editing, setEditing] = useKept<{ id: string | null; text: string; original: string; at?: string | undefined; empty: boolean } | null>(`case:${caseId}:report:${s.key}:editing`, null);
  const editBox = useRef<LocalTextHandle>(null);
  const edit = (id: string | null, text: string, at?: string) => setEditing({ id, text, original: text, at, empty: !text.trim() });
  /** "ביטול" or Esc with a changed text: asked first, so one key never loses what she typed. */
  const [leaving, setLeaving] = useState(false);
  function close() {
    setLeaving(false);
    setEditing(null);
  }
  function cancel() {
    if (!editing) return;
    const typed = editBox.current?.value() ?? editing.text;
    if (typed.trim() === editing.original.trim()) close();
    else setLeaving(true);
  }
  /** Just after an action: the second click of a double click lands on what the first one
   *  revealed (an insert slot where the bar was), so that slot is held for a moment. */
  const [settling, setSettling] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [open, setOpen] = useState<"sources" | null>(null);
  /** "✦ לשנות" open under the draft. */
  const [changing, setChanging] = useState(false);
  /** "מה יצא מהמחשב" for this section, from its ⋯ menu. */
  const [sentOpen, setSentOpen] = useState(false);
  /** The paragraph whose "✦ with Claude" card is open. */
  const [paraPop, setParaPop] = useState<string | null>(null);
  /** The paragraph whose earlier wordings are open (UX-4). */
  const [history, setHistory] = useState<string | null>(null);
  /** The paragraph whose "why did you write this?" card is open (AI-7). */
  const [whyPop, setWhyPop] = useState<string | null>(null);
  /** The paragraph whose delete button asks "למחוק?". */
  const [deleting, setDeleting] = useState<string | null>(null);
  const holdLive = useHoldUnsaved(caseId, `פסקה בסעיף "${s.title}"`, editing?.text ?? null);
  // "In your own words" for Claude: one wish, kept while a card closes and another opens.
  const [wish, setWish] = useState("");
  const [wishEmpty, setWishEmpty] = useState(true);
  const paraWish = useRef<LocalTextHandle>(null);
  const draftWish = useRef<LocalTextHandle>(null);
  // "מה חשוב לך שייכתב כאן?" (round 48): kept with the section while the vault is open, in
  // memory only, like the points of her own sections; it leaves only inside the request.
  const [brief, setBrief] = useKept(`case:${caseId}:report:${s.key}:brief`, "");
  const briefBox = useRef<LocalTextHandle>(null);
  const [sourcesChanged, setSourcesChanged] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const job = api.jobs.find((j) => j.section === s.key);
  const ok = s.paragraphs.filter((p) => p.status === "approved");
  const proposed = s.paragraphs.filter((p) => p.status === "proposed");
  // A new wording of an approved paragraph is shown beside it, not as a new draft.
  const reworded = new Map(proposed.filter((p) => p.replaces).map((p) => [p.replaces ?? "", p]));
  const pending = proposed.filter((p) => !p.replaces);
  // Her own sections (round 30): nothing is drafted from the materials, so no AI control here;
  // her points are worded on the section's own screen.
  const mine = s.mode === "self";
  const canWrite = !mine && (s.source_count > 0 || !s.sortable);
  const busy = preparing || job !== undefined;
  const sources = detail.routing
    .filter((r) => r.feeds.includes(s.key))
    .map((r) => detail.inputs.find((i) => i.id === r.input_id))
    .filter((i) => i !== undefined);
  const sourceNames = sources.map((i) => i.title || kindLabel[i.kind]);

  async function act(fn: () => Promise<unknown>): Promise<boolean> {
    setError(null);
    setSettling(true);
    try {
      await fn();
      await reload();
      return true;
    } catch (e) {
      setError(fail(e as never));
      return false;
    } finally {
      window.setTimeout(() => setSettling(false), 400);
    }
  }

  async function write(instruction?: string, replaces?: string) {
    onPick();
    setOpen(null);
    setChanging(false);
    setParaPop(null);
    setSourcesChanged(false);
    setError(null);
    setPreparing(true);
    try {
      await api.draft(s.key, instruction, replaces);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setPreparing(false);
    }
  }

  /** "✓ לאשר": the whole draft goes into the report, and the next draft waiting comes up. */
  async function approveAll() {
    const ids = pending.map((p) => p.id);
    if (ids.length === 0) return onNext();
    if (await act(async () => { for (const id of ids) await ipc.approveParagraph(caseId, id); })) onNext();
  }

  function writeFromBrief() {
    const typed = briefBox.current?.value() ?? brief;
    setBrief(typed);
    void write(draftRequest(s.key, typed));
  }

  /** The wish in a box, taken out of it to be sent; `null` when there is nothing to send. */
  function takeWish(box: RefObject<LocalTextHandle | null>): string | null {
    const w = box.current?.value() ?? "";
    if (!w.trim()) return null;
    box.current?.set("");
    return w;
  }

  async function save() {
    if (!editing) return;
    // What is in the box now, not the debounced copy.
    const text = editBox.current?.value() ?? editing.text;
    const { id, at } = editing;
    // A new paragraph left empty is just not added.
    if (!text.trim() && !id) return close();
    await act(async () => {
      // All the text deleted and saved: the paragraph goes away.
      if (id && !text.trim()) await ipc.rejectParagraph(caseId, id);
      else if (id) await ipc.editParagraph(caseId, id, text);
      else await ipc.addOwnParagraph(caseId, s.key, text, at ?? "end");
      close();
    });
  }

  const openGuide = () => {
    // Her points are at the top of her section; elsewhere the guide opens on arrival.
    if (!mine) setKept(guideOpenKey(caseId, s.key), false)(true);
    go({ name: "case", id: caseId, view: s.key });
  };

  /** Keys on the section's heading (round 48): Enter approves the draft and moves on to the
   *  next one, Ctrl+Enter opens "✦ לשנות", Esc closes it. */
  function headKey(e: ReactKeyboardEvent) {
    if (e.target !== e.currentTarget) return;
    const ev = e.nativeEvent;
    if (e.key === "Escape" && changing) {
      e.preventDefault();
      setChanging(false);
    } else if (isCombo(ev, KEYS.changeWithAi) || isCombo(ev, KEYS.approve)) {
      e.preventDefault();
      if (pending.length > 0 && !mine && !busy) setChanging(true);
    } else if (e.key === "Enter" && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
      e.preventDefault();
      // One press approves one draft: a held key (auto-repeat) or a press while the last
      // approval is still saving does nothing, so holding Enter never approves the report.
      if (e.repeat || job || busy || settling) return;
      if (pending.length > 0) void approveAll();
      else if (s.paragraphs.length === 0 && canWrite) briefBox.current?.focus();
      else onNext();
    }
  }

  /** One paragraph on the page: click to edit, "✦" to change it with Claude. */
  const para = (p: Section["paragraphs"][number], note?: string) =>
    editing?.id === p.id ? (
      <div key={p.id}>{editor}</div>
    ) : (
      <div key={p.id} className={`a4-para${p.status === "proposed" ? " is-proposed" : ""}${paraPop === p.id || deleting === p.id || history === p.id ? " is-active" : ""}`}>
        <p className={p.by_ai ? "a4-p a4-editable" : "a4-p a4-editable a4-own"} tabIndex={0} title={focused ? note ?? "לחיצה לעריכה" : "לחיצה לבחירת הסעיף"}
          aria-label={`${p.status === "proposed" ? "טיוטה שממתינה לאישור" : "פסקה"}${p.by_ai ? " (AI)" : ""}: ${p.text}`}
          aria-describedby={`keys-${s.key}`}
          onClick={() => {
            // A draft out of focus is chosen by the first click (the section takes it); the next one edits it.
            if (!focused && p.status === "proposed") return;
            edit(p.id, p.text);
          }}
          onKeyDown={(e) => paraKey(e, p)}>
          <MarkedText text={p.text} marks={p.status === "proposed" && focused ? p.style?.marks ?? [] : []} />
        </p>
        {p.warnings.map((w, i) => (
          <div key={i} className="note-warn small"><WarnIcon /><span>{w}</span></div>
        ))}
        {focused && p.status === "proposed" && p.style && !mine && (
          <StyleHintView hint={p.style} busy={busy}
            onCloser={(how) => void write(`נסחי מחדש את הפסקה הבאה כך שתישמע כמו הכתיבה שלי: ${how}. באותו תוכן ובלי להוסיף עובדות: "${p.text}"`, p.id)}
            onApply={(m) => void act(() => ipc.editParagraph(caseId, p.id, applyMark(p.text, m)))} />
        )}
        {focused && p.style_note && <span className="small muted style-note">{p.style_note}</span>}
        {focused && !busy && (
          <span className="a4-pop-anchor a4-para-tool">
            {p.by_ai && <button type="button" className="a4-why" aria-expanded={whyPop === p.id} aria-label="למה כתבת את זה?" title="למה כתבת את זה? (המקורות של הפסקה)"
              onClick={() => { setParaPop(null); setWhyPop(whyPop === p.id ? null : p.id); }}>?</button>}
            {whyPop === p.id && (
              <Popover label="המקורות של הפסקה" onClose={() => setWhyPop(null)}>
                <b>למה כתבתי את זה?</b>
                <Why caseId={caseId} draftId={p.id} />
              </Popover>
            )}
            {canWrite && <button type="button" className="a4-ai" aria-expanded={paraPop === p.id} aria-label="לשנות את הפסקה עם AI" title="לשנות את הפסקה עם AI"
              onClick={() => setParaPop(paraPop === p.id ? null : p.id)}><SparkIcon size={16} /></button>}
            {p.has_versions && (
              <button type="button" className="a4-hist" aria-expanded={history === p.id} aria-label="גרסאות קודמות של הפסקה" title="גרסאות קודמות (Ctrl+Shift+H)"
                onClick={() => { setParaPop(null); setHistory(history === p.id ? null : p.id); }}><HistoryIcon /></button>
            )}
            {history === p.id && (
              <Popover label="גרסאות קודמות של הפסקה" onClose={() => setHistory(null)}>
                <ParagraphVersions caseId={caseId} draftId={p.id} onRestored={async () => { setHistory(null); await reload(); }} />
              </Popover>
            )}
            {deleting === p.id ? (
              <span className="a4-del-ask" role="group" aria-label="מחיקת הפסקה">
                <span>למחוק?</span>
                <button type="button" className="a4-del-yes" onClick={() => void act(async () => { await ipc.rejectParagraph(caseId, p.id); setDeleting(null); })}>כן</button>
                <button type="button" className="a4-del-no" onClick={() => setDeleting(null)}>לא</button>
              </span>
            ) : (
              <button type="button" className="a4-del" aria-label="מחיקת הפסקה" title="מחיקת הפסקה" onClick={() => { setParaPop(null); setDeleting(p.id); }}><TrashIcon /></button>
            )}
            {paraPop === p.id && (
              <Popover label="מה לשנות בפסקה" onClose={() => setParaPop(null)}>
                <b>מה לשנות בפסקה הזאת?</b>
                <div className="row wrap-row">
                  {PARA_CHANGES.map(([label, text]) => (
                    <button key={label} type="button" className="pill-btn"
                      onClick={() => void write(`${text} הפסקה: "${p.text}"`, p.id)}>{label}</button>
                  ))}
                </div>
                <WishBox boxRef={paraWish} hints={PARAGRAPH_HINTS} wish={wish} typing={!wishEmpty} onText={setWish} onEmptyChange={setWishEmpty}
                  onSend={() => { const w = takeWish(paraWish); if (w !== null) void write(`${w} הפסקה: "${p.text}"`, p.id); }} />
                <div className="a4-pop-foot">
                  <span className="small muted grow">{p.status === "approved" ? "יופיע ליד הקיים, ויחליף אותו רק באישורך." : "יחליף את הפסקה בטיוטה."}</span>
                  <button type="button" className="btn btn-primary btn-small" disabled={wishEmpty}
                    onClick={() => { const w = takeWish(paraWish); if (w !== null) void write(`${w} הפסקה: "${p.text}"`, p.id); }}>שליחה</button>
                </div>
              </Popover>
            )}
          </span>
        )}
        {reworded.get(p.id) && (
          <div className="a4-reword">
            <span className="a4-flag">ניסוח חדש של {ai} לפסקה הזאת</span>
            <p className="a4-p">{reworded.get(p.id)?.text}</p>
            {focused && (
              <span className="row">
                <button type="button" className="btn btn-primary btn-small" onClick={() => void act(() => ipc.approveParagraph(caseId, reworded.get(p.id)?.id ?? ""))}>✓ להחליף</button>
                <button type="button" className="btn btn-small" onClick={() => void act(() => ipc.rejectParagraph(caseId, reworded.get(p.id)?.id ?? ""))}>להשאיר את הקודם</button>
              </span>
            )}
          </div>
        )}
      </div>
    );

  /** Keys on a paragraph that has the focus (UX-5). */
  function paraKey(e: ReactKeyboardEvent, p: Section["paragraphs"][number]) {
    if (e.target !== e.currentTarget) return;
    const ev = e.nativeEvent;
    if (isCombo(ev, KEYS.approveDraft) && pending.length > 0) {
      e.preventDefault();
      void approveAll();
    } else if (isCombo(ev, KEYS.approve)) {
      e.preventDefault();
      if (p.status === "proposed") void act(() => ipc.approveParagraph(caseId, p.id));
    } else if (isCombo(ev, KEYS.changeWithAi)) {
      e.preventDefault();
      if (canWrite && !busy) { setHistory(null); setParaPop(p.id); }
    } else if (isCombo(ev, KEYS.versions)) {
      e.preventDefault();
      if (p.has_versions && !busy) { setParaPop(null); setHistory(p.id); }
    } else if (e.key === "Enter" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      edit(p.id, p.text);
    }
  }

  /** The box being typed in. Its text stays in the box; the page re-renders only when it turns
   *  empty or back, and when the debounced copy for the vault lock arrives (F1a). */
  const editor = editing && (
    <div className="a4-edit">
      <LocalText key={editing.id ?? `new-${editing.at ?? "end"}`} ref={editBox} className="a4-edit-text" resize={fitRows} autoFocus
        aria-label={editing.id ? `עריכת פסקה בסעיף ${s.title}` : `פסקה חדשה בסעיף ${s.title}`} initial={editing.text} onLive={holdLive}
        placeholder="כותבים כאן, כמו בוורד. שמות מוסתרים לפני כל שליחה."
        onText={(text) => { const { id, at } = editing; setEditing((e) => (e && e.id === id && e.at === at ? { ...e, text } : e)); }}
        onEmptyChange={(empty) => { const { id, at } = editing; setEditing((e) => (e && e.id === id && e.at === at ? { ...e, empty } : e)); }}
        onKeyDown={(e) => { if (e.key === "Escape") { if (leaving) close(); else cancel(); } if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void save(); }} />
      {leaving ? (
        <div className="row" role="group" aria-label="לבטל את העריכה?">
          <span className="small grow">לבטל את מה שכתבת כאן? הטקסט לא נשמר.</span>
          <button type="button" className="btn btn-small danger-text" onClick={close}>כן, לבטל</button>
          <button type="button" className="btn btn-primary btn-small" onClick={() => { setLeaving(false); editBox.current?.focus(); }}>להמשיך לערוך</button>
        </div>
      ) : (
      <div className="row">
        <button type="button" className="btn btn-primary btn-small" disabled={!editing.id && editing.empty} onClick={() => void save()}>
          {editing.id && editing.empty ? "שמירה (הפסקה תימחק)" : "שמירה"}
        </button>
        <button type="button" className="btn btn-small" onClick={cancel}>ביטול</button>
        <span className="small muted grow">Ctrl+Enter לשמירה · Esc לביטול</span>
        {editing.id && (
          <button type="button" className="link-btn small danger-text" onClick={() => { const id = editing.id ?? ""; void act(async () => { await ipc.rejectParagraph(caseId, id); close(); }); }}>מחיקת הפסקה</button>
        )}
      </div>
      )}
    </div>
  );

  /** Between paragraphs, and at the top and bottom: "+ פסקה משלי כאן", or the new paragraph being written there. */
  const slot = (at: string) =>
    editing && editing.id === null && editing.at === at ? (
      <div key={`slot-${at}`}>{editor}</div>
    ) : focused ? (
      <div key={`slot-${at}`} className="a4-insert">
        <button type="button" className="a4-insert-btn" disabled={editing !== null || settling} aria-label="הוספת פסקה משלי כאן"
          onClick={() => edit(null, "", at)}>+ פסקה משלי כאן</button>
      </div>
    ) : null;

  // Everything else the section can do, behind one "⋯" (round 48).
  const menu: MenuItem[] = [
    mine ? { label: "הנקודות שלך לסעיף", run: openGuide } : { label: "איך לכתוב את הסעיף", run: openGuide },
    ...(s.sortable && !mine ? [{ label: "מאיזה חומרים לכתוב", run: () => setOpen("sources") }] : []),
    { label: "+ פסקה משלי בסוף הסעיף", run: () => edit(null, "", "end"), disabled: editing !== null },
    { label: "מה יצא מהמחשב בשביל הסעיף", run: () => setSentOpen(true) },
    ...(pending.length > 0 ? [{ label: "הסרת הטיוטה", danger: true, run: () => void act(async () => { for (const p of pending) await ipc.rejectParagraph(caseId, p.id); }) }] : []),
  ];
  const empty = s.paragraphs.length === 0;
  const state = mine ? "mine" : pending.length > 0 ? "draft" : empty ? "empty" : "approved";

  return (
    <div id={`sec-${s.key}`} className={`a4-section a4-st-${state}${focused ? " is-focus" : ""}${pending.length ? " has-pending" : ""}`}
      // A click anywhere on a section out of focus brings it into focus, after the button it hit
      // (if any) did its work. The keyboard arriving on its heading or a paragraph does the same;
      // on a button it waits for the click, so the button is still there to take it.
      onClick={() => { if (!focused) onPick(); }}
      onFocus={(e) => { if (!focused && !(e.target instanceof HTMLButtonElement)) onPick(); }}>
      <div className="a4-heading-row">
        <h3 ref={heading} className="a4-heading" tabIndex={focused ? 0 : -1} aria-describedby={focused && pending.length ? `head-keys-${s.key}` : undefined}
          onKeyDown={headKey}>{s.title}</h3>
        <span id={`keys-${s.key}`} className="visually-hidden">Enter לעריכה, Ctrl+Enter לאישור, Ctrl+K לשינוי עם AI</span>
        {focused && pending.length > 0 && <span id={`head-keys-${s.key}`} className="visually-hidden">Enter לאישור ומעבר לטיוטה הבאה, Ctrl+Enter לשינוי</span>}
        {!focused && pending.length > 0 && !job && <span className="a4-wait">טיוטה, מחכה לתורה</span>}
        <span className="a4-side" aria-hidden="true">{state === "approved" ? "✓" : state === "draft" && !focused ? "○" : ""}</span>
        {focused && !job && <ActionsMenu items={menu} label={`עוד על הסעיף "${s.title}"`} />}
      </div>

      {mine && <p className="small muted a4-mine">את כותבת את הסעיף הזה: שום חומר לא נשלח בשבילו.</p>}
      {focused && s.sortable && !mine && (
        <div className="a4-sources">
          <span>{!sources.length ? "אין חומרים לסעיף." : pending.length ? "טיוטה · מתוך" : empty ? "מתוך" : "נכתב מתוך"}</span>
          {sourceNames.slice(0, 6).map((name, i) => <span key={i} className="a4-source">{name}</span>)}
          {sources.length > 6 && <span className="a4-source">ועוד {sources.length - 6}</span>}
          <span className="a4-pop-anchor">
            <button type="button" className="a4-source-edit" aria-expanded={open === "sources"} onClick={() => setOpen(open === "sources" ? null : "sources")}>
              {sources.length ? "+ חומר / הסרה" : "+ קישור חומרים"}
            </button>
            {open === "sources" && (
              <Popover label={`מאיזה חומרים לכתוב את "${s.title}"`} onClose={() => setOpen(null)}>
                <b>מאיזה חומרים לכתוב את "{s.title}"?</b>
                <span className="small muted">הבחירה שלך גוברת על המיון.</span>
                <div className="a4-pop-list">
                  {detail.inputs.map((i) => {
                    const r = detail.routing.find((x) => x.input_id === i.id);
                    const on = r?.feeds.includes(s.key) ?? false;
                    const others = (r?.feeds ?? []).filter((k) => k !== s.key).length;
                    return (
                      <label key={i.id} className={on ? "a4-pop-row on" : "a4-pop-row"}>
                        <input type="checkbox" checked={on} disabled={!r}
                          onChange={() => void act(async () => {
                            if (!r) return;
                            await ipc.setInputSections(caseId, i.id, on ? r.feeds.filter((k) => k !== s.key) : [...r.feeds, s.key]);
                            setSourcesChanged(true);
                          })} />
                        <span className="grow stack" style={{ gap: 0 }}>
                          <b>{i.title || kindLabel[i.kind]}</b>
                          <span className="small muted">{kindLabel[i.kind]}{others > 0 ? ` · מזין עוד ${others} סעיפים` : ""}</span>
                        </span>
                      </label>
                    );
                  })}
                  {detail.inputs.length === 0 && <span className="small muted">עוד אין חומרים בתיק.</span>}
                </div>
                <button type="button" className="link-small" onClick={() => go({ name: "case", id: caseId, view: "materials" })}>+ להעלות מסמך חדש לתיק</button>
                <div className="a4-pop-foot">
                  {sourcesChanged && s.paragraphs.length > 0 ? (
                    <>
                      <span className="small grow">החומרים השתנו מאז הטיוטה.</span>
                      <button type="button" className="btn btn-primary btn-small" disabled={busy} onClick={() => void write()}>✦ לכתוב מחדש</button>
                    </>
                  ) : <span className="grow" />}
                  <button type="button" className="btn btn-small" onClick={() => setOpen(null)}>סגירה</button>
                </div>
              </Popover>
            )}
          </span>
        </div>
      )}

      {ok.length > 0 && slot("first")}
      {ok.map((p, i) => [para(p), slot(i === ok.length - 1 ? "end" : p.id)])}

      {job && (
        <div className="a4-writing">
          <ProgressLine started={job.started} estimate={job.estimate} label={job.label} approval={job.approval} />
        </div>
      )}
      {preparing && !job && <p className="small muted">מכינה את הבקשה…</p>}

      {pending.length > 0 && !job && (
        <div className={focused ? "a4-pending" : "a4-pending is-dim"}>
          {pending.map((p) => para(p, "לחיצה לעריכה (העריכה מאשרת את הפסקה)"))}
          {focused && (
            <div className="a4-actions" role="group" aria-label={`הטיוטה של "${s.title}"`}>
              <button type="button" className="btn btn-primary" onClick={() => void approveAll()}>✓ לאשר</button>
              {!mine && <button type="button" className="btn" aria-expanded={changing} disabled={busy} onClick={() => setChanging(!changing)}>✦ לשנות</button>}
              <button type="button" className="btn" disabled={editing !== null} onClick={() => { const p = pending[0]; if (p) edit(p.id, p.text); }}>לערוך</button>
            </div>
          )}
          {focused && changing && !mine && (
            <ChangePanel onClose={() => { setChanging(false); heading.current?.focus(); }}>
              <div className="row wrap-row">
                {CHANGES.map(([label, text]) => (
                  <button key={label} type="button" className="pill-btn" onClick={() => void write(text)}>{label}</button>
                ))}
              </div>
              <WishBox boxRef={draftWish} hints={DRAFT_HINTS} wish={wish} typing={!wishEmpty} onText={setWish} onEmptyChange={setWishEmpty}
                onSend={() => { const w = takeWish(draftWish); if (w !== null) void write(w); }} />
              {brief.trim() && <span className="small muted">ביקשת לפני הניסוח: {brief.trim()}</span>}
              <div className="a4-pop-foot">
                <span className="small muted grow">הטיוטה החדשה תחליף את זו. מה שאישרת נשאר.</span>
                <button type="button" className="btn btn-small" onClick={() => { setChanging(false); heading.current?.focus(); }}>סגירה</button>
                <button type="button" className="btn btn-primary btn-small" disabled={wishEmpty} onClick={() => { const w = takeWish(draftWish); if (w !== null) void write(w); }}>שליחה</button>
              </div>
            </ChangePanel>
          )}
        </div>
      )}

      {ok.length === 0 && editing && editing.id === null && editor}

      {empty && !job && !editing && !preparing && (
        mine ? (
          <span className="a4-gap-row">
            <button type="button" className="a4-gap" onClick={openGuide}>✦ לנסח מהנקודות שלי ←</button>
            {focused && <button type="button" className="a4-gap a4-gap-own" onClick={() => edit(null, "", "end")}>לכתוב בעצמי</button>}
          </span>
        ) : focused && canWrite ? (
          <div className="a4-brief">
            <label className="a4-brief-label" htmlFor={`brief-${s.key}`}>מה חשוב לך שייכתב כאן?</label>
            <LocalText ref={briefBox} id={`brief-${s.key}`} className="textarea a4-brief-text" rows={1} resize={briefRows} initial={brief}
              placeholder="לא חובה. למשל: להתחיל מהקשר עם הצוות בגן."
              onText={setBrief}
              onKeyDown={(e) => {
                if (e.key === "Escape") { e.preventDefault(); heading.current?.focus(); return; }
                if (e.key !== "Enter" || e.shiftKey) return;
                e.preventDefault();
                if (!busy) writeFromBrief();
              }} />
            <div className="row a4-brief-row">
              <button type="button" className="btn btn-primary" disabled={busy} onClick={writeFromBrief}>✦ לנסח</button>
              <button type="button" className="link-btn" onClick={() => edit(null, "", "end")}>לכתוב בעצמי</button>
              <span className="grow" />
              <button type="button" className="link-small" onClick={openGuide}>איך לכתוב את הסעיף ←</button>
            </div>
          </div>
        ) : canWrite ? (
          <div className="a4-gap-line">
            <span className="grow">עוד לא נכתב.{sourceNames.length ? ` יש חומר: ${sourceNames.slice(0, 2).join(", ")}${sourceNames.length > 2 ? ` ועוד ${sourceNames.length - 2}` : ""}` : ""}</span>
            <button type="button" className="link-btn" disabled={busy} onClick={() => void write()}>✦ לנסח</button>
          </div>
        ) : (
          <div className="a4-gap-line">
            <span className="grow">אין חומר לסעיף.</span>
            <button type="button" className="link-btn" onClick={() => setOpen("sources")}>לקשר חומרים</button>
            {focused && <button type="button" className="link-btn" onClick={() => edit(null, "", "end")}>לכתוב בעצמי</button>}
          </div>
        )
      )}
      <ErrorLine error={error} />
      {sentOpen && <SentHistoryDialog caseId={caseId} place={s.key} onClose={() => setSentOpen(false)} />}
    </div>
  );
}

/** "מה חשוב לך שייכתב כאן?" grows to three lines with what she types. */
function briefRows(box: HTMLTextAreaElement) {
  const text = box.value;
  box.rows = Math.min(3, Math.max(1, Math.ceil(text.length / 70) + text.split("\n").length - 1));
}

/** The paragraph box grows with its text, as on the page: a line per ~70 letters and per line break. */
function fitRows(box: HTMLTextAreaElement) {
  const text = box.value;
  box.rows = Math.max(2, Math.ceil(text.length / 70) + text.split("\n").length - 1);
}

/** The AI mark: the same sparkle whichever model writes (D-036). */

function HistoryIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M3 12a9 9 0 1 0 3-6.7" />
      <path d="M3 4v5h5" />
      <path d="M12 8v4l3 2" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 7h16" />
      <path d="M10 11v6M14 11v6" />
      <path d="M6 7l1 13h10l1-13" />
      <path d="M9 7V4h6v3" />
    </svg>
  );
}
