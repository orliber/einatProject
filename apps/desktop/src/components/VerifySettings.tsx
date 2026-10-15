// The switch for the check of a draft against its sources (off by default: one more request).
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { Switch } from "./Switch";

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
      <Switch id="verify-stage" label="לאפשר בדיקה של טיוטה מול החומרים" checked={on.data ?? false} disabled={on.data === undefined}
        hint="ה-AI מסמן לכל פסקה אם היא נתמכת בחומרים. בקשה נוספת, באותו סינון, ובלי לשנות שום פסקה."
        onChange={(v) => void change(v)} />
    </section>
  );
}
