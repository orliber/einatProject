// Typing Hebrew the way a Windows keyboard sends it (L6, D-117). The bug of F1(a): under a Hebrew
// keyboard in WebView2, a box that re-rendered the screen on every key made the typing stop. The
// boxes now keep their text (LocalText); these tests type into the same boxes in a real Chromium
// with real key events of the Hebrew layout and with IME composition, and check that nothing is
// lost, nothing is reordered and the caret stays where she types, right to left.
// Every text here is invented.
import { expect, test, type CDPSession, type Locator, type Page } from "@playwright/test";
import { createCase, stage, unlock } from "./helpers";
import { TEXT_DEBOUNCE_MS } from "../src/components/LocalText";
import { hebrewKey } from "../src/test/hebrewKeyboard";

/** The browser's own input, below the page: the same path a key from the OS takes. */
async function cdp(page: Page): Promise<CDPSession> {
  return page.context().newCDPSession(page);
}

/** One key of the Hebrew layout: key down (with its letter), key up. Hebrew is not on the US
 *  layout, so Playwright's own `keyboard.type` would only insert the text, with no key event. */
async function pressHebrew(session: CDPSession, ch: string): Promise<void> {
  const { code, keyCode } = hebrewKey(ch);
  const common = { key: ch, code, windowsVirtualKeyCode: keyCode, nativeVirtualKeyCode: keyCode };
  await session.send("Input.dispatchKeyEvent", { type: "keyDown", ...common, text: ch, unmodifiedText: ch });
  await session.send("Input.dispatchKeyEvent", { type: "keyUp", ...common });
}

async function typeHebrew(session: CDPSession, text: string): Promise<void> {
  for (const ch of text) await pressHebrew(session, ch);
}

/** A word through an input method: composition grows letter by letter, then is committed, as a
 *  Windows IME (TSF) in WebView2 sends it. Returns once the commit has landed. */
async function composeHebrew(session: CDPSession, word: string): Promise<void> {
  let shown = "";
  for (const ch of word) {
    shown += ch;
    await session.send("Input.imeSetComposition", { text: shown, selectionStart: shown.length, selectionEnd: shown.length });
  }
  await session.send("Input.insertText", { text: word });
}

/** What the box heard, recorded on the box itself (nothing in the app is switched on for this). */
async function listen(box: Locator): Promise<void> {
  await box.evaluate((el) => {
    const log: string[] = [];
    (el as unknown as { __heard: string[] }).__heard = log;
    for (const type of ["keydown", "compositionstart", "compositionupdate", "compositionend"]) {
      el.addEventListener(type, (e) => log.push(type === "keydown" ? `keydown:${(e as KeyboardEvent).code}` : type));
    }
    el.addEventListener("input", (e) => log.push((e as InputEvent).isComposing ? "input:composing" : "input"));
  });
}

async function heard(box: Locator): Promise<string[]> {
  return box.evaluate((el) => (el as unknown as { __heard: string[] }).__heard.splice(0));
}

async function caret(box: Locator): Promise<[number, number]> {
  return box.evaluate((el) => [(el as HTMLTextAreaElement).selectionStart, (el as HTMLTextAreaElement).selectionEnd]);
}

async function putCaret(box: Locator, at: number): Promise<void> {
  await box.evaluate((el, i) => (el as HTMLTextAreaElement).setSelectionRange(i, i), at);
}

/**
 * The whole check, on one box that already has the focus and starts with `start`:
 *  1. key by key on the Hebrew layout, with a pause longer than the box's report in the middle
 *     (the screen re-renders then): every key arrives, in order, the caret follows each key;
 *  2. a word through IME composition, at the end;
 *  3. a word typed into the middle, after the screen re-rendered: inserted there, the caret
 *     right after it, the rest unchanged;
 *  4. the caret moves right to left: the arrow key to the right goes back one letter.
 */
async function typeAndCheck(page: Page, box: Locator, start = ""): Promise<string> {
  const session = await cdp(page);
  await expect(box).toBeFocused();
  await expect(box).toHaveCSS("direction", "rtl");
  await putCaret(box, start.length);
  await listen(box);

  // 1. Real keys, with a pause in the middle.
  const first = "הילד בנה מגדל גבוה";
  const second = " וצחק כשנפל.";
  let expected = start;
  for (const ch of first) {
    await pressHebrew(session, ch);
    expected += ch;
  }
  expect(await caret(box)).toEqual([expected.length, expected.length]);
  await page.waitForTimeout(TEXT_DEBOUNCE_MS + 300);
  await typeHebrew(session, second);
  expected += second;
  await expect(box).toHaveValue(expected);
  expect(await caret(box)).toEqual([expected.length, expected.length]);
  const keys = await heard(box);
  // One key down and one input per letter: nothing typed was dropped or merged.
  expect(keys.filter((e) => e.startsWith("keydown"))).toHaveLength([...first + second].length);
  expect(keys.filter((e) => e === "input")).toHaveLength([...first + second].length);
  expect(keys[0]).toBe(`keydown:${hebrewKey(first[0] ?? "").code}`);

  // 2. A word through composition.
  await composeHebrew(session, " שוב");
  expected += " שוב";
  await expect(box).toHaveValue(expected);
  expect(await caret(box)).toEqual([expected.length, expected.length]);
  const composed = await heard(box);
  expect(composed[0]).toBe("compositionstart");
  expect(composed).toContain("compositionupdate");
  expect(composed).toContain("input:composing");
  expect(composed).toContain("compositionend");

  // 3. Into the middle, after the screen heard the text and re-rendered.
  await page.waitForTimeout(TEXT_DEBOUNCE_MS + 300);
  const at = start.length + "הילד".length;
  await putCaret(box, at);
  await typeHebrew(session, " הקטן");
  expected = expected.slice(0, at) + " הקטן" + expected.slice(at);
  await expect(box).toHaveValue(expected);
  expect(await caret(box)).toEqual([at + " הקטן".length, at + " הקטן".length]);
  // Composition in the middle too, and another pause: still where she left it.
  await composeHebrew(session, " מאוד");
  const mid = at + " הקטן".length;
  expected = expected.slice(0, mid) + " מאוד" + expected.slice(mid);
  await page.waitForTimeout(TEXT_DEBOUNCE_MS + 300);
  await expect(box).toHaveValue(expected);
  expect(await caret(box)).toEqual([mid + " מאוד".length, mid + " מאוד".length]);

  // 4. Right to left: → goes back one letter, ← goes forward one (the caret follows the line).
  const here = mid + " מאוד".length;
  await page.keyboard.press("ArrowRight");
  expect(await caret(box)).toEqual([here - 1, here - 1]);
  await page.keyboard.press("ArrowLeft");
  expect(await caret(box)).toEqual([here, here]);
  await expect(box).toBeFocused();
  return expected;
}

test.describe("Hebrew typing (real keys and IME)", () => {
  test("the material's text box", async ({ page }) => {
    await unlock(page);
    await createCase(page, "יעל בדויה", { consent: false });
    await stage(page, /חומרים/).click();
    await page.getByRole("button", { name: "אינטייק עם ההורים (רישום אינטייק)" }).click();
    const write = page.getByRole("dialog", { name: "הוספת טקסט" });
    const box = write.getByLabel("הטקסט");
    const typed = await typeAndCheck(page, box);
    await write.getByLabel("כותרת").fill("אינטייק בדוי");
    await write.getByRole("button", { name: "שמירה בתיק" }).click();
    await expect(write).toBeHidden();
    // What was saved is exactly what was typed.
    await page.getByRole("region", { name: "רשימת החומרים" }).getByRole("button", { name: /אינטייק בדוי/ }).click();
    await expect(page.getByRole("dialog", { name: "אינטייק בדוי" })).toContainText(typed.replace(/\s+/g, " ").trim().slice(0, 20));
  });

  test("a new paragraph on the report page", async ({ page }) => {
    await unlock(page);
    await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
    const report = page.getByRole("article", { name: "הדוח, כמו בקובץ" });
    const section = report.locator(".a4-section.is-focus");
    await section.getByRole("button", { name: "לכתוב בעצמי" }).first().click();
    const box = report.getByRole("textbox", { name: /^פסקה חדשה בסעיף/ });
    const typed = await typeAndCheck(page, box);
    await page.keyboard.press("Control+Enter");
    await expect(box).toBeHidden();
    await expect(report).toContainText(typed.slice(0, 12));
  });

  test("editing a paragraph that is already on the page", async ({ page }) => {
    await unlock(page);
    await page.getByRole("button", { name: /בתיק של נועם$/ }).click();
    const report = page.getByRole("article", { name: "הדוח, כמו בקובץ" });
    // The first paragraph on the page: the first click picks its section, the second edits it.
    const para = report.locator("p.a4-editable").first();
    const before = (await para.innerText()).trim();
    await para.click();
    await para.click();
    const box = report.getByRole("textbox", { name: /^עריכת פסקה בסעיף/ });
    await expect(box).toHaveValue(before);
    await typeAndCheck(page, box, before);
  });

  test("the question to the consultation, where Enter sends but not while composing", async ({ page }) => {
    await unlock(page);
    await page.getByRole("button", { name: "התייעצות", exact: true }).first().click();
    const box = page.locator("#consult-q");
    await box.click();
    const typed = await typeAndCheck(page, box);
    // Enter that ends a composition belongs to the input method: it commits the word, and
    // neither sends the question nor adds a line.
    const session = await cdp(page);
    await putCaret(box, typed.length);
    await session.send("Input.imeSetComposition", { text: " סוף", selectionStart: 4, selectionEnd: 4 });
    const enter = { key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13 };
    await session.send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...enter });
    await session.send("Input.insertText", { text: " סוף" });
    await session.send("Input.dispatchKeyEvent", { type: "keyUp", ...enter });
    await expect(box).toHaveValue(`${typed} סוף`);
    // Enter after it sends, with every letter typed.
    await page.keyboard.press("Enter");
    await expect(box).toHaveValue("");
    await expect(page.getByRole("region", { name: "השיחה" }).locator(".chat-bubble-me").last()).toHaveText(`${typed} סוף`);
  });
});
