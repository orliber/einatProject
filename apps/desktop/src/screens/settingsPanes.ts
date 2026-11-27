// The parts of Settings, one page each, like the settings of other professional programs.
// Laid out as on a Mac (D-205): an icon beside each part; a part with parts of its own (the
// clinic) is entered, and the side then lists its parts with the way back above them; a line
// above the page says where she is; and a search finds a setting by its name.

export type SettingsPane =
  | "general" | "ai" | "privacy" | "report" | "tests" | "backup" | "security" | "about"
  | "clinic-calendar" | "clinic-visits" | "clinic-pathway" | "clinic-messages";

/** The name of the icon beside a part (components/SettingsIcon.tsx). */
export type SettingsIconName =
  | "general" | "ai" | "privacy" | "report" | "tests" | "clinic" | "backup" | "security" | "about"
  | "calendar" | "money" | "path" | "message";

export interface PaneEntry {
  id: SettingsPane;
  label: string;
  icon: SettingsIconName;
  /** One calm line under the part's title: what is here, in her words. */
  intro: string;
  /** The names of what is inside, for the search: the headings of the part, its settings and
   *  its buttons, each written as on the page, where the search marks it (a test holds this). */
  finds: string[];
  /** Other words she may look for: they open the part, with nothing on the page to mark. */
  words?: string[];
}

/** A part that holds parts of its own: entering it lists them in the side. */
export interface GroupEntry {
  id: "clinic";
  label: string;
  icon: SettingsIconName;
  panes: PaneEntry[];
}

export type NavEntry = PaneEntry | GroupEntry;

export const isGroup = (e: NavEntry): e is GroupEntry => "panes" in e;

export const CLINIC_PANES: PaneEntry[] = [
  { id: "clinic-calendar", label: "יומן ושעות", intro: "מתי את עובדת, ואיך היומן מציע שעות פנויות.", icon: "calendar", finds: ["שעות וימי עבודה", "ימי עבודה", "דקות פנויות לפני ואחרי כל פגישה", "חגי ישראל", "סנכרון עם יומן גוגל"], words: ["שעות היומן"] },
  { id: "clinic-visits", label: "פגישות ומחירים", intro: "כמה זמן נמשכת כל פגישה, כמה היא עולה, ואיפה היא מתקיימת.", icon: "money", finds: ["סוגי הפגישות", "מקומות"], words: ["משך פגישה", "מחיר פגישה"] },
  { id: "clinic-pathway", label: "מסלול ומשפחה חדשה", intro: "השלבים של אבחון, והמעקבים שנפתחים לבד למשפחה חדשה.", icon: "path", finds: ["מסלול האבחון", "חבילת אבחון", "מחיר חבילת אבחון", "ערכת פתיחה למשפחה חדשה"] },
  { id: "clinic-messages", label: "הודעות ותזכורות", intro: "הנוסחים להורים, התבניות לסיכום, ומתי התוכנה מזכירה לך דברים.", icon: "message", finds: ["הודעות להורים", "נוסח התזכורת לפגישה", "נוסח הודעת התשלום", "תבניות לסיכום פגישה", "מתי להסב את תשומת הלב"], words: ["וואטסאפ", "דוח מתעכב", "חוב פתוח", "המתנה ארוכה"] },
];

/** The side, in order. */
export const SETTINGS_NAV: NavEntry[] = [
  { id: "general", label: "כללי", intro: "איך התוכנה נראית, והשם שלך בדוחות.", icon: "general", finds: ["מוכנה לתיקים אמיתיים?", "תצוגה ופרטים", "גודל הטקסט", "מראה", "השמות שלך"], words: ["מצב כהה", "לילה"] },
  { id: "ai", label: "חיבור ל-AI", intro: "החיבור לשירות שמנסח איתך. שמות ופרטים מזהים מוסתרים תמיד לפני שליחה.", icon: "ai", finds: ["חיבור", "ספק", "מפתח API", "דגם", "מהירות התשובות", "שימוש ועלות", "בדיקה מול המקורות", "בדיקת הדוח כולו"], words: ["ZDR"] },
  { id: "privacy", label: "פרטיות", intro: "מה יוצא מהמחשב, ומה נשאר רק אצלך.", icon: "privacy", finds: ["לפני כל שליחה", "להציג לי כל בקשה לפני שליחה", "זיהוי שמות", "נעילה", "נעילה אוטומטית אחרי"] },
  { id: "report", label: "הדוח", intro: "איך הדוח נראה כשמוציאים אותו: כותרת, חתימה, תבנית ולוגו.", icon: "report", finds: ["פרטי הדוח", "כותרת", "גופן", "חתימה", "חתימת המדריכה", "גרסה להורים", "סעיפים שאת כותבת בעצמך", "התבנית שלי (Word)", "לוגו", "בדיקת מקוריות של קובץ"] },
  { id: "tests", label: "המבחנים", intro: "הטבלאות שהתוכנה מחשבת לפיהן. כאן מאשרים כל מבחן פעם אחת.", icon: "tests", finds: ["טבלאות המבחנים", "חוזקות וחולשות יחסיות"], words: ["ערך קריטי"] },
  { id: "clinic", label: "הקליניקה", icon: "clinic", panes: CLINIC_PANES },
  { id: "backup", label: "גיבוי", intro: "עותק מוצפן של כל התיקים, כדי ששום דבר לא יאבד.", icon: "backup", finds: ["גיבוי", "עותק מוצפן בגוגל דרייב"], words: ["שחזור"] },
  { id: "security", label: "סיסמה ואבטחה", intro: "הסיסמה, עצירת החירום, ומה עושים אם שוכחים את הסיסמה.", icon: "security", finds: ["רשת הביטחון", "סיסמה", "החלפת סיסמה", "כניסה עם גוגל כששוכחים סיסמה", "עצירת חירום", "ערכת שחזור"] },
  { id: "about", label: "עדכונים ועזרה", intro: "גרסה חדשה, בדיקה שהכל עובד, ודיווח כשמשהו לא בסדר.", icon: "about", finds: ["עדכונים", "יומן פעולות", "בדיקת מערכת", "משהו לא עובד?"] },
];

/** Every part that is a page, the clinic's four too, in the order of the side. */
export const SETTINGS_PANES: PaneEntry[] = SETTINGS_NAV.flatMap((e) => (isGroup(e) ? e.panes : [e]));

/** The part a page belongs to, when it is inside one (the clinic's four), else null. */
export function groupOf(pane: SettingsPane): GroupEntry | null {
  return SETTINGS_NAV.find((e): e is GroupEntry => isGroup(e) && e.panes.some((p) => p.id === pane)) ?? null;
}

/** The clinic's four panes. */
export type ClinicPane = Extract<SettingsPane, `clinic-${string}`>;
export const isClinicPane = (pane: SettingsPane): pane is ClinicPane => pane.startsWith("clinic-");

/** The clinic's part a pane shows, or null for the program's own parts. */
export type ClinicGroup = "calendar" | "visits" | "pathway" | "messages";
export function clinicGroupOf(pane: SettingsPane): ClinicGroup | null {
  return pane.startsWith("clinic-") ? (pane.slice("clinic-".length) as ClinicGroup) : null;
}

/** One answer of the search: the part, and the heading or setting inside it that matched. */
export interface Found {
  pane: PaneEntry;
  /** The group the part is in ("הקליניקה"), if any. */
  group: string | null;
  /** What matched inside the part; null when the part's own name matched. */
  inside: string | null;
  /** `inside` is written on the page, where it is brought into view and marked. */
  mark: boolean;
}

/** Spaces and letter case do not count; final letters count as their usual form. */
function fold(text: string): string {
  return text
    .toLowerCase()
    .replace(/[ךםןףץ]/g, (c) => ({ ך: "כ", ם: "מ", ן: "נ", ף: "פ", ץ: "צ" })[c] ?? c)
    .replace(/[\s"'״׳()?\-–]+/g, "");
}

/** A word she ends with "א" where the settings write "ה" ("סיסמא", "דוגמא"): also looked
 *  for as written here. Only the end of a word, and only as a second try, so a word typed
 *  halfway ("מלא" on the way to "מלאי") still finds what it found. */
function alefAsHe(text: string): string {
  return text.replace(/א(?=[\s"'״׳()?\-–]|$)/g, "ה");
}

/** The parts whose name, or a heading or setting inside them, holds what she typed: names
 *  first, then the inside of a part, one answer per part. */
export function searchSettings(query: string): Found[] {
  const plain = fold(query);
  if (plain === "") return [];
  const other = fold(alefAsHe(query));
  const has = (text: string) => fold(text).includes(plain) || fold(text).includes(other);
  const out: Found[] = [];
  const add = (pane: PaneEntry, group: string | null) => {
    if (has(pane.label) || (group !== null && has(group))) {
      out.push({ pane, group, inside: null, mark: false });
      return;
    }
    const inside = pane.finds.find((f) => has(f));
    if (inside !== undefined) {
      out.push({ pane, group, inside, mark: true });
      return;
    }
    const word = pane.words?.find((w) => has(w));
    if (word !== undefined) out.push({ pane, group, inside: word, mark: false });
  };
  for (const e of SETTINGS_NAV) {
    if (isGroup(e)) e.panes.forEach((p) => add(p, e.label));
    else add(e, null);
  }
  return [...out.filter((f) => f.inside === null), ...out.filter((f) => f.inside !== null)];
}

/** The part she was in last: the gear opens there again while the program is open (memory
 *  only, nothing about a case). The first time, and after the program opens, it is "כללי". */
let lastPane: SettingsPane = "general";

export function lastSettingsPane(): SettingsPane {
  return lastPane;
}

export function rememberSettingsPane(pane: SettingsPane): void {
  lastPane = pane;
}

/** For the tests: each starts on "כללי". */
export function forgetSettingsPane(): void {
  lastPane = "general";
}

/** A new part starts at its top: the page was perhaps scrolled far down in the part before,
 *  and the new one would open in its middle. */
export function toTop(): void {
  if (typeof window !== "undefined" && window.scrollY > 0) window.scrollTo({ top: 0 });
}
