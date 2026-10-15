// Safety properties of the screens in a real browser: document and model text is shown as text,
// nothing is fetched from outside the page, and the keyboard reaches what the mouse does.
// All people and texts are invented.
import { expect, test, type Page } from "@playwright/test";
import { createCase, expectAccessible, reviewEveryRequest, stage, unlock } from "./helpers";

const PAYLOAD = `<img src=x onerror="window.__xss=1"><script>window.__xss=2</script><b id="injected">מודגש</b>`;

async function noInjection(page: Page) {
  expect(await page.evaluate(() => (window as unknown as { __xss?: number }).__xss)).toBeUndefined();
  await expect(page.locator('img[src="x"]')).toHaveCount(0);
  await expect(page.locator("#injected")).toHaveCount(0);
}

test("markup in a material is shown as text, not run as HTML", async ({ page }) => {
  await unlock(page);
  await createCase(page, "גיל בדוי");
  await stage(page, /חומרים/).click();
  await page.getByRole("button", { name: "אינטייק עם ההורים (רישום אינטייק)" }).click();
  const write = page.getByRole("dialog", { name: "הוספת טקסט" });
  await write.getByLabel("כותרת").fill("חומר עם תגיות");
  await write.getByLabel("הטקסט").fill(`גיל בונה מגדלים. ${PAYLOAD}`);
  await write.getByRole("button", { name: "שמירה בתיק" }).click();
  await expect(write).toBeHidden();

  await page.getByRole("region", { name: "רשימת החומרים" }).getByRole("button", { name: /חומר עם תגיות/ }).click();
  const material = page.getByRole("dialog", { name: "חומר עם תגיות" });
  // The markup is visible as characters on the page.
  await expect(material).toContainText('<img src=x onerror="window.__xss=1">');
  await expect(material).toContainText("<script>");
  await noInjection(page);
});

test("a consultation question with markup is echoed as text", async ({ page }) => {
  await unlock(page);
  await reviewEveryRequest(page);
  await page.getByRole("button", { name: "התייעצות", exact: true }).click();
  await page.locator("#consult-q").fill(`מה כדאי לבדוק? ${PAYLOAD}`);
  await page.getByRole("button", { name: "שליחה", exact: true }).click();
  const review = page.getByRole("dialog", { name: /^לפני שליחה ל-Claude/ });
  await expect(review).toBeVisible();
  await expect(review).toContainText("<script>");
  await noInjection(page);
  await review.getByRole("button", { name: /^שליחה/ }).click();
  await expect(review).toBeHidden();
  const chat = page.getByRole("region", { name: "השיחה" });
  await expect(chat).toContainText("<script>");
  // The answer arrives (the demo core answers from local text).
  await expect(chat.locator(".chat-row.pending")).toHaveCount(0, { timeout: 20_000 });
  await expect(chat.locator(".chat-row-ai").first()).toBeVisible();
  await noInjection(page);
  await expectAccessible(page);
});

test("the preview loads nothing from outside its own address", async ({ page, baseURL }) => {
  const outside: string[] = [];
  page.on("request", (r) => {
    const url = r.url();
    if (!url.startsWith(String(baseURL)) && !url.startsWith("data:") && !url.startsWith("blob:")) outside.push(url);
  });
  await unlock(page);
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
  await stage(page, /חומרים/).click();
  await stage(page, /הוצאת הדוח/).click();
  await page.getByRole("button", { name: "→ תיקים" }).click();
  await page.getByRole("button", { name: "הגדרות", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 }).first()).toBeVisible();
  expect(outside).toEqual([]);
});

test("the lock screen works from the keyboard alone", async ({ page }) => {
  await page.goto("/preview.html");
  // The password field has focus on arrival; Enter opens the vault.
  await expect(page.getByLabel("סיסמה")).toBeFocused();
  await page.keyboard.type("סיסמה בדויה");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "+ תיק חדש" })).toBeVisible();
});

test("Ctrl+L locks at once, also with the Hebrew layout", async ({ page }) => {
  await unlock(page);
  // With the Hebrew layout on, the L key types "ך"; the shortcut goes by the physical key.
  await page.keyboard.down("Control");
  await page.keyboard.press("KeyL");
  await page.keyboard.up("Control");
  await expect(page.getByLabel("סיסמה")).toBeVisible();
  await expect(page.getByRole("button", { name: "+ תיק חדש" })).toHaveCount(0);
});

test("F1 opens the shortcuts sheet and Escape closes it", async ({ page }) => {
  await unlock(page);
  await page.keyboard.press("F1");
  const sheet = page.getByRole("dialog", { name: "קיצורי מקלדת" });
  await expect(sheet).toBeVisible();
  await expectAccessible(page, "[role=dialog]");
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
});

test("the new case dialog takes focus, Tab stays useful, and Escape asks before losing typing", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: "+ תיק חדש" }).click();
  const dialog = page.getByRole("dialog", { name: "תיק חדש" });
  await expect(dialog).toBeFocused();
  await expectAccessible(page, "[role=dialog]");
  // Tab moves through the dialog's fields: the first stop is inside it.
  await page.keyboard.press("Tab");
  expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);

  await dialog.locator("#name-0").fill("עדי בדויה");
  await page.keyboard.press("Escape");
  const confirm = dialog.getByRole("alertdialog", { name: "לסגור בלי לשמור" });
  await expect(confirm).toBeVisible();
  // Escape again keeps the typing.
  await page.keyboard.press("Escape");
  await expect(confirm).toBeHidden();
  await expect(dialog.locator("#name-0")).toHaveValue("עדי בדויה");
  await page.keyboard.press("Escape");
  await dialog.getByRole("button", { name: "לסגור בלי לשמור" }).click();
  await expect(dialog).toBeHidden();
});

test("a new case without the child's name is refused with a clear message", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: "+ תיק חדש" }).click();
  const dialog = page.getByRole("dialog", { name: "תיק חדש" });
  await dialog.getByRole("button", { name: "פתיחת התיק" }).click();
  await expect(dialog.getByRole("alert")).toContainText("צריך את שם הילד/ה");
  await expect(dialog).toBeVisible();
});

test("a case without consent cannot send, and says how to fix it", async ({ page }) => {
  await unlock(page);
  await createCase(page, "רון בדוי", { consent: false });
  await expect(page.getByText("חסרה הסכמה")).toBeVisible();
  await stage(page, /חומרים/).click();
  const next = page.getByRole("contentinfo", { name: "הצעד הבא" }).or(page.locator("footer.next-bar"));
  await expect(next).toContainText("צריך לרשום את הסכמת ההורים");
  await next.getByRole("button", { name: "רישום ההסכמה" }).click();
  await expect(page.getByRole("heading", { level: 1 }).first()).toBeVisible();
  await expectAccessible(page);
});
