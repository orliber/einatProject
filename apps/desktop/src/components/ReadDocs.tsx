// Documents opened to read (D-212): one of her past reports ("הדוחות שלי") as the vault keeps
// it, and, beside the report she is writing, the case's materials and her reports in one list.
// Everything here is shown on her computer only and as plain text; nothing is sent.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useApp } from "../App";
import { kindLabel } from "../i18n/he";
import { ipc, type CaseInput } from "../ipc/client";
import { Dialog, ErrorLine } from "./ui";
import "./ReadDocs.css";

/** The text of one of her reports: the parts she kept, each under its heading. */
function ReportText({ id }: { id: string }) {
  const { fail } = useApp();
  const text = useQuery({ queryKey: ["styleSourceText", id], queryFn: () => ipc.styleSourceText(id) });
  if (text.isError) return <ErrorLine error={fail(text.error as never)} />;
  if (!text.data) return <p className="muted small" role="status">פותחת את הדוח…</p>;
  return (
    <div className="read-doc">
      <p className="small muted">הדוח כפי שהוא שמור: אחרי הסתרה, עם תפקידים במקום שמות ובלי מספרים. הקובץ עצמו לא נשמר.</p>
      {text.data.parts.map((p, i) => (
        <section key={i} className="read-doc-part">
          <h3>{p.heading}</h3>
          <div className="read-doc-text serif">{p.text}</div>
        </section>
      ))}
    </div>
  );
}

/** One of her reports, from the list on "הסגנון שלי". */
export function MyReportDialog({ id, title, onClose }: { id: string; title: string; onClose: () => void }) {
  return (
    <Dialog wide title={title} subtitle="מהדוחות שלי" onClose={onClose}>
      <ReportText id={id} />
    </Dialog>
  );
}

type Open = { type: "material"; id: string } | { type: "report"; id: string; title: string };

/** Beside the report she is writing: the case's materials and her own reports, to open and
 *  read. `first`: a material to open straight away (its name was pressed in a section). */
export function ReadDocsDialog({ inputs, first, onClose }: { inputs: CaseInput[]; first?: string | undefined; onClose: () => void }) {
  const { fail } = useApp();
  const [open, setOpen] = useState<Open | null>(first ? { type: "material", id: first } : null);
  const reports = useQuery({ queryKey: ["style"], queryFn: ipc.styleOverview });
  const material = open?.type === "material" ? inputs.find((i) => i.id === open.id) : undefined;
  const back = <button type="button" className="link-btn read-doc-back" onClick={() => setOpen(null)}>→ לכל המסמכים</button>;

  if (open?.type === "report") {
    return (
      <Dialog wide title={open.title} subtitle="מהדוחות שלי" onClose={onClose}>
        {back}
        <ReportText id={open.id} />
      </Dialog>
    );
  }
  if (material) {
    return (
      <Dialog wide title={material.title || kindLabel[material.kind]} subtitle={kindLabel[material.kind]} onClose={onClose}>
        {back}
        <div className="read-doc">
          <div className="read-doc-text serif">{material.content}</div>
        </div>
      </Dialog>
    );
  }
  const sources = reports.data?.sources ?? [];
  return (
    <Dialog narrow title="מסמכים לקריאה" subtitle="נפתחים כאן, במחשב שלך בלבד." onClose={onClose}>
      <div className="stack read-docs">
        <section className="stack" aria-labelledby="read-docs-case">
          <b id="read-docs-case">חומרי התיק</b>
          {inputs.length === 0 && <span className="small muted">עוד אין חומרים בתיק.</span>}
          <ul className="read-docs-list">
            {inputs.map((i) => (
              <li key={i.id}>
                <button type="button" className="link-btn" onClick={() => setOpen({ type: "material", id: i.id })}>{i.title || kindLabel[i.kind]}</button>
                <span className="small muted"> · {kindLabel[i.kind]}</span>
              </li>
            ))}
          </ul>
        </section>
        <section className="stack" aria-labelledby="read-docs-mine">
          <b id="read-docs-mine">הדוחות שלי</b>
          {reports.isError && <ErrorLine error={fail(reports.error as never)} />}
          {reports.data && sources.length === 0 && <span className="small muted">עוד לא הוספת דוחות שלך. מוסיפים אותם במסך "הסגנון שלי".</span>}
          <ul className="read-docs-list">
            {sources.map((s) => (
              <li key={s.id}>
                <button type="button" className="link-btn" onClick={() => setOpen({ type: "report", id: s.id, title: s.title })}>{s.title}</button>
              </li>
            ))}
          </ul>
        </section>
      </div>
    </Dialog>
  );
}
