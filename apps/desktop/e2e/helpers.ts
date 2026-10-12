// Shared steps for the end-to-end tests. Every name and text is invented.
import axe from "axe-core";
import { expect, type Locator, type Page } from "@playwright/test";

/** Open the preview and unlock it (the demo core accepts any password). */
export async function unlock(page: Page): Promise<void> {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/preview.html");
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  await expect(page.locator("html")).toHaveAttribute("lang", "he");
  await page.getByLabel("סיסמה").fill("סיסמה בדויה לבדיקה");
  await page.getByRole("button", { name: "פתיחה", exact: true }).click();
  await expect(page.getByRole("button", { name: "+ תיק חדש" })).toBeVisible();
  expect(errors).toEqual([]);
}

/**
 * axe-core (WCAG 2.1 A and AA, the same rules as the component tests) on the live page.
 * Unlike jsdom, a real browser lays out and paints, so colour contrast is checked too.
 */
export async function a11yViolations(page: Page, scope?: string): Promise<string[]> {
  // Dialogs and rows fade in (also with reduced motion): measure colours after the fade.
  await page.evaluate(() =>
    Promise.all(
      document.getAnimations()
        .filter((a) => a.effect?.getTiming().iterations !== Infinity)
        .map((a) => a.finished.catch(() => undefined)),
    ),
  );
  if (!(await page.evaluate(() => "axe" in window))) await page.addScriptTag({ content: axe.source });
  return page.evaluate(async (selector) => {
    const live = (window as unknown as { axe: { run: (ctx: unknown, opts: unknown) => Promise<AxeResult> } }).axe;
    const result = await live.run(selector ?? document, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
    });
    return result.violations.map(
      (v) => `${v.id}: ${v.help} → ${v.nodes.map((n) => n.target.join(" ")).join(" | ")}`,
    );
  }, scope);
}

interface AxeResult {
  violations: { id: string; help: string; nodes: { target: string[] }[] }[];
}

export async function expectAccessible(page: Page, scope?: string): Promise<void> {
  expect(await a11yViolations(page, scope)).toEqual([]);
}

/** A new case with an invented child, parents and recorded consent; returns on the case screen. */
export async function createCase(page: Page, child: string, opts: { consent?: boolean } = {}): Promise<void> {
  await page.getByRole("button", { name: "+ תיק חדש" }).click();
  const dialog = page.getByRole("dialog", { name: "תיק חדש" });
  await expect(dialog).toBeVisible();
  await dialog.locator("#name-0").fill(child);
  await dialog.locator("#name-1").fill("מיכל בדויה");
  await dialog.locator("#name-2").fill("אלון בדוי");
  if (opts.consent ?? true) await dialog.getByRole("checkbox", { name: /ההורים חתמו על הסכמה/ }).check();
  await dialog.getByRole("button", { name: "פתיחת התיק" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("navigation", { name: "שלבי התיק" })).toBeVisible();
}

export function stage(page: Page, name: RegExp): Locator {
  return page.getByRole("navigation", { name: "שלבי התיק" }).getByRole("button", { name });
}
