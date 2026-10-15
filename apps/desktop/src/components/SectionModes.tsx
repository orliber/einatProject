// Settings → הדוח: which sections she writes herself (round 30). For each one nothing of the
// case is sent, the full draft skips it, and the sorting puts no material in it.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { ErrorLine, Spinner } from "./ui";

export function SectionModesSettings() {
  const { fail } = useApp();
  const qc = useQueryClient();
  const modes = useQuery({ queryKey: ["section-modes"], queryFn: ipc.sectionModes });
  const [error, setError] = useState<string | null>(null);
  return (
    <section className="card setting" aria-labelledby="s-modes">
      <h2 id="s-modes">סעיפים שאת כותבת בעצמך</h2>
      <span className="hint">בסעיף שסימנת שום חומר מהתיק לא נשלח, ו"כתיבת כל הדוח" מדלגת עליו. אפשר לכתוב נקודות ולבקש לנסח מהן. חל על כל התיקים.</span>
      {modes.isLoading && <Spinner />}
      <ul className="stack" style={{ listStyle: "none", margin: 0, padding: 0, gap: 4 }}>
        {(modes.data ?? []).map((m) => (
          <li key={m.key}>
            <label className="row">
              <input type="checkbox" checked={m.mode === "self"}
                onChange={(e) => void ipc.setSectionMode(m.key, e.target.checked ? "self" : "ai")
                  .then(async () => { setError(null); await qc.invalidateQueries({ queryKey: ["section-modes"] }); })
                  .catch((x: unknown) => setError(fail(x as never)))} />
              <span>{m.title}</span>
              {m.mode !== m.template_mode && <span className="small muted">(שונה מהמכון)</span>}
            </label>
          </li>
        ))}
      </ul>
      <ErrorLine error={error ?? (modes.error ? fail(modes.error as never) : null)} />
    </section>
  );
}
