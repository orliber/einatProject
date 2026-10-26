import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApp } from "../../App";
import { ErrorLine } from "../../components/ui";
import { ipc, type StyleOverview } from "../../ipc/client";
import { dateOf } from "./labels";

// הסגנון שלי · versions

export function Versions({ o }: { o: StyleOverview }) {
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
    </section>
  );
}
