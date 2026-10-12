// The switch for the check of a draft against its sources (off by default: one more request).
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc } from "../ipc/client";

export function VerifySettings() {
  const { notify, fail } = useApp();
  const qc = useQueryClient();
  const on = useQuery({ queryKey: ["verify-stage"], queryFn: ipc.verifyStageOn });

  async function change(value: boolean) {
    try {
      await ipc.setVerifyStage(value);
      await qc.invalidateQueries({ queryKey: ["verify-stage"] });
      notify(value ? "הבדיקה מול המקורות פעילה." : "הבדיקה מול המקורות כבויה.");
    } catch (e) {
      notify(fail(e as never));
    }
  }

  return (
    <section className="card setting" aria-labelledby="s-verify">
      <h2 id="s-verify">בדיקה מול המקורות</h2>
      <label className="row">
        <input type="checkbox" checked={on.data ?? false} disabled={on.data === undefined}
          onChange={(e) => void change(e.target.checked)} />
        <span>לאפשר בדיקה של טיוטה מול החומרים</span>
      </label>
      <span className="hint">
        ה-AI מסמן לכל פסקה אם היא נתמכת בחומרים. בקשה נוספת, באותו סינון, ובלי לשנות שום פסקה.
      </span>
    </section>
  );
}
