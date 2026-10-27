import { useState } from "react";
import { ActionsMenu, type MenuItem } from "../../components/Menu";
import { UploadIcon } from "../../components/ui";
import type { StyleOverview, StyleSourceView } from "../../ipc/client";
import { dateOf, sinceLabel } from "./labels";

// הסגנון שלי · my reports

export function Reports(props: {
  o: StyleOverview;
  adding: boolean;
  onAdd: () => void;
  onDelete: (s: StyleSourceView) => void;
  more: MenuItem[];
}) {
  const { o } = props;
  const [list, setList] = useState(false);
  const last = Math.max(...o.sources.map((s) => s.added_at));
  const notRead = o.sources.filter((s) => !s.analyzed).length;
  return (
    <section className="card style-reports" aria-labelledby="style-sources">
      <div className="style-reports-row">
        <h2 id="style-sources">הדוחות שלי</h2>
        <span className="muted grow">
          {o.sources.length === 1 ? "דוח אחד" : `${o.sources.length} דוחות`}, האחרון נוסף {sinceLabel(last)}.
          {notRead > 0 && ` ${notRead === 1 ? "אחד עוד לא נותח" : `${notRead} עוד לא נותחו`}.`}
          {" "}נשמר רק הטקסט אחרי הסתרה, לא הקובץ.
        </span>
        <div className="style-reports-actions">
          <button type="button" className="btn btn-ghost" aria-expanded={list} aria-controls="style-source-list" onClick={() => setList((v) => !v)}>
            {list ? "להסתיר את הרשימה" : "לראות את הרשימה"}
          </button>
          <button type="button" className="btn" disabled={props.adding} onClick={props.onAdd}><UploadIcon /> הוספת דוח</button>
          {props.more.length > 0 && <ActionsMenu label="עוד פעולות על הסגנון" items={props.more} />}
        </div>
      </div>
      {list && (
        <ul id="style-source-list" className="style-sources">
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
                  : (
                    <span className="style-source-learned">
                      <span className="chip chip-ok">נותח · {s.analysis_items - s.analysis_inferences} פריטי סגנון</span>
                      {s.analysis_inferences > 0 && <span className="chip chip-ok">{s.analysis_inferences === 1 ? "כלל הסקה אחד" : `${s.analysis_inferences} כללי הסקה`}</span>}
                    </span>
                  )
                : <span className="chip chip-warn">עוד לא נותח</span>}
              <button type="button" className="btn btn-small btn-danger-quiet" onClick={() => props.onDelete(s)}>מחיקה</button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
