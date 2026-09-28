// Browser preview entry (preview.html): the real screens over an in-memory demo core.
import { FakeCore } from "./fakeCore";
import "./preview.css";

const core = new FakeCore();
const internals = {
  invoke: (cmd: string, args: unknown, options?: { headers?: Record<string, string> }) => core.handle(cmd, args, options),
  transformCallback: () => 0,
  unregisterCallback: () => undefined,
  convertFileSrc: (path: string) => path,
};
Object.assign(window, { __TAURI_INTERNALS__: internals });

// The page may be served inside a frame that supplies its own <html>; the app is Hebrew, RTL.
document.documentElement.lang = "he";
document.documentElement.dir = "rtl";
document.title = "הדמיית כספת האבחון";

// The app is laid out for a desktop window. In a narrow panel, scale the whole window down
// instead of letting it scroll sideways.
const DESIGN_WIDTH = 1180;
function fit() {
  const root = document.getElementById("root");
  if (!root) return;
  const z = Math.min(1, window.innerWidth / DESIGN_WIDTH);
  root.style.setProperty("zoom", String(z));
  // Under CSS zoom, pixel lengths are scaled and percentages are not.
  root.style.width = z < 1 ? `${window.innerWidth / z}px` : "";
  root.style.height = z < 1 ? `${window.innerHeight / z}px` : "";
}
fit();
window.addEventListener("resize", fit);

const note = document.createElement("div");
note.className = "preview-note";
note.setAttribute("role", "note");
note.textContent = "הדמיה בדפדפן · נתונים בדויים · סיסמה: כל טקסט";
document.body.append(note);

await import("../main");
