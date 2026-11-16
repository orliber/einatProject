// What the settings tab needs to know besides the settings themselves: the settings of a
// clinic nobody touched yet (`ClinicSettings::default` in the core, kept in defaultSettings.json), the placeholders of
// a message, and how a message looks once it is filled.
import type { ClinicSettings } from "../../ipc/generated/ClinicSettings";
import DEFAULT_SETTINGS from "./defaultSettings.json";

/** A fresh copy each time: the caller may edit it. The file is held to the core's defaults by a
 *  test in the core (`the_windows_default_settings_are_the_cores`). */
export function defaultClinicSettings(): ClinicSettings {
  return structuredClone(DEFAULT_SETTINGS as ClinicSettings);
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
