// The pictures of the help (D-206), taken from the preview's invented cases and saved as small
// WebP files in src/help/shots/. Not part of the usual run: only with DV_HELP_SHOTS=1, after a
// screen changes on purpose. Every name and detail in the pictures is invented.
//   pnpm build:preview && DV_HELP_SHOTS=1 pnpm e2e help-shots
import { expect, test, type Locator, type Page } from "@playwright/test";
import { stage, unlock } from "./helpers";

declare const process: { env: Record<string, string | undefined> };

test.skip(!process.env.DV_HELP_SHOTS, "Only when the help's pictures are taken again (DV_HELP_SHOTS=1).");

/** The widest picture kept: enough to read the buttons in the help window, small on disk. */
const MAX_WIDTH = 1100;

/** Saves what is on the page (or one window of it) as `src/help/shots/<name>.webp`. */
async function shot(page: Page, name: string, target?: Locator): Promise<void> {
  // Nothing half-loaded in a picture: a lazy part or a list still on its way.
  await expect(page.locator('[aria-busy="true"]')).toHaveCount(0);
  await page.mouse.move(0, 0);
  await page.evaluate(() =>
    Promise.all(
      document.getAnimations()
        .filter((a) => a.effect?.getTiming().iterations !== Infinity)
        .map((a) => a.finished.catch(() => undefined)),
    ),
  );
  const png = await (target ?? page).screenshot({ animations: "disabled" });
  // The browser turns the picture into WebP: no image tool in the repository.
  const webp = await page.evaluate(async ({ b64, max }) => {
    const img = new Image();
    img.src = `data:image/png;base64,${b64}`;
    await img.decode();
    const scale = Math.min(1, max / img.naturalWidth);
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(img.naturalWidth * scale);
    canvas.height = Math.round(img.naturalHeight * scale);
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("no canvas");
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
    return canvas.toDataURL("image/webp", 0.82).replace(/^data:image\/webp;base64,/, "");
  }, { b64: png.toString("base64"), max: MAX_WIDTH });
  const bytes = Uint8Array.from(atob(webp), (c) => c.charCodeAt(0));
  const fs = (await import("node:fs" as string)) as { writeFileSync: (p: URL, d: Uint8Array) => void };
  fs.writeFileSync(new URL(`../src/help/shots/${name}.webp`, import.meta.url), bytes);
}

/** The preview's own badges ("הדמיה בדפדפן", the demo chip) are not part of her program. */
async function open(page: Page): Promise<void> {
  await unlock(page);
  await page.addStyleTag({ content: ".preview-note, .topbar-status { display: none !important; }" });
}

const dialog = (page: Page) => page.getByRole("dialog").last();

/** Closes every open window, one Escape at a time. */
async function closeAll(page: Page): Promise<void> {
  for (let i = 0; i < 5 && (await page.getByRole("dialog").count()) > 0; i++) {
    await page.keyboard.press("Escape");
    await page.waitForTimeout(250);
  }
  await expect(page.getByRole("dialog")).toHaveCount(0);
}

async function toCase(page: Page): Promise<void> {
  await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
  await expect(page.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeVisible();
}

async function toClinic(page: Page, tab: string): Promise<void> {
  await page.getByRole("navigation", { name: "ניווט ראשי" }).getByRole("button", { name: /ניהול הקליניקה/ }).click();
  await page.getByRole("tab", { name: tab }).click();
  await expect(page.getByRole("tab", { name: tab })).toHaveAttribute("aria-selected", "true");
}

async function toPatientCard(page: Page): Promise<void> {
  await toClinic(page, "מטופלים");
  await page.locator(".patients-row", { hasText: "נועם" }).first().click();
  await expect(page.getByRole("button", { name: "עריכת הפרטים" })).toBeVisible();
}

// A weekday morning, so the clinic's day has visits before and after "now".
test.beforeEach(async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-11-17T10:12:00+02:00"));
});

test.describe("כתיבת הדוחות", () => {
  test("cases and a new case", async ({ page }) => {
    await open(page);
    await shot(page, "cases");
    await page.getByRole("button", { name: "+ תיק חדש" }).click();
    await shot(page, "new-case", dialog(page));
  });

  test("a case's stages", async ({ page }) => {
    await open(page);
    await toCase(page);
    await shot(page, "case-report");
    await stage(page, /חומרים/).click();
    await expect(page.getByRole("heading", { name: "מה יש בתיק" })).toBeVisible();
    await shot(page, "case-materials");
    await stage(page, /הוצאת הדוח/).click();
    await expect(stage(page, /הוצאת הדוח/)).toHaveAttribute("aria-current", "step");
    await shot(page, "case-finish");
    await page.getByRole("button", { name: "הפקת הקובץ" }).first().click();
    await shot(page, "export", dialog(page));
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: /עוד פעולות/ }).click();
    await page.getByRole("menuitem", { name: "פרטים ושמות להסתרה" }).click();
    await expect(page.getByRole("heading", { name: "פרטי התיק ושמות להסתרה" })).toBeVisible();
    await shot(page, "case-details");
  });

  test("scores, a material and the search", async ({ page }) => {
    await open(page);
    await toCase(page);
    await stage(page, /חומרים/).click();
    await page.getByRole("region", { name: "רשימת החומרים" }).getByRole("button", { name: /ציוני WPPSI-IV/ }).click();
    await page.getByRole("dialog").getByRole("button", { name: "עריכה" }).click();
    await expect(page.getByRole("img", { name: /פרופיל המדדים/ })).toBeVisible();
    await shot(page, "case-scores");
    await page.getByRole("button", { name: "ביטול" }).click();
    await page.keyboard.press("Escape");
    await page.getByRole("region", { name: "רשימת החומרים" }).getByRole("button", { name: /שיחה עם הגננת/ }).click();
    await page.getByRole("dialog").getByRole("button", { name: "עריכה" }).click();
    await expect(page.getByRole("dialog", { name: "עריכת חומר" })).toBeVisible();
    await shot(page, "material", dialog(page));
    await closeAll(page);
    await page.getByRole("button", { name: "חיפוש", exact: true }).click();
    await dialog(page).getByRole("searchbox").or(dialog(page).getByRole("textbox")).first().fill("גן");
    await shot(page, "case-search", dialog(page));
  });

  test("before sending, writing the whole report, and what went out", async ({ page }) => {
    await open(page);
    await page.getByRole("button", { name: "הגדרות", exact: true }).click();
    await page.getByRole("navigation", { name: "חלקי ההגדרות" }).getByRole("button", { name: "פרטיות" }).click();
    await page.getByRole("switch", { name: /להציג לי כל בקשה לפני שליחה/ }).click();
    await page.getByRole("navigation", { name: "ניווט ראשי" }).getByRole("button", { name: "כתיבת הדוחות" }).click();
    await toCase(page);
    await page.getByRole("button", { name: /✦ לנסח את הסעיף/ }).click();
    await expect(page.getByRole("dialog", { name: /לפני שליחה/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^שליחה/ })).toBeEnabled();
    await shot(page, "review", dialog(page));
    await page.getByRole("button", { name: /^שליחה/ }).click();
    await expect(page.getByRole("dialog", { name: /לפני שליחה/ })).toBeHidden();
    await page.getByRole("button", { name: /✦ לכתוב את \d+ הריקים/ }).click();
    await expect(page.getByRole("dialog", { name: /כתיבת כל הדוח/ })).toBeVisible();
    await shot(page, "report-run", dialog(page));
    await closeAll(page);
    await page.getByRole("button", { name: /עוד פעולות/ }).click();
    await page.getByRole("menuitem", { name: /מה יצא מהמחשב/ }).click();
    await expect(page.getByRole("dialog", { name: "מה יצא מהמחשב" })).toBeVisible();
    await shot(page, "sent-history", dialog(page));
  });

  for (const [name, file] of [["התייעצות", "consult"], ["הסגנון שלי", "style"]] as const) {
    test(file, async ({ page }) => {
      await open(page);
      await page.getByRole("button", { name, exact: true }).first().click();
      await expect(page.getByRole("button", { name, exact: true }).first()).toHaveAttribute("aria-current", "page");
      await shot(page, file);
    });
  }
});

test.describe("ניהול הקליניקה", () => {
  const tabs = [
    ["היום", "clinic-today"], ["יומן", "clinic-calendar"], ["מטופלים", "clinic-patients"], ["המתנה", "clinic-waiting"],
    ["מעקבים", "clinic-tasks"], ["כספים", "clinic-money"], ["תמונת מצב", "clinic-insights"],
  ] as const;
  test("the tabs", async ({ page }) => {
    await open(page);
    for (const [tab, file] of tabs) {
      await toClinic(page, tab);
      await shot(page, file);
    }
  });

  test("a patient's card and its windows", async ({ page }) => {
    await open(page);
    await toPatientCard(page);
    await shot(page, "clinic-patient");
    await page.getByRole("button", { name: /פגישה חדשה/ }).first().click();
    await shot(page, "visit", dialog(page));
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "עריכת הפרטים" }).click();
    await shot(page, "patient", dialog(page));
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: /תקבול חדש/ }).click();
    await shot(page, "payment", dialog(page));
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: /מעקב חדש/ }).click();
    await shot(page, "task", dialog(page));
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "לקבוע את כל המסלול" }).click();
    await shot(page, "pathway", dialog(page));
  });

  test("a visit's summary", async ({ page }) => {
    await open(page);
    await toPatientCard(page);
    await page.getByRole("button", { name: "לכתוב סיכום" }).first().click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await shot(page, "visit-note", dialog(page));
  });
});

test.describe("הגדרות", () => {
  const panes = [
    ["כללי", "settings-general"], ["חיבור ל-AI", "settings-ai"], ["פרטיות", "settings-privacy"], ["הדוח", "settings-report"],
    ["המבחנים", "settings-tests"], ["הקליניקה", "settings-clinic"], ["גיבוי", "settings-backup"], ["סיסמה ואבטחה", "settings-security"], ["עדכונים ועזרה", "settings-about"],
  ] as const;
  test("the panes and the password window", async ({ page }) => {
    await open(page);
    await page.getByRole("button", { name: "הגדרות", exact: true }).click();
    for (const [pane, file] of panes) {
      await page.getByRole("navigation", { name: "חלקי ההגדרות" }).getByRole("button", { name: pane }).click();
      await shot(page, file);
    }
    await page.getByRole("navigation", { name: "חלקי ההגדרות" }).getByRole("button", { name: "סיסמה ואבטחה" }).click();
    await page.getByRole("button", { name: "החלפת סיסמה…" }).first().click();
    await shot(page, "password", dialog(page));
  });
});
