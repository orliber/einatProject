// The window as it is on a laptop with Windows at 125 % scaling: a 1366×768 screen leaves
// 1093×614 under the taskbar. Nothing is cut off at the sides, nothing scrolls sideways, and a
// dialog keeps its buttons on screen, also with the text one step larger. `?nofit` shows the
// preview at the window's real size instead of scaling it down. All names are invented.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { createCase, unlock } from "./helpers";

test.use({ viewport: { width: 1093, height: 614 } });

const PAGE = "/preview.html?nofit";

/** The element is wholly inside the window. */
async function expectOnScreen(page: Page, el: Locator): Promise<void> {
  await expect(el).toBeVisible();
  const box = await el.boundingBox();
  const view = page.viewportSize();
  if (!box || !view) throw new Error("no box");
  expect(box.x, "left edge").toBeGreaterThanOrEqual(0);
  expect(box.y, "top edge").toBeGreaterThanOrEqual(0);
  expect(box.x + box.width, "right edge").toBeLessThanOrEqual(view.width + 0.5);
  expect(box.y + box.height, "bottom edge").toBeLessThanOrEqual(view.height + 0.5);
}

/** The page never scrolls sideways; `el` (a bar) is not wider than its content box. */
async function expectNoSidewaysScroll(page: Page, el?: Locator): Promise<void> {
  expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBe(0);
  if (el) expect(await el.evaluate((e) => e.scrollWidth - e.clientWidth)).toBe(0);
}

test("the lock screen fits without a scrollbar", async ({ page }) => {
  await page.goto(PAGE);
  await expect(page.getByLabel("סיסמה")).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollHeight - document.documentElement.clientHeight)).toBe(0);
  await expectOnScreen(page, page.locator(".lock-version"));
});

test("the cases screen keeps its bar on one line and the first case on screen", async ({ page }) => {
  await unlock(page, PAGE);
  await expectNoSidewaysScroll(page, page.locator(".topbar"));
  const bar = await page.locator(".topbar").boundingBox();
  expect(bar?.height).toBeLessThanOrEqual(64);
  await expectOnScreen(page, page.getByRole("button", { name: "נעילה" }));
  await expectOnScreen(page, page.getByRole("button", { name: "+ תיק חדש" }));
  await expectOnScreen(page, page.locator("table.cases tbody tr").first());
});

test("a long child's name leaves the stages and the actions on screen", async ({ page }) => {
  await unlock(page, PAGE);
  await createCase(page, "נועם-אליהו בן-ציון הכהן-לוי");
  const bar = page.locator(".case-bar");
  await expectNoSidewaysScroll(page, bar);
  const name = await page.locator(".case-child").boundingBox();
  expect(name?.width, "the name is readable, not one letter").toBeGreaterThanOrEqual(60);
  await expectOnScreen(page, page.getByRole("navigation", { name: "שלבי התיק" }));
  await expectOnScreen(page, page.getByRole("button", { name: /עוד פעולות/ }));
  await expectOnScreen(page, page.getByRole("button", { name: "→ תיקים" }));
});

test("with larger text, a dialog keeps its close button and its foot on screen", async ({ page }) => {
  await unlock(page, PAGE);
  // Ctrl + twice: 110 %, then 125 %.
  await page.keyboard.press("Control+Equal");
  await page.keyboard.press("Control+Equal");
  await expect(page.locator("html")).toHaveCSS("zoom", "1.25");
  await page.keyboard.press("F1");
  const dialog = page.getByRole("dialog", { name: "קיצורי מקלדת" });
  await expectOnScreen(page, dialog);
  await expectOnScreen(page, dialog.getByRole("button", { name: /סגירה/ }));
  await expectNoSidewaysScroll(page, page.locator(".topbar"));
  await expectNoSidewaysScroll(page, page.locator(".case-bar").or(page.locator(".topbar")).first());
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+Digit0");
  await expect(page.locator("html")).not.toHaveCSS("zoom", "1.25");
});
