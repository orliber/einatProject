// Light or dark (D-102): by the computer unless chosen in Settings. The colours themselves
// are in styles/tokens.css; this only sets `data-theme` on <html>. Kept on this computer
// only, like the text size; it holds nothing about any case.
const KEY = "dv.theme";

export type Theme = "system" | "light" | "dark";

export const THEMES: { value: Theme; label: string }[] = [
  { value: "system", label: "לפי המחשב" },
  { value: "light", label: "בהיר" },
  { value: "dark", label: "כהה" },
];

export function readTheme(): Theme {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

export function applyTheme(theme: Theme): void {
  if (theme === "system") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", theme);
  try {
    if (theme === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, theme);
  } catch {
    // Not kept: applies until the window closes.
  }
}
