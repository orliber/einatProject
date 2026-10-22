// Browser preview only: the writing-style screen (D-043) over the preview's small filter. The real
// neutralizing, checks and gate run in Rust; this shows the flow with fabricated reports.
import type { InterpretationView } from "../ipc/generated/InterpretationView";
import type { Prepared } from "../ipc/generated/Prepared";
import type { StyleImportPreview } from "../ipc/generated/StyleImportPreview";
import type { StyleItem } from "../ipc/generated/StyleItem";
import type { StyleKind } from "../ipc/generated/StyleKind";
import type { StyleLearned } from "../ipc/generated/StyleLearned";
import type { StyleOverview } from "../ipc/generated/StyleOverview";
import type { StylePartView } from "../ipc/generated/StylePartView";
import type { StyleProfile } from "../ipc/generated/StyleProfile";
import type { StyleProfileView } from "../ipc/generated/StyleProfileView";
import type { StyleSourceView } from "../ipc/generated/StyleSourceView";
import type { UiError } from "../ipc/generated/UiError";
import structure from "../../../../templates/report_structure.json";
import { readDocx } from "./docx";
import { filter, type Person } from "./filter";

type Args = Record<string, unknown>;

const SECTIONS = structure.parts.flatMap((p) => p.sections).filter((s) => s.key !== "signature");
/** The preview's short list of subtests whose scores it keeps (the real list is the fixed tables). */
const SUBTESTS: [string, string][] = [["block_design", "קוביות"], ["matrix", "מטריצות"], ["animal_coding", "קידוד חיות"], ["picture_memory", "זיכרון תמונות"], ["bug_search", "חיפוש חרקים"]];
const INCLUDED = ["appearance", "cognitive", "adaptive", "communication", "dsm", "emotional", "summary", "recommendations"];
const HEADINGS: [string, string[]][] = [
  ["dsm", ["DSM"]], ["summary", ["סיכום"]], ["recommendations", ["המלצות"]], ["referral", ["סיבת הפניה"]],
  ["background", ["רקע", "הריון", "לידה"]], ["kindergarten", ["מסגרת", "גן"]], ["cognitive", ["קוגניטיבי", "WPPSI", "WISC"]],
  ["communication", ["תקשורת", "ADOS"]], ["emotional", ["רגשי", "משחק"]], ["appearance", ["הופעה", "התרשמות", "תצפית"]],
];

function fail(code: string, message: string): never {
  const e: UiError = { code, message, details: [], action: { kind: "dismiss" } };
  throw e;
}

const now = () => Math.floor(Date.now() / 1000);
let counter = 0;
const newId = (p: string) => `${p}-${++counter}`;

interface Source {
  id: string;
  title: string;
  format: string;
  added: number;
  parts: { section: string | null; heading: string; text: string }[];
  analysis: { kind: StyleKind; section: string | null; text: string }[] | null;
}

interface Version {
  id: string;
  version: number;
  status: "draft" | "active" | "retired";
  created: number;
  profile: StyleProfile;
}

function headingOf(line: string): string | null | undefined {
  const t = line.trim();
  if (t.length < 2 || t.length > 70 || t.endsWith(".") || /סיכום אבחון/.test(t)) return undefined;
  const hit = HEADINGS.find(([, words]) => words.some((w) => t.includes(w)));
  if (hit && t.split(/\s+/).length <= 6) return hit[0];
  return t.endsWith(":") ? null : undefined;
}

export class FakeStyle {
  private sources: Source[] = [];
  private versions: Version[] = [];
  private enabled = true;
  private staged: { token: string; title: string; format: string; parts: Source["parts"] } | null = null;
  private pending = new Map<string, { type: "analysis"; id: string } | { type: "profile" } | { type: "interpret" }>();
  private interpretations: InterpretationView[] = [];

  static handles(cmd: string): boolean {
    return cmd.includes("style") || cmd.includes("interpretation");
  }

  private neutral(text: string, people: Person[], practitioner: string[]): { text: string; hidden: number; numbers: number } {
    const f = filter(text, { caseId: "style", people: people.map((p) => ({ ...p, caseId: "style" })), practitioner, allowed: new Set() });
    let hidden = 0;
    const out = f.tagged_segments.map((s) => {
      if (!s.mark) return s.text;
      hidden += 1;
      if (s.mark === "suspect") return "[אדם_1]";
      if (s.mark === "relative") return "[תאריך]";
      return s.text.replace(/^\[([^\]]+?)_\d+\]$/, "[$1_1]");
    }).join("");
    // A score beside a subtest of the preview's short list stays as a score tag (round 32).
    const scored = out.replace(new RegExp(`(${SUBTESTS.map(([, n]) => n).join("|")})( \\(| )(\\d{1,3})(\\)?)`, "g"),
      (_m, name: string, open: string, v: string, close: string) => `${name}${open}[תוצאה ${v}]${close ?? ""}`);
    let numbers = 0;
    const masked = scored.replace(/(?<![A-Za-z-]|תוצאה \d*)\d+([.:/]\d+)*/g, () => { numbers += 1; return "[מספר]"; });
    return { text: masked, hidden, numbers };
  }

  private view(s: Source): StyleSourceView {
    return {
      id: s.id, title: s.title, format: s.format, added_at: s.added,
      words: s.parts.reduce((n, p) => n + p.text.split(/\s+/).filter(Boolean).length, 0),
      headings: s.parts.map((p) => p.heading), analyzed: s.analysis !== null, analysis_demo: s.analysis !== null,
      analysis_items: s.analysis?.length ?? 0,
      analysis_inferences: s.analysis?.filter((i) => i.kind === "inference").length ?? 0,
    };
  }

  private profileView(v: Version): StyleProfileView {
    return { id: v.id, version: v.version, status: v.status, created_at: v.created, profile: v.profile, dropped: 0, demo: true };
  }

  private add(status: Version["status"], profile: StyleProfile): StyleProfileView {
    if (status === "active") this.versions.forEach((v) => { if (v.status === "active") v.status = "retired"; });
    this.versions = this.versions.filter((v) => v.status !== "draft");
    const version = Math.max(0, ...this.versions.map((v) => v.version)) + 1;
    const v: Version = { id: newId("ver"), version, status, created: now(), profile };
    this.versions.unshift(v);
    return this.profileView(v);
  }

  /** Invented figures for the preview, once there is a report to learn from. */
  private learned(): StyleLearned {
    const n = this.sources.length;
    const has = n > 0;
    return {
      from_reports: 14 * n, from_edits: has ? 3 : 0, from_finals: 0, finished_reports: 0,
      words_per_sentence: has ? 13.4 : null, sentences_per_paragraph: has ? 3.2 : null, commas_per_sentence: has ? 1.1 : null,
      hedges: has ? 22 : null, contrasts: has ? 9 : null, first_person: has ? 4 : null,
      fit_recent: has ? 81 : null, fit_earlier: has ? 72 : null, fit_count: has ? 40 : 0,
      fit_by_month: has ? [{ month: "2026-08", average: 68, count: 12 }, { month: "2026-09", average: 74, count: 15 }, { month: "2026-10", average: 81, count: 13 }] : [],
    };
  }

  /** The preview's subtests that have a score tag in a saved report. */
  private scored(): [string, string][] {
    const text = this.sources.flatMap((s) => s.parts.map((p) => p.text)).join("\n");
    return SUBTESTS.filter(([, name]) => text.includes(`${name} [תוצאה`) || text.includes(`${name} ([תוצאה`));
  }

  private prepared(label: string, text: string, approval: string): Prepared {
    const seg = [{ text, mark: null, label: null }];
    return { approval_id: approval, parts: [{ label, original: seg, outgoing: seg }], suspects: [], auto_hidden: [], hidden: [], checks: { declared_names: 0, patterns: 0, name_suspects: 0, indirect_suspects: 0 }, blocked: [], demo_mode: true };
  }

  async handle(cmd: string, a: Args, args: unknown, headers: Record<string, string>, people: Person[], practitioner: string[]): Promise<unknown> {
    switch (cmd) {
      case "style_overview": {
        const active = this.versions.find((v) => v.status === "active");
        const draft = this.versions.find((v) => v.status === "draft");
        return {
          sources: this.sources.map((s) => this.view(s)),
          active: active ? this.profileView(active) : null,
          draft: draft ? this.profileView(draft) : null,
          versions: this.versions.filter((v) => v.status !== "draft").map((v) => ({ id: v.id, version: v.version, status: v.status, created_at: v.created, items: v.profile.items.length, reports: v.profile.reports })),
          suggestions: [],
          enabled: this.enabled,
          sections: SECTIONS.map((s) => ({ key: s.key, title: s.title })),
          demo_mode: true,
          learned: this.learned(),
          interpretations: this.interpretations,
          scored_measures: this.scored().length,
          skeletons: [],
          omissions: [],
        } satisfies StyleOverview;
      }
      case "import_style_source": {
        const name = decodeURIComponent(headers["x-file-name"] ?? "דוח");
        const ext = name.toLowerCase().split(".").pop();
        const bytes = args as Uint8Array;
        let body = "";
        if (ext === "docx") body = (await readDocx(bytes)).body;
        else if (ext === "txt") body = new TextDecoder().decode(bytes);
        else fail("preview", "בהדמיה בדפדפן אפשר לייבא Word או טקסט. PDF ו-ODT נקראים בתוכנה המותקנת.");
        if (!body.trim()) fail("empty", "לא נמצא טקסט במסמך.");
        const parts: Source["parts"] = [];
        let cur: Source["parts"][number] = { section: null, heading: "פתיחת הדוח", text: "" };
        for (const line of body.split("\n")) {
          const h = headingOf(line);
          if (h !== undefined) {
            if (cur.text.trim()) parts.push(cur);
            cur = { section: h, heading: line.trim().replace(/:$/, ""), text: "" };
          } else cur.text += `${line}\n`;
        }
        if (cur.text.trim()) parts.push(cur);
        let hidden = 0;
        let numbers = 0;
        const neutral = parts.map((p) => {
          const t = this.neutral(p.text.trim(), people, practitioner);
          hidden += t.hidden;
          numbers += t.numbers;
          return { section: p.section, heading: this.neutral(p.heading, people, practitioner).text, text: t.text };
        });
        const token = newId("upload");
        const title = this.neutral(name.replace(/\.[^.]+$/, ""), people, practitioner).text;
        this.staged = { token, title, format: ext === "docx" ? "docx" : "text", parts: neutral };
        return {
          token, title, format: this.staged.format, hidden, numbers, warnings: [],
          parts: neutral.map((p, i): StylePartView => ({ index: i, section: p.section, heading: p.heading, text: p.text, words: p.text.split(/\s+/).filter(Boolean).length, included: p.section !== null && INCLUDED.includes(p.section) })),
        } satisfies StyleImportPreview;
      }
      case "save_style_source": {
        if (!this.staged || this.staged.token !== a.token) return fail("not_found", "ההעלאה פגה. אפשר לבחור את הקובץ שוב.");
        const keep = new Set((a.included as number[]) ?? []);
        const parts = this.staged.parts.filter((_, i) => keep.has(i));
        if (!parts.length) return fail("refused", "צריך לסמן לפחות קטע אחד לשמירה.");
        const s: Source = { id: newId("src"), title: a.title ? String(a.title) : this.staged.title, format: this.staged.format, added: now(), parts, analysis: null };
        this.sources.push(s);
        this.staged = null;
        return this.view(s);
      }
      case "discard_style_upload":
        this.staged = null;
        return null;
      case "delete_style_source":
        this.sources = this.sources.filter((s) => s.id !== a.id);
        return null;
      case "prepare_style_analysis": {
        const s = this.sources.find((x) => x.id === a.sourceId) ?? fail("not_found", "לא נמצא: הדוח");
        const approval = newId("approval");
        this.pending.set(approval, { type: "analysis", id: s.id });
        return this.prepared(`${s.title} · קטעים`, s.parts.map((p) => p.text).join("\n\n"), approval);
      }
      case "send_style_analysis": {
        const p = this.pending.get(String(a.approvalId));
        if (p?.type !== "analysis") return fail("refused", "האישור לא תקף. יש להכין את השליחה מחדש.");
        this.pending.delete(String(a.approvalId));
        await new Promise((r) => setTimeout(r, 1200));
        const s = this.sources.find((x) => x.id === p.id) ?? fail("not_found", "הדוח נמחק בינתיים.");
        const text = s.parts.map((x) => x.text).join(" ");
        s.analysis = [
          { kind: "rule", section: null, text: "גוף נסתר וזמן הווה בתיאור הילד; גוף ראשון בתצפית המאבחנת" },
          ...(/עם זאת/.test(text) ? [{ kind: "rule" as const, section: null, text: "מבנה של חוזקה ואחריה הקושי, עם \"עם זאת\"" }] : []),
          ...(/כך לדוגמ/.test(text) ? [{ kind: "phrase" as const, section: null, text: "כך לדוגמא" }] : []),
          { kind: "template", section: "cognitive", text: "הציג תפקוד {רמה} ({תוצאה}), עם זאת ניכר קושי ב{תחום}" },
          { kind: "inference", section: null, text: "בתצפית: הילד עוזב משימה באמצע וחוזר אליה רק אחרי תיווך ← מסקנה: קושי בהתמדה ובוויסות, שנקבע כשהוא חוזר גם בדיווח הגן ← ניסוח: ניכר קושי ב{תחום}, הבא לידי ביטוי ב{דוגמה} ← המלצה: תיווך של מבוגר בתחילת משימה" },
        ];
        return { source_id: s.id, kept: s.analysis.length, dropped: 0, demo: true };
      }
      case "prepare_style_profile": {
        const analyzed = this.sources.filter((s) => s.analysis?.length);
        if (!analyzed.length) return fail("refused", "קודם צריך לנתח לפחות דוח אחד.");
        const approval = newId("approval");
        this.pending.set(approval, { type: "profile" });
        return this.prepared("ניתוחי הסגנון", analyzed.flatMap((s) => s.analysis ?? []).map((i) => i.text).join("\n"), approval);
      }
      case "send_style_profile": {
        const p = this.pending.get(String(a.approvalId));
        if (p?.type !== "profile") return fail("refused", "האישור לא תקף. יש להכין את השליחה מחדש.");
        this.pending.delete(String(a.approvalId));
        await new Promise((r) => setTimeout(r, 1600));
        const analyzed = this.sources.filter((s) => s.analysis?.length);
        const items: StyleItem[] = [];
        for (const i of analyzed.flatMap((s) => s.analysis ?? [])) {
          const same = items.find((x) => x.text === i.text && x.kind === i.kind);
          if (same) same.support += 1;
          else items.push({ id: newId("item"), section: i.section, kind: i.kind, text: i.text, enabled: true, origin: "reports", support: 1 });
        }
        items.push({ id: newId("item"), section: "cognitive", kind: "example", enabled: true, origin: "reports", support: 1,
          text: "הילד ניגש למשימות בסקרנות ובשיתוף פעולה. בתחום המילולי הציג תפקוד ממוצע ({מספר}), עם זאת ניכר קושי בזיכרון העבודה. כך לדוגמא, כשהמשימה חולקה לשלבים הצליח להשלים אותה." });
        const keep = this.versions.find((v) => v.status === "active")?.profile.items.filter((i) => i.origin !== "reports") ?? [];
        return this.add("draft", { items: [...items, ...keep], reports: analyzed.length });
      }
      case "prepare_interpretations": {
        const scored = this.scored();
        if (!scored.length) return fail("refused", "בדוחות הישנים אין עדיין תוצאה ליד שם של מבחן או תת-מבחן.");
        const approval = newId("approval");
        this.pending.set(approval, { type: "interpret" });
        const lines = this.sources.flatMap((s) => s.parts.flatMap((p) => p.text.split(/(?<=\.)\s+|\n/))).filter((l) => l.includes("[תוצאה"));
        return this.prepared("פרשנות התוצאות", lines.join("\n"), approval);
      }
      case "send_interpretations": {
        const p = this.pending.get(String(a.approvalId));
        if (p?.type !== "interpret") return fail("refused", "האישור לא תקף. יש להכין את השליחה מחדש.");
        this.pending.delete(String(a.approvalId));
        await new Promise((r) => setTimeout(r, 1200));
        this.interpretations = this.interpretations.filter((i) => i.item.approved || i.item.edited);
        const added = this.scored().map(([key, name]): InterpretationView => ({
          instrument_name: "WPPSI-IV", measure_name: name,
          item: { id: newId("interp"), instrument: "wppsi_iv", measure: key, case: "כללי", support: 1, approved: false, enabled: true, edited: false,
            text: "מצב הדגמה: כאן תופיע הפרשנות שלך לתת-המבחן, כפי שעולה מהדוחות שלך, במילים כלליות." },
        }));
        // As the core does: a text she already keeps is not added twice.
        const fresh = added.filter((v) => !this.interpretations.some((i) => i.item.measure === v.item.measure && i.item.text === v.item.text));
        this.interpretations.push(...fresh);
        return { kept: fresh.length, dropped: added.length - fresh.length, demo: true };
      }
      case "update_interpretation": {
        const v = this.interpretations.find((i) => i.item.id === a.id) ?? fail("not_found", "לא נמצא: הפרשנות");
        if (typeof a.text === "string") {
          const t = a.text.trim().replace(/\s+/g, " ");
          if (!t || t.length > 600) fail("refused", "הפרשנות צריכה להיות באורך של עד 600 תווים.");
          if (/[[\]]/.test(t)) fail("refused", "בטקסט יש משהו שנראה כמו שם או פרט מזהה. הפרשנות נשמרת במילים כלליות בלבד.");
          if (t !== v.item.text) v.item = { ...v.item, text: t, edited: true };
        }
        if (typeof a.approved === "boolean") v.item = { ...v.item, approved: a.approved };
        if (typeof a.enabled === "boolean") v.item = { ...v.item, enabled: a.enabled };
        return v;
      }
      case "delete_interpretation": {
        const before = this.interpretations.length;
        this.interpretations = this.interpretations.filter((i) => i.item.id !== a.id);
        if (this.interpretations.length === before) fail("not_found", "לא נמצא: הפרשנות");
        return null;
      }
      case "save_style_draft": {
        const profile = a.profile as StyleProfile;
        const items = profile.items.map((i) => (i.id ? i : { ...i, id: newId("item"), origin: "manual" as const }));
        if (items.some((i) => /[[\]<>]/.test(i.text))) fail("refused", "בפריט יש סוגריים מרובעים או משולשים. אפשר לכתוב \"הילד\" או את התפקיד במילים.");
        return this.add("draft", { ...profile, items });
      }
      case "approve_style_draft": {
        const d = this.versions.find((v) => v.status === "draft") ?? fail("not_found", "אין טיוטה לאישור.");
        return this.add("active", d.profile);
      }
      case "discard_style_draft":
        this.versions = this.versions.filter((v) => v.status !== "draft");
        return null;
      case "restore_style_version": {
        const old = this.versions.find((v) => v.id === a.id && v.status !== "draft") ?? fail("not_found", "לא נמצא: הגרסה");
        return this.add("active", old.profile);
      }
      case "set_style_enabled":
        this.enabled = Boolean(a.on);
        return null;
      case "reset_style":
        this.versions = [];
        return null;
      case "forget_style_learning":
        return null;
      case "accept_style_suggestion":
      case "dismiss_style_suggestion":
        return fail("not_found", "לא נמצא: ההצעה");
      default:
        return fail("unsupported", `פקודה לא מוכרת בהדמיה: ${cmd}`);
    }
  }
}
