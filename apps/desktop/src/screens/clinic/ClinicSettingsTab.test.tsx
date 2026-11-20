// The clinic's settings over the demo core: what is shown, saving only when something
// changed, the core's refusal as the message, places, the message texts with their
// placeholders and preview, templates, thresholds, changes that survive a look at another
// part of the settings, the way back to the defaults, and accessibility. Every detail is invented.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { ClinicSettings } from "../../ipc/generated/ClinicSettings";
import { a11yViolations } from "../../test/a11y";
import { defaultClinicSettings as standInDefaults, PLACEHOLDERS as STAND_IN_PLACEHOLDERS } from "../../web/fakeClinic";
import { forgetDiscreet } from "./ClinicScreen";
import { forgetSettingsDraft, fromDraft, moved, toDraft } from "./ClinicSettingsTab";
import { defaultClinicSettings, fillPreview, PLACEHOLDERS, sampleValues } from "./clinicDefaults";
import { ask, holding, openOn, refusing } from "./officeTestKit";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  forgetSettingsDraft();
  window.history.replaceState({}, "", "/");
});

/** The clinic's settings are the part "הקליניקה" of the program's settings (D-205). */
const SETTINGS = { name: "settings", pane: "clinic" } as const;
const paneButton = (name: string) => within(screen.getByRole("navigation", { name: "חלקי ההגדרות" })).getByRole("button", { name });
const form = () => screen.findByRole("form", { name: "הגדרות הקליניקה" });
/** One of the four small tabs of the settings (D-205). */
const topic = (name: string) => screen.getByRole("radio", { name });
const part = (name: string) => screen.getByRole("region", { name });
const saveButton = () => screen.getByRole("button", { name: "שמירה" });
const bar = () => screen.getByRole("group", { name: "שמירת ההגדרות" });
const sentSettings = (requests: () => { op: string }[]) =>
  requests().filter((r) => r.op === "save_settings").map((r) => (r as unknown as { settings: ClinicSettings }).settings);

describe("the pure pieces", () => {
  it("the defaults and the placeholders are the ones the clinic itself has", () => {
    expect(defaultClinicSettings()).toEqual(standInDefaults());
    expect(PLACEHOLDERS.map((p) => p.token)).toEqual([...STAND_IN_PLACEHOLDERS]);
  });

  it("a draft goes back to the settings it came from", () => {
    const s = defaultClinicSettings();
    expect(fromDraft(toDraft(s))).toEqual({ settings: s });
    // A day that ends at midnight is written 00:00.
    const late = { ...s, day_end: 1440 };
    expect(toDraft(late).end).toBe("00:00");
    expect(fromDraft(toDraft(late))).toEqual({ settings: late });
  });

  it("what is not a number or an hour is said before anything is sent", () => {
    const d = toDraft(defaultClinicSettings());
    const problem = (patch: Partial<typeof d>) => (fromDraft({ ...d, ...patch }) as { problem?: string }).problem;
    expect(problem({ start: "בוקר" })).toContain("שעת ההתחלה");
    expect(problem({ end: "25:00" })).toContain("שעת הסיום");
    expect(problem({ kinds: d.kinds.map((k) => (k.kind === "therapy" ? { ...k, minutes: "שעה" } : k)) })).toBe("המשך של פגישת טיפול צריך להיות מספר של דקות, למשל 50.");
    expect(problem({ kinds: d.kinds.map((k) => (k.kind === "intake" ? { ...k, fee: "4-50" } : k)) })).toContain("המחיר של פגישת היכרות");
    expect(problem({ reportDays: "" })).toContain("הדוח");
    expect(problem({ debtDays: "-3" })).toContain("חוב");
    expect(problem({ waitingWeeks: "1.5" })).toContain("רשימת ההמתנה");
    // The first step of the pathway has no box for a gap: whatever its draft holds, it is 0.
    const first = fromDraft({ ...d, pathway: d.pathway.map((p, i) => (i === 0 ? { ...p, gap: "" } : p)) });
    expect("settings" in first && first.settings.pathway[0]?.gap_days).toBe(0);
    expect(problem({ pathway: d.pathway.map((p, i) => (i === 1 ? { ...p, gap: "" } : p)) })).toContain("בשלב 2");
    // An empty fee is no fee; time she keeps for herself never has one.
    const free = fromDraft({ ...d, kinds: d.kinds.map((k) => ({ ...k, fee: k.kind === "block" ? "999" : "" })) });
    expect("settings" in free && free.settings.defaults.every((k) => k.fee === 0)).toBe(true);
  });

  it("moves an item one place, and leaves the ends where they are", () => {
    expect(moved(["a", "b", "c"], 1, -1)).toEqual(["b", "a", "c"]);
    expect(moved(["a", "b", "c"], 1, 1)).toEqual(["a", "c", "b"]);
    expect(moved(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
    expect(moved(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
  });

  it("the preview fills every placeholder with an invented value, and leaves the rest", () => {
    const values = sampleValues("");
    expect(fillPreview("שלום {הורה}, {ילד} ב{מקום} {לא מוכר} {סכום} ₪ {הורה}", values)).toBe("שלום ישראלה, דני בהקליניקה {לא מוכר} 450 ₪ ישראלה");
    expect(sampleValues("שיחת וידאו")["{מקום}"]).toBe("שיחת וידאו");
  });
});

describe("the clinic's settings", () => {
  it("shows the settings as they are stored, with nothing to save yet", async () => {
    const { view, user } = await openOn(SETTINGS);
    await form();
    const hours = part("שעות וימי עבודה");
    expect(within(hours).getByLabelText("היום מתחיל ב-")).toHaveValue("08:00");
    expect(within(hours).getByLabelText("ומסתיים ב-")).toHaveValue("20:00");
    const days = within(hours).getByRole("group", { name: "ימי עבודה" });
    expect(within(days).getAllByRole("checkbox").map((c) => (c as HTMLInputElement).checked)).toEqual([true, true, true, true, true, false, false]);
    // Four small tabs, the first open; the other tabs' parts are not shown.
    expect(screen.getAllByRole("radio").map((r) => r.textContent)).toEqual(["יומן ושעות", "פגישות ומחירים", "מסלול ומשפחה חדשה", "הודעות ותזכורות"]);
    expect(topic("יומן ושעות")).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("region", { name: "סוגי הפגישות" })).toBeNull();
    expect(await a11yViolations(view.container)).toEqual([]);

    await user.click(topic("פגישות ומחירים"));
    const kinds = within(part("סוגי הפגישות")).getByRole("table", { name: "המשך והמחיר הרגילים של כל סוג פגישה" });
    expect(within(kinds).getAllByRole("rowheader").map((h) => h.textContent)).toEqual(["פגישת היכרות", "מפגש אבחון", "תצפית", "שיחת משוב", "פגישת טיפול", "הדרכת הורים", "פגישה מקצועית", "פגישה אחרת", "זמן שמור"]);
    expect(within(kinds).getByLabelText("משך של מפגש אבחון, בדקות")).toHaveValue("90");
    expect(within(kinds).getByLabelText("מחיר של מפגש אבחון, בשקלים")).toHaveValue("600");
    // Time she keeps for herself has a length and no fee.
    expect(within(kinds).getByLabelText("משך של זמן שמור, בדקות")).toHaveValue("60");
    expect(within(kinds).queryByLabelText("מחיר של זמן שמור, בשקלים")).toBeNull();

    expect(within(within(part("מקומות")).getByRole("list", { name: "המקומות, לפי הסדר" })).getAllByRole("listitem").map((p) => p.textContent)).toEqual(
      ["הקליניקהמוצע ראשוןלמעלהלמטה", "בגן או בבית הספרלמעלהלמטה", "שיחת וידאולמעלהלמטה"]);
    expect(screen.queryByRole("region", { name: "שעות וימי עבודה" })).toBeNull();
    await user.click(topic("הודעות ותזכורות"));
    expect(within(part("הודעות להורים")).getByLabelText("נוסח התזכורת לפגישה")).toHaveValue("שלום {הורה}, תזכורת לפגישה ביום {יום}, {תאריך}, בשעה {שעה}. להתראות, {שמי}");
    expect(part("הודעות להורים")).toHaveTextContent("התוכנה לא שולחת הודעות בעצמה.");
    expect(part("הודעות להורים")).toHaveTextContent("לא כולל את שם הילד בכוונה");
    expect(within(part("תבניות לסיכום פגישה")).getAllByRole("group")).toHaveLength(4);
    const thresholds = part("מתי להסב את תשומת הלב");
    expect(within(thresholds).getByLabelText("ימים עד שהדוח אמור להיות מוכן")).toHaveValue("21");
    expect(within(thresholds).getByLabelText("ימים עד שהדוח אמור להיות מוכן")).toHaveAccessibleDescription(/מופיע כ״דוח מתעכב״/);
    expect(within(thresholds).getByLabelText("ימים עד שמצביעים על חוב")).toHaveValue("30");
    expect(within(thresholds).getByLabelText("ימים עד שמצביעים על חוב")).toHaveAccessibleDescription(/מופיע כ״חוב פתוח״/);
    expect(within(thresholds).getByLabelText("שבועות ברשימת ההמתנה")).toHaveValue("6");
    expect(within(thresholds).getByLabelText("שבועות ברשימת ההמתנה")).toHaveAccessibleDescription(/מופיעה כ״המתנה ארוכה״/);

    expect(saveButton()).toBeDisabled();
    expect(within(bar()).getByRole("status")).toHaveTextContent("כל ההגדרות שמורות.");
    expect(within(bar()).queryByRole("button", { name: "ביטול השינויים" })).toBeNull();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("“שמירה” wakes when something changed, saves everything at once, and rests again", async () => {
    const served = await openOn(SETTINGS);
    const { user, requests, notes } = served;
    await form();
    await user.click(topic("פגישות ומחירים"));
    const fee = screen.getByLabelText("מחיר של פגישת טיפול, בשקלים");
    await user.clear(fee);
    await user.type(fee, "420.50");
    // Changes on several tabs wait together for the one "שמירה".
    await user.click(topic("יומן ושעות"));
    await user.click(screen.getByLabelText("שישי"));
    await user.clear(screen.getByLabelText("ומסתיים ב-"));
    await user.type(screen.getByLabelText("ומסתיים ב-"), "19:30");
    await user.click(topic("הודעות ותזכורות"));
    await user.clear(screen.getByLabelText("שבועות ברשימת ההמתנה"));
    await user.type(screen.getByLabelText("שבועות ברשימת ההמתנה"), "4");
    expect(saveButton()).toBeEnabled();
    expect(within(bar()).getByRole("status")).toHaveTextContent("יש שינויים שעוד לא נשמרו.");
    await user.click(saveButton());

    await waitFor(() => expect(notes).toContain("הגדרות הקליניקה נשמרו"));
    const sent = sentSettings(requests);
    expect(sent).toHaveLength(1);
    expect(sent[0]).toMatchObject({ day_start: 480, day_end: 1170, work_days: [0, 1, 2, 3, 4, 5], waiting_weeks: 4, report_days: 21, debt_days: 30 });
    expect(sent[0]?.defaults.find((d) => d.kind === "therapy")).toEqual({ kind: "therapy", minutes: 50, fee: 42_050 });
    expect(sent[0]?.defaults).toHaveLength(9);
    await waitFor(() => expect(saveButton()).toBeDisabled());
    await user.click(topic("פגישות ומחירים"));
    expect(screen.getByLabelText("מחיר של פגישת טיפול, בשקלים")).toHaveValue("420.50");
    await user.click(topic("הודעות ותזכורות"));
    // The vault holds it.
    expect((await ask(served, { op: "settings" }, "settings")).waiting_weeks).toBe(4);
    // Putting a value back by hand is not a change.
    await user.clear(screen.getByLabelText("שבועות ברשימת ההמתנה"));
    expect(saveButton()).toBeEnabled();
    await user.type(screen.getByLabelText("שבועות ברשימת ההמתנה"), "4");
    expect(saveButton()).toBeDisabled();
  });

  it("the core's refusal is the message, and what she changed stays to be fixed", async () => {
    const { user, requests, notes } = await openOn(SETTINGS);
    await form();
    for (const dayName of ["ראשון", "שני", "שלישי", "רביעי", "חמישי"]) await user.click(screen.getByLabelText(dayName));
    await user.click(saveButton());
    expect(await within(bar()).findByRole("alert")).toHaveTextContent("צריך לבחור לפחות יום עבודה אחד.");
    expect(sentSettings(requests)).toHaveLength(1);
    expect(notes).not.toContain("הגדרות הקליניקה נשמרו");
    expect(saveButton()).toBeEnabled();
    expect(screen.getByLabelText("ראשון")).not.toBeChecked();
    // Fixed and saved: the message goes.
    await user.click(screen.getByLabelText("שני"));
    await user.click(saveButton());
    await waitFor(() => expect(notes).toContain("הגדרות הקליניקה נשמרו"));
    expect(within(bar()).queryByRole("alert")).toBeNull();
    expect(sentSettings(requests)[1]?.work_days).toEqual([1]);
  });

  it("a length that is out of range is refused by the core, in its words", async () => {
    const { user } = await openOn(SETTINGS);
    await form();
    await user.click(topic("פגישות ומחירים"));
    const minutes = screen.getByLabelText("משך של תצפית, בדקות");
    await user.clear(minutes);
    await user.type(minutes, "3");
    await user.click(saveButton());
    expect(await within(bar()).findByRole("alert")).toHaveTextContent("משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
  });

  it("a box that holds no number at all is said here, and nothing is sent", async () => {
    const { user, requests } = await openOn(SETTINGS);
    await form();
    await user.click(topic("פגישות ומחירים"));
    const fee = screen.getByLabelText("מחיר של תצפית, בשקלים");
    await user.clear(fee);
    await user.type(fee, "חמש מאות");
    await user.click(saveButton());
    expect(within(bar()).getByRole("alert")).toHaveTextContent("המחיר של תצפית צריך להיות סכום בשקלים, למשל 450 או 450.50, עד 100,000.");
    expect(sentSettings(requests)).toHaveLength(0);
  });

  it("a save the vault refuses keeps every change on screen", async () => {
    const { user, notes } = await openOn(SETTINGS, refusing("save_settings", "ההגדרות לא נשמרו, כי הכספת עסוקה."));
    await form();
    await user.click(screen.getByLabelText("שבת"));
    await user.click(saveButton());
    expect(await within(bar()).findByRole("alert")).toHaveTextContent("ההגדרות לא נשמרו, כי הכספת עסוקה.");
    expect(screen.getByLabelText("שבת")).toBeChecked();
    expect(saveButton()).toBeEnabled();
    expect(notes).toEqual([]);
  });

  it("places are added, removed and reordered with buttons", async () => {
    const { user, requests, view } = await openOn(SETTINGS);
    await form();
    await user.click(topic("פגישות ומחירים"));
    const places = part("מקומות");
    const names = () => within(within(places).getByRole("list", { name: "המקומות, לפי הסדר" })).getAllByRole("listitem").map((li) => li.firstElementChild?.textContent);
    expect(within(places).getByRole("button", { name: "להעלות את הקליניקה" })).toBeDisabled();
    expect(within(places).getByRole("button", { name: "להוריד את שיחת וידאו" })).toBeDisabled();
    await user.click(within(places).getByRole("button", { name: "להעלות את שיחת וידאו" }));
    await user.click(within(places).getByRole("button", { name: "להעלות את שיחת וידאו" }));
    expect(names()).toEqual(["שיחת וידאו", "הקליניקה", "בגן או בבית הספר"]);
    await user.click(within(places).getByRole("button", { name: "להוריד את הקליניקה" }));
    expect(names()).toEqual(["שיחת וידאו", "בגן או בבית הספר", "הקליניקה"]);
    await user.click(within(places).getByRole("button", { name: "הסרת בגן או בבית הספר" }));

    // Enter adds the place; it does not save the form.
    const add = within(places).getByRole("button", { name: "הוספת מקום" });
    expect(add).toBeDisabled();
    await user.type(within(places).getByLabelText("מקום חדש"), "  בבית   המשפחה {Enter}");
    expect(names()).toEqual(["שיחת וידאו", "הקליניקה", "בבית המשפחה"]);
    expect(sentSettings(requests)).toHaveLength(0);
    expect(within(places).getByLabelText("מקום חדש")).toHaveValue("");
    // A place that is already there is not added twice.
    await user.type(within(places).getByLabelText("מקום חדש"), "הקליניקה");
    expect(within(places).getByText("המקום הזה כבר ברשימה.")).toBeInTheDocument();
    expect(add).toBeDisabled();
    await user.clear(within(places).getByLabelText("מקום חדש"));
    expect(await a11yViolations(view.container)).toEqual([]);

    await user.click(saveButton());
    await waitFor(() => expect(sentSettings(requests)).toHaveLength(1));
    expect(sentSettings(requests)[0]?.places).toEqual(["שיחת וידאו", "הקליניקה", "בבית המשפחה"]);
    // The first place is the one a new visit is offered.
    expect(within(places).getByText("מוצע ראשון").closest("li")).toHaveTextContent("שיחת וידאו");
  });

  it("a placeholder chip goes in where the cursor is, and the preview follows what she types", async () => {
    const { user, requests } = await openOn(SETTINGS);
    await form();
    await user.click(topic("הודעות ותזכורות"));
    const messages = part("הודעות להורים");
    const reminder = within(messages).getByLabelText("נוסח התזכורת לפגישה") as HTMLTextAreaElement;
    const preview = () => document.getElementById("settings-reminder-preview") as HTMLElement;
    expect(preview()).toHaveTextContent("כך ההודעה תיראה (הפרטים כאן מומצאים):");
    expect(preview()).toHaveTextContent("שלום ישראלה, תזכורת לפגישה ביום שלישי, 01.01.2030, בשעה 10:00. להתראות, השם שלך");
    expect(reminder).toHaveAccessibleDescription(/שלום ישראלה/);

    const chips = within(messages).getByRole("group", { name: "מילים שמתמלאות לבד, עבור נוסח התזכורת לפגישה" });
    expect(within(chips).getAllByRole("button").map((b) => b.textContent)).toEqual(["{הורה}", "{ילד}", "{יום}", "{תאריך}", "{שעה}", "{מקום}", "{סכום}", "{שמי}"]);
    // After "שלום {הורה}": the cursor is put there, and the chip goes in at that spot.
    await user.click(reminder);
    reminder.setSelectionRange(11, 11);
    await user.click(within(chips).getByRole("button", { name: "להוסיף {ילד}: שם הילד או הילדה" }));
    expect(reminder).toHaveValue("שלום {הורה}{ילד}, תזכורת לפגישה ביום {יום}, {תאריך}, בשעה {שעה}. להתראות, {שמי}");
    expect(reminder).toHaveFocus();
    expect(reminder.selectionStart).toBe(16);
    // Typing goes on from there.
    await user.keyboard(" היקר");
    expect(preview()).toHaveTextContent("שלום ישראלהדני היקר, תזכורת לפגישה");
    // A chip replaces what is selected.
    reminder.setSelectionRange(0, 4);
    await user.click(within(chips).getByRole("button", { name: /^להוסיף {מקום}/ }));
    expect(reminder.value.startsWith("{מקום} {הורה}{ילד} היקר")).toBe(true);
    expect(preview()).toHaveTextContent("הקליניקה ישראלהדני היקר");

    const payment = within(messages).getByLabelText("נוסח הודעת התשלום");
    await user.clear(payment);
    expect(document.getElementById("settings-payment-preview")).toHaveTextContent("הנוסח ריק, ולכן ההודעה תהיה ריקה.");
    await user.type(payment, "נשאר לשלם ");
    await user.click(within(within(messages).getByRole("group", { name: "מילים שמתמלאות לבד, עבור נוסח הודעת התשלום" })).getByRole("button", { name: /^להוסיף {סכום}/ }));
    expect(payment).toHaveValue("נשאר לשלם {סכום}");
    expect(document.getElementById("settings-payment-preview")).toHaveTextContent("נשאר לשלם 450");

    await user.click(saveButton());
    await waitFor(() => expect(sentSettings(requests)).toHaveLength(1));
    expect(sentSettings(requests)[0]).toMatchObject({ payment_text: "נשאר לשלם {סכום}" });
    expect(sentSettings(requests)[0]?.reminder_text.startsWith("{מקום} {הורה}{ילד} היקר, תזכורת")).toBe(true);
  });

  it("note templates are added, edited and removed", async () => {
    const { user, requests } = await openOn(SETTINGS);
    await form();
    await user.click(topic("הודעות ותזכורות"));
    const templates = part("תבניות לסיכום פגישה");
    expect(templates).toHaveTextContent("4 תבניות");
    const observation = within(templates).getByRole("group", { name: "תבנית 3: תצפית" });
    await user.click(within(observation).getByRole("button", { name: "הסרת התבנית" }));
    expect(within(templates).queryByRole("group", { name: /תצפית/ })).toBeNull();
    const second = within(templates).getByRole("group", { name: "תבנית 2: מפגש אבחון" });
    await user.type(within(second).getByLabelText("הטקסט של התבנית"), "שאלות להורים:");
    await user.click(within(templates).getByRole("button", { name: "תבנית חדשה" }));
    const added = within(templates).getByRole("group", { name: "תבנית 4" });
    await user.type(within(added).getByLabelText("שם התבנית"), "שיחת טלפון");
    await user.type(within(added).getByLabelText("הטקסט של התבנית"), "עם מי:");
    expect(within(templates).getByRole("group", { name: "תבנית 4: שיחת טלפון" })).toBeInTheDocument();
    await user.click(saveButton());
    await waitFor(() => expect(sentSettings(requests)).toHaveLength(1));
    const sent = sentSettings(requests)[0]?.note_templates ?? [];
    expect(sent.map((t) => t.name)).toEqual(["פגישת היכרות עם ההורים", "מפגש אבחון", "שיחת משוב", "שיחת טלפון"]);
    expect(sent[1]?.text.endsWith("להמשך:\nשאלות להורים:")).toBe(true);
    expect(sent[3]).toEqual({ name: "שיחת טלפון", text: "עם מי:" });
  });

  it("a template with no name is refused by the core", async () => {
    const { user } = await openOn(SETTINGS);
    await form();
    await user.click(topic("הודעות ותזכורות"));
    await user.click(within(part("תבניות לסיכום פגישה")).getByRole("button", { name: "תבנית חדשה" }));
    await user.click(saveButton());
    expect(await within(bar()).findByRole("alert")).toHaveTextContent("צריך לתת שם לכל תבנית של סיכום.");
  });

  it("unsaved changes survive a look at another tab, and she is told where they wait", async () => {
    const { user, notes, requests } = await openOn(SETTINGS);
    await form();
    await user.click(screen.getByLabelText("שישי"));
    await user.click(topic("הודעות ותזכורות"));
    await user.clear(screen.getByLabelText("ימים עד שמצביעים על חוב"));
    await user.type(screen.getByLabelText("ימים עד שמצביעים על חוב"), "45");
    await user.click(paneButton("כללי"));
    await waitFor(() => expect(notes).toEqual(["השינויים בהגדרות הקליניקה עוד לא נשמרו. הם מחכים בהגדרות, בחלק \"הקליניקה\"."]));
    expect(sentSettings(requests)).toHaveLength(0);

    await user.click(paneButton("הקליניקה"));
    await form();
    // She comes back to the small tab she was on, with every change on every tab.
    expect(topic("הודעות ותזכורות")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByLabelText("ימים עד שמצביעים על חוב")).toHaveValue("45");
    await user.click(topic("יומן ושעות"));
    expect(screen.getByLabelText("שישי")).toBeChecked();
    expect(screen.getByText("השינויים שעשית כאן קודם עוד לא נשמרו, והם כאן כמו שהשארת אותם. אפשר לשמור אותם או לבטל.")).toBeInTheDocument();
    expect(saveButton()).toBeEnabled();

    // "ביטול השינויים" goes back to what is stored, and then nothing waits any more.
    await user.click(within(bar()).getByRole("button", { name: "ביטול השינויים" }));
    expect(screen.getByLabelText("שישי")).not.toBeChecked();
    expect(screen.getByLabelText("ימים עד שמצביעים על חוב")).toHaveValue("30");
    expect(saveButton()).toBeDisabled();
    await user.click(paneButton("כללי"));
    await user.click(paneButton("הקליניקה"));
    await form();
    expect(screen.queryByText(/עוד לא נשמרו, והם כאן/)).toBeNull();
    expect(notes).toHaveLength(1);
  });

  it("leaving with everything saved says nothing", async () => {
    const { user, notes } = await openOn(SETTINGS);
    await form();
    await user.click(paneButton("כללי"));
    await new Promise((r) => setTimeout(r, 30));
    expect(notes).toEqual([]);
  });

  it("“לחזור לברירות המחדל” asks first, says what will change, and then saves the defaults", async () => {
    const served = await openOn(SETTINGS);
    const { user, requests, notes } = served;
    await form();
    await user.click(screen.getByLabelText("שבת"));
    await user.click(within(bar()).getByRole("button", { name: "לחזור לברירות המחדל" }));
    const confirm = await screen.findByRole("dialog", { name: "לחזור לברירות המחדל" });
    expect(confirm).toHaveTextContent("המחירים שקבעת יתאפסו");
    expect(confirm).toHaveTextContent("המטופלים, הפגישות והכספים לא משתנים.");
    expect(await a11yViolations(confirm)).toEqual([]);
    // Cancelling changes nothing, not even what she was in the middle of.
    await user.click(within(confirm).getByRole("button", { name: "ביטול" }));
    expect(sentSettings(requests)).toHaveLength(0);
    expect(screen.getByLabelText("שבת")).toBeChecked();

    await user.click(within(bar()).getByRole("button", { name: "לחזור לברירות המחדל" }));
    await user.click(within(await screen.findByRole("dialog", { name: "לחזור לברירות המחדל" })).getByRole("button", { name: "להחזיר את ברירות המחדל" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(notes).toContain("הגדרות הקליניקה חזרו לברירות המחדל");
    expect(sentSettings(requests)).toEqual([defaultClinicSettings()]);
    expect(await ask(served, { op: "settings" }, "settings")).toEqual(defaultClinicSettings());
    expect(screen.getByLabelText("שבת")).not.toBeChecked();
    await user.click(topic("פגישות ומחירים"));
    expect(screen.getByLabelText("מחיר של מפגש אבחון, בשקלים")).toHaveValue("0");
    expect(within(part("מקומות")).getAllByRole("listitem")).toHaveLength(1);
    await waitFor(() => expect(saveButton()).toBeDisabled());
  });

  it("a reset the vault refuses says so in the dialog, and the settings stay", async () => {
    const { user } = await openOn(SETTINGS, refusing("save_settings", "ההגדרות לא נשמרו."));
    await form();
    await user.click(within(bar()).getByRole("button", { name: "לחזור לברירות המחדל" }));
    const confirm = await screen.findByRole("dialog", { name: "לחזור לברירות המחדל" });
    await user.click(within(confirm).getByRole("button", { name: "להחזיר את ברירות המחדל" }));
    expect(await within(confirm).findByRole("alert")).toHaveTextContent("ההגדרות לא נשמרו.");
    await user.click(within(confirm).getByRole("button", { name: "ביטול" }));
    expect(screen.getByLabelText("מחיר של מפגש אבחון, בשקלים")).toHaveValue("600");
  });

  it("shows the shape of the page while it loads, and settings that could not be read say so", async () => {
    const { over, release } = holding("settings");
    const first = await openOn(SETTINGS, over);
    expect(await screen.findByRole("status", { name: "טוען את הגדרות הקליניקה" })).toBeInTheDocument();
    release();
    await form();
    first.view.unmount();

    await openOn(SETTINGS, refusing("settings", "ההגדרות לא נקראו מהכספת."));
    expect(await screen.findByRole("alert")).toHaveTextContent("ההגדרות לא נקראו מהכספת.");
  });

  it("the copy to Google Calendar says what goes, turns on and off at once, and is outside the form's save", async () => {
    const { user, calls, notes } = await openOn(SETTINGS);
    await form();
    const sync = await screen.findByRole("region", { name: "סנכרון עם יומן גוגל" });
    expect(sync).toHaveTextContent("רק השעה והמילה \"פגישה\"");
    expect(sync).toHaveTextContent("בלי שם, סוג פגישה, מקום או הערה");
    const toggle = within(sync).getByRole("switch", { name: "להעתיק את הפגישות ליומן נפרד בגוגל" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await user.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(calls.some((c) => c.cmd === "calendar_turn_on")).toBe(true);
    expect(notes.at(-1)).toContain("בלי שמות");
    await user.click(within(sync).getByRole("button", { name: "לסנכרן עכשיו" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "calendar_sync_now")).toBe(true));
    await user.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
    expect(notes.at(-1)).toBe("הסנכרון כובה, והיומן \"פגישות\" נמחק מגוגל.");
    // The switch acts by itself: the form has nothing to save.
    expect(saveButton()).toBeDisabled();
  });
});
