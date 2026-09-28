/** True only in the browser preview build (`vite build --mode preview-web`); false, and removed, in the app. */
export const PREVIEW = import.meta.env.MODE === "preview-web";
