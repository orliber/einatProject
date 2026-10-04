// "הסגנון שלי" (D-043): Claude learns how Einat writes from her past reports. Three steps, one
// main action at a time: add reports → analyze and build → review and approve. Every request
// goes through the same review screen as everything else that leaves the computer.
import { useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { estimateMs, ProgressLine, recordDuration } from "../components/Progress";
import { ReviewDialog } from "../components/ReviewDialog";
import { StyleImportDialog } from "../components/StyleImportDialog";
import { TopBar } from "../components/TopBar";
import { ErrorLine, Spinner, UploadIcon } from "../components/ui";
import {
  ipc,
  type Prepared,
  type StyleImportPreview,
  type StyleItem,
  type StyleKind,
  type StyleOverview,
  type StyleProfile,
  type StyleProfileView,
  type StyleSourceView,
} from "../ipc/client";
import { ConfirmDialog } from "./library/LibraryDialogs";
import "./StyleScreen.css";

const KIND_LABEL: Record<StyleKind, string> = {
  rule: "כללי כתיבה",
  phrase: "ביטויים אופייניים",
  avoid: "מה את לא כותבת",
  template: "תבניות משפט",
  example: "דוגמאות בדויות",
};
const KIND_HINT: Record<StyleKind, string> = {
  rule: "",
  phrase: "עד 5 מילים.",
  avoid: "",
  template: "במקום התוכן באים סוגריים מסולסלים, למשל {תחום}.",
  example: "פסקאות על ילד בדוי. הן מראות ל-Claude את הסגנון, ואין בהן שום דבר מהדוחות.",
};
const KINDS: StyleKind[] = ["rule", "phrase", "avoid", "template", "example"];
const RECOMMENDED = 3;

type Step = { kind: "analysis"; source: StyleSourceView } | { kind: "profile" };

function dateOf(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString("he-IL", { day: "numeric", month: "long", year: "numeric" });
}

export function StyleScreen() {
  const { status, fail, notify } = useApp();
  const qc = useQueryClient();
  const overview = useQuery({ queryKey: ["style"], queryFn: ipc.styleOverview });
  const fileInput = useRef<HTMLInputElement>(null);
  const [importing, setImporting] = useState<StyleImportPreview | null>(null);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<StyleSourceView | null>(null);
  const [resetting, setResetting] = useState(false);
  // The run: what is left to send, the review on screen, and the request on its way.
  const [queue, setQueue] = useState<Step[]>([]);
  const [review, setReview] = useState<{ step: Step; prepared: Prepared } | null>(null);
  const [sending, setSending] = useState<{ label: string; approval: string; started: number } | null>(null);

  const o = overview.data;
  const reload = () => qc.invalidateQueries({ queryKey: ["style"] });
  const sectionTitle = (key: string | null) =>
    key ? (o?.sections.find((s) => s.key === key)?.title ?? key) : "לא זוהה סעיף";

  async function pick(file: File) {
    setError(null);
    setReading(true);
    try {
      setImporting(await ipc.importStyleSource(file));
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setReading(false);
      if (fileInput.current) fileInput.current.value = "";
    }
  }

  // ---------------------------------------------------------------- the run

  async function prepare(step: Step) {
    return step.kind === "analysis" ? ipc.prepareStyleAnalysis(step.source.id) : ipc.prepareStyleProfile();
  }

  async function next(steps: Step[]) {
    setQueue(steps);
    const step = steps[0];
    if (!step) {
      await reload();
      return;
    }
    setError(null);
    try {
      const prepared = await prepare(step);
      const clean = prepared.approval_id && !prepared.auto_hidden.length && !prepared.blocked.length;
      if (status.review_only_suspect && clean && prepared.approval_id) await send(step, prepared.approval_id, steps);
      else setReview({ step, prepared });
    } catch (e) {
      setError(fail(e as never));
      setQueue([]);
    }
  }

  async function send(step: Step, approval: string, steps: Step[]) {
    const label = step.kind === "analysis" ? `מנתח את הסגנון ב"${step.source.title}"` : "בונה את פרופיל הסגנון מכל הניתוחים";
    const started = Date.now();
    setSending({ label, approval, started });
    try {
      if (step.kind === "analysis") {
        const r = await ipc.sendStyleAnalysis(approval);
        if (r.dropped > 0) notify(`${r.kept} פריטים נשמרו, ${r.dropped} לא נשמרו (נראו כמו העתקה או פרט מזהה).`);
      } else {
        await ipc.sendStyleProfile(approval);
        notify("טיוטת הפרופיל מוכנה. כדאי לעבור עליה ולאשר.");
      }
      recordDuration("draft", status.speed, Date.now() - started);
      setSending(null);
      await reload();
      await next(steps.slice(1));
    } catch (e) {
      setSending(null);
      setError(fail(e as never));
      setQueue([]);
      await reload();
    }
  }

  function startBuild(analyzeAll: boolean) {
    if (!o) return;
    const todo = o.sources.filter((s) => analyzeAll || !s.analyzed || (s.analysis_demo && !o.demo_mode));
    void next([...todo.map((source): Step => ({ kind: "analysis", source })), { kind: "profile" }]);
  }

  if (!o) {
    return (
      <div className="page">
        <TopBar active="style" />
        <main className="page-main style">{overview.error ? <ErrorLine error={fail(overview.error as never)} /> : <Spinner />}</main>
      </div>
    );
  }

  const analyzed = o.sources.filter((s) => s.analyzed).length;
  const waiting = o.sources.filter((s) => !s.analyzed || (s.analysis_demo && !o.demo_mode)).length;
  const shown = o.draft ?? o.active;
  const stage = o.sources.length === 0 ? 0 : o.draft ? 2 : o.active && waiting === 0 ? 3 : 1;
  const running = queue.length > 0 || sending !== null;

  return (
    <div className="page">
      <TopBar active="style" />
      <main className="page-main style">
        <header className="style-hero card">
          <div className="stack grow">
            <h1>הסגנון שלי</h1>
            <p className="style-lead">
              Claude לומד לכתוב כמוך מתוך דוחות ישנים שלך. לפני שמשהו נשמר, שמות, מקומות, תאריכים ומספרים מוסתרים, ונשמרים רק הקטעים שבחרת.
              אחר כך כל דוח נשלח לניתוח בנפרד, במסך "מה יוצא מהמחשב", ובכל ניסוח חדש נשלח רק הפרופיל שאישרת.
            </p>
            <ol className="style-steps" aria-label="שלבים">
              <li className={stage === 0 ? "now" : "done"}>1. דוחות ישנים</li>
              <li className={stage === 1 ? "now" : stage > 1 ? "done" : ""}>2. ניתוח ובניית הפרופיל</li>
              <li className={stage === 2 ? "now" : stage > 2 ? "done" : ""}>3. עיון ואישור</li>
            </ol>
          </div>
          <div className="style-hero-action stack">
            {stage === 0 && (
              <button type="button" className="btn btn-primary btn-big" disabled={reading} onClick={() => fileInput.current?.click()}>
                <UploadIcon /> הוספת דוח ישן
              </button>
            )}
            {stage === 1 && (
              <button type="button" className="btn btn-primary btn-big" disabled={running} onClick={() => startBuild(false)}>
                {analyzed === 0 ? `ניתוח ${o.sources.length === 1 ? "הדוח" : `${o.sources.length} הדוחות`} ובניית הפרופיל` : waiting > 0 ? `ניתוח ${waiting} דוחות חדשים ועדכון הפרופיל` : "בניית הפרופיל"}
              </button>
            )}
            {stage === 2 && <a className="btn btn-primary btn-big" href="#style-profile">עיון בטיוטה ואישור</a>}
            {stage === 3 && <span className="chip chip-ok">הפרופיל בשימוש · גרסה {o.active?.version}</span>}
            {o.sources.length > 0 && o.sources.length < RECOMMENDED && (
              <span className="hint">מומלץ {RECOMMENDED} דוחות לפחות, כדי ש-Claude יבחין במה שחוזר אצלך ולא במקרי.</span>
            )}
            {o.demo_mode && <span className="hint">מצב הדגמה: הניתוח נעשה במחשב, כדוגמה. עם מפתח API הוא נעשה באמת.</span>}
          </div>
        </header>

        <input ref={fileInput} type="file" hidden accept=".docx,.odt,.pdf,.txt"
          onChange={(e) => { const f = e.target.files?.[0]; if (f) void pick(f); }} />
        {reading && <p className="muted"><Spinner /> קורא את הדוח ומסתיר פרטים מזהים…</p>}
        {sending && (
          <ProgressLine started={sending.started} estimate={estimateMs("draft", status.speed, status.demo_mode)} label={sending.label} approval={sending.approval} />
        )}
        {queue.length > 1 && !sending && !review && <p className="muted small">נשארו {queue.length} שלבים.</p>}
        <ErrorLine error={error} />

        <Sources o={o} onAdd={() => fileInput.current?.click()} adding={reading || running} onDelete={setDeleting}
          onReanalyze={() => startBuild(true)} running={running} />

        {o.suggestions.length > 0 && <Suggestions o={o} />}

        {shown && <ProfileEditor key={`${shown.id}`} view={shown} o={o} />}

        {o.versions.length > 0 && <Versions o={o} onReset={() => setResetting(true)} />}
      </main>

      {importing && (
        <StyleImportDialog preview={importing} sectionTitle={sectionTitle} onClose={() => setImporting(null)}
          onSaved={(s) => { setImporting(null); notify(`"${s.title}" נשמר ב"הדוחות שלי".`); void reload(); }} />
      )}
      {review && (
        <ReviewDialog title={review.step.kind === "analysis" ? `ניתוח סגנון · ${review.step.source.title}` : "בניית פרופיל הסגנון"}
          caseId={null} prepared={review.prepared} reprepare={() => prepare(review.step)}
          onClose={() => { setReview(null); setQueue([]); }}
          onSend={async (id) => { const step = review.step; setReview(null); await send(step, id, queue); }} />
      )}
      {deleting && (
        <ConfirmDialog danger title="מחיקת דוח" action="מחיקה"
          text={`"${deleting.title}" יימחק מ"הדוחות שלי". פרופיל שכבר אושר לא משתנה עד שבונים אותו מחדש.`}
          onClose={() => setDeleting(null)}
          onConfirm={async () => { await ipc.deleteStyleSource(deleting.id); setDeleting(null); await reload(); }} />
      )}
      {resetting && (
        <ConfirmDialog danger title="מחיקת הפרופיל" action="מחיקת הפרופיל והלמידה"
          text="כל הגרסאות של פרופיל הסגנון, ומה שנלמד מהתיקונים שלך, יימחקו. הדוחות הישנים נשארים, ואפשר לבנות פרופיל חדש. הניסוח יחזור לסגנון ברירת המחדל."
          onClose={() => setResetting(false)}
          onConfirm={async () => { await ipc.resetStyle(); setResetting(false); await reload(); }} />
      )}
    </div>
  );
}

// ------------------------------------------------------------------ my reports

function Sources(props: {
  o: StyleOverview;
  adding: boolean;
  running: boolean;
  onAdd: () => void;
  onDelete: (s: StyleSourceView) => void;
  onReanalyze: () => void;
}) {
  const { o } = props;
  if (o.sources.length === 0) {
    return (
      <section className="card style-card style-empty" aria-labelledby="style-sources">
        <h2 id="style-sources">הדוחות שלי</h2>
        <p className="muted">
          עוד אין דוחות. אפשר להוסיף דוחות אבחון שכתבת בעבר (Word, ODT או PDF). מומלץ 3 עד 10 דוחות, מגילאים ומאבחנות שונים, כדי שהפרופיל ילמד את
          הסגנון ולא תוכן מסוים.
        </p>
      </section>
    );
  }
  return (
    <section className="card style-card" aria-labelledby="style-sources">
      <div className="row">
        <h2 id="style-sources" className="grow">הדוחות שלי <span className="muted small">({o.sources.length})</span></h2>
        {o.sources.some((s) => s.analyzed) && (
          <button type="button" className="btn btn-small btn-ghost" disabled={props.running} onClick={props.onReanalyze}>ניתוח מחדש של כולם</button>
        )}
        <button type="button" className="btn btn-small" disabled={props.adding} onClick={props.onAdd}><UploadIcon /> הוספת דוח</button>
      </div>
      <ul className="style-sources">
        {o.sources.map((s) => (
          <li key={s.id} className="style-source">
            <span className="fmt">{s.format.toUpperCase()}</span>
            <div className="stack grow" style={{ gap: 4 }}>
              <strong>{s.title}</strong>
              <span className="muted small">
                נוסף ב-{dateOf(s.added_at)} · {s.words.toLocaleString("he-IL")} מילים · {s.headings.join(" · ")}
              </span>
            </div>
            {s.analyzed
              ? s.analysis_demo
                ? <span className="chip chip-sand" title="נותח במצב הדגמה">נותח כדוגמה</span>
                : <span className="chip chip-ok">נותח · {s.analysis_items} פריטים</span>
              : <span className="chip chip-warn">עוד לא נותח</span>}
            <button type="button" className="btn btn-small btn-danger-quiet" onClick={() => props.onDelete(s)}>מחיקה</button>
          </li>
        ))}
      </ul>
      <p className="hint">נשמר רק הטקסט המנוטרל של הקטעים שבחרת. הקבצים עצמם לא נשמרים בכספת.</p>
    </section>
  );
}

// ------------------------------------------------------------------ suggestions from her edits

function Suggestions({ o }: { o: StyleOverview }) {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  async function act(f: () => Promise<unknown>, done?: string) {
    setError(null);
    try {
      await f();
      if (done) notify(done);
      await qc.invalidateQueries({ queryKey: ["style"] });
    } catch (e) {
      setError(fail(e as never));
    }
  }
  return (
    <section className="card style-card" aria-labelledby="style-sugg">
      <h2 id="style-sugg">מה למדתי מהתיקונים שלך</h2>
      <p className="hint">כשאת מתקנת פסקה ש-Claude כתב, התוכנה רושמת במחשב שלך החלפות קצרות שחוזרות. שום דבר מזה לא נשלח עד שאת מוסיפה אותו לפרופיל.</p>
      <ul className="style-suggestions">
        {o.suggestions.map((s) => (
          <li key={s.id} className="row">
            <span className="grow">במקום <q>{s.from}</q> את כותבת <q>{s.to}</q> <span className="muted small">({s.count} פעמים)</span></span>
            <button type="button" className="btn btn-small btn-primary" onClick={() => void act(() => ipc.acceptStyleSuggestion(s.id), "נוסף לפרופיל.")}>להוסיף לפרופיל</button>
            <button type="button" className="btn btn-small btn-ghost" onClick={() => void act(() => ipc.dismissStyleSuggestion(s.id))}>לא צריך</button>
          </li>
        ))}
      </ul>
      <ErrorLine error={error} />
    </section>
  );
}

// ------------------------------------------------------------------ the profile

function ProfileEditor({ view, o }: { view: StyleProfileView; o: StyleOverview }) {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  const [items, setItems] = useState<StyleItem[]>(view.profile.items);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const isDraft = view.status === "draft";

  const groups = useMemo(() => {
    const keys: (string | null)[] = [null, ...o.sections.map((s) => s.key)];
    return keys
      .map((key) => ({ key, title: key ? (o.sections.find((s) => s.key === key)?.title ?? key) : "כל הדוח", items: items.filter((i) => i.section === key) }))
      .filter((g) => g.key === null || g.items.length > 0);
  }, [items, o.sections]);

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
    <section id="style-profile" className={isDraft ? "card style-card style-profile style-draft" : "card style-card style-profile"} aria-labelledby="style-profile-title">
      <div className="row style-profile-head">
        <h2 id="style-profile-title" className="grow">{isDraft ? "טיוטת פרופיל הסגנון" : "פרופיל הסגנון"}</h2>
        {isDraft ? <span className="chip chip-warn">טיוטה · עוד לא בשימוש</span> : <span className="chip chip-ok">בשימוש · גרסה {view.version}</span>}
        {view.profile.reports > 0 && <span className="chip chip-sand">נבנה מ-{view.profile.reports} דוחות</span>}
        {view.demo && <span className="chip chip-sand">נבנה במצב הדגמה</span>}
      </div>
      {!isDraft && (
        <label className="row style-switch">
          <input type="checkbox" checked={o.enabled}
            onChange={(e) => void act(() => ipc.setStyleEnabled(e.target.checked), e.target.checked ? "הפרופיל חזר לשימוש." : "הניסוח חזר לסגנון ברירת המחדל.")} />
          <span>להשתמש בפרופיל בכל ניסוח</span>
        </label>
      )}
      <p className="hint">
        {isDraft
          ? "עברי על הפריטים: אפשר לכבות, לערוך, למחוק ולהוסיף. רק מה שמסומן ייכנס. הפרופיל ישפיע על הניסוח רק אחרי האישור."
          : "כל שינוי נשמר כטיוטה, ומשפיע על הניסוח אחרי שתאשרי אותו."}
        {view.dropped > 0 && ` ${view.dropped} פריטים שחזרו מ-Claude לא נשמרו, כי נראו כמו העתקה מדוח או כמו פרט מזהה.`}
      </p>

      {groups.map((g) => (
        <div key={g.key ?? "general"} className="style-group">
          <h3>{g.title}</h3>
          {KINDS.map((kind) => {
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
                        <textarea className="textarea grow" rows={kind === "example" ? 4 : 2} autoFocus defaultValue={item.text}
                          onBlur={(e) => { const text = e.target.value.trim(); setEditing(null); if (text && text !== item.text) change((all) => all.map((x) => (x.id === item.id ? { ...x, text } : x))); }} />
                      ) : (
                        <button type="button" className={kind === "example" ? "style-item-text style-example grow" : "style-item-text grow"} title="עריכה" onClick={() => setEditing(item.id)}>
                          {item.text}
                        </button>
                      )}
                      {item.origin === "edits" && <span className="chip chip-sand">מהתיקונים שלך</span>}
                      {item.origin === "manual" && <span className="chip chip-sand">שלך</span>}
                      {item.origin === "reports" && item.support > 1 && <span className="chip">ב-{item.support} דוחות</span>}
                      <button type="button" className="btn icon-btn" aria-label="מחיקה" onClick={() => change((all) => all.filter((x) => x.id !== item.id))}>×</button>
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
          <AddItem onAdd={(kind, text) => change((all) => [...all, { id: "", section: g.key, kind, text, enabled: true, origin: "manual", support: 0 }])} />
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
            onClick={() => void act(async () => { if (dirty) await ipc.saveStyleDraft(profile()); await ipc.approveStyleDraft(); }, "הפרופיל אושר. מעכשיו Claude כותב בסגנון שלך.")}>
            אישור הפרופיל
          </button>
        )}
      </div>
    </section>
  );
}

function AddItem({ onAdd }: { onAdd: (kind: StyleKind, text: string) => void }) {
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState<StyleKind>("rule");
  const [text, setText] = useState("");
  if (!open) {
    return <button type="button" className="btn btn-ghost btn-small style-add" onClick={() => setOpen(true)}>+ הוספת פריט משלך</button>;
  }
  return (
    <form className="row style-add-form" onSubmit={(e) => { e.preventDefault(); if (text.trim()) { onAdd(kind, text.trim()); setText(""); setOpen(false); } }}>
      <select className="select" aria-label="סוג" value={kind} onChange={(e) => setKind(e.target.value as StyleKind)}>
        {KINDS.filter((k) => k !== "example").map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
      </select>
      <input className="input grow" autoFocus placeholder="למשל: פסקת הסיכום פותחת בחוזקות של הילד" value={text} onChange={(e) => setText(e.target.value)} />
      <button type="submit" className="btn btn-small" disabled={!text.trim()}>הוספה</button>
      <button type="button" className="btn btn-small btn-ghost" onClick={() => setOpen(false)}>ביטול</button>
    </form>
  );
}

// ------------------------------------------------------------------ versions

function Versions({ o, onReset }: { o: StyleOverview; onReset: () => void }) {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  return (
    <section className="card style-card" aria-labelledby="style-versions">
      <h2 id="style-versions">גרסאות</h2>
      <ul className="style-versions">
        {o.versions.map((v) => (
          <li key={v.id} className="row">
            <span className="grow">
              גרסה {v.version} · {dateOf(v.created_at)} · {v.items} פריטים{v.reports > 0 ? ` · מ-${v.reports} דוחות` : ""}
            </span>
            {v.status === "active"
              ? <span className="chip chip-ok">בשימוש</span>
              : (
                <button type="button" className="btn btn-small btn-ghost"
                  onClick={() => { setError(null); ipc.restoreStyleVersion(v.id).then(() => { notify(`גרסה ${v.version} חזרה לשימוש (כגרסה חדשה).`); return qc.invalidateQueries({ queryKey: ["style"] }); }).catch((e) => setError(fail(e as never))); }}>
                  חזרה לגרסה הזו
                </button>
              )}
          </li>
        ))}
      </ul>
      <ErrorLine error={error} />
      <button type="button" className="btn btn-small btn-danger-quiet style-reset" onClick={onReset}>מחיקת הפרופיל ומה שנלמד</button>
    </section>
  );
}
