import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";

afterEach(() => {
  cleanup();
  clearMocks();
});

// jsdom does not lay out the page, so it has no scrolling.
if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => undefined;
