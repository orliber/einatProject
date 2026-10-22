import { useEffect, useState, type ReactNode } from "react";
import { BugReportSettings } from "../components/BugReport";
import { useApp } from "../App";
import { ActivitySettings } from "../components/ActivityLog";
import { BackupSettings } from "../components/Backup";
import { PasswordSettings } from "../components/PasswordSettings";
import { TopBar } from "../components/TopBar";
import { UpdateSettings } from "../components/Update";
import { UsageSettings } from "../components/UsageSettings";
import { VerifySettings } from "../components/VerifySettings";
import { ReportCheckSettings } from "../components/ReportCheck";
import { NameModelSettings } from "../components/NameModelSettings";
import { ReadinessSettings } from "../components/ReadinessSettings";
import { InstrumentSettings } from "../components/InstrumentSettings";
import { SecuritySettings } from "../components/SecuritySettings";
import { PrivacyStatus } from "../components/PrivacyStatus";
import { Segmented, Switch } from "../components/Switch";
import { ErrorLine } from "../components/ui";
import { ipc, type ReportSettings, type TemplateView } from "../ipc/client";
import { PROVIDERS, providerOf } from "../ai";

import { TEXT_SIZES, applyTextSize, readTextSize } from "../textSize";
import { SectionModesSettings } from "../components/SectionModes";
import { SETTINGS_PANES, type SettingsPane } from "./settingsPanes";
import "./SettingsScreen.css";

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
  const [pane, setPane] = useState<SettingsPane>(initial ?? "general");
  const current = SETTINGS_PANES.find((p) => p.id === pane);
  const [apiKey, setApiKey] = useState("");
  const [ack, setAck] = useState(false);
  const provider = providerOf(status.model);
  const ai = provider.name;
  const hasKey = status.keys.includes(provider.id);
  const [names, setNames] = useState(status.practitioner.join(", "));
  const [report, setReport] = useState<ReportSettings | null>(null);
  const [template, setTemplate] = useState<TemplateView | null>(null);
  const [original, setOriginal] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [textSize, setTextSize] = useState(readTextSize);

  useEffect(() => {
    ipc.reportSettings().then(setReport).catch((e) => setError(fail(e as never)));
    ipc.reportTemplate().then(setTemplate).catch(() => setTemplate(null));
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
          {SETTINGS_PANES.map((p) => (
            <button key={p.id} type="button" className="settings-nav-btn" aria-current={p.id === pane ? "page" : undefined}
              onClick={() => { setError(null); setPane(p.id); }}>{p.label}</button>
          ))}
        </nav>

        <div className="settings-pane">
          <h2 className="settings-pane-title">{current?.label}</h2>
          <ErrorLine error={error} />

          {pane === "general" && (
            <>
              <ReadinessSettings />
              <section className="card setting" aria-labelledby="s-general">
                <h2 id="s-general">תצוגה ופרטים</h2>
                <Row label="גודל הטקסט" htmlFor="text-size" hint="גם Ctrl ו-+ או −">
                  <select id="text-size" className="select" value={textSize}
                    onChange={(e) => { const n = Number(e.target.value); applyTextSize(n); setTextSize(n); }}>
                    {TEXT_SIZES.map((n) => <option key={n} value={n}>{n === 100 ? "רגיל" : `${n}%`}</option>)}
                  </select>
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

          {pane === "report" && report && (
            <>
              <section className="card setting" aria-labelledby="s-report">
                <h2 id="s-report">פרטי הדוח</h2>
                <div className="field">
                  <label htmlFor="r-title">כותרת</label>
                  <input id="r-title" className="input" value={report.title} onChange={(e) => setReport({ ...report, title: e.target.value })} />
                </div>
                <Row label="גופן" htmlFor="r-font">
                  <select id="r-font" className="select" value={report.font} onChange={(e) => setReport({ ...report, font: e.target.value })}>
                    {["David", "Arial", "Narkisim", "Frank Ruehl", "Times New Roman"].map((f) => <option key={f} value={f}>{f}</option>)}
                  </select>
                </Row>
                <div className="field">
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
              <section className="card setting" aria-labelledby="s-original">
                <h2 id="s-original">בדיקת מקוריות של PDF</h2>
                <Row label="האם הקובץ שונה מאז שהופק?" hint="לקובץ PDF שהופק כאן.">
                  <label className="btn btn-small">
                    בחירת PDF
                    <input type="file" accept=".pdf" hidden
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

          {pane === "tests" && <InstrumentSettings />}

          {pane === "backup" && <BackupSettings />}

          {pane === "security" && (
            <>
              <SecuritySettings />
              <PasswordSettings />
            </>
          )}

          {pane === "about" && (
            <>
              <UpdateSettings />
              <ActivitySettings />
              <BugReportSettings />
            </>
          )}
        </div>
      </main>
    </div>
  );
}
