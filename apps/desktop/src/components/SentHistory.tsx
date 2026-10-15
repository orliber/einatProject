// "מה יצא מהמחשב" (round 27, D-062): every request that carried a case's text, when, for what,
// what was hidden in it, and the exact text that left. The check after the fact that stands in
// for showing each request first. Everything is plain text; nothing here sends anything.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import structure from "../../../../templates/report_structure.json";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import type { SentItem } from "../ipc/generated/SentItem";
import { heDateTime } from "./Backup";
import { Dialog, displayTag, ErrorLine, Spinner } from "./ui";
import "./SentHistory.css";

const SECTION_TITLES = new Map(structure.parts.flatMap((p) => p.sections.map((s) => [s.key, s.title] as const)));

/** What a request was for, in her words. */
export function placeLabel(place: string): string {
  if (place === "sorting") return "מיון החומרים לסעיפים";
  if (place === "consult") return "שאלה מתוך התיק";
  if (place === "letter_parents") return "מכתב להורים";
  if (place === "letter_school") return "מכתב לבית הספר";
  return SECTION_TITLES.get(place) ?? place;
}

function hiddenLine(item: SentItem): string {
  const n = item.hidden.length;
  return n === 0 ? "לא הוסתר דבר" : n === 1 ? "הוסתר פרט אחד" : `הוסתרו ${n} פרטים`;
}

/** One request, opened: what was hidden, the text as it left, and its hash. */
function SentDetail({ item }: { item: SentItem }) {
  const { fail } = useApp();
  const full = useQuery({ queryKey: ["sent-text", item.case_id, item.id], queryFn: () => ipc.sentText(item.case_id, item.id) });
  if (full.error) return <ErrorLine error={fail(full.error as never)} />;
  if (!full.data) return <Spinner />;
  const t = full.data;
  return (
    <div className="sent-detail stack">
      {t.item.hidden.length > 0 && (
        <div className="stack" style={{ gap: 4 }}>
          <b className="small">מה הוסתר (נשאר במחשב)</b>
          <ul className="sent-hidden">
            {t.item.hidden.map((h) => (
              <li key={h.tag}>
                <b>{h.word || "פרט מזהה"}</b>
                <span aria-hidden="true"> ← </span>
                <span className="visually-hidden"> יצא כ: </span>
                <span className="mark-role">{displayTag(h.tag)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {t.parts.map((p, i) => (
        <section key={i} className="stack" style={{ gap: 4 }} aria-label={p.label}>
          <b className="small">{p.label}</b>
          <pre className="sent-text serif">{p.text}</pre>
        </section>
      ))}
      {t.hash_ok ? (
        <p className="small muted">✓ הטקסט השמור זהה בדיוק למה שיצא (טביעת SHA-256: <span dir="ltr" className="sent-hash">{t.item.sha256}</span>).</p>
      ) : (
        <ErrorLine error="הטקסט השמור של הבקשה הזו לא תואם את הטביעה שנרשמה כשיצאה. כדאי לפנות לאור." report />
      )}
      <details>
        <summary className="small">הבקשה בדיוק כפי שיצאה (JSON)</summary>
        <pre className="sent-text sent-exact" dir="ltr">{t.exact}</pre>
      </details>
    </div>
  );
}

/** The history: one case (optionally one section of it), or every case. */
export function SentHistoryDialog(props: { caseId?: string; place?: string; onClose: () => void }) {
  const { fail } = useApp();
  const [place, setPlace] = useState(props.place);
  const [typed, setTyped] = useState("");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const caseId = props.caseId ?? null;
  const list = useQuery({
    queryKey: ["sent", caseId, place ?? null, query],
    queryFn: () => ipc.sentHistory(caseId, place ?? null, query),
  });
  const items = list.data ?? [];
  const where = place ? `"${placeLabel(place)}"` : caseId ? "בתיק הזה" : "בכל התיקים";
  return (
    <Dialog wide title="מה יצא מהמחשב" onClose={props.onClose}
      subtitle={`כל בקשה ${where}, מהחדשה לישנה: מתי, בשביל מה, מה הוסתר בה, והטקסט המדויק שיצא.`}
      footer={<>
        <span className="muted small grow">נשמר מוצפן בכספת, עם התיק.</span>
        {place && caseId && <button type="button" className="btn" onClick={() => setPlace(undefined)}>כל התיק</button>}
        <button type="button" className="btn" onClick={props.onClose}>סגירה</button>
      </>}>
      <div className="stack">
        <form className="row" role="search" onSubmit={(e) => { e.preventDefault(); setQuery(typed.trim()); }}>
          <label className="visually-hidden" htmlFor="sent-q">חיפוש בבקשות</label>
          <input id="sent-q" className="input grow" value={typed} placeholder="חיפוש מילה בבקשות…" onChange={(e) => setTyped(e.target.value)} />
          <button type="submit" className="btn">חיפוש</button>
          {query && <button type="button" className="btn btn-ghost" onClick={() => { setTyped(""); setQuery(""); }}>ניקוי</button>}
        </form>
        <ol className="sent-list" aria-label="בקשות, מהחדשה לישנה">
          {items.map((item) => (
            <li key={item.id}>
              <div className="sent-row">
                <time className="muted small">{heDateTime(item.at)}</time>
                <span className="grow">{placeLabel(item.place)}</span>
                {!caseId && <span className="chip chip-sand">{item.case_code}</span>}
                <span className="small muted">{hiddenLine(item)}</span>
                <span className="small muted">{item.demo ? "הדגמה, לא יצא" : item.ai}</span>
                <button type="button" className="btn btn-small" aria-expanded={open === item.id}
                  onClick={() => setOpen(open === item.id ? null : item.id)}>
                  {open === item.id ? "הסתרה" : "הטקסט שיצא"}
                </button>
              </div>
              {item.hidden_match && <p className="small sent-match">"{query}" הוסתר בבקשה הזו: לא יצא מהמחשב.</p>}
              {open === item.id && <SentDetail item={item} />}
            </li>
          ))}
          {list.data && items.length === 0 && (
            <li className="muted">{query ? `"${query}" לא מופיע בשום בקשה שיצאה.` : "עוד לא יצאה שום בקשה."}</li>
          )}
        </ol>
        {list.isLoading && <Spinner />}
        <ErrorLine error={list.error ? fail(list.error as never) : null} />
      </div>
    </Dialog>
  );
}

/** Settings → פרטיות: the history of every case. */
export function SentHistorySettings() {
  const [open, setOpen] = useState(false);
  return (
    <section className="card setting" aria-labelledby="s-sent">
      <h2 id="s-sent">מה יצא מהמחשב</h2>
      <p className="muted small">כל בקשה שיצאה עם טקסט מתיק: מתי, בשביל מה, מה הוסתר בה, והטקסט המדויק. אפשר לחפש בה.</p>
      <button type="button" className="btn align-start" onClick={() => setOpen(true)}>פתיחת ההיסטוריה</button>
      {open && <SentHistoryDialog onClose={() => setOpen(false)} />}
    </section>
  );
}
