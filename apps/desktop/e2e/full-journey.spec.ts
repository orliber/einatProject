// The whole journey, from the first run to the file, over the browser preview (the in-memory
// demo core, D-021). Every test stands on its own and starts from a fresh page; every name and
// text is invented. What the demo core cannot do is said where it is skipped:
//  - any password opens the demo vault, so a wrong password cannot be refused here (only an
//    empty one is not sent);
//  - no file is written: the export answers with a sentence instead of a path;
//  - the idle countdown never runs down by itself, so the last minute is reached by setting
//    the lock to one minute through the same IPC the settings use (the UI offers 5 at least).
import { expect, test, type Page } from "@playwright/test";
import { createCase, expectAccessible, reviewEveryRequest, stage, unlock } from "./helpers";

const PASSWORD = "סיסמה בדויה לבדיקה";
const NEW_PASSWORD = "משפט חדש וארוך לבדיקה";

/** One command straight to the demo core, as the window sends it (for what the UI has no button for). */
function invokeFake(page: Page, cmd: string, args: unknown): Promise<unknown> {
  return page.evaluate(([c, a]) => (window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__.invoke(c as string, a), [cmd, args]);
}

function settingsPane(page: Page, label: string) {
  return page.getByRole("navigation", { name: "חלקי ההגדרות" }).getByRole("button", { name: label });
}

async function openSettings(page: Page, pane: string): Promise<void> {
  await page.getByRole("navigation", { name: "ניווט ראשי" }).getByRole("button", { name: "הגדרות" }).click();
  await settingsPane(page, pane).click();
  await expect(settingsPane(page, pane)).toHaveAttribute("aria-current", "page");
}

/** An intake with the (invented) names of the child and the parents. */
async function addIntake(page: Page, child: string): Promise<void> {
  await stage(page, /חומרים/).click();
  await page.getByRole("button", { name: "אינטייק עם ההורים (רישום אינטייק)" }).click();
  const write = page.getByRole("dialog", { name: "הוספת טקסט" });
  await expect(write).toBeVisible();
  await write.getByLabel("כותרת").fill("אינטייק בדוי");
  await write.getByLabel("הטקסט").fill(
    `${child} בן חמש. מיכל בדויה, האם, מספרת שהוא אוהב לבנות מגדלים ומתקשה להמתין לתורו בגן. ` +
    "אלון בדוי, האב, מוסיף שהשינה רגועה ושהדיבור התפתח בזמן. הגננת רונית מתארת ילד סקרן.",
  );
  await write.getByRole("button", { name: "שמירה בתיק" }).click();
  await expect(write).toBeHidden();
  await expect(page.getByRole("region", { name: "רשימת החומרים" }).getByRole("button", { name: /אינטייק בדוי/ })).toBeVisible();
}

/** The lock screen, with nothing of any case left on the page. */
async function expectLockedAndClean(page: Page, names: string[]): Promise<void> {
  await expect(page.getByLabel("סיסמה")).toBeVisible();
  await expect(page.getByRole("button", { name: "+ תיק חדש" })).toHaveCount(0);
  await expect(page.getByRole("navigation", { name: "שלבי התיק" })).toHaveCount(0);
  const text = await page.locator("body").innerText();
  for (const name of names) expect(text, `"${name}" is still on the screen after the lock`).not.toContain(name);
}

test("first run: a vault is created, Google and the first backup are offered, and the cases list opens", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/preview.html?setup");
  await expect(page.getByRole("heading", { name: "ברוכה הבאה לכספת האבחון" })).toBeVisible();
  await expect(page.getByRole("list", { name: "שלבי ההגדרה" })).toBeVisible();
  await expect(page.locator("#pw1")).toBeFocused();
  await expectAccessible(page);

  // Too short, then two that differ: refused with a plain sentence, nothing created.
  await page.locator("#pw1").fill("קצר מדי");
  await page.locator("#pw2").fill("קצר מדי");
  await page.getByRole("button", { name: "יצירת הכספת" }).click();
  await expect(page.getByRole("alert")).toContainText("לפחות 12 תווים");
  await page.locator("#pw1").fill(PASSWORD);
  await page.locator("#pw2").fill(`${PASSWORD} אחרת`);
  await page.getByRole("button", { name: "יצירת הכספת" }).click();
  await expect(page.getByRole("alert")).toContainText("לא זהות");
  await expect(page.getByRole("list", { name: "דרישות הסיסמה" })).toContainText("12 תווים לפחות");

  await page.locator("#pw2").fill(PASSWORD);
  await page.getByRole("button", { name: "יצירת הכספת" }).click();

  // Sign in with Google when the password is forgotten (D-103): offered, and left for later.
  await expect(page.getByRole("heading", { name: /להפעיל כניסה עם גוגל/ })).toBeVisible();
  await page.getByRole("button", { name: "אחר כך" }).click();

  // The first backup to a removable drive: the demo core answers with a path.
  await expect(page.getByRole("heading", { name: "גיבוי לדיסק נייד" })).toBeVisible();
  await expectAccessible(page);
  await page.getByRole("button", { name: /בחירת מקום וגיבוי ראשון/ }).click();
  await expect(page.getByRole("heading", { name: "הפרטים שלך" })).toBeVisible();
  await expect(page.getByRole("status")).toContainText("הגיבוי הראשון נשמר");
  const steps = page.getByRole("list", { name: "שלבי ההגדרה" });
  await expect(steps.locator("[aria-current=step]")).toHaveText(/הפרטים שלך/);

  await page.locator("#names").fill("רותם בדויה, מכון בדוי");
  await page.getByRole("button", { name: "סיום וכניסה" }).click();
  await expect(page.getByRole("button", { name: "+ תיק חדש" })).toBeVisible();
  // Her first name is in the greeting; the practitioner's names are hidden before any send.
  await expect(page.getByRole("heading", { level: 1 })).toContainText("רותם");
  expect(errors).toEqual([]);
});

test("the vault locks from the keyboard, nothing of the case stays on screen, and it opens again", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
  await expect(page.locator(".case-child")).toContainText("נועם");

  // Ctrl+L, by the physical key (the Hebrew layout types "ך" on it).
  await page.keyboard.down("Control");
  await page.keyboard.press("KeyL");
  await page.keyboard.up("Control");
  await expectLockedAndClean(page, ["נועם", "תיק-1024", "דנה", "יוסי"]);
  await expectAccessible(page);

  // An empty password is not sent at all; the demo core opens with any other one (a wrong
  // password cannot be refused in the preview, so that is left to the core's own tests).
  await page.getByLabel("סיסמה").press("Enter");
  await expect(page.getByLabel("סיסמה")).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);

  // "Forgot the password": with Google off, said plainly, with the way back.
  await page.getByRole("button", { name: "שכחתי את הסיסמה" }).click();
  await expect(page.getByRole("note").filter({ hasText: /אי אפשר לבחור סיסמה חדשה מכאן/ })).toBeVisible();
  await page.getByRole("button", { name: "חזרה לפתיחה עם סיסמה" }).click();
  await expect(page.getByLabel("סיסמה")).toBeFocused();

  await page.getByLabel("סיסמה").fill(PASSWORD);
  await page.getByRole("button", { name: "פתיחה", exact: true }).click();
  await expect(page.getByRole("button", { name: "+ תיק חדש" })).toBeVisible();
  // The lock brings her back to the list, never into the case she left.
  await expect(page.getByRole("navigation", { name: "שלבי התיק" })).toHaveCount(0);
});

test("a new case with consent, an intake, a WPPSI-IV table, and the review that hides the names on the way to writing", async ({ page }) => {
  await unlock(page);
  await reviewEveryRequest(page);
  const child = "ליאור בדוי";
  await createCase(page, child);
  await expect(page.getByText("✓ הסכמה")).toBeVisible();
  await addIntake(page, child);

  // One instrument, entered from the keyboard: the band in words and the profile beside it.
  await page.getByRole("button", { name: /ציוני מבחנים \(הזנת ציונים\)/ }).click();
  const scores = page.getByRole("region", { name: "הזנת ציונים" });
  await expect(scores.getByRole("radio", { name: "WPPSI-IV" })).toHaveAttribute("aria-checked", "true");
  await scores.getByLabel("מנת משכל כללית", { exact: true }).fill("96");
  await scores.getByLabel("הבנה מילולית", { exact: true }).fill("104");
  await scores.getByLabel("זיכרון עבודה", { exact: true }).fill("85");
  await expect(scores.getByRole("img", { name: /פרופיל המדדים: מנת משכל כללית 96/ })).toBeVisible();
  await expect(scores.getByText("3 ציונים הוזנו")).toBeVisible();
  // A score outside the measure's range is marked at once, and not saved.
  const working = scores.getByLabel("מהירות עיבוד", { exact: true });
  await working.fill("250");
  await expect(working).toHaveAttribute("aria-invalid", "true");
  await scores.getByRole("button", { name: "שמירה בתיק" }).click();
  await expect(scores.getByRole("alert")).toContainText("מחוץ לטווח");
  await working.fill("");
  await expect(scores.getByRole("region", { name: "מה יישמר בתיק" })).toContainText("96");
  await expectAccessible(page);
  await scores.getByRole("button", { name: "שמירה בתיק" }).click();
  await expect(page.getByRole("heading", { name: "מה יש בתיק" })).toBeVisible();
  const list = page.getByRole("region", { name: "רשימת החומרים" });
  await expect(list.getByRole("button", { name: /WPPSI-IV/ })).toBeVisible();

  // A material opens in a dialog, and its editor opens above it: Escape closes one at a time,
  // and focus goes back to the button that opened the material.
  const opener = list.getByRole("button", { name: /אינטייק בדוי/ });
  await opener.focus();
  await page.keyboard.press("Enter");
  const material = page.getByRole("dialog", { name: "אינטייק בדוי" });
  await expect(material).toBeVisible();
  await material.getByRole("button", { name: "עריכה" }).click();
  const editor = page.getByRole("dialog", { name: "עריכת חומר" });
  await expect(editor).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(editor).toBeHidden();
  await expect(material).toBeVisible();
  // Tab stays inside the open dialog.
  for (let i = 0; i < 12; i++) await page.keyboard.press("Tab");
  await expect(material.locator(":focus")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(material).toBeHidden();
  await expect(opener).toBeFocused();

  // On the way to writing, the materials are sorted: first what leaves the computer.
  await page.getByRole("button", { name: "לכתיבת הדוח ←" }).click();
  const review = page.getByRole("dialog", { name: /^לפני שליחה ל-Claude/ });
  await expect(review).toBeVisible();
  await expect(review).toContainText("מיון החומרים לסעיפים");
  const outgoing = review.getByLabel("מה Claude יקבל");
  // (The preview's stand-in filter hides the names of the case; the real gate also catches a
  // name after "הגננת". Only the case's names are checked here.)
  for (const name of ["ליאור", "מיכל", "אלון", "בדוי"]) await expect(outgoing).not.toContainText(name);
  await expect(review.getByLabel("מה שכתוב בתיק")).toContainText("ליאור");
  await expect(review.getByRole("button", { name: /^שליחה/ })).toBeEnabled();
  await expectAccessible(page, "[role=dialog]");
  // "Back to editing" sends nothing, and she stays on the materials page; the same button asks again.
  await review.getByRole("button", { name: "חזרה לעריכה" }).click();
  await expect(review).toBeHidden();
  await expect(page.getByRole("heading", { name: "מה יש בתיק" })).toBeVisible();
  await expect(stage(page, /חומרים/)).toHaveAttribute("aria-current", "step");
  await page.getByRole("button", { name: "לכתיבת הדוח ←" }).click();
  await expect(review).toBeVisible();
  await expect(review.getByLabel("מה Claude יקבל")).not.toContainText("ליאור");
  await review.getByRole("button", { name: /^שליחה/ }).click();
  await expect(review).toBeHidden();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
  await expect(stage(page, /כתיבה/)).toHaveAttribute("aria-current", "step");
  const contents = page.getByRole("complementary", { name: "תוכן הדוח" });
  await expect(contents.getByRole("status")).toHaveCount(0, { timeout: 15_000 });
  await expect(page.getByRole("status").filter({ hasText: /מוין|מיון/ })).toBeVisible();
});

test("two sections are written and approved, a paragraph is edited by hand, and its earlier wording is brought back", async ({ page }) => {
  await unlock(page);
  const child = "נגה בדויה";
  await createCase(page, child);
  await addIntake(page, child);
  // Requests go at once (the review screen is off by default, D-062): the sorting too.
  await page.getByRole("button", { name: "לכתיבת הדוח ←" }).click();
  const report = page.getByRole("article", { name: "הדוח, כמו בקובץ" });
  await expect(report).toBeVisible();
  const contents = page.getByRole("complementary", { name: "תוכן הדוח" });
  await expect(contents).toContainText(/0 \/ \d+ מאושרים/);

  // The first section: from the section in focus.
  const focused = report.locator(".a4-section.is-focus");
  await expect(focused).toHaveCount(1);
  const firstId = await focused.getAttribute("id");
  await focused.getByRole("button", { name: "✦ לנסח" }).click();
  const approve = report.getByRole("button", { name: "✓ לאשר" });
  await expect(approve).toBeVisible({ timeout: 20_000 });
  await expect(report.locator(".a4-section.has-pending")).toHaveAttribute("id", firstId ?? "");
  // The draft is written back with the names, locally.
  await expect(report.locator(".a4-section.has-pending")).toContainText("נגה");
  await approve.click();
  await expect(approve).toHaveCount(0);
  await expect(contents).toContainText(/1 \/ \d+ מאושרים/);

  // The second: another section with material, out of focus, from its one line.
  const gap = report.locator(".a4-gap-line").getByRole("button", { name: "✦ לנסח" }).first();
  const secondId = await gap.evaluate((b) => b.closest(".a4-section")?.id ?? "");
  expect(secondId).not.toBe(firstId);
  // Out of focus, "✦ לנסח" opens the section's box (D-140); its own "✦ לנסח" sends.
  await gap.click();
  await report.locator(`#${secondId}`).getByRole("button", { name: "✦ לנסח" }).click();
  await expect(approve).toBeVisible({ timeout: 20_000 });
  await expect(report.locator(".a4-section.has-pending")).toHaveAttribute("id", secondId);
  await approve.click();
  await expect(approve).toHaveCount(0);
  await expect(contents).toContainText(/2 \/ \d+ מאושרים/);
  await expect(report.locator(`#${secondId}`)).toHaveClass(/is-focus/);
  await expectAccessible(page);

  // By hand: the first paragraph of the section in focus.
  const para = report.locator(`#${secondId} .a4-p`).first();
  const original = (await para.innerText()).trim();
  expect(original.length).toBeGreaterThan(10);
  await para.click();
  const editor = report.getByRole("textbox", { name: /^עריכת פסקה בסעיף/ });
  await expect(editor).toBeFocused();
  await expect(editor).toHaveValue(original);
  // (No names here: the demo core writes a tagged name back as the full name of the case.)
  const edited = "פסקה שנערכה ביד: הגיעה למפגש בשמחה ונפרדה בקלות, ושיתפה פעולה לאורך כל המפגש.";
  await editor.fill(edited);
  await report.getByRole("button", { name: "שמירה", exact: true }).click();
  await expect(editor).toHaveCount(0);
  await expect(report.locator(`#${secondId} .a4-p`).first()).toHaveText(edited);
  await expect(contents).toContainText(/2 \/ \d+ מאושרים/);

  // The earlier wording is kept, and comes back in one click (UX-4): the paragraph's tools
  // show when the keyboard is on it, and Ctrl+Shift+H opens the versions.
  await report.locator(`#${secondId} .a4-p`).first().focus();
  await page.keyboard.press("Control+Shift+KeyH");
  const versions = page.getByRole("dialog", { name: "גרסאות קודמות של הפסקה" });
  await expect(versions).toBeVisible();
  await expect(versions.locator(".version-text").first()).toHaveText(original);
  await versions.getByRole("button", { name: "להחזיר את הניסוח הזה" }).first().click();
  await expect(versions).toBeHidden();
  await expect(report.locator(`#${secondId} .a4-p`).first()).toHaveText(original);
  // Esc on the editor with a change asks first; nothing is lost by one key.
  await report.locator(`#${secondId} .a4-p`).first().click();
  await editor.fill(`${original} ועוד.`);
  await editor.press("Escape");
  await expect(report.getByRole("group", { name: "לבטל את העריכה?" })).toBeVisible();
  await report.getByRole("button", { name: "להמשיך לערוך" }).click();
  await expect(editor).toHaveValue(`${original} ועוד.`);
  await report.getByRole("button", { name: "ביטול" }).click();
  await report.getByRole("button", { name: "כן, לבטל" }).click();
  await expect(editor).toHaveCount(0);
  await expect(report.locator(`#${secondId} .a4-p`).first()).toHaveText(original);
});

test("the finish page checks the report, and the Word, PDF and parents' versions are made (no file in the demo)", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
  await stage(page, /הוצאת הדוח/).click();
  await expect(stage(page, /הוצאת הדוח/)).toHaveAttribute("aria-current", "step");
  const side = page.getByRole("region", { name: "לפני שהקובץ יוצא" });
  await expect(side.getByRole("heading", { level: 1 })).toHaveText(/הדוח מוכן להוצאה|כמעט מוכן/);
  await expect(side.locator(".finish-checks")).toContainText(/\d+ סעיפים מאושרים ייכנסו לקובץ/);
  await expect(side.locator(".finish-checks")).toContainText("השמות חוזרים למקומם בקובץ.");
  // The page beside it is the file: the name back in place, the sections as they will print.
  const preview = page.getByRole("region", { name: "תצוגה מקדימה של הקובץ" });
  await expect(preview).toContainText("נועם");
  await expect(preview).toContainText("דוח אבחון פסיכולוגי");
  await expectAccessible(page);

  // Word.
  await side.getByRole("button", { name: "הפקת הקובץ" }).click();
  const dialog = page.getByRole("dialog", { name: "הפקת דוח Word" });
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".checklist")).toContainText(/סעיפים עם פסקאות מאושרות/);
  await expect(dialog).toContainText("הקובץ נפתח בלי סיסמה");
  await expect(dialog).toContainText(/שם הקובץ:.*תיק-1024\.docx/);
  await expect(dialog).not.toContainText(/שם הקובץ:.*נועם/);
  await expectAccessible(page, "[role=dialog]");
  await dialog.getByRole("button", { name: "הפקת הדוח" }).click();
  const done = page.getByRole("dialog", { name: "הדוח מוכן" });
  await expect(done).toBeVisible();
  await expect(done).toContainText("בהדמיה בדפדפן לא נוצר קובץ");
  await done.locator("footer").getByRole("button", { name: "סגירה" }).click();
  await expect(done).toBeHidden();

  // PDF, print only.
  await side.getByRole("button", { name: "הפקת הקובץ" }).click();
  await dialog.getByRole("radio", { name: /PDF להדפסה בלבד/ }).check();
  await expect(dialog).toContainText("את ה-PDF אי אפשר לשנות");
  await dialog.getByRole("button", { name: "הפקת הדוח" }).click();
  await expect(done).toContainText("בהדמיה בדפדפן לא נוצר קובץ");
  await done.locator("footer").getByRole("button", { name: "סגירה" }).click();

  // The parents' version (D-095): no numbers unless she says so, and its own file name.
  await side.getByRole("button", { name: "הפקת הקובץ" }).click();
  await dialog.getByRole("radio", { name: "גרסה להורים" }).check();
  await expect(dialog.getByRole("switch", { name: /להציג ציונים בגרסה להורים/ })).toHaveAttribute("aria-checked", "false");
  await expect(dialog).toContainText(/שם הקובץ:.*- להורים\.docx/);
  await dialog.getByRole("button", { name: "הפקת הגרסה להורים" }).click();
  await expect(page.getByRole("dialog", { name: "הגרסה להורים מוכנה" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("what left the computer: the request is listed with the child's name hidden, and a search for the name finds only that it was hidden", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  const report = page.getByRole("article", { name: "הדוח, כמו בקובץ" });
  await expect(report).toBeVisible();
  // One request (the review screen is off: it goes at once, hidden).
  await report.locator(".a4-section.is-focus").getByRole("button", { name: "✦ לנסח" }).click();
  await expect(report.getByRole("button", { name: "✓ לאשר" })).toBeVisible({ timeout: 20_000 });

  // From settings → privacy: every case.
  await page.getByRole("button", { name: "→ תיקים" }).click();
  await openSettings(page, "פרטיות");
  await page.getByRole("region", { name: "שום שם לא יצא מהמחשב." }).getByRole("button", { name: "מה יצא מהמחשב" }).click();
  const sent = page.getByRole("dialog", { name: "מה יצא מהמחשב" });
  await expect(sent).toBeVisible();
  const rows = sent.getByRole("list", { name: "בקשות, מהחדשה לישנה" }).getByRole("listitem");
  await expect(rows.first()).toContainText("תיק-1024");
  await expect(rows.first()).toContainText(/הוסתר/);
  await rows.first().getByRole("button", { name: "הטקסט שיצא" }).click();
  const asked = sent.getByRole("region", { name: "בקשה" });
  await expect(asked).toBeVisible();
  for (const name of ["נועם", "דנה", "יוסי"]) await expect(asked).not.toContainText(name);
  await expect(sent).toContainText("זהה בדיוק למה שיצא");
  await expectAccessible(page, "[role=dialog]");

  // The child's name is not in any request; the search says it was hidden.
  await sent.getByLabel("חיפוש בבקשות").fill("נועם");
  await sent.getByRole("button", { name: "חיפוש" }).click();
  await expect(sent.locator(".sent-match").first()).toContainText("הוסתר בבקשה הזו: לא יצא מהמחשב");
  await sent.getByRole("button", { name: "ניקוי" }).click();
  await sent.locator("footer").getByRole("button", { name: "סגירה" }).click();
  await expect(sent).toBeHidden();
});

test("settings: a backup is written and checked, the password is changed, a new recovery kit is confirmed, and readiness is ticked", async ({ page }) => {
  await unlock(page);
  await openSettings(page, "גיבוי");
  const backup = page.getByRole("region", { name: "גיבוי", exact: true });
  // The demo vault starts with a backup from two days ago.
  await expect(backup).toContainText("הגיבוי האחרון");
  await expectAccessible(page);
  await backup.getByRole("button", { name: "גיבוי עכשיו…" }).click();
  await expect(page.getByRole("status").filter({ hasText: /הגיבוי נשמר מוצפן/ })).toBeVisible();
  await expect(backup).toContainText("הגיבוי האחרון: היום");
  await expect(backup.locator(".chip").first()).toHaveText("עדכני");

  // The restore drill (D-024): the backup opens in a scratch copy, with the password.
  await backup.getByRole("button", { name: "בדיקת גיבוי…" }).click();
  const drill = page.getByRole("dialog", { name: "בדיקת גיבוי" });
  await expect(drill).toBeVisible();
  await expect(drill.locator("#check-pw")).toBeFocused();
  await drill.locator("#check-pw").fill(PASSWORD);
  await drill.getByRole("button", { name: "בדיקה" }).click();
  await expect(drill.getByRole("status")).toContainText("הגיבוי נפתח ותקין");
  await expect(drill.getByRole("status")).toContainText(/נמצאו בו .*תיקים/);
  await expectAccessible(page, "[role=dialog]");
  await drill.locator("footer").getByRole("button", { name: "סגירה" }).click();
  await expect(drill).toBeHidden();
  await expect(backup).toContainText("בדיקת השחזור האחרונה");

  // A new password: too short is refused; then it goes, and the old backup is marked.
  await settingsPane(page, "סיסמה ואבטחה").click();
  await page.getByRole("button", { name: "החלפת סיסמה…" }).click();
  const change = page.getByRole("dialog", { name: "החלפת סיסמה" });
  await expect(change.locator("#current-secret")).toBeFocused();
  await change.locator("#current-secret").fill(PASSWORD);
  await change.locator("#new-pw").fill("קצרה");
  await change.locator("#new-pw2").fill("קצרה");
  await change.getByRole("button", { name: "החלפה" }).click();
  await expect(change.getByRole("alert")).toContainText("12 תווים לפחות");
  await expect(change).toBeVisible();
  await change.locator("#new-pw").fill(NEW_PASSWORD);
  await change.locator("#new-pw2").fill(NEW_PASSWORD);
  await expect(change.getByRole("list", { name: "דרישות הסיסמה" }).locator(".met")).toHaveCount(2);
  await change.getByRole("button", { name: "החלפה" }).click();
  await expect(change).toBeHidden();
  await expect(page.getByRole("status").filter({ hasText: /הסיסמה הוחלפה/ })).toBeVisible();

  // The printed kit, under "מתקדם" (D-103): made, then typed back before the window closes.
  await page.getByText("מתקדם: ערכת שחזור מודפסת").click();
  await page.getByRole("button", { name: "יצירת ערכה…" }).click();
  const kit = page.getByRole("dialog", { name: "ערכת שחזור חדשה" });
  await kit.locator("#current-secret").fill(NEW_PASSWORD);
  await kit.getByRole("button", { name: "יצירת ערכה חדשה" }).click();
  const shown = page.getByRole("dialog", { name: "ערכת השחזור החדשה" });
  await expect(shown).toBeVisible();
  // No closing until it is confirmed: Escape and × are not there.
  await expect(shown.getByRole("button", { name: "סגירה" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(shown).toBeVisible();
  const key = (await shown.locator(".kit-grid").innerText()).replace(/\s+/g, "-").replace(/^-|-$/g, "");
  expect(key).toMatch(/^[A-Z0-9-]{20,}$/);
  await expect(shown.getByRole("button", { name: "סיום" })).toBeDisabled();
  await shown.locator("#kit-typed").fill(key);
  await shown.getByRole("button", { name: "סיום" }).click();
  await expect(shown).toBeHidden();
  await expect(page.getByRole("status").filter({ hasText: /ערכת השחזור החדשה נשמרה/ })).toBeVisible();

  // The old backup no longer opens with the new secret: said on the backup page.
  await settingsPane(page, "גיבוי").click();
  await expect(backup).toContainText("הסיסמה הוחלפה מאז");
  await expect(backup.locator(".chip").first()).toHaveText("לחדש");

  // "Ready for real cases": a manual item is confirmed and counted.
  await settingsPane(page, "כללי").click();
  const ready = page.getByRole("region", { name: "מוכנה לתיקים אמיתיים?" });
  const before = Number(/(\d+) מתוך/.exec(await ready.locator(".ready-count").innerText())?.[1]);
  await ready.getByRole("button", { name: /^אישרתי:/ }).first().click();
  await expect(ready.locator(".ready-count")).toContainText(`${before + 1} מתוך`);
  await expect(ready.getByRole("button", { name: /^ביטול האישור:/ }).first()).toBeVisible();
});

test("the last minute before the idle lock offers to keep working, and the lock from the case menu clears the screen", async ({ page }) => {
  await unlock(page);
  await openSettings(page, "פרטיות");
  // The shortest the settings offer is 5 minutes; the demo's countdown is the setting itself,
  // so one minute is set through the same command the buttons send.
  await page.getByRole("radiogroup", { name: "נעילה אוטומטית אחרי" }).getByRole("radio", { name: "5", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: /תינעל אחרי 5 דקות/ })).toBeVisible();
  await invokeFake(page, "set_lock_minutes", { minutes: 1 });
  // The next reading of the status (every setting saved reads it again) brings the warning.
  const review = page.getByRole("switch", { name: /להציג לי כל בקשה לפני שליחה/ });
  await review.click();
  const warning = page.getByRole("alert").filter({ hasText: /הכספת תינעל בעוד/ });
  await expect(warning).toBeVisible();
  await expect(warning).toContainText(/כ-60 שניות/);
  await expectAccessible(page);
  await invokeFake(page, "set_lock_minutes", { minutes: 10 });
  await warning.getByRole("button", { name: "להמשיך לעבוד" }).click();
  await expect(warning).toBeHidden();
  await review.click();

  // Into a case, then the lock from its menu.
  await page.getByRole("navigation", { name: "ניווט ראשי" }).getByRole("button", { name: "תיקים" }).click();
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  await expect(page.locator(".case-child")).toContainText("נועם");
  await page.getByRole("button", { name: /^עוד פעולות/ }).click();
  await page.getByRole("menuitem", { name: /^נעילה/ }).click();
  await expectLockedAndClean(page, ["נועם", "תיק-1024", "דנה", "יוסי"]);
});
