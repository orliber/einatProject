// The name of the AI she chose, for every place the program speaks of it (D-038).
import { useApp } from "./App";

export * from "./providers";

/** The name of the AI she chose: "Claude", "Gemini" or "ChatGPT". */
export function useAi(): string {
  return useApp().status.ai_name || "Claude";
}
