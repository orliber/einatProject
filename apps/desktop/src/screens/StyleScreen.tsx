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
import { ErrorLine, Skeleton, Spinner, UploadIcon, CloseIcon } from "../components/ui";
import {
  ipc,
  type FitMonth,
  type InterpretationView,
  type Prepared,
  type StyleImportPreview,
  type StyleItem,
  type StyleKind,
  type StyleLearned,
  type StyleOverview,
  type StyleProfile,
  type StyleProfileView,
  type StyleSourceView,
  type StyleSuggestion,
} from "../ipc/client";
import { ConfirmDialog } from "./library/LibraryDialogs";
import "./StyleScreen.css";
import { useAi } from "../ai";
import { notSentMessage } from "../sending";

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
  example: "פסקאות על ילד בדוי. הן מראות ל-AI את הסגנון, ואין בהן שום דבר מהדוחות.",
};
const KINDS: StyleKind[] = ["rule", "phrase", "avoid", "template", "example"];
const RECOMMENDED = 3;

type Step = { kind: "analysis"; source: StyleSourceView } | { kind: "profile" } | { kind: "interpret" };

const STEP_TITLE = (step: Step) =>
  step.kind === "analysis" ? `ניתוח סגנון · ${step.source.title}` : step.kind === "interpret" ? "איך פירשת תוצאות" : "בניית פרופיל הסגנון";

function dateOf(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString("he-IL", { day: "numeric", month: "long", year: "numeric" });
}

export function StyleScreen() {
  const { status, fail, notify } = useApp();
  const ai = useAi();
  const qc = useQueryClient();
  const overview = useQuery({ queryKey: ["style"], queryFn: ipc.styleOverview });
  const fileInput = useRef<HTMLInputElement>(null);
  const [importing, setImporting] = useState<StyleImportPreview | null>(null);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<StyleSourceView | null>(null);
  const [resetting, setResetting] = useState(false);
  const [forgetting, setForgetting] = useState(false);
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
    if (step.kind === "analysis") return ipc.prepareStyleAnalysis(step.source.id);
    return step.kind === "interpret" ? ipc.prepareInterpretations() : ipc.prepareStyleProfile();
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
      // Round 26: what the gate cleared goes at once; the review only when she asked for it.
      if (status.review_always) setReview({ step, prepared });
      else if (prepared.approval_id) await send(step, prepared.approval_id, steps);
      else {
        setError(notSentMessage(prepared));
        setQueue([]);
      }
    } catch (e) {
      setError(fail(e as never));
      setQueue([]);
    }
  }

  async function send(step: Step, approval: string, steps: Step[]) {
    const label = step.kind === "analysis"
      ? `מנתח את הסגנון ב"${step.source.title}"`
      : step.kind === "interpret" ? "לומד מהדוחות איך פירשת תוצאות" : "בונה את פרופיל הסגנון מכל הניתוחים";
    const started = Date.now();
    setSending({ label, approval, started });
    try {
      if (step.kind === "analysis") {
        const r = await ipc.sendStyleAnalysis(approval);
        if (r.dropped > 0) notify(`${r.kept} פריטים נשמרו, ${r.dropped} נפסלו (העתקה או פרט מזהה).`);
      } else if (step.kind === "interpret") {
        const r = await ipc.sendInterpretations(approval);
        notify(r.kept === 0
          ? "לא נמצאה פרשנות חדשה בדוחות."
          : `${r.kept === 1 ? "פרשנות אחת מחכה" : `${r.kept} פרשנויות מחכות`} לאישור שלך.${r.dropped > 0 ? ` ${r.dropped} נפסלו (העתקה או פרט מזהה).` : ""}`);
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
        <main className="page-main style">{overview.error
          ? <ErrorLine error={fail(overview.error as never)} retry={() => void qc.invalidateQueries({ queryKey: ["style"] })} />
          : <div className="stack" style={{ gap: 16 }}><Skeleton card lines={4} label="טוען את הסגנון" /><Skeleton card lines={3} label="טוען" /></div>}</main>
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
              {ai} לומד לכתוב כמוך מדוחות ישנים שלך. נשמרים רק הקטעים שבחרת, בלי פרטים מזהים.
              בכל ניסוח נשלח רק הפרופיל שאישרת.
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
              <span className="hint">מומלץ {RECOMMENDED} דוחות לפחות.</span>
            )}
            {o.demo_mode && <span className="hint">מצב הדגמה: ניתוח לדוגמה בלבד.</span>}
          </div>
        </header>

        <input ref={fileInput} type="file" hidden accept=".docx,.doc,.odt,.rtf,.pdf,.txt"
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

        <Learned o={o} onForget={() => setForgetting(true)} />

        <Interpretations o={o} running={running} onRead={() => void next([{ kind: "interpret" }])} />

        {shown && <ProfileEditor key={`${shown.id}`} view={shown} o={o} />}

        {o.versions.length > 0 && <Versions o={o} onReset={() => setResetting(true)} />}
      </main>

      {importing && (
        <StyleImportDialog preview={importing} sectionTitle={sectionTitle} onClose={() => setImporting(null)}
          onSaved={(s) => { setImporting(null); notify(`"${s.title}" נשמר ב"הדוחות שלי".`); void reload(); }} />
      )}
      {review && (
        <ReviewDialog title={STEP_TITLE(review.step)}
          caseId={null} prepared={review.prepared} reprepare={() => prepare(review.step)}
          onClose={() => { setReview(null); setQueue([]); }}
          onSend={async (id) => { const step = review.step; setReview(null); await send(step, id, queue); }} />
      )}
      {deleting && (
        <ConfirmDialog danger title="מחיקת דוח" action="מחיקה"
          text={`"${deleting.title}" יימחק. הפרופיל שאושר לא משתנה עד בנייה מחדש.`}
          onClose={() => setDeleting(null)}
          onConfirm={async () => { await ipc.deleteStyleSource(deleting.id); setDeleting(null); await reload(); }} />
      )}
      {forgetting && (
        <ConfirmDialog danger title="לשכוח את מה שנלמד" action="לשכוח"
          text="מה שנלמד מהתיקונים שלך יימחק. הדוחות הישנים והפרופיל נשארים."
          onClose={() => setForgetting(false)}
          onConfirm={async () => { await ipc.forgetStyleLearning(); setForgetting(false); notify("מה שנלמד נמחק."); await reload(); }} />
      )}
      {resetting && (
        <ConfirmDialog danger title="מחיקת הפרופיל" action="מחיקת הפרופיל והלמידה"
          text="הפרופיל ומה שנלמד מהתיקונים יימחקו. הדוחות הישנים נשארים."
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
      <p className="hint">נשמר רק הטקסט המנוטרל, לא הקובץ.</p>
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
      <h2 id="style-sugg">מה למדתי ממך</h2>
      <p className="hint">נלמד במחשב מהתיקונים שלך. לא נשלח עד שתוסיפי לפרופיל.</p>
      <ul className="style-suggestions">
        {o.suggestions.map((s) => (
          <li key={s.id} className="row">
            <span className="grow">
              <SuggestionText s={s} />{" "}
              <span className="muted small">({suggestionDetail(s, o)})</span>
            </span>
            <button type="button" className="btn btn-small btn-primary" onClick={() => void act(() => ipc.acceptStyleSuggestion(s.id), "נוסף לפרופיל.")}>להוסיף לפרופיל</button>
            <button type="button" className="btn btn-small btn-ghost" onClick={() => void act(() => ipc.dismissStyleSuggestion(s.id))}>לא צריך</button>
          </li>
        ))}
      </ul>
      <ErrorLine error={error} />
    </section>
  );
}

function SuggestionText({ s }: { s: StyleSuggestion }) {
  switch (s.kind) {
    case "replace":
      return <>במקום <q>{s.from}</q> את כותבת <q>{s.to}</q></>;
    case "avoid":
      return <>את מוחקת <q>{s.from}</q></>;
    case "phrase":
      return <>ביטוי שחוזר בדוחות שלך: <q>{s.from}</q></>;
    case "opening":
      return <>כך את פותחת את הסעיף בדרך כלל: <q>{s.from}…</q></>;
    case "closing":
      return <>כך את מסיימת את הסעיף בדרך כלל: <q>{s.from}…</q></>;
    case "formula":
      return <>משפט שאת כותבת באותו נוסח בכמה דוחות: <q>{s.from}</q></>;
  }
}

function suggestionDetail(s: StyleSuggestion, o: StyleOverview): string {
  const title = s.section ? o.sections.find((x) => x.key === s.section)?.title : undefined;
  const where = title ? ` · בסעיף ${title}` : "";
  const fromReports = s.kind !== "replace" && s.kind !== "avoid";
  return fromReports ? `ב-${s.count} דוחות${where}` : `${s.count} פעמים${where}`;
}

// ------------------------------------------------------------------ what was measured

/** Her usual figures, in words: what the program measured, numbers only. */
export function learnedLines(l: StyleLearned): string[] {
  const out: string[] = [];
  const round = (x: number) => Math.round(x);
  if (l.words_per_sentence != null) out.push(`בממוצע ${round(l.words_per_sentence)} מילים במשפט`);
  if (l.sentences_per_paragraph != null) out.push(`בדרך כלל ${round(l.sentences_per_paragraph)} משפטים בפסקה`);
  if (l.commas_per_sentence != null) out.push(`כ-${l.commas_per_sentence.toFixed(1)} פסיקים במשפט`);
  if (l.hedges != null) out.push(`הסתייגות («נראה», «ניכר») ב-${round(l.hedges)} מכל 100 משפטים`);
  if (l.contrasts != null) out.push(`מילת ניגוד («אך», «עם זאת») ב-${round(l.contrasts)} מכל 100 משפטים`);
  if (l.first_person != null) out.push(`גוף ראשון («התרשמתי») ב-${round(l.first_person)} מכל 100 משפטים`);
  return out;
}

const MONTHS_HE = ["ינואר", "פברואר", "מרץ", "אפריל", "מאי", "יוני", "יולי", "אוגוסט", "ספטמבר", "אוקטובר", "נובמבר", "דצמבר"];

/** "2026-10" → ["אוק׳ 26" as a short label, "אוקטובר 2026" in full]. */
export function monthLabel(month: string): { short: string; full: string } {
  const [y = "", m = "1"] = month.split("-");
  const name = MONTHS_HE[Number(m) - 1] ?? month;
  return { short: `${Number(m)}.${y.slice(2)}`, full: `${name} ${y}` };
}

/** ST12: the average closeness by month, oldest first: is drafting getting closer to her? */
function FitTrend({ months }: { months: FitMonth[] }) {
  return (
    <div className="fit-trend">
      <span className="small muted">לאורך זמן (ממוצע לחודש, מתוך 100):</span>
      <ol className="fit-bars">
        {months.map((m) => {
          const { short, full } = monthLabel(m.month);
          return (
            <li key={m.month} className="fit-bar" aria-label={`${full}: ${m.average} (${m.count} פסקאות)`}>
              <span className="fit-value" aria-hidden="true">{m.average}</span>
              <span className="fit-fill" style={{ height: `${Math.max(3, Math.round(m.average * 0.8))}px` }} aria-hidden="true" />
              <span className="fit-month" aria-hidden="true">{short}</span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

function Learned({ o, onForget }: { o: StyleOverview; onForget: () => void }) {
  const ai = useAi();
  const l = o.learned;
  const lines = learnedLines(l);
  const from = [
    l.from_reports > 0 && `${l.from_reports} פסקאות מהדוחות הישנים`,
    l.from_edits > 0 && `${l.from_edits} מהתיקונים שלך`,
    l.finished_reports > 0 && (l.finished_reports === 1 ? "דוח אחד שסיימת בתוכנה" : `${l.finished_reports} דוחות שסיימת בתוכנה`),
  ].filter(Boolean) as string[];
  const canForget = l.from_edits > 0 || l.finished_reports > 0 || l.fit_count > 0;
  if (from.length === 0 && l.fit_count === 0) return null;
  return (
    <section className="card style-card" aria-labelledby="style-learned">
      <h2 id="style-learned">מה התוכנה מדדה בכתיבה שלך</h2>
      <p className="hint">נמדד במחשב שלך, ונשמרים רק מספרים. {from.length > 0 && `נלמד מ${from.join(", ")}.`}</p>
      {lines.length > 0 ? (
        <ul className="style-learned">{lines.map((t) => <li key={t}>{t}</li>)}</ul>
      ) : (
        <p className="muted small">עוד אין מספיק פסקאות כדי לומר מה אופייני לך (צריך לפחות 12).</p>
      )}
      {l.fit_recent != null && (
        <p className="style-fit">
          כמה הפסקאות ש-{ai} כתב קרובות לסגנון שלך: <b>{l.fit_recent} מתוך 100</b>
          {l.fit_earlier != null
            ? ` ב-30 הפסקאות האחרונות, לעומת ${l.fit_earlier} ב-30 שלפניהן.`
            : ` (${l.fit_count} פסקאות עד עכשיו).`}
        </p>
      )}
      {l.fit_by_month.length >= 2 && <FitTrend months={l.fit_by_month} />}
      {o.skeletons.length > 0 && (
        <div className="stack style-skeletons">
          <h3 className="small">הסדר שבו את כותבת את הסעיפים</h3>
          <ul className="style-learned">
            {o.skeletons.map((k) => (
              <li key={`${k.section}-${k.band ?? ""}`}>
                <b>{k.title}{k.band ? ` (${k.band})` : ""}:</b> {k.topics.join(" · ")}{" "}
                <span className="muted small">(מ-{k.reports} דוחות)</span>
              </li>
            ))}
          </ul>
          <p className="hint">כל עוד לא כתבת הנחיה משלך לסעיף, זו ההנחיה שלו בטיוטה.</p>
        </div>
      )}
      {o.omissions.length > 0 && (
        <ul className="style-learned" aria-label="מה את לא כותבת">
          {o.omissions.map((t) => <li key={t}>{t}</li>)}
        </ul>
      )}
      {canForget && (
        <div className="row">
          <button type="button" className="btn btn-small btn-danger-quiet" onClick={onForget}>לשכוח את מה שנלמד מהתיקונים ומהדוחות שסיימתי</button>
        </div>
      )}
    </section>
  );
}

// ------------------------------------------------------------------ interpretation (round 33)

/** How she interprets each subtest's score, learned from her reports; nothing is used before she approves it. */
function Interpretations({ o, running, onRead }: { o: StyleOverview; running: boolean; onRead: () => void }) {
  const { fail } = useApp();
  const qc = useQueryClient();
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  if (o.scored_measures === 0 && o.interpretations.length === 0) return null;
  const waiting = o.interpretations.filter((i) => !i.item.approved).length;
  const groups = new Map<string, InterpretationView[]>();
  for (const i of o.interpretations) {
    const key = `${i.instrument_name} · ${i.measure_name}`;
    groups.set(key, [...(groups.get(key) ?? []), i]);
  }

  async function act(f: () => Promise<unknown>) {
    setError(null);
    try {
      await f();
      setEditing(null);
      await qc.invalidateQueries({ queryKey: ["style"] });
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <section className="card style-card" aria-labelledby="style-interpret">
      <h2 id="style-interpret">איך את מפרשת תוצאות</h2>
      <p className="hint">
        מהמשפטים בדוחות הישנים שבהם כתבת על תוצאה של תת-מבחן. נשלחים רק המשפטים האלה, בלי פרטים מזהים, ונשמרת פרשנות כללית.
        שום פרשנות לא משמשת בדוח לפני שאישרת אותה.
      </p>
      <div className="row">
        <button type="button" className="btn btn-small" disabled={running || o.scored_measures === 0} onClick={onRead}>
          {o.interpretations.length === 0 ? "ללמוד מהדוחות איך פירשתי תוצאות" : "לקרוא שוב את הדוחות"}
        </button>
        {o.scored_measures > 0 && <span className="hint">{o.scored_measures === 1 ? "תת-מבחן אחד" : `${o.scored_measures} תת-מבחנים`} עם תוצאות בדוחות.</span>}
        {waiting > 0 && <span className="chip">{waiting === 1 ? "אחת מחכה" : `${waiting} מחכות`} לאישור</span>}
      </div>
      <ErrorLine error={error} />
      {[...groups.entries()].map(([title, items]) => (
        <div key={title} className="style-group" role="group" aria-label={title}>
          <h3>{title}</h3>
          <ul className="style-items">
            {items.map(({ item }) => (
              <li key={item.id} className={item.enabled ? "" : "off"}>
                <span className="chip">{item.case}</span>{" "}
                {editing === item.id ? (
                  <span className="stack">
                    <textarea aria-label="הפרשנות" value={draft} rows={3} onChange={(e) => setDraft(e.target.value)} />
                    <span className="row">
                      <button type="button" className="btn btn-small btn-primary" onClick={() => void act(() => ipc.updateInterpretation(item.id, draft, true, null))}>שמירה ואישור</button>
                      <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
                    </span>
                  </span>
                ) : (
                  <>
                    <span>{item.text}</span>
                    {!item.approved && <span className="chip chip-warn">מחכה לאישור</span>}
                    {item.support > 1 && <span className="hint"> · ב-{item.support} דוחות</span>}
                    <span className="row">
                      {!item.approved && <button type="button" className="btn btn-small btn-primary" onClick={() => void act(() => ipc.updateInterpretation(item.id, null, true, null))}>אישור</button>}
                      <button type="button" className="btn btn-small" onClick={() => { setEditing(item.id); setDraft(item.text); }}>עריכה</button>
                      {item.approved && (
                        <button type="button" className="btn btn-small" onClick={() => void act(() => ipc.updateInterpretation(item.id, null, null, !item.enabled))}>
                          {item.enabled ? "כיבוי" : "הדלקה"}
                        </button>
                      )}
                      <button type="button" className="btn btn-small btn-danger-quiet" onClick={() => void act(() => ipc.deleteInterpretation(item.id))}>מחיקה</button>
                    </span>
                  </>
                )}
              </li>
            ))}
          </ul>
        </div>
      ))}
    </section>
  );
}

// ------------------------------------------------------------------ the profile

function ProfileEditor({ view, o }: { view: StyleProfileView; o: StyleOverview }) {
  const { fail, notify } = useApp();
  const ai = useAi();
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
          ? "עברי על הפריטים. רק מה שמסומן ייכנס, ורק אחרי האישור."
          : "כל שינוי משפיע רק אחרי אישור."}
        {view.dropped > 0 && ` ${view.dropped} פריטים נפסלו (העתקה או פרט מזהה).`}
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
                      <button type="button" className="btn icon-btn" aria-label="מחיקה" onClick={() => change((all) => all.filter((x) => x.id !== item.id))}><CloseIcon /></button>
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
            onClick={() => void act(async () => { if (dirty) await ipc.saveStyleDraft(profile()); await ipc.approveStyleDraft(); }, `הפרופיל אושר. מעכשיו ${ai} כותב בסגנון שלך.`)}>
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
