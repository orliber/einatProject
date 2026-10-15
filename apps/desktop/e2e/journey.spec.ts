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
  await page.getByRole("button", { name: "רישום אינטייק (אינטייק עם ההורים)" }).click();
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
  await report.getByRole("button", { name: "✦ לכתוב עם Claude" }).first().click();
  const sectionReview = page.getByRole("dialog", { name: /^לפני שליחה ל-Claude/ });
  await expect(sectionReview).toBeVisible();
  await expect(sectionReview.getByLabel("מה Claude יקבל")).not.toContainText("יונתן");
  await sectionReview.getByRole("button", { name: /שליחה/ }).click();
  const approve = page.getByRole("button", { name: "✓ לאשר" }).first();
  await expect(approve).toBeVisible({ timeout: 20_000 });
  // The draft on the page is written back with the names, locally.
  await approve.click();
  await expectAccessible(page);
});

test("the export page lists what goes into the file and is accessible", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
  await stage(page, /הוצאת הדוח/).click();
  await expect(stage(page, /הוצאת הדוח/)).toHaveAttribute("aria-current", "step");
  await expect(page.getByText(/סיסמה/).first()).toBeVisible();
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
