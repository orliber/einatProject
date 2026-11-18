// The one-way copy of the clinic's calendar to Google Calendar (D-202). Off by default.
// A calendar of its own in her Google account, "פגישות", with one event for each planned or
// held visit: its time and one word, nothing else. The rounds run quietly in the background;
// a failure is one quiet line here.
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../../App";
import { heDateTime } from "../../components/Backup";
import { Switch } from "../../components/Switch";
import { ErrorLine } from "../../components/ui";
import { ipc, type CalendarOff, type CalendarProblem, type CalendarStatus } from "../../ipc/client";
import { Section } from "./parts";

const PROBLEM: Record<CalendarProblem, string> = {
  offline: "הסנכרון האחרון לא עבר, כי לא היה חיבור לאינטרנט. התוכנה תנסה שוב מעצמה.",
  failed: "הסנכרון האחרון לא עבר. התוכנה תנסה שוב מעצמה.",
  reconnect: "ההרשאה בגוגל בוטלה, ולכן היומן לא מתעדכן. אפשר לחבר שוב.",
  paused: "עצירת החירום פעילה, ולכן שום דבר לא יוצא מהמחשב, גם לא ליומן. כשתכבי אותה, הסנכרון ימשיך.",
};

/** Still owed at Google after turning off: said, never passed off as done. */
export const CALENDAR_CLEANUP_PENDING =
  "הסנכרון כבוי, ושום פגישה לא עוברת יותר. את היומן \"פגישות\" שבגוגל התוכנה עוד לא הצליחה למחוק. היא תנסה שוב מעצמה בזמן שהכספת פתוחה.";

/** The permission was gone at Google already: the calendar there is out of reach. */
export const CALENDAR_WITHDRAWN =
  "הסנכרון כובה. ההרשאה בגוגל כבר הייתה מבוטלת, ולכן את היומן \"פגישות\" התוכנה לא יכולה למחוק. אפשר למחוק אותו ביומן גוגל: הגדרות ← היומנים שלי ← פגישות ← הסרת יומן.";

/** Before the app is registered with Google (Or's step), the switch cannot turn on. */
export const CALENDAR_NOT_READY =
  "כדי להדליק, צריך קודם להשלים את הרישום של התוכנה אצל גוגל (המדריך \"רישום התוכנה אצל גוגל\"). עד אז שום דבר לא יוצא.";

const TURNED_OFF: Record<Exclude<CalendarOff, "pending" | "withdrawn">, string> = {
  cleaned: "הסנכרון כובה, והיומן \"פגישות\" נמחק מגוגל.",
  off: "הסנכרון עם יומן גוגל כבוי.",
};

function message(err: unknown, fallback: string): string {
  return (err as { message?: string }).message ?? fallback;
}

export function CalendarSyncSection() {
  const { notify } = useApp();
  const qc = useQueryClient();
  // While a cleanup is owed the program tries it again by itself, and while it is on a round
  // may finish at any time: read again now and then.
  const { data: status } = useQuery({
    queryKey: ["calendar-sync"], queryFn: ipc.calendarStatus, retry: false,
    refetchInterval: (q) => (q.state.data?.on || q.state.data?.cleanup_pending ? 60_000 : false),
  });
  const [busy, setBusy] = useState<"on" | "off" | "now" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState<string | null>(null);
  if (!status) return null;
  const s: CalendarStatus = status;

  async function turnOn() {
    setError(null);
    setLeft(null);
    setBusy("on");
    try {
      qc.setQueryData(["calendar-sync"], await ipc.calendarTurnOn());
      notify("מעכשיו הפגישות מופיעות גם ביומן \"פגישות\" בגוגל: שעה ומילה אחת, בלי שמות.");
    } catch (err) {
      setError(message(err, "החיבור לגוגל לא הושלם."));
    } finally {
      setBusy(null);
    }
  }

  async function turnOff() {
    setError(null);
    setLeft(null);
    setBusy("off");
    try {
      const result = await ipc.calendarTurnOff();
      await qc.invalidateQueries({ queryKey: ["calendar-sync"] });
      if (result === "withdrawn") setLeft(CALENDAR_WITHDRAWN);
      else if (result !== "pending") notify(TURNED_OFF[result]);
    } catch (err) {
      setError(message(err, "הכיבוי לא הושלם."));
    } finally {
      setBusy(null);
    }
  }

  async function syncNow() {
    setError(null);
    setBusy("now");
    try {
      const after = await ipc.calendarSyncNow();
      qc.setQueryData(["calendar-sync"], after);
      if (after.problem === null) notify("היומן בגוגל מעודכן.");
    } catch (err) {
      setError(message(err, "הסנכרון לא הושלם."));
    } finally {
      setBusy(null);
    }
  }

  const hint = busy === "on"
    ? "נפתח חלון של גוגל בדפדפן. בוחרים חשבון ומאשרים, ואז חוזרים לכאן."
    : !s.available && !s.on
      ? CALENDAR_NOT_READY
      : s.on && s.last_at !== null
        ? `עודכן לאחרונה: ${heDateTime(s.last_at)}.`
        : s.on
          ? "הסנכרון הראשון רץ עכשיו."
          : undefined;

  return (
    <Section title="סנכרון עם יומן גוגל">
      <Switch id="calendar-sync" label="להעתיק את הפגישות ליומן נפרד בגוגל" checked={s.on}
        disabled={busy !== null || (!s.available && !s.on)} hint={hint}
        onChange={(on) => void (on ? turnOn() : turnOff())} />
      <ul className="small">
        <li>התוכנה פותחת בחשבון הגוגל שלך יומן נפרד בשם "פגישות", ורואה רק אותו. את שאר היומנים שלך היא לא רואה.</li>
        <li>לכל פגישה עוברים רק השעה והמילה "פגישה" (וזמן שחסמת לעצמך: "לא פנויה"). בלי שם, סוג פגישה, מקום או הערה.</li>
        <li>הכיוון הוא אחד: מהתוכנה ליומן. שינוי ביומן של גוגל לא חוזר לתוכנה, והסנכרון הבא מחזיר את מה שבתוכנה.</li>
        <li>עוברות פגישות משבועיים אחורה ועד חצי שנה קדימה. פגישה שבוטלה נמחקת מהיומן.</li>
        <li>כיבוי מוחק את היומן "פגישות" מגוגל ומבטל את ההרשאה.</li>
      </ul>
      {s.on && (
        <button type="button" className="btn btn-small align-start" disabled={busy !== null} onClick={() => void syncNow()}>
          {busy === "now" ? "מסנכרן…" : "לסנכרן עכשיו"}
        </button>
      )}
      {busy === "on" && (
        <button type="button" className="btn align-start" onClick={() => void ipc.googleCancel()}>ביטול</button>
      )}
      {s.on && s.problem && busy !== "on" && (
        <p className="hint" role="status">
          {PROBLEM[s.problem]}
          {s.problem === "reconnect" && (
            <> <button type="button" className="link-btn" onClick={() => void turnOn()}>חיבור מחדש</button></>
          )}
        </p>
      )}
      {left ? (
        <p className="hint" role="status">{left}</p>
      ) : !s.on && busy !== "off" && s.cleanup_pending ? (
        <p className="hint" role="status">{CALENDAR_CLEANUP_PENDING}</p>
      ) : !s.on && s.last_off === "withdrawn" && (
        <p className="hint" role="status">{CALENDAR_WITHDRAWN}</p>
      )}
      <ErrorLine error={error} />
    </Section>
  );
}
