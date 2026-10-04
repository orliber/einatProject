import { useEffect, useState } from "react";
import { useApp } from "../App";
import { ActivitySettings } from "../components/ActivityLog";
import { BackupSettings } from "../components/Backup";
import { PasswordSettings } from "../components/PasswordSettings";
import { TopBar } from "../components/TopBar";
import { UpdateSettings } from "../components/Update";
import { UsageSettings } from "../components/UsageSettings";
import { ReadinessSettings } from "../components/ReadinessSettings";
import { ErrorLine } from "../components/ui";
import { ipc, type ReportSettings } from "../ipc/client";
import { PROVIDERS, providerOf } from "../ai";

import { TEXT_SIZES, applyTextSize, readTextSize } from "../textSize";
import "./SettingsScreen.css";

/** The screen-capture switch is hidden while that protection is off (D-039). */
const SCREEN_PROTECTION_SETTING = false;

const JUMPS: [string, string][] = [
  ["s-ready", "מוכנה לעבודה"],
  ["s-claude", "חיבור ל-AI"],
  ["s-usage", "שימוש ועלות"],
  ["s-privacy", "פרטיות ונעילה"],
  ["s-report", "הדוח"],
  ["s-backup", "גיבוי"],
  ["s-password", "סיסמה"],
  ["s-activity", "יומן"],
  ["s-update", "עדכונים"],
  ["s-sec", "מצב האבטחה"],
];

export function SettingsScreen() {
  const { status, refresh, notify, fail } = useApp();
  const [apiKey, setApiKey] = useState("");
  const [ack, setAck] = useState(false);
  const provider = providerOf(status.model);
  const ai = provider.name;
  const hasKey = status.keys.includes(provider.id);
  const [minutes, setMinutes] = useState(status.lock_minutes);
  const [names, setNames] = useState(status.practitioner.join(", "));
  const [report, setReport] = useState<ReportSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [textSize, setTextSize] = useState(readTextSize);

  useEffect(() => {
    ipc.reportSettings().then(setReport).catch((e) => setError(fail(e as never)));
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
        <h1 className="settings-title">הגדרות</h1>
        {/* A long page: one row of jumps to each part. */}
        <nav className="settings-jump" aria-label="חלקי ההגדרות">
          {JUMPS.map(([id, label]) => (
            <button key={id} type="button" className="settings-jump-btn"
              onClick={() => document.getElementById(id)?.closest("section")?.scrollIntoView({ block: "start", behavior: "smooth" })}>{label}</button>
          ))}
        </nav>
        <ErrorLine error={error} />

        <ReadinessSettings />

        <section className="card setting" aria-labelledby="s-claude">
          <h2 id="s-claude">חיבור ל-AI</h2>
          <div className="field">
            <label htmlFor="provider">עם איזה AI לעבוד</label>
            <select id="provider" className="select" value={provider.id}
              onChange={(e) => {
                const next = PROVIDERS.find((p) => p.id === e.target.value);
                const model = next?.models[0]?.[0];
                if (!next || !model) return;
                setApiKey("");
                setAck(false);
                void run(() => ipc.setModel(model), `מעכשיו עובדים עם ${next.name}.`);
              }}>
              {PROVIDERS.map((p) => <option key={p.id} value={p.id}>{p.name} ({p.company}){p.id === "anthropic" ? " · מומלץ" : ""}</option>)}
            </select>
            <span className="hint">בכל מקרה השמות והפרטים המזהים מוסתרים במחשב לפני השליחה, ומסך "מה יוצא מהמחשב" מראה בדיוק מה נשלח.</span>
          </div>
          {provider.setup && <p className="note-sand small" role="note">{provider.setup}</p>}
          {provider.keyPlaceholder !== null && (
            <>
              <p className="muted small">
                {status.demo_mode
                  ? `כרגע אין מפתח של ${ai}, ולכן התוכנה במצב הדגמה: התשובות נבנות במחשב, ושום דבר לא נשלח.`
                  : `מפתח API של ${ai} מוגדר ונשמר מוצפן בתוך הכספת. אפשר להחליף או למחוק.`}
                {provider.id === "anthropic" && status.demo_mode && " כדי לעבוד עם Claude מזינים מפתח API של חשבון עם הסכם אפס שמירת מידע (ZDR)."}
              </p>
              {provider.retention && (
                <div className="note-sand small" role="note">
                  <b>מה {provider.company} שומרת:</b> {provider.retention}
                </div>
              )}
              <div className="row">
                <label htmlFor="api" className="visually-hidden">מפתח API של {ai}</label>
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
                  <span>יש לי הסכם בכתב עם {provider.company} על אפס שמירת מידע (ZDR) לחשבון הזה. בלי הסכם כזה התוכנה לא תשלח אליה דבר.</span>
                </label>
              )}
            </>
          )}
          <div className="field">
            <label htmlFor="model">דגם</label>
            <select id="model" className="select" value={status.model}
              onChange={(e) => void run(() => ipc.setModel(e.target.value), "הדגם עודכן.")}>
              {provider.models.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
            {provider.id === "anthropic" && <span className="hint">רק דגמים שנכללים בהסכם אפס שמירת מידע.</span>}
          </div>
          <div className="field">
            <label htmlFor="speed">מהירות התשובות</label>
            <select id="speed" className="select" value={status.speed}
              onChange={(e) => void run(() => ipc.setSpeed(e.target.value as "fast" | "balanced" | "thorough"), "המהירות עודכנה.")}>
              <option value="fast">מהיר: תשובות תוך שניות, פחות עמוק</option>
              <option value="balanced">מאוזן (מומלץ)</option>
              <option value="thorough">יסודי: {ai} חושב יותר, לוקח יותר זמן</option>
            </select>
            <span className="hint">מיון החומרים תמיד מהיר. הבחירה משפיעה על ניסוח הסעיפים ועל ההתייעצות. גם דגם "מהיר יותר" מקצר את הזמן.</span>
          </div>
        </section>

        <UsageSettings />

        <section className="card setting" aria-labelledby="s-privacy">
          <h2 id="s-privacy">פרטיות ונעילה</h2>
          <div className="field">
            <label htmlFor="names">השמות שלך (יוחלפו תמיד ב"המאבחנת")</label>
            <div className="row">
              <input id="names" className="input grow" value={names} onChange={(e) => setNames(e.target.value)} />
              <button type="button" className="btn" onClick={() => void run(() => ipc.setPractitioner(names.split(",").map((n) => n.trim()).filter(Boolean)), "השמות נשמרו.")}>שמירה</button>
            </div>
          </div>
          <div className="field">
            <label htmlFor="lock">נעילה אוטומטית אחרי (דקות ללא פעילות)</label>
            <div className="row">
              <input id="lock" className="input narrow" type="number" min={1} max={60} value={minutes} onChange={(e) => setMinutes(Number(e.target.value))} />
              <button type="button" className="btn" onClick={() => void run(() => ipc.setLockMinutes(minutes), "זמן הנעילה עודכן.")}>שמירה</button>
            </div>
          </div>
          <div className="field">
            <label htmlFor="text-size">גודל הטקסט בתוכנה</label>
            <select id="text-size" className="select" value={textSize}
              onChange={(e) => { const n = Number(e.target.value); applyTextSize(n); setTextSize(n); }}>
              {TEXT_SIZES.map((n) => <option key={n} value={n}>{n === 100 ? "רגיל (100%)" : `${n}%`}</option>)}
            </select>
            <span className="hint">גם במקלדת: Ctrl ו-+ להגדלה, Ctrl ו-− להקטנה, Ctrl ו-0 לחזרה לרגיל.</span>
          </div>
          <label className="row">
            <input type="checkbox" checked={status.review_only_suspect} disabled={!status.review_choice_available}
              onChange={(e) => void run(() => ipc.setReviewOnlySuspect(e.target.checked), "ההגדרה עודכנה.")} />
            <span>להציג את מסך "מה יוצא מהמחשב" רק כשמשהו הוסתר אוטומטית</span>
          </label>
          {!status.review_choice_available && <span className="hint">בשבועיים הראשונים המסך מוצג לפני כל שליחה, כדי להכיר את הסינון.</span>}
          {SCREEN_PROTECTION_SETTING && (
            <>
              <label className="row">
                <input type="checkbox" checked={status.screen_protection}
                  onChange={(e) => void run(() => ipc.setScreenProtection(e.target.checked), e.target.checked ? "ההגנה מצילום מסך הודלקה." : "ההגנה מצילום מסך כובתה עד הנעילה הבאה של הכספת.")} />
                <span>להסתיר את התוכנה מצילומי מסך ומשיתוף מסך (Zoom, Teams)</span>
              </label>
              <span className="hint">
                {status.screen_protection
                  ? "מומלץ להשאיר דלוק: מי שמצלם או משתף מסך רואה חלון ריק."
                  : "כבוי: אפשר לצלם ולשתף את המסך. מסך הנעילה תמיד מוגן, וכדאי להדליק שוב כשמסיימים."}
              </span>
            </>
          )}
        </section>

        {report && (
          <section className="card setting" aria-labelledby="s-report">
            <h2 id="s-report">הדוח</h2>
            <div className="field">
              <label htmlFor="r-title">כותרת הדוח</label>
              <input id="r-title" className="input" value={report.title} onChange={(e) => setReport({ ...report, title: e.target.value })} />
            </div>
            <div className="field">
              <label htmlFor="r-font">גופן</label>
              <select id="r-font" className="select" value={report.font} onChange={(e) => setReport({ ...report, font: e.target.value })}>
                {["David", "Arial", "Narkisim", "Frank Ruehl", "Times New Roman"].map((f) => <option key={f} value={f}>{f}</option>)}
              </select>
            </div>
            <div className="field">
              <label htmlFor="r-conf">שורת חיסיון בראש כל עמוד</label>
              <input id="r-conf" className="input" value={report.confidentiality} onChange={(e) => setReport({ ...report, confidentiality: e.target.value })} />
            </div>
            <div className="field">
              <label htmlFor="r-sig">חתימה (שורה לכל פרט: שם, תואר, מספר רישיון)</label>
              <textarea id="r-sig" className="textarea" rows={3} value={report.signature.join("\n")}
                onChange={(e) => setReport({ ...report, signature: e.target.value.split("\n") })} />
            </div>
            <button type="button" className="btn btn-primary align-start"
              onClick={() => void run(() => ipc.setReportSettings({ ...report, signature: report.signature.filter((l) => l.trim()) }), "הגדרות הדוח נשמרו.")}>שמירה</button>
          </section>
        )}

        <BackupSettings />

        <PasswordSettings />

        <ActivitySettings />

        <UpdateSettings />

        <section className="card setting" aria-labelledby="s-sec">
          <h2 id="s-sec">מצב האבטחה</h2>
          <ul className="sec-list">
            <li><span className="chip chip-ok">פעיל</span> הכספת מוצפנת (AES-256), ומפתח נפרד לכל תיק</li>
            <li><span className={status.fips_active ? "chip chip-ok" : "chip chip-sand"}>{status.fips_active ? "פעיל" : "רגיל"}</span> מודול הצפנה {status.fips_active ? "מאושר FIPS 140-3" : "סטנדרטי"}</li>
            <li>
              <span className={status.disk_encryption === "on" ? "chip chip-ok" : "chip chip-warn"}>{status.disk_encryption === "on" ? "פעיל" : status.disk_encryption === "off" ? "כבוי" : "לא ידוע"}</span>
              הצפנת הדיסק של המחשב (BitLocker / FileVault)
              {status.disk_encryption !== "on" && <span className="hint">מומלץ להפעיל: כך גם קבצים אחרים במחשב מוגנים אם הוא נגנב.</span>}
            </li>
          </ul>
        </section>
      </main>
    </div>
  );
}
