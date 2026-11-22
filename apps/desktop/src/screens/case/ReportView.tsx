import { Fragment, useEffect, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { useApp } from "../../App";
import { ipc, type CaseDetail, type ReportSettings, type TemplateView } from "../../ipc/client";
import { kindLabel, STALE_EDIT } from "../../i18n/he";
import { ageWords } from "../../components/AgeField";
import { ErrorLine, SparkIcon } from "../../components/ui";
import { Why } from "../../components/Why";
import { ProgressLine, WaitLine } from "../../components/Progress";
import { useHoldUnsaved } from "../../components/Unsaved";
import { useKept } from "../../kept";
import { SectionGuide, guideOpenKey } from "../../components/SectionGuide";
import { SectionLibraryPanel } from "../../components/SectionLibrary";
import { DsmSeverityPanel } from "../../components/DsmSeverity";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { PARAGRAPH_HINTS } from "../../i18n/suggestions";
import { ParagraphVersions } from "../../components/ParagraphVersions";
import { isCombo, KEYS } from "../../shortcuts";
import { ActionsMenu, type MenuItem } from "../../components/Menu";
import { SentHistoryDialog } from "../../components/SentHistory";
import { DRAFT_FROM_APPROVED, DRAFT_FROM_SOURCES } from "./instructions";
import type { CaseApi } from "../CaseScreen";
import "./ReportView.css";
import { MarkedText, StyleHintView, applyMark } from "./StyleHintView";
import { useAi } from "../../ai";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { BriefBox, CHANGE_CHIPS, STYLE_CHIPS, type BriefLeft } from "./BriefBox";
import { ParagraphNotes, SectionNotesFold } from "./notes";
import { CLOSING_KEYS, ClosingFlow, closingAnchor, ownEmptyLine } from "./closing";

type Section = CaseDetail["sections"][number];

/** A change needs a direction: a quick change or her own words. */
const CHANGE_NEEDS = { what: "wish", say: "בחרי שינוי מהיר, או כתבי מה לשנות." } as const;

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
  focus?: { key: string; at: number; move: boolean; para?: string } | null;
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
    // To one paragraph of it when asked (the first one held back by "approve all", D-161).
    const para = focus.para ? document.getElementById(`para-${focus.para}`) : null;
    if (para) para.scrollIntoView({ block: "center", behavior: "smooth" });
    else el.scrollIntoView({ block: "start", behavior: "smooth" });
    el.classList.add("a4-flash");
    el.querySelector<HTMLElement>(".a4-heading")?.focus({ preventScroll: true });
    const t = setTimeout(() => el.classList.remove("a4-flash"), 1600);
    return () => clearTimeout(t);
  }, [focus]);

  // Arriving at the page, the section in focus comes into view (nothing moves when it already
  // is), with the keyboard on it: with a draft waiting, on its heading, so Enter approves it.
  // Any other section's heading took the keyboard itself when nothing else had it (D-204, the
  // brief folded), and that heading must not be below the window either. It waits for her
  // wishes to be read, so the section is measured with its own row ("✦ לנסח את הסעיף", or the
  // box), not with the short line in its place before them, and comes into view whole.
  const briefs = useQuery({ queryKey: ["draftBriefs"], queryFn: ipc.draftBriefs, staleTime: 60_000 });
  const briefsRead = briefs.data !== undefined || briefs.isError;
  const arrived = useRef(false);
  useEffect(() => {
    if (arrived.current || focus?.move || !current || !briefsRead) return;
    arrived.current = true;
    const el = document.getElementById(`sec-${current}`);
    if (!el) return;
    el.scrollIntoView({ block: "nearest" });
    const s = detail.sections.find((x) => x.key === current);
    if (s?.paragraphs.some((p) => p.status === "proposed")) el.querySelector<HTMLElement>(".a4-heading")?.focus({ preventScroll: true });
  }, [current, detail.sections, focus, briefsRead]);

  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const childLabel = detail.meta.child_gender === "female" ? "שם הילדה" : detail.meta.child_gender === "male" ? "שם הילד" : "שם הילד/ה";
  const parts = Array.from(new Set(detail.sections.map((s) => s.part)));
  const content = detail.sections.filter((s) => s.key !== "signature");
  const signature = detail.sections.find((s) => s.key === "signature");
  const signatureLines = signature?.paragraphs.filter((p) => p.status === "approved").flatMap((p) => p.text.split("\n")) ?? [];
  const supervisor = settings?.supervisor ?? [];
  const closing = closingAnchor(content);
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
                <Fragment key={s.key}>
                  {/* D-163: the closing sections in order, where the closing part begins. */}
                  {s.key === closing && <ClosingFlow api={api} current={current} onPick={onPick} />}
                  <A4Section api={api} section={s} focused={current === s.key}
                    onPick={() => { if (current !== s.key) onPick(s.key); }}
                    onNext={() => { const n = nextWaiting(content, s.key); if (n) onPick(n, true); }} />
                </Fragment>
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

const PARA_CHANGES: readonly (readonly [string, string])[] = [
  ["לקצר", "קצרי את הפסקה הבאה ושמרי על כל הממצאים שבה."],
  ["לנסח אחרת", "נסחי מחדש את הפסקה הבאה, באותו תוכן ובלי להוסיף עובדות."],
  ["להרחיב", "הרחיבי את הפסקה הבאה, רק ממה שיש במקורות."],
  ["לפשט להורים", "נסחי את הפסקה הבאה בשפה פשוטה וברורה להורים, באותו תוכן."],
];

const DERIVED = CLOSING_KEYS;

/** The request for a first draft, with what she wrote in "מה חשוב לך שייכתב כאן?" (round 48).
 *  It goes the way every request of hers goes: prepared, through the gate, then sent. */
export function draftRequest(sectionKey: string, brief: string): string | undefined {
  const wish = brief.trim();
  if (!wish) return undefined;
  const base = DERIVED.includes(sectionKey) ? DRAFT_FROM_APPROVED : DRAFT_FROM_SOURCES;
  return `${base} מה שחשוב לי שייכתב בסעיף: ${wish}`;
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
    ref.current?.querySelector<HTMLElement>("textarea, button")?.focus();
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
  const { go, fail, notify } = useApp();
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
  // "מה חשוב לך שייכתב כאן?" (round 48): kept with the section while the vault is open, in
  // memory only, like the points of her own sections; it leaves only inside the request.
  const [brief, setBrief] = useKept(`case:${caseId}:report:${s.key}:brief`, "");
  // Her recent wishes and the ones kept per section (D-140), from the vault.
  const qc = useQueryClient();
  const briefs = useQuery({ queryKey: ["draftBriefs"], queryFn: ipc.draftBriefs, staleTime: 60_000 });
  const keptBrief = briefs.data?.sections[s.key] ?? "";
  // D-204 §3: the box waits behind one quiet line, "איך לנסח? (לא חובה)", until she asks for it.
  // A wish kept for the section, or one she wrote before, opens it by itself: a wish is never
  // sent unseen, nor dropped without her seeing it. Asked for, it stays open when she leaves
  // the page and comes back (kept like `brief`).
  const [briefAsked, setBriefAsked] = useKept(`case:${caseId}:report:${s.key}:briefAsked`, false);
  /** The box as it last went (folded by "ביטול" or Escape, the section or the report page left,
   *  her own writing opened): her text, the styles pressed and "לזכור" as she left them, so
   *  Enter never keeps what she unticked as the section's wish for the other children. Kept like
   *  `brief`, in memory only and emptied when the vault locks (never stored; it goes out only
   *  when she sends it from the box), and in the box again when it opens, ahead of an older
   *  wish (sent or kept) and also when she emptied it. null: the box has not gone since the
   *  last send, so it opens with the wish as the vault has it. The folded "✦ לנסח את הסעיף"
   *  never sends it (a wish is never sent unseen). It also notes the wish kept for the section
   *  when it went: if another report kept a newer one since, that newer wish opens instead (as
   *  without a draft), so an older draft never overwrites it unseen, and a line under the box
   *  offers her draft back ("להחזיר"), so it is never dropped unseen either; it stays offered
   *  until she changes the box, sends from it, or takes it back. If the kept wish was only
   *  removed since, the draft opens with her text and styles, but "לזכור" unticked: the removal
   *  is newer than her tick, and Enter must not keep that wish again for the other children.
   *  Only a box she changed is a draft: one she only looked at leaves the draft as it was.
   *  `about` is the kept wish her "לזכור" is about: the one the box showed as it opened. Unticked,
   *  Enter forgets the section's wish only while it is still that one, so a newer wish kept in
   *  another report is never forgotten by an untick in a box that never showed it (the core
   *  checks it). */
  const [briefDraft, setBriefDraft] = useKept<(BriefLeft & { kept: string; about: string }) | null>(`case:${caseId}:report:${s.key}:briefDraft`, null);
  const draft = briefDraft === null ? null
    : briefDraft.kept === keptBrief ? briefDraft
    : keptBrief === "" ? { ...briefDraft, keep: false }
    : null;
  /** The kept wish her "לזכור" is about in the box as it opens now: the one it shows (opened
   *  with the kept wish), else the draft's, whatever she does with the checkbox. */
  const boxAbout = draft === null || draft.text.trim() === keptBrief.trim() ? keptBrief : draft.about;
  /** Her draft, behind a newer wish kept for the section in another report since it went:
   *  offered back when it holds words of hers (not the old kept wish, nor the newer one) or
   *  styles. With styles only, "להחזיר" brings them back with the newer wish, ticked. */
  const ownWords = briefDraft !== null && briefDraft.text.trim() !== ""
    && briefDraft.text.trim() !== briefDraft.kept.trim() && briefDraft.text.trim() !== keptBrief.trim();
  const earlier = briefDraft !== null && draft === null && (ownWords || briefDraft.picked.length > 0) ? briefDraft : null;
  // The kept wish removed since her draft went: "לזכור" is unticked in it for good, so the same
  // wish kept again later does not bring her tick back (the removal is newer than the tick).
  useEffect(() => {
    if (briefs.data !== undefined && briefDraft !== null && briefDraft.kept !== "" && keptBrief === "")
      setBriefDraft({ ...briefDraft, keep: false, kept: "" });
  }, [briefs.data, briefDraft, keptBrief, setBriefDraft]);
  /** "להחזיר": the box opens again with her draft (a new box, so the one with the kept wish
   *  going away does not count as what she left), with the keyboard in it. */
  const [briefRound, setBriefRound] = useState(0);
  const [briefFocus, setBriefFocus] = useState(false);
  const restoring = useRef(false);
  /** Sent from the box: the draft goes once the request went. If it did not (she went back
   *  from the review, the gate held it, or it failed), the box opens again as it was when she
   *  sent from it, styles and "לזכור" too, also when she sent a draft unchanged. */
  const briefSent = useRef(false);
  const sentLeft = useRef<{ left: BriefLeft; changed: boolean } | null>(null);
  /** Sent, and not settled yet: the box waits, so it never opens with what it held before. */
  const [briefPending, setBriefPending] = useState(false);
  const briefToggle = useRef<HTMLButtonElement>(null);
  /** Folded back by "ביטול" or Escape: the keyboard goes back to the line that opened it. */
  const backToToggle = useRef(false);
  /** A wish she sees in the box: kept for the section, or her own (sent, or a box she worked in,
   *  which `briefAsked` keeps open). */
  const hasWish = keptBrief !== "" || brief.trim() !== "";
  const briefShown = briefAsked || hasWish;
  /** Her wishes are still being read: whether one is kept is not known yet. */
  const briefWaiting = (briefs.data === undefined && !briefs.isError) || briefPending;
  useEffect(() => {
    if (!briefShown && backToToggle.current) {
      backToToggle.current = false;
      briefToggle.current?.focus();
    }
  }, [briefShown]);
  // Her points for her own section, the same ones the section's page keeps (round 30).
  const [points, setPoints] = useKept(`case:${caseId}:section:${s.key}:points`, "");
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
  /** No material is linked to it (D-210): the AI words it from her points only. */
  const unfed = !mine && s.sortable && s.source_count === 0;
  const [asking, setAsking] = useState(false);
  const busy = preparing || job !== undefined;
  const waiting = pending.length > 0;
  // D-204: with the brief folded, the section in focus may have nothing that takes the keyboard:
  // opened from the cases list, its "✦ לנסח" line pressed (it goes away under the click), or
  // "✦ לנסח את הסעיף" pressed (it goes away while the request is prepared, and the draft comes
  // in its place). Then the keyboard lands on its heading instead of being lost on the page, so
  // Enter goes on from there: on to "✦ לנסח את הסעיף", or, once the draft is in, approving it.
  // Only when it is nowhere: where she is typing or reading keeps it, and the box is not focused
  // from here. Nothing scrolls for it (the page brings a section into view when it arrives).
  useEffect(() => {
    if (!focused) return;
    const at = document.activeElement;
    if (at && at !== document.body && at !== document.documentElement && at.isConnected) return;
    heading.current?.focus({ preventScroll: true });
  }, [focused, busy, waiting]);
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

  /** Resolves true when the request went (false: she went back, the gate held it, or it failed). */
  async function write(instruction?: string, replaces?: string, points = false): Promise<boolean> {
    onPick();
    setOpen(null);
    setChanging(false);
    setParaPop(null);
    setSourcesChanged(false);
    setError(null);
    setPreparing(true);
    try {
      return await api.draft(s.key, instruction, replaces, points);
    } catch (e) {
      setError(fail(e as never));
      return false;
    } finally {
      setPreparing(false);
    }
  }

  /** "✓ לאשר": the whole draft goes into the report, and the next draft waiting comes up. */
  async function approveAll() {
    const ids = pending.map((p) => p.id);
    if (ids.length === 0) return onNext();
    if (await act(() => ipc.approveParagraphs(caseId, ids))) onNext();
  }

  /** "✦ לנסח" from the box: the wish is kept (recent, and for the section when she asked),
   *  then the request is prepared like every request of hers. Keeping it is a convenience:
   *  if it fails, the draft is still written. */
  function sendBrief(wish: string, typed: string, keep: boolean, from: BriefLeft) {
    const seen = boxAbout;
    const fromDraft = draft !== null;
    setBrief(typed);
    briefSent.current = true;
    sentLeft.current = null;
    setBriefPending(true);
    const keeping = ipc.keepDraftBrief(s.key, typed, keep, seen).then((b) => qc.setQueryData(["draftBriefs"], b), () => undefined);
    void Promise.all([write(draftRequest(s.key, wish)), keeping]).then(([sent]) => {
      const gone = sentLeft.current;
      sentLeft.current = null;
      briefSent.current = false;
      setBriefPending(false);
      if (sent) {
        setBriefDraft(null);
        return;
      }
      // Not sent: what she sent from is her draft when she changed it or it was her draft
      // already, stamped with the wish kept now (her send may have kept it), so the box opens
      // again as it was, styles and "לזכור" too. A box opened with the kept wish and sent as
      // it was is no draft of hers, and an earlier one stays offered; a box still open keeps it.
      const kept = qc.getQueryData<{ sections: Record<string, string> }>(["draftBriefs"])?.sections[s.key] ?? "";
      if (fromDraft || gone?.changed) {
        setBriefDraft({ ...(gone?.left ?? from), kept, about: seen });
        setBriefAsked(true);
      }
    });
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
      else if (id) {
        if (await ipc.editParagraph(caseId, id, text, editing.original)) notify(STALE_EDIT);
      } else await ipc.addOwnParagraph(caseId, s.key, text, at ?? "end");
      close();
    });
  }

  // D-168: the section's guide opens here, on the page, for the section in focus (the same
  // open state the section's own page keeps). Her own section's points and examples are
  // still on its page.
  const [guideOpen, setGuideOpen] = useKept(guideOpenKey(caseId, s.key), false);
  const toSectionPage = () => go({ name: "case", id: caseId, view: s.key });
  const openGuide = () => {
    if (mine) toSectionPage();
    else setGuideOpen(!guideOpen);
  };
  // D-168: the DSM level she set, in one line; open until she sets one, because the draft
  // is written with no level until then.
  const severity = detail.meta.dsm_severity;
  const severitySet = severity?.social != null || severity?.restricted != null;
  const [severityOpen, setSeverityOpen] = useState(!severitySet);
  // D-168: her diagnoses and recommendations from her past reports. On the diagnoses section
  // the list leads, open, before any drafting: the psychologist diagnoses, not the AI.
  const hasLibrary = s.key === "diagnoses" || s.key === "recommendations";
  const library = focused && hasLibrary && !job && (
    <SectionLibraryPanel caseId={caseId} sectionKey={s.key} present={s.paragraphs.map((p) => p.text)}
      lead={s.key === "diagnoses" && s.paragraphs.length === 0} onAdded={reload} />
  );

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
      // The box when it is open, else "✦ לנסח את הסעיף": a second Enter drafts.
      else if (s.paragraphs.length === 0 && canWrite) (document.getElementById(`brief-${s.key}`) ?? document.getElementById(`draft-${s.key}`))?.focus();
      else onNext();
    }
  }

  /** One paragraph on the page: click to edit, "✦" to change it with Claude. */
  const para = (p: Section["paragraphs"][number], note?: string) =>
    editing?.id === p.id ? (
      <div key={p.id}>{editor}</div>
    ) : (
      <div key={p.id} id={`para-${p.id}`} className={`a4-para${p.status === "proposed" ? " is-proposed" : ""}${paraPop === p.id || deleting === p.id || history === p.id ? " is-active" : ""}`}>
        <p className={p.by_ai ? "a4-p a4-editable" : "a4-p a4-editable a4-own"} tabIndex={0} title={focused ? note ?? "לחיצה לעריכה" : "לחיצה לבחירת הסעיף"}
          aria-label={`${p.status === "proposed" ? "טיוטה שממתינה לאישור" : "פסקה"}${p.by_ai ? " (AI)" : ""}: ${p.text}`}
          aria-describedby={`keys-${s.key}`}
          onClick={() => {
            // A draft out of focus is chosen by the first click (the section takes it); the next one edits it.
            if (!focused && p.status === "proposed") return;
            edit(p.id, p.text);
          }}
          onKeyDown={(e) => paraKey(e, p)}>
          {(() => {
            // D-214: a first line that is one of the section's sub-headings is shown as a heading.
            const marks = p.status === "proposed" && focused ? p.style?.marks ?? [] : [];
            const [first = "", ...rest] = p.text.split("\n");
            if (rest.length === 0 || !(s.subheadings ?? []).includes(first.trim())) return <MarkedText text={p.text} marks={marks} />;
            return <><strong className="a4-subhead">{first.trim()}</strong>{"\n"}<MarkedText text={rest.join("\n")} marks={marks} /></>;
          })()}
        </p>
        {/* D-161: everything there is to look at, in one folded line (a number unlike the
            score table, a name in the answer or a contradiction keeps a small mark on it). */}
        <ParagraphNotes paragraph={p} />
        {p.polish.length > 0 && (
          // D-167: proofreading, folded into one quiet line that opens on a click; advice for
          // the finish, never a warning and never a block. Nothing at all when there is none.
          <details className="note-polish small" aria-label="הגהה">
            <summary>הגהה · {p.polish.length}</summary>
            <ul>{p.polish.map((n, i) => <li key={i}>{n}</li>)}</ul>
          </details>
        )}
        {focused && p.status === "proposed" && p.style && !mine && (
          <StyleHintView hint={p.style} busy={busy}
            onCloser={(how) => void write(`נסחי מחדש את הפסקה הבאה כך שתישמע כמו הכתיבה שלי: ${how}. באותו תוכן ובלי להוסיף עובדות: "${p.text}"`, p.id)}
            onApply={(m) => void act(async () => { await ipc.editParagraph(caseId, p.id, applyMark(p.text, m)); })} />
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
                <BriefBox id={`para-${p.id}`} title="מה לשנות בפסקה הזאת?" chips={PARA_CHANGES} chipsTitle="שינויים מהירים"
                  hint="בחרי כיוון, או כתבי במילים שלך. רק הפסקה הזאת תנוסח מחדש."
                  placeholder={PARAGRAPH_HINTS[0] ?? ""} recent={briefs.data?.recent} sendLabel="✦ לנסח מחדש" busy={busy} needs={CHANGE_NEEDS}
                  foot={p.status === "approved" ? "יופיע ליד הקיים, ויחליף אותו רק באישורך." : "יחליף את הפסקה בטיוטה."}
                  onSend={(w) => { if (w.trim()) void write(`${w} הפסקה: "${p.text}"`, p.id); }}
                  onCancel={() => setParaPop(null)} />
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
    mine ? { label: "הנקודות שלך לסעיף", run: openGuide } : { label: guideOpen ? "לסגור את ההנחיה לסעיף" : "איך לכתוב את הסעיף", run: openGuide },
    ...(s.sortable && !mine ? [{ label: "מאיזה חומרים לכתוב", run: () => setOpen("sources") }] : []),
    { label: "+ פסקה משלי בסוף הסעיף", run: () => edit(null, "", "end"), disabled: editing !== null },
    ...(s.paragraphs.length > 0 && pending.length === 0 ? [{ label: `✦ לבקש מ-${ai} להוסיף לסעיף`, run: () => { onPick(); setAsking(true); }, disabled: busy }] : []),
    { label: "מה יצא מהמחשב בשביל הסעיף", run: () => setSentOpen(true) },
    // The section's own page (its conversation, the check against the sources) stays one click away.
    ...(mine ? [] : [{ label: "עמוד הסעיף", run: toSectionPage }]),
    ...(pending.length > 0 ? [{ label: "הסרת הטיוטה", danger: true, run: () => void act(async () => { for (const p of pending) await ipc.rejectParagraph(caseId, p.id); }) }] : []),
  ];
  const empty = s.paragraphs.length === 0;
  /** Her points, worded by the AI: in a section of her own, and in one with no material (D-210). */
  const pointsBox = (title: string, hint: string) => (
    <BriefBox id={`brief-${s.key}`} title={title} chips={STYLE_CHIPS} chipsTitle="סגנון"
      hint={hint}
      placeholder="למשל: הקשר עם הגננת טוב · קשה במעברים · התקדמות מאז הסתיו"
      initial={points} sendLabel="✦ לנסח מהנקודות" busy={busy} autoFocus
      needs={{ what: "typed", say: "כתבי קודם את הנקודות שלך. רק הן יוצאות." }}
      onSend={(wish, typed) => {
        if (!typed) return;
        setPoints(typed);
        setAsking(false);
        // The points first, as she wrote them; a chosen style after them.
        const style = wish.slice(0, Math.max(0, wish.length - typed.length)).trim();
        void write(style ? `${typed}\n\n${style}` : typed, undefined, true);
      }}
      onCancel={() => { setAsking(false); heading.current?.focus(); }} />
  );
  const state = mine ? "mine" : pending.length > 0 ? "draft" : empty ? "empty" : "approved";

  return (
    <div id={`sec-${s.key}`} className={`a4-section a4-st-${state}${focused ? " is-focus" : ""}${pending.length ? " has-pending" : ""}`}
      // A click anywhere on a section out of focus brings it into focus, after the button it hit
      // (if any) did its work. The keyboard arriving on its heading or a paragraph does the same;
      // on a button it waits for the click, so the button is still there to take it.
      onClick={(e) => {
        if (focused) return;
        // What she clicked stays under the pointer while the section opens around it.
        const anchor = e.target instanceof HTMLElement ? e.target : e.currentTarget;
        const before = anchor.getBoundingClientRect().top;
        onPick();
        keepInPlace(anchor, e.currentTarget, before);
      }}
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
          {/* D-212: a material's name opens it to read, beside the report. */}
          {sources.slice(0, 6).map((i) => api.readDoc
            ? <button key={i.id} type="button" className="a4-source a4-source-open" title="לפתוח את החומר לקריאה" onClick={() => api.readDoc?.(i.id)}>{i.title || kindLabel[i.kind]}</button>
            : <span key={i.id} className="a4-source">{i.title || kindLabel[i.kind]}</span>)}
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

      {/* D-168: for the section in focus only, each folded into one quiet line. */}
      {focused && !mine && guideOpen && (
        <div className="a4-panel">
          <SectionGuide caseId={caseId} sectionKey={s.key} guide={s.guide} isDefault={s.guide_is_default}
            learnedFrom={s.guide_learned_from} caseGuide={s.case_guide} onSaved={reload} />
        </div>
      )}
      {focused && s.key === "dsm" && (
        <details className="a4-fold" open={severityOpen} onToggle={(e) => setSeverityOpen(e.currentTarget.open)}>
          <summary>רמת החומרה: {severityWords(severity)}</summary>
          {!severitySet && <span className="small muted">עד שתקבעי דרגה, הטיוטה של הסעיף נכתבת בלי דרגה.</span>}
          <DsmSeverityPanel caseId={caseId} severity={severity} onSaved={reload} />
        </details>
      )}
      {s.key === "diagnoses" && library && <div className="a4-panel">{library}</div>}

      {ok.length > 0 && slot("first")}
      {ok.map((p, i) => [para(p), slot(i === ok.length - 1 ? "end" : p.id)])}

      {job && (
        <div className="a4-writing">
          <ProgressLine started={job.started} estimate={job.estimate} label={job.label} approval={job.approval} />
        </div>
      )}
      {preparing && !job && <WaitLine task="prepare" speed="section" label="מכינה את הבקשה: מסתירה שמות ובודקת מה יוצא" />}

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
              <BriefBox id={`change-${s.key}`} title="מה לשנות בטיוטה?" chips={CHANGE_CHIPS} chipsTitle="שינויים מהירים"
                hint="בחרי כיוון, או כתבי במילים שלך. רק הסעיף הזה ינוסח מחדש."
                placeholder="למשל: להוריד את המשפט על המעברים, ולהוסיף את הקשר עם הגננת"
                recent={briefs.data?.recent} sendLabel="✦ לנסח מחדש" busy={busy} needs={CHANGE_NEEDS}
                foot="הטיוטה החדשה תחליף את זו. מה שאישרת נשאר."
                onSend={(w) => { if (w.trim()) void write(w); }}
                onCancel={() => { setChanging(false); heading.current?.focus(); }} />
              {brief.trim() && <span className="small muted">ביקשת לפני הניסוח: {brief.trim()}</span>}
            </ChangePanel>
          )}
        </div>
      )}

      {ok.length === 0 && editing && editing.id === null && editor}

      {s.key !== "diagnoses" && library && <div className="a4-panel">{library}</div>}
      {/* D-161: what the last answer said about the section, for the section in focus only. */}
      {focused && !job && <SectionNotesFold notes={s.notes} ai={ai} />}

      {empty && !job && !editing && !preparing && (
        mine && focused ? (
          <div className="a4-brief">
            {pointsBox("הנקודות שלך לסעיף", `כתבי בנקודות מה חשוב. רק הנקודות יוצאות, ו-${ai} מנסח מהן בסגנון שלך, בלי להוסיף עובדות.`)}
            <div className="row a4-brief-row">
              <button type="button" className="link-btn" onClick={() => edit(null, "", "end")}>לכתוב בעצמי</button>
              <span className="grow" />
              <button type="button" className="link-small" onClick={openGuide}>דוגמאות מהדוחות שלך ←</button>
            </div>
          </div>
        ) : mine ? (
          <div className="a4-gap-line">
            <span className="grow">הסעיף שלך: כותבים נקודות, ו-{ai} מנסח מהן.</span>
            <button type="button" className="link-btn" disabled={busy} onClick={onPick}>✦ לנסח מהנקודות</button>
          </div>
        ) : focused && canWrite ? (
          <div className="a4-brief">
            {briefWaiting ? (
              <WaitLine task="prepare" speed="section" label="פותחת את תיבת ההנחיה" />
            ) : briefShown ? (
              <>
              <BriefBox id={`brief-${s.key}`} title="איך לנסח את הסעיף? (לא חובה)" chips={STYLE_CHIPS} chipsTitle="סגנון"
                hint="כתבי מה חשוב לך, או בחרי סגנון. בלי כלום הסעיף ינוסח כמו תמיד, מתוך החומרים."
                placeholder="למשל: להתחיל מהקשר עם הצוות בגן, ובלי מונחים מקצועיים"
                key={briefRound}
                recent={briefs.data?.recent} initial={draft ? draft.text : keptBrief || brief}
                picked={draft?.picked} keep={draft ? draft.keep : keptBrief !== ""}
                describedBy={earlier ? `brief-${s.key}-earlier` : undefined}
                sendLabel="✦ לנסח" busy={busy} autoFocus={briefAsked || briefFocus || s.key !== "diagnoses"}
                onSend={sendBrief}
                // Whenever the box goes, what is in it then is what it opens with next time: her
                // newest text (also emptied), styles and "לזכור", and nothing once it was sent.
                // Taken as it goes, not on every key, so typing never re-renders the section (F1a).
                onGone={(left, changed) => {
                  if (restoring.current) {
                    restoring.current = false;
                    return;
                  }
                  setBriefFocus(false);
                  if (briefSent.current) {
                    sentLeft.current = { left, changed };
                    return;
                  }
                  // Only what she changed is a draft: a box she only looked at leaves the draft
                  // as it was (an earlier one of hers stays offered under the box).
                  if (!changed) return;
                  setBriefDraft({ ...left, kept: keptBrief, about: boxAbout });
                  // A box she worked in stays open, like one she asked for, also if the wish that
                  // opened it is removed meanwhile; folded ("ביטול", Escape), it stays folded.
                  if (!backToToggle.current) setBriefAsked(true);
                }}
                onCancel={() => {
                  // With a wish the box stays open (its words are hers to see); else it folds
                  // back, and the keyboard returns to the line that opened it. "ביטול" folds,
                  // it does not erase: what she typed is there when she opens it again.
                  if (hasWish) heading.current?.focus();
                  else {
                    backToToggle.current = true;
                    setBriefAsked(false);
                  }
                }} />
              {earlier && (
                // Her draft, behind a newer wish kept for the section in another report: never
                // dropped unseen. "להחזיר" puts it back in the box as she left it; when her
                // words there were only the old kept wish, the styles come back with the newer one.
                <p className="small muted a4-brief-earlier">
                  <span id={`brief-${s.key}-earlier`} className="a4-brief-earlier-text">
                    {ownWords
                      ? `מה שכתבת כאן ולא נשלח: ״${earlier.text.trim()}״${earlier.picked.length > 0 ? `, בסגנון: ${earlier.picked.join(", ")}` : ""}`
                      : `הסגנון שבחרת כאן ולא נשלח: ${earlier.picked.join(", ")}`}
                  </span>
                  <button type="button" className="link-btn" disabled={busy} onClick={() => {
                    restoring.current = true;
                    setBriefDraft(ownWords ? { ...earlier, kept: keptBrief } : { ...earlier, text: keptBrief, keep: true, kept: keptBrief, about: keptBrief });
                    setBriefFocus(true);
                    setBriefRound((n) => n + 1);
                  }}>להחזיר</button>
                </p>
              )}
              </>
            ) : (
              // D-204 §3: folded, one row. "✦ לנסח את הסעיף" sends what an empty box sends (no
              // wish, the usual request), through the same review; nothing is kept or deleted.
              <div className="row wrap-row a4-brief-row a4-brief-closed">
                <button id={`draft-${s.key}`} type="button" className="btn btn-primary" disabled={busy} onClick={() => void write()}>✦ לנסח את הסעיף</button>
                <button type="button" className="btn" onClick={() => edit(null, "", "end")}>לכתוב בעצמי</button>
                <button ref={briefToggle} type="button" className="link-btn a4-brief-toggle" aria-expanded={false} onClick={() => setBriefAsked(true)}>איך לנסח? (לא חובה)</button>
              </div>
            )}
            {/* D-163: a closing section drafted now cannot reflect her own sections still empty. */}
            {DERIVED.includes(s.key) && ownEmptyLine(detail.sections) && <span className="small muted a4-own-empty">{ownEmptyLine(detail.sections)}</span>}
            {(briefShown || briefWaiting) && (
              <div className="row a4-brief-row">
                <button type="button" className="link-btn" onClick={() => edit(null, "", "end")}>לכתוב בעצמי</button>
                <span className="grow" />
                <button type="button" className="link-small" onClick={openGuide}>איך לכתוב את הסעיף ←</button>
              </div>
            )}
          </div>
        ) : canWrite ? (
          <div className="a4-gap-line">
            <span className="grow">עוד לא נכתב.{sourceNames.length ? ` יש חומר: ${sourceNames.slice(0, 2).join(", ")}${sourceNames.length > 2 ? ` ועוד ${sourceNames.length - 2}` : ""}` : ""}</span>
            <button type="button" className="link-btn" disabled={busy} onClick={onPick}>✦ לנסח</button>
          </div>
        ) : focused && asking ? (
          // D-210: a section with no material linked to it is still written with the AI, from
          // what she types here. Only her points go, as in a section of her own.
          <div className="a4-brief">
            {pointsBox("מה לכתוב בסעיף?", `אין חומרים מקושרים לסעיף. כתבי בנקודות מה לכתוב: רק הנקודות יוצאות, ו-${ai} מנסח מהן בסגנון שלך, בלי להוסיף עובדות.`)}
          </div>
        ) : (
          <div className="a4-gap-line">
            <span className="grow">אין חומר לסעיף.</span>
            <button type="button" className="link-btn" disabled={busy} onClick={() => { onPick(); setAsking(true); }}>✦ לנסח מנקודות</button>
            <button type="button" className="link-btn" onClick={() => setOpen("sources")}>לקשר חומרים</button>
            {focused && <button type="button" className="link-btn" onClick={() => edit(null, "", "end")}>לכתוב בעצמי</button>}
          </div>
        )
      )}
      {/* D-210: more from the AI in a section that already has text. What is there stays: the
          answer comes as a draft beside it. */}
      {!empty && focused && asking && !job && !preparing && pending.length === 0 && (
        <div className="a4-brief">
          {unfed || mine ? (
            pointsBox("מה להוסיף לסעיף?", `כתבי בנקודות מה להוסיף: רק הנקודות יוצאות, ו-${ai} מנסח מהן. מה שכבר כתוב בסעיף נשאר.`)
          ) : (
            <BriefBox id={`more-${s.key}`} title="מה לבקש לסעיף?" chips={CHANGE_CHIPS} chipsTitle="בקשות מהירות"
              hint="התשובה תופיע כטיוטה ליד מה שכבר כתוב. מה שאישרת נשאר."
              placeholder="למשל: להוסיף פסקה על הקשר עם הצוות בגן"
              recent={briefs.data?.recent} sendLabel="✦ לנסח" busy={busy} needs={CHANGE_NEEDS} autoFocus
              onSend={(w) => { if (w.trim()) { setAsking(false); void write(w); } }}
              onCancel={() => { setAsking(false); heading.current?.focus(); }} />
          )}
        </div>
      )}
      <ErrorLine error={error} />
      {sentOpen && <SentHistoryDialog caseId={caseId} place={s.key} onClose={() => setSentOpen(false)} />}
    </div>
  );
}

/** The DSM level in a few words: each domain's, or that none was set. */
export function severityWords(severity: { social: number | null; restricted: number | null } | null | undefined): string {
  const social = severity?.social ?? null;
  const restricted = severity?.restricted ?? null;
  if (social === null && restricted === null) return "לא נקבעה";
  const level = (n: number | null) => (n === null ? "לא נקבעה" : `דרגה ${n}`);
  return `תקשורת חברתית: ${level(social)} · התנהגות חוזרת: ${level(restricted)}`;
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

/** How far to scroll so a point that was at `before` (px from the top) and is now at `after`
 *  is back where it was; nothing for a shift under a pixel. */
export function scrollShift(before: number, after: number): number {
  const d = after - before;
  return Math.abs(d) < 1 ? 0 : d;
}

/** After the section clicked into focus has drawn its tools, scroll its page so what she
 *  clicked is where it was (it moved by the height of what opened above it). Measured on the
 *  clicked element, or on the section when that element was drawn anew. */
function keepInPlace(anchor: HTMLElement, section: HTMLElement, before: number) {
  const sectionBefore = section.getBoundingClientRect().top;
  requestAnimationFrame(() => requestAnimationFrame(() => {
    const shift = anchor.isConnected
      ? scrollShift(before, anchor.getBoundingClientRect().top)
      : scrollShift(sectionBefore, section.getBoundingClientRect().top);
    if (!shift) return;
    let box: HTMLElement | null = section.parentElement;
    while (box && !(box.scrollHeight > box.clientHeight && /auto|scroll/.test(getComputedStyle(box).overflowY))) box = box.parentElement;
    box?.scrollBy({ top: shift });
  }));
}
