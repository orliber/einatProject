// Browser preview only: a small stand-in for the privacy pipeline (crates/dv-privacy), enough to
// show what the review screen looks like. The real filter has many more layers and runs in Rust.
import type { FilterOutcome } from "../ipc/generated/FilterOutcome";
import type { Role } from "../ipc/generated/Role";
import type { Segment } from "../ipc/generated/Segment";
import type { Suspect } from "../ipc/generated/Suspect";
import { roleLabel } from "../i18n/he";

export interface Person {
  caseId: string;
  role: Role;
  tag: string;
  value: string;
  aliases: string[];
}

export interface FilterContext {
  caseId: string;
  people: Person[];
  practitioner: string[];
  /** Words the psychologist marked "not a name" in this case. */
  allowed: Set<string>;
}

const LETTER = "\\u0590-\\u05FFA-Za-z0-9";
const PREFIX = "([והבכלמש]{0,3})";
/** First names the preview recognises when they are not in the case's list. */
const KNOWN_NAMES = ["יובל", "אלון", "שירה", "נועה", "איתי", "עומר", "תמר", "רוני", "אורי", "יעל", "דניאל", "אריאל", "ליאור", "עדי", "שקד", "נגה", "הילה", "אביגיל", "יהונתן", "מיכאל", "איתמר", "עידו", "ירדן", "מאיה", "נועם", "דנה", "יוסי", "מיכל", "רותם"];
const LATIN_OK = new Set(["WPPSI", "WISC", "ADOS", "ASRS", "CBCL", "CARS", "ABAS", "DSM", "Bayley", "Total", "Word", "PDF"]);

type Hit = { start: number; end: number; mark: "replaced" | "relative" | "suspect"; out: string; label: string; suspect?: Suspect };

function escape(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function wordRe(name: string): RegExp {
  return new RegExp(`(?<![${LETTER}])${PREFIX}(${escape(name)})(?![${LETTER}])`, "g");
}

function monthsAgo(day: number, month: number, year: number | null): string {
  const now = new Date();
  let y = year ?? now.getFullYear();
  if (year === null && new Date(y, month - 1, day) > now) y -= 1;
  const then = new Date(y, month - 1, day);
  const days = Math.round((now.getTime() - then.getTime()) / 86_400_000);
  if (days < 0) return "בעתיד הקרוב";
  if (days < 10) return "לפני כמה ימים";
  if (days < 21) return "לפני כשבועיים";
  if (days < 45) return "לפני כחודש";
  const months = Math.round(days / 30.4);
  if (months < 12) return months === 2 ? "לפני כחודשיים" : `לפני כ-${months} חודשים`;
  const years = Math.round(months / 12);
  return years === 1 ? "לפני כשנה" : years === 2 ? "לפני כשנתיים" : `לפני כ-${years} שנים`;
}

export function filter(text: string, ctx: FilterContext): FilterOutcome {
  const hits: Hit[] = [];
  const add = (h: Hit) => {
    if (!hits.some((x) => h.start < x.end && x.start < h.end)) hits.push(h);
  };
  const names = (p: Person) => [p.value, ...p.aliases, ...p.value.split(" ")].filter((n) => n.trim().length >= 2);

  // Declared names of this case (longest first), then the practitioner's own names.
  const own = ctx.people.filter((p) => p.caseId === ctx.caseId);
  const declared = own.flatMap((p) => names(p).map((n) => ({ n, p }))).sort((a, b) => b.n.length - a.n.length);
  for (const { n, p } of declared) {
    for (const m of text.matchAll(wordRe(n))) {
      const pre = m[1] ?? "";
      add({ start: m.index, end: m.index + m[0].length, mark: "replaced", out: pre + p.tag, label: `שם ${roleLabel[p.role]}` });
    }
  }
  for (const n of ctx.practitioner.flatMap((x) => [x, ...x.split(" ")]).filter((x) => x.length >= 2)) {
    for (const m of text.matchAll(wordRe(n))) {
      add({ start: m.index, end: m.index + m[0].length, mark: "replaced", out: (m[1] ?? "") + "[מאבחנת]", label: "שם המאבחנת" });
    }
  }
  // Another case's names: never a tag here, always a question.
  for (const p of ctx.people.filter((x) => x.caseId !== ctx.caseId)) {
    for (const n of names(p)) {
      for (const m of text.matchAll(wordRe(n))) {
        const token = m[2] ?? m[0];
        add({ start: m.index, end: m.index + m[0].length, mark: "suspect", out: m[0], label: "שם מתיק אחר",
          suspect: { token, kind: "other_case_identity", message: "השם מופיע בתיק אחר", suggested_role: "other_child" } });
      }
    }
  }
  // Dates become relative ("לפני כחודש"); scores and ages stay.
  for (const m of text.matchAll(/(?<!\d)(\d{1,2})[./](\d{1,2})(?:[./](\d{4}|\d{2}))?(?!\d)/g)) {
    const before = text.slice(0, m.index).trim().split(/\s+/).pop() ?? "";
    if (/(ציון|גיל|בגיל|בן|בת|ממוצע)$/.test(before)) continue;
    const d = Number(m[1]);
    const mo = Number(m[2]);
    const y = m[3] ? (m[3].length === 2 ? 2000 + Number(m[3]) : Number(m[3])) : null;
    if (d < 1 || d > 31 || mo < 1 || mo > 12) continue;
    add({ start: m.index, end: m.index + m[0].length, mark: "relative", out: monthsAgo(d, mo, y), label: "תאריך" });
  }
  // Long numbers (ID, phone, file numbers).
  for (const m of text.matchAll(/(?<!\d)\d[\d-]{3,}\d(?!\d)/g)) {
    if (m[0].replace(/\D/g, "").length < 5) continue;
    add({ start: m.index, end: m.index + m[0].length, mark: "replaced", out: "[מספר]", label: "מספר מזהה" });
  }
  // Names that are not on the case's list.
  const known = new Set(ctx.people.flatMap((p) => names(p)));
  for (const n of KNOWN_NAMES.filter((x) => !known.has(x) && !ctx.allowed.has(x))) {
    for (const m of text.matchAll(wordRe(n))) {
      const token = m[2] ?? m[0];
      add({ start: m.index, end: m.index + m[0].length, mark: "suspect", out: m[0], label: "שם לא מוכר",
        suspect: { token, kind: "unknown_name", message: "שם שלא מופיע ברשימת התיק", suggested_role: "other_child" } });
    }
  }
  for (const m of text.matchAll(/(?<![A-Za-z])[A-Z][a-z]+(?![A-Za-z])/g)) {
    if (LATIN_OK.has(m[0]) || ctx.allowed.has(m[0])) continue;
    add({ start: m.index, end: m.index + m[0].length, mark: "suspect", out: m[0], label: "מילה באנגלית",
      suspect: { token: m[0], kind: "latin_name", message: "מילה באנגלית שעשויה להיות שם", suggested_role: "other" } });
  }

  hits.sort((a, b) => a.start - b.start);
  const original: Segment[] = [];
  const tagged: Segment[] = [];
  let at = 0;
  let out = "";
  for (const h of hits) {
    if (h.start > at) {
      const plain = text.slice(at, h.start);
      original.push({ text: plain, mark: null, label: null });
      tagged.push({ text: plain, mark: null, label: null });
      out += plain;
    }
    original.push({ text: text.slice(h.start, h.end), mark: h.mark === "suspect" ? "suspect" : "replaced", label: h.label });
    tagged.push({ text: h.out, mark: h.mark === "suspect" ? "suspect" : h.mark === "relative" ? "relative" : "tag", label: h.label });
    out += h.out;
    at = h.end;
  }
  if (at < text.length) {
    const rest = text.slice(at);
    original.push({ text: rest, mark: null, label: null });
    tagged.push({ text: rest, mark: null, label: null });
    out += rest;
  }
  const suspects: Suspect[] = [];
  for (const h of hits) if (h.suspect && !suspects.some((s) => s.token === h.suspect?.token)) suspects.push(h.suspect);
  return {
    tagged: out,
    original_segments: original,
    tagged_segments: tagged,
    suspects,
    hidden: Array.from(new Set(hits.filter((h) => h.mark !== "suspect").map((h) => h.label))),
    checks: {
      declared_names: hits.filter((h) => h.label.startsWith("שם ") && h.mark === "replaced").length,
      patterns: hits.filter((h) => h.label === "תאריך" || h.label === "מספר מזהה").length,
      name_suspects: suspects.length,
      indirect_suspects: 0,
    },
  };
}

/** Tags back to names, for showing drafts on screen. */
export function restore(text: string, people: Person[], practitioner: string[]): string {
  let out = text.replaceAll("[מאבחנת]", practitioner[0] ?? "המאבחנת");
  for (const p of people) out = out.replaceAll(p.tag, p.value);
  return out;
}
