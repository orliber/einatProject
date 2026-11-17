import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { forgetDiscreet } from "../discreet";

afterEach(() => {
  cleanup();
  clearMocks();
  // The two switches the program keeps between runs (D-191, D-192): each test starts afresh.
  forgetDiscreet();
  try {
    localStorage.removeItem("dv.area");
  } catch {
    // no storage
  }
});

// jsdom does not lay out the page, so it has no scrolling.
if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => undefined;
