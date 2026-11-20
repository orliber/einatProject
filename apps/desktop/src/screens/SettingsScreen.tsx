import { lazy, Suspense, useEffect, useRef, useState, type ReactNode } from "react";
import { BugReportSettings } from "../components/BugReport";
import { SystemCheckSettings } from "../components/SystemCheck";
import { useApp } from "../App";
import { ActivitySettings } from "../components/ActivityLog";
import { BackupSettings } from "../components/Backup";
import { DriveBackupSettings } from "../components/DriveBackupSettings";
import { PasswordSettings } from "../components/PasswordSettings";
import { TopBar } from "../components/TopBar";
import { UpdateSettings } from "../components/Update";
import { UsageSettings } from "../components/UsageSettings";
import { VerifySettings } from "../components/VerifySettings";
import { ReportCheckSettings } from "../components/ReportCheck";
import { NameModelSettings } from "../components/NameModelSettings";
import { ReadinessSettings } from "../components/ReadinessSettings";
import { SafetyNet } from "../components/SafetyNet";
import { InstrumentSettings } from "../components/InstrumentSettings";
import { CriticalValuesSettings } from "../components/CriticalValuesSettings";
import { SecuritySettings } from "../components/SecuritySettings";
import { PrivacyStatus } from "../components/PrivacyStatus";
import { Segmented, Switch } from "../components/Switch";
import { ErrorLine, SearchIcon, Skeleton } from "../components/ui";
import { ipc, type LogoView, type ReportSettings, type TemplateView } from "../ipc/client";
import { PROVIDERS, providerOf } from "../ai";

import { TEXT_SIZES, applyTextSize, readTextSize } from "../textSize";
import { THEMES, applyTheme, readTheme } from "../theme";
import { SectionModesSettings } from "../components/SectionModes";
import { SettingsIcon } from "../components/SettingsIcon";
import {
  clinicGroupOf, groupOf, isGroup, lastSettingsPane, rememberSettingsPane, searchSettings, SETTINGS_NAV, SETTINGS_PANES, toTop,
  type GroupEntry, type PaneEntry, type SettingsPane,
} from "./settingsPanes";
import "./SettingsScreen.css";

/** The clinic's settings (D-205): a part of these settings, loaded with the clinic's code only when opened. */
const ClinicSettingsTab = lazy(() => import("./clinic/ClinicSettingsTab").then((m) => ({ default: m.ClinicSettingsTab })));

/** The screen-capture switch is hidden while that protection is off (D-039). */
const SCREEN_PROTECTION_SETTING = false;

/** One setting: its name and a short line on the start side, the control on the end side. */
function Row({ label, htmlFor, hint, children }: { label: string; htmlFor?: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="set-row">
      <div className="set-row-text">
        {htmlFor ? <label htmlFor={htmlFor} className="set-row-label">{label}</label> : <span className="set-row-label">{label}</span>}
        {hint && <span className="hint">{hint}</span>}
      </div>
      <div className="set-row-control">{children}</div>
    </div>
  );
}

/** Settings as in other professional programs: the parts on the side, one part at a time. */
export function SettingsScreen({ pane: initial }: { pane?: SettingsPane | undefined }) {
  const { status, refresh, notify, fail } = useApp();
  // A link names its part; the gear opens the part she was in last (D-205).
  const [pane, setPaneState] = useState<SettingsPane>(() => {
    const first = initial ?? lastSettingsPane();
    rememberSettingsPane(first);
    return first;
  });
  /** The program's own part she was in before entering a group (the way back goes there). */
  const [outside, setOutside] = useState<SettingsPane>(() => (groupOf(initial ?? lastSettingsPane()) ? "general" : (initial ?? lastSettingsPane())));
  /** Inside a group: the part of it she was in last, where entering it again opens. */
  const lastInside = useRef(new Map<string, SettingsPane>());
  /** What the search found and she chose: the heading to bring into view once the part shows. */
  const [target, setTarget] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const found = searchSettings(query);
  const setPane = (next: SettingsPane) => {
    rememberSettingsPane(next);
    const g = groupOf(next);
    if (g) lastInside.current.set(g.id, next);
    else setOutside(next);
    if (next !== pane) toTop();
    setError(null);
    setPaneState(next);
  };
  const enter = (g: GroupEntry) => {
    const next = lastInside.current.get(g.id) ?? g.panes[0]?.id;
    if (next) setPane(next);
  };
  const current = SETTINGS_PANES.find((p) => p.id === pane);
  const group = groupOf(pane);
  const clinicGroup = clinicGroupOf(pane);

  // A setting chosen in the search: its heading is brought into view and marked for a moment.
  // The clinic's parts load on first use, so the heading is looked for a few times.
  useEffect(() => {
    if (target === null) return;
    const want = target.replace(/\s+/g, " ").trim();
    let tries = 0;
    let timer: number | undefined;
    const look = () => {
      const all = document.querySelectorAll<HTMLElement>(".settings-pane h2, .settings-pane h3, .settings-pane legend, .settings-pane label, .settings-pane .set-row-label");
      const hit = [...all].find((el) => (el.textContent ?? "").replace(/\s+/g, " ").includes(want));
      if (hit) {
        hit.scrollIntoView({ block: "center" });
        hit.classList.add("settings-found-here");
        timer = window.setTimeout(() => hit.classList.remove("settings-found-here"), 1800);
        setTarget(null);
      } else if (tries++ < 20) {
        timer = window.setTimeout(look, 100);
      } else {
        setTarget(null);
      }
    };
    look();
    return () => window.clearTimeout(timer);
  }, [target, pane]);

  function choose(p: PaneEntry, inside: string | null) {
    setQuery("");
    setPane(p.id);
    setTarget(inside);
  }
  const [apiKey, setApiKey] = useState("");
  const [ack, setAck] = useState(false);
  const provider = providerOf(status.model);
  const ai = provider.name;
  const hasKey = status.keys.includes(provider.id);
  const [names, setNames] = useState(status.practitioner.join(", "));
  const [report, setReport] = useState<ReportSettings | null>(null);
  const [template, setTemplate] = useState<TemplateView | null>(null);
  const [logo, setLogo] = useState<LogoView | null>(null);
  const [original, setOriginal] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The report's details could not be read: said in the report part, which is empty without them. */
  const [reportError, setReportError] = useState<string | null>(null);
  const [textSize, setTextSize] = useState(readTextSize);
  const [theme, setTheme] = useState(readTheme);

  useEffect(() => {
    ipc.reportSettings().then(setReport).catch((e) => setReportError(fail(e as never)));
    ipc.reportTemplate().then(setTemplate).catch(() => setTemplate(null));
    ipc.reportLogo().then(setLogo).catch(() => setLogo(null));
  }, [fail]);

  async function run(fn: () => Promise<void>, done: string) {
    setError(null);
    try {
      await fn();
      await refresh();
      notify(done);
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <div className="page">
      <TopBar active="settings" />
      <main className="page-main settings">
        <nav className="settings-nav" aria-label="חלקי ההגדרות">
          <h1 className="settings-title">הגדרות</h1>
          <div className="settings-search">
            <SearchIcon size={16} />
            <input type="search" className="settings-search-input" placeholder="חיפוש בהגדרות" aria-label="חיפוש בהגדרות"
              value={query} onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape" && query !== "") { e.preventDefault(); setQuery(""); }
                const first = found[0];
                if (e.key === "Enter" && first) { e.preventDefault(); choose(first.pane, first.inside); }
              }} />
          </div>

          {query.trim() !== "" ? (
            found.length === 0 ? (
              <p className="hint settings-none" role="status">לא נמצאה הגדרה כזו. אפשר לנסות מילה אחרת.</p>
            ) : (
              <ul className="settings-found" aria-label="מה שנמצא">
                {found.map((f) => (
                  <li key={`${f.pane.id}:${f.inside ?? ""}`}>
                    <button type="button" className="settings-nav-btn settings-found-btn" onClick={() => choose(f.pane, f.inside)}>
                      <SettingsIcon name={f.pane.icon} />
                      <span className="settings-found-text">
                        <span>{f.inside ?? f.pane.label}</span>
                        <span className="settings-found-where">{[f.group, f.pane.label].filter(Boolean).join(" › ")}</span>
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )
          ) : group ? (
            <>
              {/* Inside a group, as on a Mac: the way back, the group's name, and its parts. */}
              <button type="button" className="settings-back" onClick={() => setPane(outside)}>
                <span aria-hidden="true">→</span> כל ההגדרות
              </button>
              <h2 className="settings-nav-group"><SettingsIcon name={group.icon} />{group.label}</h2>
              {group.panes.map((p) => (
                <button key={p.id} type="button" className="settings-nav-btn" aria-current={p.id === pane ? "page" : undefined}
                  onClick={() => setPane(p.id)}><SettingsIcon name={p.icon} />{p.label}</button>
              ))}
            </>
          ) : (
            SETTINGS_NAV.map((e) => isGroup(e) ? (
              <button key={e.id} type="button" className="settings-nav-btn" onClick={() => enter(e)}>
                <SettingsIcon name={e.icon} />{e.label}<span className="settings-nav-more" aria-hidden="true">‹</span>
              </button>
            ) : (
              <button key={e.id} type="button" className="settings-nav-btn" aria-current={e.id === pane ? "page" : undefined}
                onClick={() => setPane(e.id)}><SettingsIcon name={e.icon} />{e.label}</button>
            ))
          )}
        </nav>

        <div className="settings-pane">
          {group && (
            <nav className="settings-crumbs" aria-label="איפה את בהגדרות">
              <button type="button" className="link-btn" onClick={() => setPane(outside)}>הגדרות</button>
              <span aria-hidden="true"> › </span>
              <button type="button" className="link-btn" onClick={() => enter(group)}>{group.label}</button>
              <span aria-hidden="true"> › </span>
              <span aria-current="page">{current?.label}</span>
            </nav>
          )}
          <h2 className="settings-pane-title">{current?.label}</h2>
          {current && <p className="settings-pane-intro">{current.intro}</p>}
          <ErrorLine error={error} />

          {pane === "general" && (
            <>
              <section className="card setting" aria-labelledby="s-general">
                <h2 id="s-general">תצוגה ופרטים</h2>
                <Row label="גודל הטקסט" htmlFor="text-size" hint="גם Ctrl ו-+ או −">
                  <select id="text-size" className="select" value={textSize}
                    onChange={(e) => { const n = Number(e.target.value); applyTextSize(n); setTextSize(n); }}>
                    {TEXT_SIZES.map((n) => <option key={n} value={n}>{n === 100 ? "רגיל" : `${n}%`}</option>)}
                  </select>
                </Row>
                <Row label="מראה" hint="דף הדוח נשאר לבן">
                  <Segmented label="מראה" value={theme} options={THEMES}
                    onChange={(t) => { applyTheme(t); setTheme(t); }} />
                </Row>
                <div className="field">
                  <label htmlFor="names">השמות שלך</label>
                  <span className="hint">בדוח ובמה שנשלח הם מוחלפים ב"המאבחנת". מפרידים בפסיק.</span>
                  <div className="row">
                    <input id="names" className="input grow" value={names} onChange={(e) => setNames(e.target.value)} />
                    <button type="button" className="btn" onClick={() => void run(() => ipc.setPractitioner(names.split(",").map((n) => n.trim()).filter(Boolean)), "השמות נשמרו.")}>שמירה</button>
                  </div>
                </div>
              </section>
              {/* The everyday settings first; the list before real cases after them (D-205).
                  The safety net is on "סיסמה ואבטחה", where its buttons belong. */}
              <ReadinessSettings />
            </>
          )}

          {pane === "ai" && (
            <>
              <section className="card setting" aria-labelledby="s-claude">
                <h2 id="s-claude">חיבור</h2>
                <Row label="ספק" htmlFor="provider" hint="שמות ופרטים מזהים מוסתרים לפני כל שליחה.">
                  <select id="provider" className="select" value={provider.id}
                    onChange={(e) => {
                      const next = PROVIDERS.find((p) => p.id === e.target.value);
                      const model = next?.models[0]?.[0];
                      if (!next || !model) return;
                      setApiKey("");
                      setAck(false);
                      void run(() => ipc.setModel(model), `מעכשיו עובדים עם ${next.name}.`);
                    }}>
                    {PROVIDERS.map((p) => <option key={p.id} value={p.id}>{p.name}{p.id === "anthropic" ? " (מומלץ)" : ""}</option>)}
                  </select>
                </Row>
                {provider.setup && <p className="note-sand small" role="note">{provider.setup}</p>}
                {provider.keyPlaceholder !== null && (
                  <div className="field">
                    <label htmlFor="api">מפתח API</label>
                    <span className="hint">
                      {status.demo_mode
                        ? `אין מפתח, ולכן התוכנה במצב הדגמה ושום דבר לא נשלח.${provider.id === "anthropic" ? " צריך מפתח של חשבון עם הסכם ZDR." : ""}`
                        : "המפתח שמור מוצפן בכספת."}
                    </span>
                    {provider.retention && (
                      <div className="note-sand small" role="note">
                        <b>מה {provider.company} שומרת:</b> {provider.retention}
                      </div>
                    )}
                    <div className="row">
                      <input id="api" className="input grow mono" dir="ltr" type="password" autoComplete="off" placeholder={hasKey ? "••••••••••••" : provider.keyPlaceholder}
                        value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
                      <button type="button" className="btn btn-primary" disabled={!apiKey.trim() || (provider.retention !== null && !ack)}
                        onClick={() => void run(async () => { await ipc.setApiKey(provider.id, apiKey, ack); setApiKey(""); setAck(false); }, "המפתח נשמר מוצפן.")}>שמירה</button>
                      {hasKey && (
                        <button type="button" className="btn" onClick={() => void run(() => ipc.setApiKey(provider.id, ""), `המפתח של ${ai} נמחק. בלי מפתח התוכנה עובדת במצב הדגמה.`)}>מחיקה</button>
                      )}
                    </div>
                    {provider.retention && (
                      <label className="row">
                        <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />
                        <span>יש לי הסכם ZDR בכתב עם {provider.company} לחשבון הזה. בלעדיו לא יישלח אליה דבר.</span>
                      </label>
                    )}
                  </div>
                )}
                <Row label="דגם" htmlFor="model" hint={provider.id === "anthropic" ? "רק דגמים שבהסכם ה-ZDR." : undefined}>
                  <select id="model" className="select" value={status.model}
                    onChange={(e) => void run(() => ipc.setModel(e.target.value), "הדגם עודכן.")}>
                    {provider.models.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                  </select>
                </Row>
                <Row label="מהירות התשובות" htmlFor="speed" hint="משפיע על הניסוח ועל ההתייעצות.">
                  <select id="speed" className="select" value={status.speed}
                    onChange={(e) => void run(() => ipc.setSpeed(e.target.value as "fast" | "balanced" | "thorough"), "המהירות עודכנה.")}>
                    <option value="fast">מהיר</option>
                    <option value="balanced">מאוזן (מומלץ)</option>
                    <option value="thorough">יסודי</option>
                  </select>
                </Row>
              </section>
              <UsageSettings />
              <VerifySettings />
              <ReportCheckSettings />
            </>
          )}

          {pane === "privacy" && (
            <>
              <PrivacyStatus />
              <section className="card setting" aria-labelledby="s-privacy">
                <h2 id="s-privacy">לפני כל שליחה</h2>
                <Switch id="review-always" label="להציג לי כל בקשה לפני שליחה" checked={status.review_always}
                  hint={status.review_always
                    ? "דולק: כל בקשה נעצרת במסך \"לפני ששולחים\" עד שתאשרי."
                    : "כבוי: התוכנה מסתירה ושולחת לבד. שמות ופרטים מזהים מוסתרים בעותק שיוצא בכל מקרה, ומה שיצא נשמר ברשימה למעלה."}
                  onChange={(on) => void run(() => ipc.setReviewAlways(on), on ? "כל בקשה תוצג לפני שליחה." : "בקשה שעברה את הסינון תישלח מיד.")} />
                <NameModelSettings />
                {SCREEN_PROTECTION_SETTING && (
                  <Switch id="screen-protection" label="להסתיר את התוכנה מצילומי מסך ומשיתוף מסך" checked={status.screen_protection}
                    onChange={(on) => void run(() => ipc.setScreenProtection(on), on ? "ההגנה מצילום מסך הודלקה." : "ההגנה מצילום מסך כובתה עד הנעילה הבאה של הכספת.")} />
                )}
              </section>
              <section className="card setting" aria-labelledby="s-lock">
                <h2 id="s-lock">נעילה</h2>
                <Row label="נעילה אוטומטית אחרי" hint="דקות בלי פעילות. Ctrl+L נועל מיד.">
                  <Segmented label="נעילה אוטומטית אחרי" value={status.lock_minutes}
                    options={Array.from(new Set([5, 10, 15, 30, status.lock_minutes])).sort((a, b) => a - b).map((m) => ({ value: m, label: String(m) }))}
                    onChange={(m) => void run(() => ipc.setLockMinutes(m), `הכספת תינעל אחרי ${m} דקות בלי פעילות.`)} />
                </Row>
                <span className="hint">כל הגדרה נשמרת מיד. מה שלא כתוב כאן, התוכנה בוחרת את האפשרות המחמירה.</span>
              </section>
            </>
          )}

          {pane === "report" && !report && <ErrorLine error={reportError} />}
          {pane === "report" && report && (
            <>
              <section className="card setting" aria-labelledby="s-report">
                <h2 id="s-report">פרטי הדוח</h2>
                <div className="field fit-l">
                  <label htmlFor="r-title">כותרת</label>
                  <input id="r-title" className="input" value={report.title} onChange={(e) => setReport({ ...report, title: e.target.value })} />
                </div>
                <Row label="גופן" htmlFor="r-font">
                  <select id="r-font" className="select" value={report.font} onChange={(e) => setReport({ ...report, font: e.target.value })}>
                    {["David", "Arial", "Narkisim", "Frank Ruehl", "Times New Roman"].map((f) => <option key={f} value={f}>{f}</option>)}
                  </select>
                </Row>
                <div className="field fit-l">
                  <label htmlFor="r-conf">שורת חיסיון בראש כל עמוד</label>
                  <input id="r-conf" className="input" value={report.confidentiality} onChange={(e) => setReport({ ...report, confidentiality: e.target.value })} />
                </div>
                <div className="field">
                  <label htmlFor="r-sig">חתימה</label>
                  <span className="hint">שורה לכל פרט: שם, תואר, מספר רישיון.</span>
                  <textarea id="r-sig" className="textarea" rows={3} value={report.signature.join("\n")}
                    onChange={(e) => setReport({ ...report, signature: e.target.value.split("\n") })} />
                </div>
                <div className="field">
                  <label htmlFor="r-sup">חתימת המדריכה</label>
                  <span className="hint">אם יש: מודפסת מתחת לחתימה שלך, שורה לכל פרט.</span>
                  <textarea id="r-sup" className="textarea" rows={2} value={report.supervisor.join("\n")}
                    onChange={(e) => setReport({ ...report, supervisor: e.target.value.split("\n") })} />
                </div>
                <div className="field">
                  <label htmlFor="r-parents">משפט הפתיחה בגרסה להורים</label>
                  <span className="hint">נכתב פעם אחת, ופותח כל גרסה להורים.</span>
                  <textarea id="r-parents" className="textarea" rows={2} value={report.parents_opening}
                    onChange={(e) => setReport({ ...report, parents_opening: e.target.value })} />
                </div>
                <button type="button" className="btn btn-primary align-start"
                  onClick={() => void run(() => ipc.setReportSettings({ ...report, signature: report.signature.filter((l) => l.trim()), supervisor: report.supervisor.filter((l) => l.trim()) }), "הגדרות הדוח נשמרו.")}>שמירה</button>
              </section>
              <SectionModesSettings />
              <section className="card setting" aria-labelledby="s-template">
                <h2 id="s-template">התבנית שלי (Word)</h2>
                <span className="hint">
                  {template
                    ? `הדוח נכתב לתוך התבנית שלך${template.images > 0 ? ", עם הלוגו" : ""}. ${template.has_marker ? "הוא נכנס במקום שכתוב {{הדוח}}." : "הכותרות העליונה והתחתונה נשמרות."}`
                    : "קובץ Word עם נייר המכתבים, הלוגו והגופנים שלך. כדי לשמור טקסט קבוע, כותבים {{הדוח}} במקום שבו הדוח נכנס."}
                </span>
                <div className="row">
                  <label className="btn btn-small">
                    {template ? "החלפת התבנית" : "בחירת תבנית"}
                    <input type="file" accept=".docx,.dotx" hidden
                      onChange={(e) => {
                        const f = e.target.files?.[0];
                        e.target.value = "";
                        if (f) void run(async () => setTemplate(await ipc.setReportTemplate(f)), "התבנית נשמרה. הדוח הבא ייכתב לתוכה.");
                      }} />
                  </label>
                  {template && (
                    <button type="button" className="btn btn-small"
                      onClick={() => void run(async () => { await ipc.clearReportTemplate(); setTemplate(null); }, "חזרה לעיצוב הרגיל של הדוח.")}>בלי תבנית</button>
                  )}
                </div>
              </section>
              <section className="card setting" aria-labelledby="s-logo">
                <h2 id="s-logo">לוגו</h2>
                <span className="hint">
                  {logo
                    ? `${logo.format}, ${logo.width}×${logo.height}, ${logo.size_kb >= 1000 ? `${(logo.size_kb / 1024).toFixed(1)}MB` : `${logo.size_kb}KB`}. מודפס בראש העמוד הראשון של הדוח${template ? " (ב-PDF; ב-Word נשאר הלוגו של התבנית)" : ""}.`
                    : "תמונת PNG או JPEG, עד 1MB. נשמרת מוצפנת בכספת, ומודפסת בראש העמוד הראשון של הדוח."}
                </span>
                <div className="row">
                  <label className="btn btn-small">
                    {logo ? "החלפת הלוגו" : "בחירת לוגו"}
                    <input type="file" accept=".png,.jpg,.jpeg,image/png,image/jpeg" hidden
                      onChange={(e) => {
                        const f = e.target.files?.[0];
                        e.target.value = "";
                        if (f) void run(async () => setLogo(await ipc.setReportLogo(f)), "הלוגו נשמר. הוא יודפס בדוח הבא.");
                      }} />
                  </label>
                  {logo && (
                    <button type="button" className="btn btn-small"
                      onClick={() => void run(async () => { await ipc.clearReportLogo(); setLogo(null); }, "הלוגו הוסר מהדוחות.")}>בלי לוגו</button>
                  )}
                </div>
              </section>
              <section className="card setting" aria-labelledby="s-original">
                <h2 id="s-original">בדיקת מקוריות של קובץ</h2>
                <Row label="האם הקובץ שונה מאז שהופק?" hint="לקובץ PDF או Word שהופק כאן, דוח או מכתב.">
                  <label className="btn btn-small">
                    בחירת קובץ
                    <input type="file" accept=".pdf,.docx" hidden
                      onChange={(e) => {
                        const f = e.target.files?.[0];
                        e.target.value = "";
                        if (f) void run(async () => {
                          const at = await ipc.checkOriginal(f);
                          setOriginal(at === null ? "הקובץ הזה לא הופק בכספת הזו, או ששונה אחרי שהופק." : `זה הקובץ המקורי: הופק ב-${new Date(at * 1000).toLocaleString("he-IL")}, ולא שונה מאז.`);
                        }, "הבדיקה הסתיימה.");
                      }} />
                  </label>
                </Row>
                {original && <span className="hint" role="status">{original}</span>}
              </section>
            </>
          )}

          {pane === "tests" && (
            <>
              <InstrumentSettings />
              <CriticalValuesSettings />
            </>
          )}

          {/* One form for the clinic's four parts: it stays open while she moves between them. */}
          {clinicGroup && (
            <Suspense fallback={<Skeleton card lines={4} label="טוען את הגדרות הקליניקה" />}>
              <ClinicSettingsTab group={clinicGroup} />
            </Suspense>
          )}

          {pane === "backup" && (
            <>
              <BackupSettings />
              <DriveBackupSettings />
            </>
          )}

          {pane === "security" && (
            <>
              <SafetyNet />
              <SecuritySettings />
              <PasswordSettings />
            </>
          )}

          {pane === "about" && (
            <>
              <UpdateSettings />
              <ActivitySettings />
              <SystemCheckSettings />
              <BugReportSettings />
            </>
          )}
        </div>
      </main>
    </div>
  );
}
