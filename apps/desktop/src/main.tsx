import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/assistant/400.css";
import "@fontsource/assistant/600.css";
import "@fontsource/assistant/700.css";
import "@fontsource/frank-ruhl-libre/500.css";
import "@fontsource/frank-ruhl-libre/700.css";
import { App } from "./App";
import { applyTextSize, readTextSize } from "./textSize";
import { applyTheme, readTheme } from "./theme";

applyTextSize(readTextSize());
applyTheme(readTheme());

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
