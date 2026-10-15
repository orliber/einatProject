// The optional name model (D-053): off by default. Turning it on downloads it once to this
// computer; from then on every material is also read by it, here, to find names no rule knows.
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { ErrorLine } from "./ui";
import { Switch } from "./Switch";

/** One switch row, for the "before every send" group of the privacy page (round 50). */
export function NameModelSettings() {
  const { notify, fail } = useApp();
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["name-model"], queryFn: ipc.nameModelStatus });
  const [downloading, setDownloading] = useState(false);
  const [done, setDone] = useState<[number, number] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!downloading) return;
    const id = window.setInterval(() => {
      ipc.nameModelProgress().then((p) => setDone(p ?? null)).catch(() => undefined);
    }, 1000);
    return () => window.clearInterval(id);
  }, [downloading]);

  async function change(on: boolean) {
    setError(null);
    try {
      if (on && !status.data?.downloaded) {
        setDownloading(true);
        await ipc.downloadNameModel();
      } else {
        await ipc.setNameModel(on);
      }
      notify(on ? "זיהוי השמות במחשב פעיל." : "זיהוי השמות במחשב כבוי, והמודל נמחק מהמחשב.");
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setDownloading(false);
      setDone(null);
      await qc.invalidateQueries({ queryKey: ["name-model"] });
    }
  }

  const s = status.data;
  const percent = done && done[1] > 0 ? Math.floor((done[0] * 100) / done[1]) : 0;
  const hint = downloading
    ? `מוריד את המודל: ${percent}% מתוך ${s?.size_mb ?? 0}MB. אפשר להמשיך לעבוד בינתיים.`
    : `מודל קטן שרץ במחשב, בלי לשלוח שום דבר מהתיקים. הורדה חד-פעמית של ${s?.size_mb ?? 700}MB, מוסיף 2–3 שניות לכל עמוד. כיבוי מוחק אותו.`;
  return (
    <>
      <Switch id="name-model" label="זיהוי שמות שלא הוזנו בתיק" checked={s?.enabled ?? false} disabled={!s || downloading}
        hint={<span role={downloading ? "status" : undefined} aria-live={downloading ? "polite" : undefined} title="DictaBERT-NER של Dicta, CC BY 4.0">{hint}</span>}
        onChange={(on) => void change(on)} />
      {s?.problem && <ErrorLine error={s.problem} />}
      <ErrorLine error={error} />
    </>
  );
}
