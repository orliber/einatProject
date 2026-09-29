import { useEffect, useState } from "react";
import { useApp } from "../App";
import { TopBar } from "../components/TopBar";
import { NewCaseDialog } from "../components/NewCaseDialog";
import { ageWords } from "../components/AgeField";
import { ErrorLine } from "../components/ui";
import { greeting } from "../i18n/he";
import { ipc, type CaseSummary } from "../ipc/client";
import "./CasesScreen.css";

const TOTAL_SECTIONS = 16;

/** What to do next in a case, in one chip and one line (canvas "תיקים"). */
function nextStep(c: CaseSummary): { chip: string; cls: string; text: string; start: boolean } {
  const n = c.approved_sections.length;
  if (!c.meta.consent) return { chip: "הסכמה", cls: "chip chip-warn", text: "לרשום את הסכמת ההורים", start: false };
  if (n >= TOTAL_SECTIONS) return { chip: "✓ מוכן", cls: "chip chip-ok", text: "להפיק דוח Word", start: false };
  if (n > 0) return { chip: "בכתיבה", cls: "chip", text: `לאשר עוד ${TOTAL_SECTIONS - n} סעיפים`, start: false };
  return { chip: "חומרים", cls: "chip chip-empty", text: "להוסיף חומרים ולהכין טיוטה", start: true };
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

  // "ד\"ר רותם בדויה" → "רותם": a title is not how anyone is greeted.
  const name = status.practitioner[0]?.split(" ").find((w) => w && !/^(ד["״']?ר|דר'|פרופ'|גב'|מר)$/.test(w));
  const inProgress = cases?.filter((c) => c.approved_sections.length < TOTAL_SECTIONS).length ?? 0;

  return (
    <div className="page">
      <TopBar active="cases" />
      <main className="page-main">
        <div className="page-head">
          <div className="stack" style={{ gap: 4 }}>
            <h1>{greeting()}{name ? `, ${name}` : ""}</h1>
            <p className="muted">{cases ? (cases.length ? `${inProgress} תיקים בעבודה.` : "עוד אין תיקים") : " "}</p>
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
                  <th>ילד/ה</th>
                  <th>הצעד הבא</th>
                  <th className="col-progress">הדוח</th>
                  <th className="col-go"><span className="visually-hidden">פתיחה</span></th>
                </tr>
              </thead>
              <tbody>
                {cases.map((c) => {
                  const s = nextStep(c);
                  const pct = Math.round((c.approved_sections.length / TOTAL_SECTIONS) * 100);
                  const open = () => go({ name: "case", id: c.id, view: "materials" });
                  return (
                    <tr key={c.id} onClick={open} className="case-row">
                      <td>
                        <div className="case-name serif">{c.child_name ?? "ללא שם"}</div>
                        <div className="small muted">
                          {c.meta.age ? `${ageWords(c.meta.age)} · ` : ""}{c.meta.code || "ללא קוד"} · עודכן {updated(c.updated_at)}
                        </div>
                      </td>
                      <td>
                        <span className="next-cell"><span className={s.cls}>{s.chip}</span><span>{s.text}</span></span>
                      </td>
                      <td>
                        <div className="progress-row">
                          <div className="bar" role="progressbar" aria-label="סעיפים שאושרו" aria-valuemin={0} aria-valuemax={TOTAL_SECTIONS} aria-valuenow={c.approved_sections.length}>
                            <div className={pct >= 100 ? "bar-fill bar-done" : "bar-fill"} style={{ width: `${pct}%` }} />
                          </div>
                          <span className="small muted num">{c.approved_sections.length} מתוך {TOTAL_SECTIONS}</span>
                        </div>
                      </td>
                      <td>
                        <button type="button" className={s.start ? "btn" : "btn btn-primary"}
                          onClick={(e) => { e.stopPropagation(); open(); }}>
                          {s.start ? "פתיחה" : "להמשיך"}<span className="visually-hidden"> בתיק של {c.child_name ?? c.meta.code}</span>
                        </button>
                      </td>
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
