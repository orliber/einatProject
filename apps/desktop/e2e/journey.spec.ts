// The main journey in the browser preview: home → new case → materials → the "what leaves the
// computer" review → the report page. All people and texts are invented.
import { expect, test } from "@playwright/test";
import { createCase, expectAccessible, reviewEveryRequest, stage, unlock } from "./helpers";

const CHILD = "יונתן בדוי";
const INTAKE =
  "יונתן בדוי בן חמש. מיכל בדויה, האם, מספרת שהוא אוהב לבנות מגדלים מקוביות ומתקשה להמתין לתורו בגן. " +
  "אלון בדוי, האב, מוסיף שהשינה רגועה ושהדיבור התפתח בזמן.";

test("the lock screen is Hebrew, right-to-left and accessible", async ({ page }) => {
  await page.goto("/preview.html");
  await expect(page.getByRole("heading", { name: "כספת האבחון" })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  await expectAccessible(page);
});

test("the cases screen lists the invented cases and is accessible", async ({ page }) => {
  await unlock(page);
  await expect(page.getByRole("button", { name: /בתיק של נועם$/ })).toBeVisible();
  await expectAccessible(page);
});

test("home → new case → materials → review before sending → report page", async ({ page }) => {
  await unlock(page);
  await reviewEveryRequest(page);

  // New case.
  await createCase(page, CHILD);
  await expect(page.getByText("✓ הסכמה")).toBeVisible();
  await stage(page, /חומרים/).click();
  await expect(page.getByRole("heading", { name: "מה יש בתיק" })).toBeVisible();
  await expectAccessible(page);

  // Materials: an intake written with the real (invented) names.
  await page.getByRole("button", { name: "אינטייק עם ההורים (רישום אינטייק)" }).click();
  const write = page.getByRole("dialog", { name: "הוספת טקסט" });
  await expect(write).toBeVisible();
  // The text box has the focus: she can start typing at once.
  await expect(write.getByLabel("הטקסט")).toBeFocused();
  await expectAccessible(page, "[role=dialog]");
  await write.getByLabel("כותרת").fill("אינטייק בדוי");
  await write.getByLabel("הטקסט").fill(INTAKE);
  await write.getByRole("button", { name: "שמירה בתיק" }).click();
  await expect(write).toBeHidden();
  const list = page.getByRole("region", { name: "רשימת החומרים" });
  await expect(list.getByRole("button", { name: /אינטייק בדוי/ })).toBeVisible();

  // The material opens with the names marked as what will be hidden.
  await list.getByRole("button", { name: /אינטייק בדוי/ }).click();
  const material = page.getByRole("dialog", { name: "אינטייק בדוי" });
  await expect(material.locator("mark.mark-real").first()).toBeVisible();
  await expectAccessible(page, "[role=dialog]");
  await page.keyboard.press("Escape");
  await expect(material).toBeHidden();

  // On the way to writing, the materials are sorted: first the review of what leaves.
  await page.getByRole("button", { name: "לכתיבת הדוח ←" }).click();
  const review = page.getByRole("dialog", { name: /^לפני שליחה ל-Claude/ });
  await expect(review).toBeVisible();
  const outgoing = review.getByLabel("מה Claude יקבל");
  await expect(outgoing).toBeVisible();
  for (const name of ["יונתן", "מיכל", "אלון", "בדוי"]) await expect(outgoing).not.toContainText(name);
  await expect(review.getByLabel("מה שכתוב בתיק")).toContainText("יונתן");
  await expectAccessible(page, "[role=dialog]");
  await review.getByRole("button", { name: /שליחה/ }).click();
  await expect(review).toBeHidden();

  // The report page.
  const report = page.getByRole("article", { name: "הדוח, כמו בקובץ" });
  await expect(report).toBeVisible();
  await expect(stage(page, /כתיבה/)).toHaveAttribute("aria-current", "step");
  await expectAccessible(page);

  // Write one section through the same review, then approve the draft on the page.
  await report.getByRole("button", { name: "✦ לנסח" }).first().click();
  const sectionReview = page.getByRole("dialog", { name: /^לפני שליחה ל-Claude/ });
  await expect(sectionReview).toBeVisible();
  await expect(sectionReview.getByLabel("מה Claude יקבל")).not.toContainText("יונתן");
  await sectionReview.getByRole("button", { name: /שליחה/ }).click();
  const approve = page.getByRole("button", { name: "✓ לאשר" }).first();
  await expect(approve).toBeVisible({ timeout: 20_000 });
  // The draft on the page is written back with the names, locally.
  await approve.click();
  await expectAccessible(page);

  // What left the computer: the request is in the case's history, with the child's name
  // hidden in the text that left (round 27).
  await page.getByRole("button", { name: /^עוד פעולות/ }).click();
  await page.getByRole("menuitem", { name: "מה יצא מהמחשב בתיק הזה" }).click();
  const sent = page.getByRole("dialog", { name: /^מה יצא מהמחשב/ });
  await expect(sent).toBeVisible();
  await sent.getByRole("button", { name: "הטקסט שיצא" }).first().click();
  const asked = sent.getByRole("region", { name: "בקשה" });
  await expect(asked).toBeVisible();
  await expect(asked).not.toContainText("יונתן");
  await expectAccessible(page);
});

test("the export page lists what goes into the file and is accessible", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
  await stage(page, /הוצאת הדוח/).click();
  await expect(stage(page, /הוצאת הדוח/)).toHaveAttribute("aria-current", "step");
  await expect(page.getByText("נשמר בתיקיית ההורדות, ונפתח בלי סיסמה.")).toBeVisible();
  await expectAccessible(page);
});

// The review of stage 3: the section's guide, and her own section's points, are reached from
// the report page (the report is the only way into a case since D-063).
test("the report page opens a section's guide and her own section's points", async ({ page }) => {
  await unlock(page);
  // The preview's cases are in the old template: she marks the background as hers first.
  await page.getByRole("navigation", { name: "ניווט ראשי" }).getByRole("button", { name: "הגדרות" }).click();
  await page.getByRole("navigation", { name: "חלקי ההגדרות" }).getByRole("button", { name: "הדוח" }).click();
  const hers = page.getByRole("checkbox", { name: /^רקע/ });
  // The box follows the saved setting, so it turns on only after the save: click and wait.
  await hers.click();
  await expect(hers).toBeChecked();
  await page.getByRole("navigation", { name: "ניווט ראשי" }).getByRole("button", { name: "תיקים" }).click();
  await createCase(page, CHILD);
  await stage(page, /כתיבה/).click();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
  // The guide is beside the brief of the section in focus (round 48).
  await page.getByRole("button", { name: "איך לכתוב את הסעיף ←" }).click();
  // The link opens the guide on arrival.
  await expect(page.getByText(/הנוסח של המכון|הנוסח שלך, לכל התיקים/)).toBeVisible();
  await page.getByRole("button", { name: "→ חזרה לדוח" }).click();
  // Her own section: its points are written in the report itself, in the same box (D-140).
  await page.getByRole("button", { name: "✦ לנסח מהנקודות" }).first().click();
  await expect(page.getByRole("textbox", { name: /^הנקודות שלך לסעיף/ })).toBeVisible();
  await expectAccessible(page);
  // Her examples are still on the section's page.
  await page.getByRole("button", { name: "דוגמאות מהדוחות שלך ←" }).click();
  await expect(page.getByRole("region", { name: "הסעיף שלך" })).toBeVisible();
});

// Round 48: the report page as the document. The section in focus asks what matters here, the
// draft comes with one row of buttons, and Enter approves it without leaving the page.
test("the report page writes and approves the section in focus with the keyboard", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  const report = page.getByRole("article", { name: "הדוח, כמו בקובץ" });
  await expect(report).toBeVisible();
  const section = report.locator(".a4-section.is-focus");
  await expect(section).toHaveCount(1);
  const brief = section.getByRole("textbox", { name: /^איך לנסח את הסעיף/ });
  await brief.fill("להתחיל מהקשר עם הצוות");
  await brief.press("Enter");
  const review = page.getByRole("dialog", { name: /^לפני שליחה ל-Claude/ });
  const draft = section.getByRole("group", { name: /^הטיוטה של/ });
  await expect(review.or(draft)).toBeVisible({ timeout: 20_000 });
  if (await review.isVisible()) await review.getByRole("button", { name: /שליחה/ }).click();
  await expect(draft).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole("button", { name: "✓ לאשר" })).toHaveCount(1);
  await expectAccessible(page);
  await section.getByRole("heading", { level: 3 }).press("Enter");
  await expect(page.getByRole("button", { name: "✓ לאשר" })).toHaveCount(0);
  await expect(page.getByRole("complementary", { name: "תוכן הדוח" })).toContainText("מאושרים");
});

// D-138: one upload, and the report is read, learned and put in use, with no approving one
// by one; what was learned is listed, and the page stays accessible. The report is invented
// text, made in the page.
test("a style report is read and learned by itself after one upload", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: "הסגנון שלי", exact: true }).first().click();
  const report = "סיכום\nלסיכום, נראה כי הילד הבדוי סקרן ובעל יכולות טובות. מומלץ להמשיך במעקב.\n";
  await page.locator("input[type=file]").evaluate((input: HTMLInputElement, text) => {
    const files = new DataTransfer();
    files.items.add(new File([text], "report.txt", { type: "text/plain" }));
    input.files = files.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
  }, report);
  // In the preview every request is reviewed first: each one is sent from its review.
  for (let i = 0; i < 2; i++) {
    const review = page.getByRole("dialog", { name: /לפני שליחה/ });
    if (await review.isVisible({ timeout: 5000 }).catch(() => false)) await review.getByRole("button", { name: /שליחה/ }).click();
  }
  await expect(page.getByText(/הפרופיל בשימוש, גרסה 1/)).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole("region", { name: "לאשר אחד-אחד" })).toHaveCount(0);
  await expect(page.getByRole("list", { name: "המסלול" })).toBeVisible();
  await expectAccessible(page);
  await page.getByRole("button", { name: /איך את כותבת/ }).click();
  await expect(page.getByRole("region", { name: "איך את כותבת" })).toBeVisible();
  await expectAccessible(page);
});

test("settings, consultation and writing style pages are accessible", async ({ page }) => {
  await unlock(page);
  for (const name of ["הגדרות", "התייעצות", "הסגנון שלי"]) {
    await page.getByRole("button", { name, exact: true }).first().click();
    await expect(page.getByRole("button", { name, exact: true }).first()).toHaveAttribute("aria-current", "page");
    await expect(page.getByRole("heading", { level: 1 }).first()).toBeVisible();
    await expectAccessible(page);
  }
});

test("a whole WISC-V is entered from the keyboard, with the profile beside it (round 54)", async ({ page }) => {
  await unlock(page);
  await createCase(page, "דניאל בדוי", { consent: false });
  await stage(page, /חומרים/).click();
  // From the materials to the table and back to the materials, without the mouse.
  await page.getByRole("button", { name: /ציוני מבחנים \(הזנת ציונים\)/ }).focus();
  await page.keyboard.press("Enter");
  const view = page.getByRole("region", { name: "הזנת ציונים" });
  await expect(view.getByRole("radio", { name: "WPPSI-IV" })).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(view.getByRole("radio", { name: "WISC-V" })).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Tab");
  // Indexes first, as the table lists them; then the ten subtests, Enter passing the others.
  const values: Record<string, string> = {
    "מנת משכל כללית": "96", "הבנה מילולית": "104", "חזותי-מרחבי": "92", "חשיבה פלואידית": "97", "זיכרון עבודה": "88", "מהירות עיבוד": "85",
    "שיתופיות": "11", "אוצר מילים": "10", "קוביות": "9", "פאזלים חזותיים": "8", "מטריצות": "10",
    "משקלות": "9", "זכירת ספרות": "8", "זכירת תמונות": "8", "צפנים": "7", "חיפוש סמלים": "8",
  };
  const ids = await view.locator("input.score-input").evaluateAll((els) => els.map((el) => el.id));
  expect(ids).toHaveLength(22);
  for (const id of ids) {
    const input = view.locator(`#${id}`);
    await expect(input).toBeFocused();
    const name = (await view.locator(`label[for="${id}"]`).innerText()).trim();
    const value = values[name];
    if (value) await page.keyboard.type(value);
    await page.keyboard.press("Enter");
  }
  await expect(view.getByRole("img", { name: /פרופיל תת-המבחנים: שיתופיות 11/ })).toBeVisible();
  await expect(view.getByRole("img", { name: /פרופיל המדדים: מנת משכל כללית 96/ })).toBeVisible();
  await expect(view.getByText("16 ציונים הוזנו")).toBeVisible();
  await expect(view.getByLabel(/הערות על ההעברה/)).toBeFocused();
  await expectAccessible(page);
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await expect(view.getByRole("button", { name: "שמירה בתיק" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "מה יש בתיק" })).toBeVisible();
  await expect(page.getByRole("region", { name: "רשימת החומרים" }).getByRole("button", { name: /WISC-V/ })).toBeVisible();
});
