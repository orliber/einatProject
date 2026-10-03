// Which AI answers (D-038). Claude stays the default; ChatGPT and Gemini are chosen in
// settings with her own API key. Every place the program speaks of the AI uses its name.
export type ProviderId = "anthropic" | "openai" | "gemini";

export interface ProviderInfo {
  id: ProviderId;
  name: string;
  company: string;
  /** `[model id, label]`, recommended first. Must match the lists in dv-ai and dv-egress. */
  models: [string, string][];
  keyPlaceholder: string;
  /** What the company keeps under its standard API terms (shown before a key is saved). */
  retention: string | null;
}

const CLAUDE: ProviderInfo = {
  id: "anthropic",
  name: "Claude",
  company: "Anthropic",
  models: [
    ["claude-opus-5", "Claude Opus 5 (מומלץ: הכי מדויק)"],
    ["claude-sonnet-5", "Claude Sonnet 5 (מהיר יותר)"],
    ["claude-opus-4-8", "Claude Opus 4.8"],
  ],
  keyPlaceholder: "sk-ant-…",
  retention: null,
};

export const PROVIDERS: ProviderInfo[] = [
  CLAUDE,

  {
    id: "gemini",
    name: "Gemini",
    company: "Google",
    models: [
      ["gemini-2-5-pro", "Gemini 2.5 Pro (מומלץ: הכי מדויק)"],
      ["gemini-2-5-flash", "Gemini 2.5 Flash (מהיר יותר)"],
    ],
    keyPlaceholder: "AIza…",
    retention:
      "בתנאים הרגילים של Gemini API, Google שומרת את הבקשות והתשובות לזמן מוגבל לצורך זיהוי שימוש לרעה (לא לאימון, בחשבון בתשלום). אין כאן הסכם אפס שמירת מידע כמו אצל Claude. בחשבון חינמי Google רשאית להשתמש בתוכן לשיפור המוצרים, ולכן צריך חשבון בתשלום.",
  },
  {
    id: "openai",
    name: "ChatGPT",
    company: "OpenAI",
    models: [
      ["gpt-5-1", "GPT-5.1 (מומלץ: הכי מדויק)"],
      ["gpt-5-mini", "GPT-5 mini (מהיר יותר)"],
    ],
    keyPlaceholder: "sk-…",
    retention:
      "בתנאים הרגילים של OpenAI API, הבקשות והתשובות נשמרות עד 30 יום לצורך זיהוי שימוש לרעה (לא לאימון). אפס שמירת מידע (ZDR) ניתן רק אחרי בקשה ואישור של OpenAI.",
  },
];

/** The provider of a model; Claude for anything unknown (the default). */
export function providerOf(model: string): ProviderInfo {
  return PROVIDERS.find((p) => p.models.some(([m]) => m === model)) ?? CLAUDE;
}
