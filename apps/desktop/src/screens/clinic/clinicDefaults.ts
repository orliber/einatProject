// What the settings tab needs to know besides the settings themselves: the settings of a
// clinic nobody touched yet (the same as `ClinicSettings::default` in the core, which has no
// request that hands them out; a test holds this copy to the stand-in's), the placeholders of
// a message, and how a message looks once it is filled.
import type { ClinicSettings } from "../../ipc/generated/ClinicSettings";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import { VISIT_KINDS } from "./format";

const USUAL_MINUTES: Record<VisitKind, number> = { intake: 60, assessment: 90, observation: 60, feedback: 60, therapy: 50, guidance: 50, meeting: 30, other: 50, block: 60 };

export function defaultClinicSettings(): ClinicSettings {
  return {
    day_start: 8 * 60,
    day_end: 20 * 60,
    work_days: [0, 1, 2, 3, 4],
    defaults: VISIT_KINDS.map((kind) => ({ kind, minutes: USUAL_MINUTES[kind], fee: 0 })),
    places: ["הקליניקה"],
    // No child's name in a message that leaves by her hand, unless she adds it (D-173).
    reminder_text: "שלום {הורה}, תזכורת לפגישה ביום {יום}, {תאריך}, בשעה {שעה}. להתראות, {שמי}",
    payment_text: "שלום {הורה}, היתרה לתשלום היא {סכום} ₪. תודה רבה, {שמי}",
    note_templates: [
      { name: "פגישת היכרות עם ההורים", text: "סיבת הפנייה:\n\nרקע התפתחותי:\n\nרקע משפחתי:\n\nמסגרת חינוכית:\n\nטיפולים ואבחונים קודמים:\n\nציפיות ההורים מהאבחון:\n" },
      { name: "מפגש אבחון", text: "כלים שהועברו:\n\nהתנהגות במפגש:\n\nשיתוף פעולה וקשב:\n\nרשמים ראשוניים:\n\nלהמשך:\n" },
      { name: "תצפית", text: "מקום התצפית ומשכה:\n\nמה נצפה:\n\nקשר עם ילדים ועם הצוות:\n\nדברי הצוות:\n" },
      { name: "שיחת משוב", text: "מה נמסר להורים:\n\nתגובת ההורים ושאלות שעלו:\n\nהמלצות שסוכמו:\n\nמה נמסר או נשלח:\n" },
    ],
    report_days: 21,
    debt_days: 30,
    waiting_weeks: 6,
  };
}

/** The placeholders of a message text, as she types them, with what each one stands for. */
export const PLACEHOLDERS = [
  { token: "{הורה}", means: "שם ההורה" },
  { token: "{ילד}", means: "שם הילד או הילדה" },
  { token: "{יום}", means: "היום בשבוע" },
  { token: "{תאריך}", means: "תאריך הפגישה" },
  { token: "{שעה}", means: "שעת הפגישה" },
  { token: "{מקום}", means: "מקום הפגישה" },
  { token: "{סכום}", means: "היתרה לתשלום" },
  { token: "{שמי}", means: "השם שלך" },
] as const;

export type Placeholder = (typeof PLACEHOLDERS)[number]["token"];

/** The values of the preview: plainly invented, so nobody takes them for a family of hers. */
export function sampleValues(place: string): Record<Placeholder, string> {
  return {
    "{הורה}": "ישראלה",
    "{ילד}": "דני",
    "{יום}": "שלישי",
    "{תאריך}": "01.01.2030",
    "{שעה}": "10:00",
    "{מקום}": place.trim() === "" ? "הקליניקה" : place,
    "{סכום}": "450",
    "{שמי}": "השם שלך",
  };
}

/** The text with every placeholder it knows replaced; anything else stays as she typed it. */
export function fillPreview(text: string, values: Record<Placeholder, string>): string {
  let out = text;
  for (const { token } of PLACEHOLDERS) out = out.split(token).join(values[token]);
  return out;
}
