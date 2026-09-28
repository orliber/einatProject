import { useEffect, useState } from "react";
import { useApp } from "../App";
import { TopBar } from "../components/TopBar";
import { NewCaseDialog } from "../components/NewCaseDialog";
import { ErrorLine } from "../components/ui";
import { greeting } from "../i18n/he";
import { ipc, type CaseSummary } from "../ipc/client";
import "./CasesScreen.css";

const TOTAL_SECTIONS = 16;

function stage(c: CaseSummary): { label: string; cls: string } {
  const n = c.approved_sections.length;
  if (n >= TOTAL_SECTIONS) return { label: "מוכן להפקה", cls: "chip chip-ok" };
  if (n > 0) return { label: "בכתיבה", cls: "chip chip-warn" };
  return { label: "איסוף חומרים", cls: "chip" };
}

function updated(ts: number): string {
  const d = new Date(ts * 1000);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) {
    return `היום, ${d.toLocaleTimeString("he-IL", { hour: "2-digit", minute: "2-digit" })}`;
  }
  return d.toLocaleDateString("he-IL", { day: "numeric", month: "numeric" });
}

export function CasesScreen() {
  const { go, fail, status } = useApp();
  const [cases, setCases] = useState<CaseSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    let alive = true;
    ipc
      .listCases()
      .then((c) => alive && setCases(c))
      .catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [fail]);

  const name = status.practitioner[0]?.split(" ")[0];
  const inProgress = cases?.filter((c) => c.approved_sections.length < TOTAL_SECTIONS).length ?? 0;

  return (
    <div className="page">
      <TopBar active="cases" />
      <main className="page-main">
        <div className="page-head">
          <div className="stack" style={{ gap: 4 }}>
            <h1>{greeting()}{name ? `, ${name}` : ""}</h1>
            <p className="muted">{cases ? (cases.length ? `${inProgress} תיקים בעבודה` : "עוד אין תיקים") : " "}</p>
          </div>
          <button type="button" className="btn btn-primary btn-big" onClick={() => setCreating(true)}>+ תיק חדש</button>
        </div>
        {status.integrity_warning && (
          <p className="error" role="alert">בדיקת השלמות של הכספת מצאה חריגה: {status.integrity_warning}</p>
        )}
        <ErrorLine error={error} />

        {cases && cases.length === 0 && (
          <div className="card empty">
            <h2>התיק הראשון</h2>
            <p className="muted">פותחים תיק, מוסיפים את החומרים שכבר יש (דוחות, אינטייק, סיכומי מפגשים וציונים), ו-Claude מציע טיוטה לכל סעיף בדוח.</p>
            <button type="button" className="btn btn-primary" onClick={() => setCreating(true)}>פתיחת תיק</button>
          </div>
        )}

        {cases && cases.length > 0 && (
          <div className="card table-card">
            <table className="cases">
              <thead>
                <tr>
                  <th>תיק</th>
                  <th>ילד/ה</th>
                  <th>גיל</th>
                  <th className="col-progress">הדוח</th>
                  <th>שלב</th>
                  <th>עודכן</th>
                </tr>
              </thead>
              <tbody>
                {cases.map((c) => {
                  const s = stage(c);
                  const pct = Math.round((c.approved_sections.length / TOTAL_SECTIONS) * 100);
                  return (
                    <tr key={c.id}>
                      <td>
                        <button type="button" className="case-link" onClick={() => go({ name: "case", id: c.id, view: "materials" })}>
                          {c.meta.code || "ללא קוד"}
                        </button>
                      </td>
                      <td className="serif case-name">{c.child_name ?? "—"}</td>
                      <td className="num">{c.meta.age ? `${c.meta.age.years}:${c.meta.age.months}` : "—"}</td>
                      <td>
                        <div className="progress-row">
                          <div className="bar" role="progressbar" aria-valuemin={0} aria-valuemax={TOTAL_SECTIONS} aria-valuenow={c.approved_sections.length}>
                            <div className={pct >= 100 ? "bar-fill bar-done" : "bar-fill"} style={{ width: `${pct}%` }} />
                          </div>
                          <span className="small muted num">{c.approved_sections.length}/{TOTAL_SECTIONS}</span>
                        </div>
                      </td>
                      <td><span className={s.cls}>{s.label}</span></td>
                      <td className="muted">{updated(c.updated_at)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <p className="small muted">השמות מוצגים רק כאן, במחשב שלך. Claude מקבל תמיד תפקידים ("הילד", "הגננת") במקום שמות.</p>
      </main>
      {creating && (
        <NewCaseDialog onClose={() => setCreating(false)} onCreated={(id) => go({ name: "case", id, view: "materials" })} />
      )}
    </div>
  );
}
