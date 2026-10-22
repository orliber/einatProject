// The encrypted copy of each backup in her Google Drive (D-099). Off by default.
// Only the sealed backup file goes up, to the app's hidden Drive folder; it opens only with
// her password or her printed recovery kit. The upload itself runs quietly after a backup:
// nothing here waits for it, and a failure is one quiet line.
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type DriveBackupStatus, type DriveCleanupResult, type DriveProblem } from "../ipc/client";
import { heDateTime } from "./Backup";
import { Switch } from "./Switch";
import { ErrorLine } from "./ui";

/** How to connect Google first, said on the switch itself while it cannot turn on. */
export const CONNECT_FIRST =
  "כדי להדליק, קודם מחברים את חשבון הגוגל: הגדרות ← סיסמה ואבטחה ← כניסה עם גוגל כששוכחים סיסמה.";

const PROBLEM: Record<DriveProblem, string> = {
  offline: "העותק האחרון לא עלה, כי לא היה חיבור לאינטרנט. הוא יעלה עם הגיבוי הבא.",
  failed: "העותק האחרון לא עלה. התוכנה תנסה שוב עם הגיבוי הבא.",
  reconnect: "ההרשאה בגוגל בוטלה, ולכן העותקים לא עולים. אפשר לחבר שוב.",
};

/** Still owed at Google after turning off: said, never passed off as done. */
export const CLEANUP_PENDING =
  "העותק בגוגל דרייב כבוי, ושום עותק לא עולה יותר. את העותקים שכבר שם התוכנה עוד לא הצליחה למחוק, וגם לא לבטל את ההרשאה בגוגל. היא תנסה שוב מעצמה בזמן שהכספת פתוחה.";

/** After a restore of a backup taken while the copy was on: its copies were left in Drive. */
export const COPIES_LEFT =
  "לפני השחזור העותק בגוגל דרייב היה דלוק, ולכן ייתכן שנשארו שם עותקים מוצפנים מלפני השחזור. כדי למחוק אותם, מחברים את אותו חשבון גוגל שהיה מחובר אז, מדליקים את העותק ואז מכבים אותו.";

/** The permission was gone at Google already, so the copies there are out of the program's reach. */
export const CLEANUP_WITHDRAWN =
  "העותק בגוגל דרייב כובה. ההרשאה בגוגל כבר הייתה מבוטלת, ולכן את העותקים שנשארו שם התוכנה לא יכולה למחוק. הם מוצפנים. אפשר למחוק אותם בגוגל דרייב: הגדרות ← ניהול אפליקציות ← כספת האבחון ← מחיקת נתוני אפליקציה מוסתרים. שימי לב: זה מוחק גם את המפתח של הכניסה עם גוגל כששוכחים סיסמה, ואז צריך להדליק אותה שוב.";

const TURNED_OFF: Record<Exclude<DriveCleanupResult, "pending" | "withdrawn">, string> = {
  cleaned: "העותק בגוגל דרייב כובה. העותקים שהיו שם נמחקו, וההרשאה שניתנה לתוכנה בוטלה.",
  off: "העותק בגוגל דרייב כבוי.",
};

function message(err: unknown, fallback: string): string {
  return (err as { message?: string }).message ?? fallback;
}

export function DriveBackupSettings() {
  const { notify } = useApp();
  const qc = useQueryClient();
  // While a cleanup is owed, the program tries it again by itself: read again now and then, so
  // the line goes once it is done.
  const { data: status } = useQuery({
    queryKey: ["drive-backup"], queryFn: ipc.driveBackupStatus, retry: false,
    refetchInterval: (q) => (q.state.data?.cleanup_pending ? 60_000 : false),
  });
  const [signingIn, setSigningIn] = useState(false);
  const [turningOff, setTurningOff] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The permission was gone at Google already: kept on screen (not a passing notice).
  const [left, setLeft] = useState<string | null>(null);
  if (!status?.available) return null;
  const s: DriveBackupStatus = status;

  async function turnOn() {
    setError(null);
    setLeft(null);
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
    setLeft(null);
    setTurningOff(true);
    try {
      const result = await ipc.driveBackupTurnOff();
      await qc.invalidateQueries({ queryKey: ["drive-backup"] });
      // Still owed: the status says so, for as long as it is owed.
      if (result === "withdrawn") setLeft(CLEANUP_WITHDRAWN);
      else if (result !== "pending") notify(TURNED_OFF[result]);
    } catch (err) {
      setError(message(err, "הכיבוי לא הושלם."));
    } finally {
      setTurningOff(false);
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
        disabled={signingIn || turningOff || !canTurnOn} hint={hint}
        onChange={(on) => void (on ? turnOn() : turnOff())} />
      <ul className="small">
        <li>עולה רק קובץ הגיבוי המוצפן, לתיקייה נסתרת של התוכנה בדרייב שלך. שום שם, תיק או טקסט לא עולה בגלוי.</li>
        <li>בלי הסיסמה שלך או ערכת השחזור המודפסת, הקובץ הזה לא שימושי לאף אחד, גם לא לגוגל.</li>
        <li>נשמרים שלושת העותקים האחרונים, והישנים נמחקים. כיבוי מוחק את כולם מגוגל.</li>
        <li>כדי להעלות בלי לשאול בכל פעם, התוכנה שומרת בתוך הכספת הרשאה לתיקייה הנסתרת בלבד, עד שתכבי. כיבוי מבטל את ההרשאה בגוגל.</li>
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
      {left ? (
        <p className="hint" role="status">{left}</p>
      ) : !s.on && !turningOff && s.cleanup_pending ? (
        <p className="hint" role="status">{CLEANUP_PENDING}</p>
      ) : !s.on && !turningOff && s.copies_left && (
        <p className="hint" role="status">{COPIES_LEFT}</p>
      )}
      <ErrorLine error={error} />
    </section>
  );
}
