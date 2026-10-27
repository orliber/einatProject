// V3: each screen of the preview compared with how it looked when it was approved. The
// stored picture is text, not an image (images are blocked from the repository): every
// visible heading, button, field and landmark with its rounded place on the page. A moved,
// vanished or renamed control fails the test; a pixel of anti-aliasing does not.
// To accept a deliberate change: `pnpm e2e screens --update-snapshots`, and review the diff.
// All names are invented.
import { expect, test, type Page } from "@playwright/test";
import { expectAccessible, stage, unlock } from "./helpers";

/** Visible landmarks and controls, in document order, with boxes rounded to 16px. */
async function layout(page: Page): Promise<string> {
  await page.evaluate(() =>
    Promise.all(
      document.getAnimations()
        .filter((a) => a.effect?.getTiming().iterations !== Infinity)
        .map((a) => a.finished.catch(() => undefined)),
    ),
  );
  return page.evaluate(() => {
    const step = 16;
    const r = (n: number) => Math.round(n / step) * step;
    const sel = "h1,h2,h3,button,input,select,textarea,a[href],nav,main,header,aside,[role=dialog],[role=tab],article";
    return Array.from(document.querySelectorAll<HTMLElement>(sel))
      .filter((el) => {
        const b = el.getBoundingClientRect();
        return b.width > 0 && b.height > 0 && getComputedStyle(el).visibility !== "hidden";
      })
      .map((el) => {
        const b = el.getBoundingClientRect();
        const role = el.getAttribute("role") ?? el.tagName.toLowerCase();
        // Dates and counts change from run to run, and the opening ideas rotate on purpose;
        // the rest of the words do not.
        const bare = el.matches("nav,main,header,aside,article,[role=dialog]") || el.closest(".ideas");
        const name = (el.getAttribute("aria-label") ?? (bare ? "" : el.innerText))
          .replace(/\d+/g, "#")
          .replace(/\s+/g, " ")
          .trim()
          .slice(0, 40);
        return `${role} "${name}" @${r(b.left)},${r(b.top)} ${r(b.width)}x${r(b.height)}`;
      })
      .join("\n");
  });
}

async function expectLooksApproved(page: Page, name: string): Promise<void> {
  // Soft: a test that walks several screens reports every changed picture in one run, so a
  // deliberate change is reviewed (and accepted) once, not one screen per CI round.
  expect.soft(await layout(page)).toMatchSnapshot(`${name}.txt`);
}

// The greeting follows the hour ("בוקר טוב", "לילה טוב"): every run sees the same night, the
// one the pictures were approved in. Timers keep running.
test.beforeEach(async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-10-14T23:00:00+03:00"));
});

test("the lock screen", async ({ page }) => {
  await page.goto("/preview.html");
  await expect(page.getByLabel("סיסמה")).toBeVisible();
  await expectLooksApproved(page, "lock");
});

test("the cases screen", async ({ page }) => {
  await unlock(page);
  await expectLooksApproved(page, "cases");
});

test("a case: report, materials and export", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
  // The pointer stays where the case list's button was; off the page, no paragraph is hovered.
  await page.mouse.move(0, 0);
  await expectLooksApproved(page, "case-report");
  await stage(page, /חומרים/).click();
  await expect(stage(page, /חומרים/)).toHaveAttribute("aria-current", "step");
  await expectLooksApproved(page, "case-materials");
  // Round 54: the score table is a view of the materials, with its profile beside it.
  await page.getByRole("region", { name: "רשימת החומרים" }).getByRole("button", { name: /ציוני WPPSI-IV/ }).click();
  await page.getByRole("dialog").getByRole("button", { name: "עריכה" }).click();
  await expect(page.getByRole("img", { name: /פרופיל המדדים/ })).toBeVisible();
  await page.mouse.move(0, 0);
  await expectLooksApproved(page, "case-scores");
  // Back where she was: the material's window, over the materials.
  await page.getByRole("button", { name: "ביטול" }).click();
  await expect(page.getByRole("dialog", { name: "ציוני WPPSI-IV" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("heading", { name: "מה יש בתיק" })).toBeVisible();
  await stage(page, /הוצאת הדוח/).click();
  await expect(stage(page, /הוצאת הדוח/)).toHaveAttribute("aria-current", "step");
  await expectLooksApproved(page, "case-export");
});

for (const [name, file] of [["הגדרות", "settings"], ["התייעצות", "consult"], ["הסגנון שלי", "style"]] as const) {
  test(`the ${file} page`, async ({ page }) => {
    await unlock(page);
    await page.getByRole("button", { name, exact: true }).first().click();
    await expect(page.getByRole("button", { name, exact: true }).first()).toHaveAttribute("aria-current", "page");
    await expectLooksApproved(page, file);
  });
}

// Dark mode (D-102): the same screens, laid out the same, readable (axe, contrast included);
// the report page stays white paper with dark text.
const DARK_BG = "rgb(17, 23, 22)";
const bodyBg = (page: Page) => page.evaluate(() => getComputedStyle(document.body).backgroundColor);

test.describe("in dark mode, by the computer", () => {
  test.use({ colorScheme: "dark" });

  test("the cases screen", async ({ page }) => {
    await unlock(page);
    expect(await bodyBg(page)).toBe(DARK_BG);
    await expectLooksApproved(page, "cases");
    await expectAccessible(page);
  });

  test("the settings page", async ({ page }) => {
    await unlock(page);
    await page.getByRole("button", { name: "הגדרות", exact: true }).first().click();
    await expect(page.getByRole("radio", { name: "לפי המחשב" })).toHaveAttribute("aria-checked", "true");
    expect(await bodyBg(page)).toBe(DARK_BG);
    await expectLooksApproved(page, "settings");
    await expectAccessible(page);
  });

  test("a case: the report page stays white", async ({ page }) => {
    await unlock(page);
    await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
    const paper = page.getByRole("article", { name: "הדוח, כמו בקובץ" });
    await expect(paper).toBeVisible();
    await page.mouse.move(0, 0);
    expect(await bodyBg(page)).toBe(DARK_BG);
    expect(await paper.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgb(255, 255, 255)");
    expect(await paper.evaluate((el) => getComputedStyle(el).color)).toBe("rgb(27, 27, 27)");
    await expectLooksApproved(page, "case-report");
    await expectAccessible(page);
  });
});

test("light, dark or by the computer: chosen in Settings and kept", async ({ page }) => {
  await unlock(page);
  await page.getByRole("button", { name: "הגדרות", exact: true }).first().click();
  const look = page.getByRole("radiogroup", { name: "מראה" });
  await look.getByRole("radio", { name: "כהה" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(await bodyBg(page)).toBe(DARK_BG);
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expectAccessible(page);
  await unlock(page);
  await page.getByRole("button", { name: "הגדרות", exact: true }).first().click();
  await expect(look.getByRole("radio", { name: "כהה" })).toHaveAttribute("aria-checked", "true");
  await look.getByRole("radio", { name: "לפי המחשב" }).click();
  await expect(page.locator("html")).not.toHaveAttribute("data-theme", /.*/);
  expect(await bodyBg(page)).toBe("rgb(244, 245, 243)");
});
