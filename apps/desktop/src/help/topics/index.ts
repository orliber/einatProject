import type { HelpId, HelpTopic } from "../types";
import { REPORT_TOPICS } from "./reports";
import { CLINIC_TOPICS } from "./clinic";
import { SETTINGS_TOPICS } from "./settings";

export const ALL_TOPICS: HelpTopic[] = [...REPORT_TOPICS, ...CLINIC_TOPICS, ...SETTINGS_TOPICS];

const BY_ID = new Map<HelpId, HelpTopic>(ALL_TOPICS.map((t) => [t.id, t]));

export function topicOf(id: HelpId): HelpTopic | undefined {
  return BY_ID.get(id);
}

export const GROUP_LABEL = { reports: "כתיבת הדוחות", clinic: "ניהול הקליניקה", settings: "הגדרות" } as const;

/** Topics whose words hold every word of the query (spaces split it), best first: the title, then the rest. */
export function searchTopics(query: string): HelpTopic[] {
  const words = query.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const textOf = (t: HelpTopic) =>
    [t.intro, ...t.can.flatMap((c) => [c.title, c.text]), ...(t.tips ?? []), ...(t.faq ?? []).flatMap((f) => [f.q, f.a])].join(" ");
  return ALL_TOPICS
    .map((t) => {
      const title = t.title;
      const rest = textOf(t);
      if (!words.every((w) => title.includes(w) || rest.includes(w))) return null;
      return { t, score: words.filter((w) => title.includes(w)).length };
    })
    .filter((x): x is { t: HelpTopic; score: number } => x !== null)
    .sort((a, b) => b.score - a.score)
    .map((x) => x.t);
}
