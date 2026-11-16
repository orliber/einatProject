import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import { useApp } from "../../App";
import { STALE_EDIT_KEPT } from "../../i18n/he";
import { ipc, type CaseDetail, type ExportCheck, type ReportSettings } from "../../ipc/client";
import { useKept } from "../../kept";
import { heldNotes } from "./notes";
import { emptyClosing } from "./closing";
import { noteTitle, noteWhy, needlesOf, Quote } from "./finishNotes";
import { ageWords } from "../../components/AgeField";
import { ErrorLine } from "../../components/ui";
import { timed, WaitLine } from "../../components/Progress";
import { useHoldUnsaved } from "../../components/Unsaved";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { REPORT_NOTE_TITLE, ReportCheckItem, useReportCheckResult } from "../../components/ReportCheck";
import type { CaseApi } from "../CaseScreen";
import { LetterCard } from "./LetterCard";
import "./FinishView.css";

type Section = CaseDetail["sections"][number];
type Paragraph = Section["paragraphs"][number];

const MARK = /\[חסר[^\]]*\]/;
const MARKS = /\[חסר[^\]]*\]/g;

/** A paragraph with its "[חסר: …]" markers highlighted. */
function Marked({ text }: { text: string }) {
  const parts = text.split(/(\[חסר[^\]]*\])/g);
  return <>{parts.map((p, i) => (p.startsWith("[חסר") ? <mark key={i} className="a4-mark">{p}</mark> : <Fragment key={i}>{p}</Fragment>))}</>;
}

type GroupId = "must" | "check" | "polish";

/** One thing said about a paragraph. */
interface CardNote {
  /** What a "leave it" is remembered by: the paragraph, its words as they are now, and the
   *  note's words. A paragraph that was edited is a new paragraph for this. */
  key: string;
  title: string;
  why: string;
  /** What to look for in the paragraph, to show the sentence. */
  needles: string[];
}

/** A paragraph with what there is to say about it. */
interface ParagraphCard {
  type: "paragraph";
  section: Section;
  p: Paragraph;
  notes: CardNote[];
}

/** Something about a section or the whole report. */
interface LineCard {
  type: "line";
  key: string;
  title: string;
  where: string;
  text: string;
  /** The section "to the section" opens, and the one shown in the preview. */
  open: string | null;
  /** Advice she may leave as it is. A state of the report (an empty section) is not. */
  leave: boolean;
}

type Card = ParagraphCard | LineCard;

/** A note she chose to leave: out of the list, and easy to bring back. */
interface Left {
  key: string;
  label: string;
}

interface Group {
  id: GroupId;
  title: string;
  cards: Card[];
  left: Left[];
  count: number;
}

const GROUP_TITLE: Record<GroupId, string> = { must: "חייבים לתקן", check: "כדאי לבדוק", polish: "ליטוש ניסוח" };

const names = (sections: Section[]) => sections.map((s) => s.title).join(", ");

/** A proofreading note says what it is before a colon ("מילה כפולה: …"): that is its title. */
function polishWords(text: string): { title: string; why: string } {
  const at = text.indexOf(": ");
  const rest = at > 0 ? noteWhy(text.slice(at + 2)) : "";
  return at > 0 && at <= 30 && rest ? { title: text.slice(0, at), why: rest } : { title: noteTitle("polish"), why: noteWhy(text) };
}

/** Takes what she left out of the cards; what is left is counted. */
function group(id: GroupId, all: Card[], leftKeys: readonly string[]): Group {
  const left: Left[] = [];
  const cards: Card[] = [];
  for (const c of all) {
    if (c.type === "line") {
      if (c.leave && leftKeys.includes(c.key)) left.push({ key: c.key, label: c.where ? `${c.title} · ${c.where}` : c.title });
      else cards.push(c);
      continue;
    }
    const notes = c.notes.filter((n) => !leftKeys.includes(n.key));
    for (const n of c.notes) if (leftKeys.includes(n.key)) left.push({ key: n.key, label: `${n.title} · ${c.section.title}` });
    if (notes.length > 0) cards.push({ ...c, notes });
  }
  const count = cards.reduce((n, c) => n + (c.type === "line" ? 1 : c.notes.length), 0);
  return { id, title: GROUP_TITLE[id], cards, left, count };
}

/** The state of the report and the one next step, in plain words. */
function statusLine(s: { loaded: boolean; nothing: boolean; must: number; check: number; complete: boolean }): string {
  if (!s.loaded) return "הוצאת הדוח";
  if (s.nothing) return "עוד אין מה להוציא.";
  if (s.must > 0) return s.must === 1 ? "נשאר דבר אחד שחייבים לתקן לפני ההוצאה." : `נשארו ${s.must} דברים שחייבים לתקן לפני ההוצאה.`;
  if (s.check > 0) return s.check === 1 ? "אפשר להוציא. דבר אחד שכדאי לבדוק קודם." : `אפשר להוציא. ${s.check} דברים שכדאי לבדוק קודם.`;
  return s.complete ? "הדוח מוכן להוצאה." : "אפשר להוציא.";
}

/**
 * The last stage: the report as the file will be, always in view, beside one line that says
 * where things stand and three folded lists: what stops the file, what is worth a look, and
 * the proofreading. A note is a card: what it is, the sentence it is about, why, and what
 * she can do (fix it right here, go to the section, or leave it). Nothing here but what
 * stops the file ever holds the file back.
 */
export function FinishView({ api, onExport, onOpen }: { api: CaseApi; onExport: () => void; onOpen: (key: string) => void }) {
  const { fail, notify } = useApp();
  const { detail, caseId, reload } = api;
  const [check, setCheck] = useState<ExportCheck | null>(null);
  const [settings, setSettings] = useState<ReportSettings | null>(null);
  /** A paragraph being fixed here: the text is in `editBox`; `text` is its debounced copy, for the lock. */
  // Kept while the vault is open: leaving and coming back finds the editor with her text.
  const [editing, setEditing] = useKept<{ id: string; text: string; original?: string; at?: GroupId } | null>(`case:${caseId}:finish:editing`, null);
  /** The notes she left as they are. Only this list reads it: the check before the file, the
   *  export and "approve all" never do. Kept while the vault is open, never written anywhere. */
  const [leftKeys, setLeftKeys] = useKept<string[]>(`case:${caseId}:finish:left`, []);
  /** A group she opened or folded herself; otherwise the first one with something in it is open. */
  const [folds, setFolds] = useState<Partial<Record<GroupId, boolean>>>({});
  const [showLeft, setShowLeft] = useState<Partial<Record<GroupId, boolean>>>({});
  /** The paragraph or section a card pointed at, lit for a moment in the preview. */
  const [shown, setShown] = useState<string | null>(null);
  const places = useRef(new Map<string, HTMLElement>());
  const fade = useRef<ReturnType<typeof setTimeout> | null>(null);
  const editBox = useRef<LocalTextHandle>(null);
  const [error, setError] = useState<string | null>(null);
  const holdLive = useHoldUnsaved(caseId, "פסקה בשלב הסיום", editing?.text ?? null);
  const reportCheck = useReportCheckResult(caseId);

  useEffect(() => {
    let alive = true;
    timed("check", ipc.checkExport(caseId)).then((c) => alive && setCheck(c)).catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, detail, fail]);
  useEffect(() => {
    ipc.reportSettings().then(setSettings).catch(() => setSettings(null));
  }, []);
  useEffect(() => () => {
    if (fade.current) clearTimeout(fade.current);
  }, []);

  const content = detail.sections.filter((s) => s.key !== "signature");
  const parts = Array.from(new Set(content.map((s) => s.part)));
  const approved = content.flatMap((s) => s.paragraphs.filter((p) => p.status === "approved").map((p) => ({ section: s, p })));
  const waiting = content.filter((s) => s.paragraphs.some((p) => p.status === "proposed" && !p.replaces));
  const toComplete = approved.filter(({ p }) => MARK.test(p.text));
  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const blocked = (check?.blocking.length ?? 0) > 0;
  const nothing = check !== null && check.included_sections === 0;
  // Round 26: what is left to complete is listed, and never stops the file. The core also marks
  // a tag it could not put words back into; those sections are listed by name.
  const ready = check !== null && !blocked && !nothing;
  // Sections that have materials and nothing approved yet: the file can go out without them,
  // but the report is not ready, and the export dialog says so in orange too.
  const emptyWithMaterial = check ? content.filter((s) => s.source_count > 0 && check.empty_sections.includes(s.title)) : [];
  // D-163: the summary, the diagnoses and the recommendations with nothing approved: the file
  // can go out without them, but the report is not ready, and it is said plainly.
  const closingEmpty = emptyClosing(content).filter((s) => !emptyWithMaterial.includes(s));
  const complete = ready && emptyWithMaterial.length === 0 && closingEmpty.length === 0;
  const markedElsewhere = (check?.to_complete ?? []).filter((t) => !toComplete.some(({ section }) => t.startsWith(`${section.title}:`)));

  const noteKey = (p: Paragraph, kind: string, text: string) => JSON.stringify([p.id, p.text, kind, text]);

  // What really stops the file, and nothing else.
  const mustCards: Card[] = (check?.blocking ?? []).map((b): LineCard => {
    const section = content.find((s) => b.startsWith(`${s.title}:`));
    return {
      type: "line", key: `must:${b}`, title: "עוצר את הקובץ", where: section?.title ?? "",
      text: section ? b.slice(section.title.length + 1).trim() : b, open: section?.key ?? "", leave: false,
    };
  });

  // Worth a look (D-161): what the core keeps about an approved paragraph (a number unlike the
  // score table, a name left in the text, a sentence from an old report, what the check
  // against the materials said) and a place marked to complete. None of these stops the file.
  const checkCards: Card[] = [];
  for (const { section, p } of approved) {
    const notes: CardNote[] = heldNotes(p).map((n) => ({
      key: noteKey(p, n.kind ?? "", n.text), title: noteTitle(n.kind), why: noteWhy(n.text), needles: needlesOf(n.text),
    }));
    for (const m of new Set(p.text.match(MARKS) ?? [])) {
      notes.push({ key: noteKey(p, "missing", m), title: "מקום שסומן להשלמה", why: "הקובץ יוצא גם כך: הסימון נשאר בו, ואפשר להשלים אותו בוורד.", needles: [m] });
    }
    if (notes.length > 0) checkCards.push({ type: "paragraph", section, p, notes });
  }
  if (markedElsewhere.length > 0) {
    checkCards.push({ type: "line", key: "marked", title: "יסומנו בקובץ להשלמה", where: "", text: markedElsewhere.join(" · "), open: null, leave: false });
  }
  if (emptyWithMaterial.length > 0) {
    const first = emptyWithMaterial.slice(0, 3);
    const more = emptyWithMaterial.length - first.length;
    checkCards.push({
      type: "line", key: "empty", title: "סעיפים עם חומרים שעוד ריקים", where: "",
      text: emptyWithMaterial.length === 1
        ? `לסעיף "${first[0]?.title}" יש חומרים, והוא עוד לא נכתב.`
        : `ל-${emptyWithMaterial.length} סעיפים יש חומרים, והם עוד לא נכתבו: ${names(first)}${more > 0 ? ` ועוד ${more}` : ""}.`,
      open: first[0]?.key ?? "", leave: false,
    });
  }
  if (closingEmpty.length > 0 && (check?.included_sections ?? 0) > 0) {
    checkCards.push({
      type: "line", key: "closing", title: closingEmpty.length === 1 ? "סעיף מסכם שעוד ריק" : "סעיפים מסכמים שעוד ריקים", where: "",
      text: `${closingEmpty.length === 1 ? "עוד לא נכתב" : "עוד לא נכתבו"}: ${names(closingEmpty)}. הקובץ יכול לצאת גם בלעדיהם.`,
      open: closingEmpty[0]?.key ?? "", leave: false,
    });
  }
  if (waiting.length > 0) {
    checkCards.push({
      type: "line", key: "waiting", title: "טיוטות שעוד לא אושרו", where: "",
      text: `לא ייכנסו לקובץ: ${names(waiting)}.`, open: waiting[0]?.key ?? "", leave: false,
    });
  }
  // The check of the whole report (D-097): its notes only point at sections. The same words can
  // be said about other sections (the local check says every repeated pair the same way), so the
  // sections are part of the key; the very same note twice is one card.
  const reportKeys = new Set<string>();
  for (const n of reportCheck?.notes ?? []) {
    const key = `report:${JSON.stringify([n.kind, [...n.sections].sort(), n.note])}`;
    if (reportKeys.has(key)) continue;
    reportKeys.add(key);
    const where = n.sections.map((k) => content.find((s) => s.key === k)?.title ?? k).join(" · ");
    checkCards.push({ type: "line", key, title: REPORT_NOTE_TITLE[n.kind], where, text: n.note, open: n.sections[0] ?? null, leave: true });
  }

  // D-167: proofreading notes in approved paragraphs. Advice; never stops the file.
  const polishCards: Card[] = approved.flatMap(({ section, p }): Card[] => {
    const texts = Array.from(new Set([...(p.notes ?? []).filter((n) => n.kind === "polish").map((n) => n.text), ...(p.polish ?? [])]));
    if (texts.length === 0) return [];
    return [{
      type: "paragraph", section, p,
      notes: texts.map((t) => ({ key: noteKey(p, "polish", t), ...polishWords(t), needles: needlesOf(t) })),
    }];
  });

  const groups = [group("must", mustCards, leftKeys), group("check", checkCards, leftKeys), group("polish", polishCards, leftKeys)];
  const [must, toCheck] = groups;
  const firstOpen = groups.find((g) => g.count > 0)?.id ?? null;
  const isOpen = (g: Group) => (editing?.at === g.id && g.cards.some((c) => c.type === "paragraph" && c.p.id === editing.id)) || (folds[g.id] ?? g.id === firstOpen);
  /** An editor is open on a card: no other card opens one over it and loses her text. */
  const editorOpen = editing !== null && groups.some((g) => (editing.at ?? "check") === g.id && g.cards.some((c) => c.type === "paragraph" && c.p.id === editing.id));
  const anyCard = groups.some((g) => g.cards.length > 0);

  /** Leave a note as it is. Its list stays open, also when this was the last one in it. */
  const leave = (g: GroupId, key: string) => {
    setFolds((f) => ({ ...f, [g]: true }));
    setLeftKeys((keys) => (keys.includes(key) ? keys : [...keys, key]));
  };
  const bringBack = (key: string) => setLeftKeys((keys) => keys.filter((k) => k !== key));

  /** Bring the paragraph (or the section) into the middle of the preview and light it for a
   *  moment. Only the preview scrolls; the list stays where it is. */
  function show(place: string | null) {
    const el = place ? places.current.get(place) : undefined;
    if (!place || !el) return;
    const calm = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (typeof el.scrollIntoView === "function") el.scrollIntoView({ block: "center", behavior: calm ? "auto" : "smooth" });
    setShown(place);
    if (fade.current) clearTimeout(fade.current);
    fade.current = setTimeout(() => setShown(null), 2600);
  }
  const place = (key: string) => (el: HTMLElement | null) => {
    if (el) places.current.set(key, el);
    else places.current.delete(key);
  };

  async function save() {
    if (!editing) return;
    const text = editBox.current?.value() ?? editing.text;
    setError(null);
    try {
      if (await ipc.editParagraph(caseId, editing.id, text, editing.original ?? null)) notify(STALE_EDIT_KEPT);
      setEditing(null);
      await reload();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  function paragraphCard(g: Group, c: ParagraphCard): ReactNode {
    const { section, p } = c;
    const here = editing?.id === p.id && (editing.at ?? "check") === g.id;
    // The same paragraph on another card: the editor moves here with what she typed. Another
    // paragraph: only once her editor is closed.
    const fixHere = () => {
      show(`p:${p.id}`);
      if (editing?.id === p.id) setEditing({ ...editing, text: editBox.current?.value() ?? editing.text, at: g.id });
      else setEditing({ id: p.id, text: p.text, original: p.text, at: g.id });
    };
    const canFix = !editorOpen || editing?.id === p.id;
    const one = c.notes.length === 1;
    return (
      <li key={p.id} className={shown === `p:${p.id}` ? "fin-card is-shown" : "fin-card"}>
        <span className="fin-where">{section.title}</span>
        {here && (
          <div className="stack" style={{ gap: 6 }}>
            <LocalText key={p.id} ref={editBox} className="textarea serif" rows={4} autoFocus initial={editing.text} onLive={holdLive} aria-label="תיקון הפסקה"
              onText={(text) => setEditing((e) => (e?.id === p.id ? { ...e, text } : e))} />
            <div className="row">
              <button type="button" className="btn btn-primary btn-small" onClick={() => void save()}>שמירה</button>
              <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
            </div>
          </div>
        )}
        <ul className="fin-notes">
          {c.notes.map((n) => (
            <li key={n.key} className="fin-note">
              <div className="fin-title-row">
                <b className="fin-title grow">{n.title}</b>
                {!here && !one && <button type="button" className="fin-link" onClick={() => leave(g.id, n.key)}>בסדר, להשאיר</button>}
              </div>
              {!here && (
                <button type="button" className="fin-quote serif" title="להראות בדוח" onClick={() => show(`p:${p.id}`)}>
                  <Quote text={p.text} needles={n.needles} />
                </button>
              )}
              <span className="fin-why">{n.why}</span>
            </li>
          ))}
        </ul>
        {!here && (
          <div className="fin-actions">
            {canFix && <button type="button" className="btn btn-small" onClick={fixHere}>לתקן כאן</button>}
            <button type="button" className="fin-link" onClick={() => onOpen(section.key)}>לעבור לסעיף</button>
            {one && <button type="button" className="fin-link" onClick={() => leave(g.id, c.notes[0]?.key ?? "")}>בסדר, להשאיר</button>}
          </div>
        )}
      </li>
    );
  }

  function lineCard(g: Group, c: LineCard): ReactNode {
    const target = c.open ? `s:${c.open}` : null;
    return (
      <li key={c.key} className={target !== null && shown === target ? "fin-card is-shown" : "fin-card"}>
        {c.where && <span className="fin-where">{c.where}</span>}
        <div className="fin-note">
          <b className="fin-title">{c.title}</b>
          {target ? (
            <button type="button" className="fin-quote" title="להראות בדוח" onClick={() => show(target)}>{c.text}</button>
          ) : (
            <span className="fin-why">{c.text}</span>
          )}
        </div>
        {(c.open !== null || c.leave) && (
          <div className="fin-actions">
            {c.open !== null && <button type="button" className="fin-link" onClick={() => onOpen(c.open ?? "")}>לעבור לסעיף</button>}
            {c.leave && <button type="button" className="fin-link" onClick={() => leave(g.id, c.key)}>בסדר, להשאיר</button>}
          </div>
        )}
      </li>
    );
  }

  function groupBlock(g: Group): ReactNode {
    if (g.cards.length === 0 && g.left.length === 0) return null;
    const open = isOpen(g);
    const leftOpen = showLeft[g.id] ?? false;
    return (
      <section key={g.id} className={`fin-group is-${g.id}`} aria-labelledby={`fin-${g.id}`}>
        <h2 id={`fin-${g.id}`}>
          <button type="button" className="fin-head" aria-expanded={open} aria-controls={`fin-${g.id}-body`} onClick={() => setFolds((f) => ({ ...f, [g.id]: !open }))}>
            <span className="fin-chevron" aria-hidden="true">‹</span>
            <span className="grow">{g.title}</span>
            <span className="fin-count" aria-label={g.count === 1 ? "דבר אחד" : `${g.count} דברים`}>{g.count}</span>
          </button>
        </h2>
        {open && (
          <div id={`fin-${g.id}-body`} className="fin-body">
            {g.cards.length > 0 && <ul className="fin-cards">{g.cards.map((c) => (c.type === "paragraph" ? paragraphCard(g, c) : lineCard(g, c)))}</ul>}
            {g.left.length > 0 && (
              <div className="fin-left">
                <button type="button" className="fin-link" aria-expanded={leftOpen} onClick={() => setShowLeft((s) => ({ ...s, [g.id]: !leftOpen }))}>
                  הערות שהשארת ({g.left.length})
                </button>
                {leftOpen && (
                  <ul>
                    {g.left.map((l) => (
                      <li key={l.key}>
                        <span className="grow">{l.label}</span>
                        <button type="button" className="fin-link" onClick={() => bringBack(l.key)}>להחזיר</button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </div>
        )}
      </section>
    );
  }

  const included = check?.included_sections ?? 0;

  return (
    <div className="view finish">
      <div className="finish-body">
        <section className="finish-side" aria-label="לפני שהקובץ יוצא">
          <div className="stack" style={{ gap: 4 }}>
            <h1>{statusLine({ loaded: check !== null, nothing, must: must?.count ?? 0, check: toCheck?.count ?? 0, complete })}</h1>
            <span className="muted small">{anyCard ? "לחיצה על הערה מראה אותה בדוח שמשמאל." : "משמאל: הדוח כמו שייצא."}</span>
          </div>
          {!check && !error && <WaitLine task="check" label="בודקת שהדוח מוכן להוצאה" />}
          {check && (
            <div className="fin-groups">
              {ready && <p className="fin-ok"><span aria-hidden="true">✓</span> אין שום דבר שעוצר את הקובץ.</p>}
              {groups.map(groupBlock)}
            </div>
          )}
          <ReportCheckItem api={api} />
          <ErrorLine error={error} />
          <section className="card finish-file">
            <b>קובץ Word או PDF</b>
            <span className="muted small">נשמר בתיקיית ההורדות, ונפתח בלי סיסמה.</span>
            <button type="button" className="btn btn-primary btn-big" disabled={!ready} onClick={onExport}>הפקת הקובץ</button>
            {!ready && check && (
              <span className="small muted">
                {nothing ? "צריך לפחות סעיף מאושר אחד." : "קודם מתקנים את מה שמסומן למעלה."}
              </span>
            )}
            {ready && (
              <span className="small muted">
                {included === 1 ? "סעיף מאושר אחד ייכנס לקובץ." : `${included} סעיפים מאושרים ייכנסו לקובץ.`} השמות חוזרים למקומם בקובץ.
              </span>
            )}
          </section>
          <LetterCard api={api} />
        </section>

        <div className="finish-preview" role="region" tabIndex={0} aria-label="תצוגה מקדימה של הקובץ">
          <article className="a4 a4-small" style={{ fontFamily: settings?.font ? `"${settings.font}", var(--font-display)` : undefined }}>
            <header className="a4-header">{settings?.confidentiality}</header>
            <h1 className="a4-title">{settings?.title ?? "דוח אבחון פסיכולוגי"}</h1>
            <dl className="a4-info">
              {child && <><dt>שם הילד/ה</dt><dd>{child}</dd></>}
              {detail.meta.age && <><dt>גיל בזמן האבחון</dt><dd>{ageWords(detail.meta.age)}</dd></>}
              <dt>תאריך הדוח</dt><dd>{new Date().toLocaleDateString("he-IL")}</dd>
            </dl>
            {parts.map((part) => {
              const sections = content.filter((s) => s.part === part);
              return (
                <section key={part}>
                  <h2 className="a4-part">{part}</h2>
                  {sections.map((s) => {
                    const ok = s.paragraphs.filter((p) => p.status === "approved");
                    return (
                      <div key={s.key} ref={place(`s:${s.key}`)} className={`a4-section${ok.length ? "" : " a4-out"}${shown === `s:${s.key}` ? " is-shown" : ""}`}>
                        <h3 className="a4-heading">{s.title}</h3>
                        {ok.length ? ok.map((p) => <p key={p.id} ref={place(`p:${p.id}`)} className={shown === `p:${p.id}` ? "a4-p is-shown" : "a4-p"}><Marked text={p.text} /></p>)
                          : <p className="a4-out-note">לא ייכנס לקובץ (אין פסקה מאושרת)</p>}
                      </div>
                    );
                  })}
                </section>
              );
            })}
          </article>
        </div>
      </div>
    </div>
  );
}
