// The Israeli standard keyboard layout (SI 1452), for the typing tests (L6, D-117): each Hebrew
// letter with the physical key that types it and that key's Windows virtual-key code, as a key
// press arrives from the OS. Shared by the component tests (user-event's keyboard map) and the
// end-to-end tests (the browser's own key events).

export const HEBREW_LAYOUT: Readonly<Record<string, readonly [code: string, keyCode: number]>> = {
  "ק": ["KeyE", 69], "ר": ["KeyR", 82], "א": ["KeyT", 84], "ט": ["KeyY", 89], "ו": ["KeyU", 85], "ן": ["KeyI", 73],
  "ם": ["KeyO", 79], "פ": ["KeyP", 80], "ש": ["KeyA", 65], "ד": ["KeyS", 83], "ג": ["KeyD", 68], "כ": ["KeyF", 70],
  "ע": ["KeyG", 71], "י": ["KeyH", 72], "ח": ["KeyJ", 74], "ל": ["KeyK", 75], "ך": ["KeyL", 76], "ף": ["Semicolon", 186],
  "ז": ["KeyZ", 90], "ס": ["KeyX", 88], "ב": ["KeyC", 67], "ה": ["KeyV", 86], "נ": ["KeyB", 66], "מ": ["KeyN", 78],
  "צ": ["KeyM", 77], "ת": ["Comma", 188], "ץ": ["Period", 190], ".": ["Slash", 191], ",": ["Quote", 222], " ": ["Space", 32],
};

/** The key that types `ch` on the Hebrew layout; throws for a character the layout lacks. */
export function hebrewKey(ch: string): { key: string; code: string; keyCode: number } {
  const found = HEBREW_LAYOUT[ch];
  if (!found) throw new Error(`no key for ${JSON.stringify(ch)} on the Hebrew layout`);
  return { key: ch, code: found[0], keyCode: found[1] };
}
