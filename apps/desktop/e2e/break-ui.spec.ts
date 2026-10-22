// V2: the screens at their edges: no cases, many cases with long Hebrew and English names,
// the narrowest window the program allows, and Windows text scaling at 150%. Nothing may
// spill sideways out of the window. All names are invented.
import { expect, test, type Page } from "@playwright/test";
import { createCase, stage } from "./helpers";

async function open(page: Page, query = ""): Promise<void> {
  await page.goto(`/preview.html${query}`);
  await page.getByLabel("סיסמה").fill("סיסמה בדויה לבדיקה");
  await page.getByRole("button", { name: "פתיחה", exact: true }).click();
  await expect(page.getByRole("button", { name: "+ תיק חדש" })).toBeVisible();
}

/** Nothing reaches past the window's right or left edge. */
async function expectNoSidewaysSpill(page: Page, where: string): Promise<void> {
  const spill = await page.evaluate(() => {
    const doc = document.documentElement;
    return { scroll: doc.scrollWidth, width: doc.clientWidth };
  });
  expect(spill.scroll, `${where}: the page scrolls sideways`).toBeLessThanOrEqual(spill.width + 1);
}

const sizes = [
  { name: "a normal window", viewport: { width: 1280, height: 800 }, scale: 1 },
  // A common laptop screen, short and wide: the explorer's tree and table have to share it.
  { name: "a laptop screen", viewport: { width: 1366, height: 700 }, scale: 1 },
  { name: "the narrowest window", viewport: { width: 1024, height: 680 }, scale: 1 },
  // 150% text scaling: the same 1280px window shows 853 CSS pixels.
  { name: "150% text scaling", viewport: { width: 853, height: 560 }, scale: 1.5 },
];

for (const size of sizes) {
  test.describe(size.name, () => {
    test.use({ viewport: size.viewport, deviceScaleFactor: size.scale });

    test("no cases yet: the empty screen says what to do", async ({ page }) => {
      await open(page, "?cases=empty");
      await expect(page.getByRole("button", { name: /לנסות קודם על תיק לדוגמה/ })).toBeVisible();
      await expectNoSidewaysSpill(page, "empty cases");
    });

    test("fifty cases with long mixed names stay in the window", async ({ page }) => {
      await open(page, "?cases=many");
      await expect(page.getByRole("button", { name: /Weissberg-Abramovich/ }).first()).toBeVisible();
      await expectNoSidewaysSpill(page, "many cases");
    });

    test("a case with a long mixed name: its pages stay in the window", async ({ page }) => {
      await open(page);
      await createCase(page, "אלכסנדרה-מרגריטה Weissberg-Abramovich בן-שושן");
      await expectNoSidewaysSpill(page, "report");
      // The case bar stays one row: the name gives way (the whole name is in its tooltip).
      const tall = await page.evaluate(() =>
        [".case-back", ".case-child", ".stage"].flatMap((sel) =>
          Array.from(document.querySelectorAll<HTMLElement>(sel))
            .filter((el) => el.getBoundingClientRect().height > 2 * parseFloat(getComputedStyle(el).fontSize) + 20)
            .map(() => sel),
        ),
      );
      expect(tall, "parts of the case bar that wrap to a second line").toEqual([]);
      await expect(page.locator(".case-child")).toHaveAttribute("title", /Weissberg-Abramovich/);
      await stage(page, /חומרים/).click();
      await expect(page.getByRole("heading", { name: "מה יש בתיק" })).toBeVisible();
      await expectNoSidewaysSpill(page, "materials");
      // A material with a long file-like name stays in its row (round 49).
      await page.getByRole("button", { name: "אינטייק עם ההורים (רישום אינטייק)" }).click();
      const write = page.getByRole("dialog", { name: "הוספת טקסט" });
      await write.getByLabel("כותרת").fill("סיכום_אינטייק_עם_ההורים_Weissberg-Abramovich_גרסה_סופית_מתוקנת_2026-10-15.docx");
      await write.getByLabel("הטקסט").fill("טקסט בדוי.");
      await write.getByRole("button", { name: "שמירה בתיק" }).click();
      await expect(write).toBeHidden();
      await expect(page.getByRole("region", { name: "רשימת החומרים" }).getByRole("button", { name: /Weissberg-Abramovich_גרסה/ })).toBeVisible();
      await expectNoSidewaysSpill(page, "materials with a long name");
      // The score table, its profile and the stored text share the window (round 54).
      await page.getByRole("button", { name: /ציוני מבחנים \(הזנת ציונים\)/ }).click();
      const scores = page.getByRole("region", { name: "הזנת ציונים" });
      await scores.getByLabel("מנת משכל כללית", { exact: true }).fill("78");
      await scores.getByLabel("שיתופיות", { exact: true }).fill("25");
      await expect(scores.getByRole("img", { name: /פרופיל המדדים/ })).toBeVisible();
      await expectNoSidewaysSpill(page, "scores");
      const inside = await page.evaluate(() => {
        const body = document.querySelector<HTMLElement>(".scores-view .scores-body");
        return body ? body.scrollWidth <= body.clientWidth + 1 : false;
      });
      expect(inside, "the score table scrolls sideways inside its view").toBe(true);
    });
  });
}
