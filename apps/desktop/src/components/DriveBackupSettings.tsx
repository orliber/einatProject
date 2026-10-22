// The encrypted copy of each backup in her Google Drive (D-099). Off by default.
// Only the sealed backup file goes up, to the app's hidden Drive folder; it opens only with
// her password or her printed recovery kit. The upload itself runs quietly after a backup:
// nothing here waits for it, and a failure is one quiet line.
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type DriveBackupStatus, type DriveProblem } from "../ipc/client";
import { heDateTime } from "./Backup";
import { Switch } from "./Switch";
import { ErrorLine } from "./ui";

/** How to connect Google first, said on the switch itself while it cannot turn on. */
export const CONNECT_FIRST =
  "כדי להדליק, קודם מחברים את חשבון הגוגל: הגדרות ← סיסמה ואבטחה ← כניסה עם גוגל כששוכחים סיסמה.";

const PROBLEM: Record<DriveProblem, string> = {
  offline: "העותק האחרון לא עלה, כי לא היה חיבור לאינטרנט. הוא יעלה עם הגיבוי הבא.",
  failed: "העותק האחרון לא עלה. ננסה שוב עם הגיבוי הבא.",
  reconnect: "ההרשאה בגוגל בוטלה, ולכן העותקים לא עולים. אפשר לחבר שוב.",
};

function message(err: unknown, fallback: string): string {
  return (err as { message?: string }).message ?? fallback;
}

export function DriveBackupSettings() {
  const { notify } = useApp();
  const qc = useQueryClient();
  const { data: status } = useQuery({ queryKey: ["drive-backup"], queryFn: ipc.driveBackupStatus, retry: false });
  const [signingIn, setSigningIn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!status?.available) return null;
  const s: DriveBackupStatus = status;

  async function turnOn() {
    setError(null);
    setSigningIn(true);
    try {
      qc.setQueryData(["drive-backup"], await ipc.driveBackupTurnOn());
      notify("מעכשיו, אחרי כל גיבוי עולה עותק מוצפן לגוגל דרייב.");
    } catch (err) {
      setError(message(err, "החיבור לגוגל לא הושלם."));
    } finally {
      setSigningIn(false);
    }
  }

  async function turnOff() {
    setError(null);
    try {
      const deleted = await ipc.driveBackupTurnOff();
      await qc.invalidateQueries({ queryKey: ["drive-backup"] });
      notify(deleted
        ? "העותק בגוגל דרייב כובה, והעותקים שהיו שם נמחקו."
        : "העותק בגוגל דרייב כובה. את העותקים שכבר שם לא הצלחתי למחוק עכשיו (אין חיבור). הם מוצפנים, ואפשר למחוק אותם מהגדרות גוגל דרייב ← ניהול אפליקציות.");
    } catch (err) {
      setError(message(err, "הכיבוי לא הושלם."));
    }
  }

  const canTurnOn = s.connected || s.on;
  const hint = signingIn
    ? "נפתח חלון של גוגל בדפדפן. בוחרים שם את אותו החשבון ומאשרים, ואז חוזרים לכאן."
    : !canTurnOn
      ? CONNECT_FIRST
      : s.on && s.last_at !== null
        ? `העותק האחרון עלה עם הגיבוי של ${heDateTime(s.last_at)}.`
        : s.on
          ? "העותק הראשון יעלה עם הגיבוי הבא."
          : undefined;

  return (
    <section className="card setting" aria-labelledby="s-drive">
      <h2 id="s-drive">עותק מוצפן בגוגל דרייב</h2>
      <Switch id="drive-backup" label="אחרי כל גיבוי, לשמור עותק מוצפן גם בגוגל דרייב" checked={s.on}
        disabled={signingIn || !canTurnOn} hint={hint}
        onChange={(on) => void (on ? turnOn() : turnOff())} />
      <ul className="small">
        <li>עולה רק קובץ הגיבוי המוצפן, לתיקייה נסתרת של התוכנה בדרייב שלך. שום שם, תיק או טקסט לא עולה בגלוי.</li>
        <li>בלי הסיסמה שלך או ערכת השחזור המודפסת, הקובץ הזה לא שימושי לאף אחד, גם לא לגוגל.</li>
        <li>נשמרים שלושת העותקים האחרונים, והישנים נמחקים. כיבוי מוחק את כולם מגוגל.</li>
      </ul>
      {signingIn && (
        <button type="button" className="btn align-start" onClick={() => void ipc.googleCancel()}>ביטול</button>
      )}
      {s.on && s.problem && !signingIn && (
        <p className="hint" role="status">
          {PROBLEM[s.problem]}
          {s.problem === "reconnect" && (
            <> <button type="button" className="link-btn" onClick={() => void turnOn()}>חיבור מחדש</button></>
          )}
        </p>
      )}
      <ErrorLine error={error} />
    </section>
  );
}
