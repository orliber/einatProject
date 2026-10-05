// Browser preview only: an in-memory stand-in for the Rust core, with fabricated cases. Nothing is
// stored and nothing leaves the page; "Claude" answers come from local demo text, like the app's
// demo mode. The installed app is the only place where real work happens (D-021).
import type { ActivityEntry } from "../ipc/generated/ActivityEntry";
import type { AppStatus } from "../ipc/generated/AppStatus";
import type { BackupStatus } from "../ipc/generated/BackupStatus";
import type { StagedBackup } from "../ipc/generated/StagedBackup";
import type { CaseDetail } from "../ipc/generated/CaseDetail";
import type { CaseInput } from "../ipc/generated/CaseInput";
import type { CaseMeta } from "../ipc/generated/CaseMeta";
import type { CaseSummary } from "../ipc/generated/CaseSummary";
import type { ChatView } from "../ipc/generated/ChatView";
import type { DraftStatus } from "../ipc/generated/DraftStatus";
import type { ExportCheck } from "../ipc/generated/ExportCheck";
import type { Identity } from "../ipc/generated/Identity";
import type { IdentityInput } from "../ipc/generated/IdentityInput";
import type { ImportPreview } from "../ipc/generated/ImportPreview";
import type { InputKind } from "../ipc/generated/InputKind";
import type { MaterialRouting } from "../ipc/generated/MaterialRouting";
import type { Folder } from "../ipc/generated/Folder";
import type { NameMatch } from "../ipc/generated/NameMatch";
import type { Prepared } from "../ipc/generated/Prepared";
import type { ReportSettings } from "../ipc/generated/ReportSettings";
import type { ReviewPart } from "../ipc/generated/ReviewPart";
import type { Role } from "../ipc/generated/Role";
import type { ScoreSheet } from "../ipc/generated/ScoreSheet";
import type { SectionResult } from "../ipc/generated/SectionResult";
import type { AutoHidden } from "../ipc/generated/AutoHidden";
import type { IdentitySource } from "../ipc/generated/IdentitySource";
import type { SuspectDecision } from "../ipc/generated/SuspectDecision";
import type { UiError } from "../ipc/generated/UiError";
import { kindLabel } from "../i18n/he";
import structure from "../../../../templates/report_structure.json";
import { readDocx } from "./docx";
import { filter, restore, type Person } from "./filter";
import { FakeStyle } from "./fakeStyle";
import { compareSheets, comparisonText, formatSheet, instruments } from "./scores";
import * as R from "./routing";
import type { SortResult } from "../ipc/generated/SortResult";

type Args = Record<string, unknown>;

interface Draft {
  id: string;
  section: string;
  /** Tagged, like the vault stores it. */
  text: string;
  status: DraftStatus;
  byAi: boolean;
  sources: string[];
  /** A new wording of this approved paragraph (D-032). */
  replaces?: string;
  /** Earlier wordings, newest first (D-046). */
  versions?: { id: string; at: number; byAi: boolean; text: string }[];
}

/** Keep the paragraph's wording before it changes in place (D-046). */
function keepVersion(d: Draft, next: string) {
  if (d.text === next) return;
  d.versions = [{ id: newId("v"), at: Math.floor(Date.now() / 1000), byAi: d.byAi, text: d.text }, ...(d.versions ?? [])];
}

/** Its earlier wordings, then those of the approved paragraphs it took over from. */
function versionsOf(drafts: Draft[], d: Draft): NonNullable<Draft["versions"]> {
  const out = [...(d.versions ?? [])];
  let from = d.replaces && d.status === "approved" ? drafts.find((x) => x.id === d.replaces) : undefined;
  for (let i = 0; from && i < 50; i++) {
    out.push({ id: from.id, at: 0, byAi: from.byAi, text: from.text }, ...(from.versions ?? []));
    from = from.replaces ? drafts.find((x) => x.id === from?.replaces) : undefined;
  }
  return out;
}

interface Case {
  id: string;
  meta: CaseMeta;
  people: (Person & { id: string })[];
  inputs: CaseInput[];
  drafts: Draft[];
  chat: Record<string, ChatView[]>;
  sheets: Map<string, ScoreSheet>;
  allowed: Set<string>;
  /** D-022: where each material goes (by input id). */
  routing: Map<string, R.Routing>;
  folderId: string | null;
  deletedAt: number | null;
  created: number;
  updated: number;
}

type Pending =
  | { type: "section"; caseId: string; section: string; refs: { sid: string; label: string; tagged: string }[]; instruction: string; replaces: string | null }
  | { type: "consult"; question: string; shown: string; hidden: string[]; caseId: string | null; conversationId: string | null }
  | { type: "sort"; caseId: string; materials: { id: string; count: number; content: string }[] };

const SECTIONS = structure.parts.flatMap((p) => p.sections.map((s) => ({ ...s, part: p.title, inputs: s.inputs as InputKind[] })));
/** Sections written from materials (the rest are written from other sections). */
const SORTABLE = SECTIONS.filter((s) => s.inputs.length > 0 && !["dsm", "summary", "diagnoses", "recommendations"].includes(s.key));
const DERIVED = ["dsm", "summary", "diagnoses", "recommendations"];
const SINGLETON: Role[] = ["child", "mother", "father", "teacher", "kindergarten", "town"];
const TAG_BASE: Record<Role, string> = {
  child: "ילד", mother: "אם", father: "אב", brother: "אח", sister: "אחות", teacher: "גננת", assistant: "סייעת", doctor: "רופא",
  slp: "קלינאית", psychologist: "פסיכולוגית", therapist: "מטפלת", other_child: "ילד_גן", kindergarten: "גן", school: "בית_ספר",
  town: "יישוב", institution: "מוסד", other: "אדם", relative: "קרוב_משפחה", school_teacher: "מורה",
  professional: "איש_מקצוע", family: "משפחה",
};

const now = () => Math.floor(Date.now() / 1000);
let counter = 0;
const newId = (p: string) => `${p}-${++counter}`;

function fail(code: string, message: string): never {
  const e: UiError = { code, message, details: [] };
  throw e;
}

function sentences(text: string, n: number): string {
  return (text.match(/[^.!?]+[.!?]?/g) ?? []).slice(0, n).join("").trim();
}

export class FakeCore {
  private unlocked = false;
  /** `?setup` in the address opens the first-run screens (for the demo page's pictures). */
  private vaultExists = typeof location === "undefined" || !location.search.includes("setup");
  // Nine days ago, so the preview shows the weekly reminder (D-024).
  private lastBackupAt: number | null = Math.floor(Date.now() / 1000) - 9 * 86_400;
  private lastCheckAt: number | null = null;
  private secretChanged = false;
  private autoBackup = true;
  private reviewedAt: number | null = null;
  private convs: { id: string; caseId: string | null; updated: number; turns: { role: string; text: string; hidden: string[]; demo: boolean; at: number }[] }[] = [];
  private activityLog: ActivityEntry[] = [];
  private practitioner = ["ד\"ר רותם בדויה"];
  private lockMinutes = 15;
  private capUsd: number | null = null;
  private ready: Record<string, number | null> = {};
  private model = "claude-opus-5";
  private speed = "balanced";
  private reviewOnlySuspect = false;
  private screenProtection = true;
  private report: ReportSettings = {
    title: "דוח אבחון פסיכולוגי-התפתחותי",
    font: "David",
    confidentiality: "חסוי – מידע רפואי-פסיכולוגי. לשימוש הגורם המטפל בלבד.",
    signature: ["ד\"ר רותם בדויה", "פסיכולוגית התפתחותית מומחית"],
  };
  private cases: Case[] = [];
  private folderList: Folder[] = [];
  private pending = new Map<string, Pending>();
  private style = new FakeStyle();

  constructor() {
    this.seed();
    this.seedActivity();
  }

  /** A believable log for the preview; real entries are added as things happen. */
  private seedActivity() {
    const t = now();
    const noam = "נועם · תיק-1024";
    const rows: [number, string, string, string, boolean, string | null][] = [
      [t - 12 * 86_400, "case_created", "case", "תיק נפתח", false, noam],
      [t - 12 * 86_400 + 60, "identities_changed", "case", "רשימת האנשים בתיק עודכנה", false, noam],
      [t - 9 * 86_400, "backup_written", "security", "גיבוי מוצפן נשמר", false, null],
      [t - 3 * 86_400, "unlock_failed", "access", "ניסיון כניסה שנכשל לפני הכניסה הזו", true, null],
      [t - 3 * 86_400 + 5, "unlock", "access", "כניסה", false, null],
      [t - 3 * 86_400 + 600, "send", "send", "החומרים נשלחו ל-Claude למיון לסעיפים (הדגמה: לא יצא מהמחשב)", false, noam],
      [t - 3 * 86_400 + 900, "send", "send", "הסעיף \"רקע התפתחותי\" נשלח ל-Claude (הדגמה: לא יצא מהמחשב)", false, noam],
      [t - 2 * 86_400, "lock", "access", "נעילה (המחשב נכנס לשינה)", false, null],
      [t - 86_400, "export", "case", "דוח Word הופק, מוגן בסיסמה", false, noam],
      [t - 86_400 + 60, "lock", "access", "נעילה", false, null],
    ];
    this.activityLog = rows.map(([ts, event, kind, text, warn, c], i) => ({ seq: i + 1, ts, event, kind, text, warn, case: c }));
  }

  private logActivity(event: string, kind: string, text: string, warn = false) {
    const seq = (this.activityLog.at(-1)?.seq ?? 0) + 1;
    this.activityLog.push({ seq, ts: now(), event, kind, text, warn, case: null });
  }

  // ------------------------------------------------------------------ fabricated cases
  private seed() {
    const a = this.newCase(
      { code: "תיק-1024", age: { years: 5, months: 4 }, child_gender: "male", current_section: null, retention_until: null,
        consent: { given_on: "2026-09-01", form_version: "v1", given_by: "שני ההורים" } },
      [
        { id: null, role: "child", value: "נועם", aliases: ["נועמי"] },
        { id: null, role: "mother", value: "דנה", aliases: [] },
        { id: null, role: "father", value: "יוסי", aliases: [] },
        { id: null, role: "teacher", value: "מיכל", aliases: [] },
      ],
    );
    a.created = now() - 86_400 * 12;
    this.addInput(a, "intake", "אינטייק עם ההורים",
      "דנה ויוסי פנו בעקבות המלצת הגננת מיכל, בשל קושי של נועם במעברים ובמשחק עם ילדים. ההריון והלידה היו תקינים. " +
      "נועם הלך בגיל שנה ושלושה חודשים ואמר מילים ראשונות בגיל שנה וחצי. לדברי ההורים הוא ילד סקרן ואוהב פאזלים, " +
      "אך מתקשה להירדם ומתעורר פעמיים בלילה. בבית הוא נוטה להתפרצויות כשמשנים לו תוכנית.");
    this.addInput(a, "kindergarten", "שיחה עם הגננת",
      "מיכל סיפרה שנועם מגיע בבוקר בשמחה ונפרד מדנה בקלות. במפגש הבוקר מתקשה לשבת לאורך זמן ומשתתף יותר בקבוצה קטנה. " +
      "בחצר משחק בעיקר לבד או ליד ילדים, ופחות איתם. כשיש שינוי בסדר היום הוא מבקש הסבר חוזר ונרגע כשמכינים אותו מראש.");
    this.addInput(a, "session_note", "מפגש שני",
      "במפגש השני נועם הגיע בשמחה ונפרד מדנה בקלות. בפינת הבנייה סיפר שהוא משחק בגן בעיקר עם יובל. " +
      "התקשה לספר רצף אירועים ונעזר בתמונות. כשביקשתי לעבור למשחק אחר, התנגד ונרגע אחרי כשתי דקות.");
    const sheet: ScoreSheet = {
      instrument: "wppsi_iv", module: "", cutoff: null, notes: "שיתף פעולה לאורך ההעברה, נעזר בעידוד בפריטים הקשים.",
      entries: [
        { measure: "fsiq", value: 102, note: "" }, { measure: "vci", value: 112, note: "" }, { measure: "vsi", value: 104, note: "" },
        { measure: "fri", value: 106, note: "" }, { measure: "wmi", value: 95, note: "" }, { measure: "psi", value: 86, note: "עבד לאט ובדייקנות" },
        { measure: "similarities", value: 13, note: "" }, { measure: "block_design", value: 11, note: "" }, { measure: "matrix", value: 12, note: "" },
      ],
    };
    const scores = this.addInput(a, "test_scores", "ציוני WPPSI-IV", formatSheet(sheet));
    a.sheets.set(scores.id, sheet);
    for (const [section, text] of [
      ["referral", "[ילד] הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו, בעקבות המלצת [גננת], בשל קושי במעברים ובמשחק משותף עם ילדים."],
      ["background", "ההריון והלידה היו תקינים. אבני הדרך המוטוריות והשפתיות הושגו בטווח התקין: [ילד] הלך בגיל שנה ושלושה חודשים ואמר מילים ראשונות בגיל שנה וחצי."],
    ] as const) {
      a.drafts.push({ id: newId("d"), section, text, status: "approved", byAi: true, sources: ["S1 · אינטייק הורים · אינטייק עם ההורים"] });
    }

    const b = this.newCase(
      { code: "תיק-1025", age: { years: 4, months: 2 }, child_gender: "female", current_section: null, retention_until: null,
        consent: { given_on: "2026-09-20", form_version: "v1", given_by: "האם" } },
      [
        { id: null, role: "child", value: "מאיה", aliases: [] },
        { id: null, role: "mother", value: "ענבל", aliases: [] },
      ],
    );
    b.created = now() - 86_400 * 3;
    this.addInput(b, "intake", "שיחת טלפון ראשונה",
      "ענבל פנתה בשל עיכוב בדיבור של מאיה. מאיה מבינה הוראות פשוטות ומשתמשת במשפטים של שתיים-שלוש מילים.");
    b.updated = now() - 86_400 * 2;

    // Folders, like in a file explorer (D-023).
    const priv = this.addFolder(null, "אבחונים פרטיים");
    this.addFolder(priv.id, "2026");
    this.addFolder(null, "הפניות מהמכון");
    b.folderId = priv.id;
  }

  private addFolder(parent: string | null, name: string): Folder {
    const f: Folder = { id: newId("folder"), parent_id: parent, name, created_at: now() };
    this.folderList.push(f);
    return f;
  }

  private folderOf(id: unknown): Folder {
    return this.folderList.find((f) => f.id === id) ?? fail("not_found", "התיקייה לא נמצאה");
  }

  private nameMatches(caseId: string | null, names: string[]): NameMatch[] {
    const words = (v: string) => v.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((w) => w.length >= 3);
    const out: NameMatch[] = [];
    for (const c of this.cases) {
      if (c.id === caseId) continue;
      for (const p of c.people) {
        const known = [p.value, ...p.aliases].flatMap(words);
        for (const typed of names) {
          if (words(typed).some((w) => known.includes(w)) && !out.some((m) => m.case_id === c.id && m.typed === typed && m.value === p.value)) {
            out.push({ typed, case_id: c.id, case_code: c.meta.code, child_name: c.people.find((x) => x.role === "child")?.value ?? null, role: p.role, value: p.value, trashed: c.deletedAt !== null });
          }
        }
      }
    }
    return out;
  }

  private newCase(meta: CaseMeta, people: IdentityInput[]): Case {
    const c: Case = { id: newId("case"), meta, people: [], inputs: [], drafts: [], chat: {}, sheets: new Map(), allowed: new Set(), routing: new Map(), folderId: null, deletedAt: null, created: now(), updated: now() };
    this.cases.push(c);
    this.setPeople(c, people);
    return c;
  }

  private setPeople(c: Case, list: IdentityInput[]) {
    const kept: Case["people"] = [];
    for (const i of list.filter((x) => x.value.trim())) {
      const old = i.id ? c.people.find((p) => p.id === i.id) : undefined;
      const used = kept.map((p) => p.tag);
      let tag = old?.role === i.role ? old.tag : "";
      if (!tag) {
        const base = TAG_BASE[i.role];
        tag = SINGLETON.includes(i.role) && !used.includes(`[${base}]`) ? `[${base}]` : "";
        for (let n = SINGLETON.includes(i.role) ? 2 : 1; !tag; n++) if (!used.includes(`[${base}_${n}]`)) tag = `[${base}_${n}]`;
      }
      const same = old?.role === i.role;
      kept.push({ id: same ? old.id : newId("p"), caseId: c.id, role: i.role, tag, value: i.value.trim(), aliases: i.aliases,
        source: same ? old.source : "manual", reason: same ? old.reason : "" });
    }
    c.people = kept;
  }

  private addInput(c: Case, kind: InputKind, title: string, content: string): CaseInput {
    const input: CaseInput = { id: newId("in"), case_id: c.id, kind, title, content, created_at: now() - 60 * (10 - c.inputs.length) };
    c.inputs.push(input);
    c.updated = now();
    return input;
  }

  private find(id: unknown): Case {
    return this.cases.find((c) => c.id === id) ?? fail("not_found", "התיק לא נמצא");
  }

  private allPeople(): Person[] {
    return this.cases.flatMap((c) => c.people);
  }

  /** Filter for the case; new names it finds are kept with the case (as the core does), and
   *  the text is filtered again so they carry their kept tags. */
  private filterFor(c: Case, text: string) {
    const run = () => filter(text, { caseId: c.id, people: this.allPeople(), practitioner: this.practitioner, allowed: c.allowed });
    const first = run();
    const found = first.auto_hidden.filter((a) => (a.kind === "name" || a.kind === "other_case") && !c.people.some((p) => p.tag === a.tag));
    if (!found.length) return first;
    this.keepFound(c, found.map((a) => ({ value: a.token, role: a.role, source: "auto", reason: a.reason })));
    return run();
  }

  /** The card also lists names kept with the case earlier, wherever their tags go out (as the core does). */
  private withKept(c: Case | null, p: Prepared): Prepared {
    if (!c) return p;
    const out = p.parts.flatMap((x) => x.outgoing.map((s) => s.text)).join("");
    const auto_hidden = [...p.auto_hidden];
    for (const person of c.people) {
      if (person.source === "manual" || !out.includes(person.tag) || auto_hidden.some((a) => a.tag === person.tag)) continue;
      auto_hidden.push({ token: person.value, tag: person.tag, role: person.role, reason: person.reason, uncertain: false, kind: "name" });
    }
    return { ...p, auto_hidden };
  }

  private keepFound(c: Case, found: { value: string; role: Role; source: IdentitySource; reason: string }[]) {
    for (const f of found) {
      if (c.people.some((p) => p.value === f.value || p.aliases.includes(f.value))) continue;
      const used = c.people.map((p) => p.tag);
      const base = TAG_BASE[f.role];
      let tag = "";
      for (let n = 1; !tag; n++) if (!used.includes(`[${base}_${n}]`)) tag = `[${base}_${n}]`;
      c.people.push({ id: newId("p"), caseId: c.id, role: f.role, tag, value: f.value, aliases: [], source: f.source, reason: f.reason });
    }
  }

  status(): AppStatus {
    return {
      vault_exists: this.vaultExists, unlocked: this.unlocked, disk_encryption: "on", cloud_synced_folder: null, fips_active: true,
      demo_mode: true, model: this.model, speed: this.speed, integrity_warning: null, lock_minutes: this.lockMinutes, idle_lock_in: this.unlocked ? this.lockMinutes * 60 : null,
      practitioner: this.practitioner, review_only_suspect: this.reviewOnlySuspect, review_choice_available: true, screen_protection: !this.unlocked || this.screenProtection,
    };
  }

  private summary(c: Case): CaseSummary {
    const approved = Array.from(new Set(c.drafts.filter((d) => d.status === "approved").map((d) => d.section)));
    return { id: c.id, meta: c.meta, child_name: c.people.find((p) => p.role === "child")?.value ?? null, created_at: c.created, updated_at: c.updated, approved_sections: approved, folder_id: c.folderId, deleted_at: c.deletedAt };
  }

  private tableFor(kind: InputKind): string[] {
    return SORTABLE.filter((s) => s.inputs.includes(kind)).map((s) => s.key);
  }

  private routingOf(c: Case, id: string): R.Routing {
    return c.routing.get(id) ?? { added: [], removed: [] };
  }

  /** What goes from one material to one section (D-022). */
  private feedOf(c: Case, i: CaseInput, section: string): R.Feed | null {
    return R.feed(this.routingOf(c, i.id), section, this.tableFor(i.kind), R.passages(i.content).length);
  }

  /** Where each material goes (D-022), like the core computes it. */
  private routing(c: Case): MaterialRouting[] {
    return c.inputs.map((i) => {
      const r = this.routingOf(c, i.id);
      const table = this.tableFor(i.kind);
      const count = R.passages(i.content).length;
      const feeds = SORTABLE.map((s) => s.key).filter((k) => R.feed(r, k, table, count) !== null);
      const sorted = R.isSorted(r, count);
      const used = sorted ? new Set(r.suggestion?.sections.filter((x) => feeds.includes(x.section)).flatMap((x) => x.passages)).size : count;
      return {
        input_id: i.id, feeds, suggested: sorted ? R.base(r, table, count) : [], table, sorted, needs_sorting: R.needsSorting(r, count),
        by_ai: false, added: r.added, removed: r.removed, passages: count, used_passages: used,
      };
    });
  }

  private detail(c: Case): CaseDetail {
    const identities: Identity[] = c.people.map((p) => ({ id: p.id, case_id: c.id, role: p.role, tag: p.tag, value: p.value, aliases: p.aliases, source: p.source, reason: p.reason }));
    const routing = this.routing(c);
    return {
      id: c.id, meta: c.meta, identities, inputs: c.inputs, routing,
      retention_default: `${new Date(c.created * 1000).getFullYear() + Math.max(7, 25 - (c.meta.age?.years ?? 25))}-${new Date(c.created * 1000).toISOString().slice(5, 10)}`,
      sections: SECTIONS.map((s) => {
        const drafts = c.drafts.filter((d) => d.section === s.key && d.status !== "rejected" && d.status !== "superseded");
        return {
          key: s.key, title: s.title, part: s.part,
          source_count: routing.filter((r) => r.feeds.includes(s.key)).length,
          sortable: SORTABLE.some((x) => x.key === s.key),
          paragraphs: drafts.map((d) => ({ id: d.id, text: restore(d.text, c.people, this.practitioner), status: d.status, by_ai: d.byAi, sources: d.sources, warnings: [], replaces: d.replaces ?? null, has_versions: versionsOf(c.drafts, d).length > 0, style_note: null })),
          approved: drafts.some((d) => d.status === "approved"),
        };
      }),
    };
  }

  private prepareSection(c: Case, key: string, instruction: string, replaces: string | null = null): Prepared {
    if (!c.meta.consent) fail("consent_missing", "לפני שליחה ל-Claude צריך לרשום בתיק את הסכמת ההורים.");
    const section = SECTIONS.find((s) => s.key === key) ?? fail("not_found", "הסעיף לא נמצא");
    const parts: ReviewPart[] = [];
    const autoHidden: AutoHidden[] = [];
    const hidden = new Set<string>();
    const checks = { declared_names: 0, patterns: 0, name_suspects: 0, indirect_suspects: 0 };
    const refs: { sid: string; label: string; tagged: string }[] = [];
    const take = (label: string, text: string) => {
      const f = this.filterFor(c, text);
      parts.push({ label, original: f.original_segments, outgoing: f.tagged_segments });
      for (const a of f.auto_hidden) if (!autoHidden.some((x) => x.token === a.token)) autoHidden.push(a);
      f.hidden.forEach((h) => hidden.add(h));
      checks.declared_names += f.checks.declared_names;
      checks.patterns += f.checks.patterns;
      checks.name_suspects += f.checks.name_suspects;
      return f.tagged;
    };
    const sources = DERIVED.includes(key)
      ? c.drafts.filter((d) => d.status === "approved" && !DERIVED.includes(d.section)).map((d) => ({ label: `סעיף מאושר · ${SECTIONS.find((s) => s.key === d.section)?.title ?? ""}`, text: restore(d.text, c.people, this.practitioner) }))
      : c.inputs.flatMap((i) => {
          const f = this.feedOf(c, i, section.key);
          if (f === null) return [];
          if (f === "whole") return [{ label: `${kindLabel[i.kind]} · ${i.title}`, text: i.content }];
          return [{ label: `${kindLabel[i.kind]} · קטעים שנבחרו לסעיף · ${i.title}`, text: R.excerpt(R.passages(i.content), f) }];
        });
    if (DERIVED.includes(key) && sources.length === 0) {
      fail("refused", "הסעיף הזה נכתב מתוך הסעיפים שכבר אישרת, ועוד לא אישרת אף סעיף. מאשרים קודם את הטיוטות בסעיפים האחרים, ואז חוזרים לכאן.");
    }
    sources.forEach((s, n) => {
      const sid = `S${n + 1}`;
      refs.push({ sid, label: `${sid} · ${s.label}`, tagged: take(`${sid} · ${s.label}`, s.text) });
    });
    if (instruction.trim()) take("הבקשה שלך", instruction);
    const approval = newId("approval");
    this.pending.set(approval, { type: "section", caseId: c.id, section: key, refs, instruction, replaces });
    return this.withKept(c, { approval_id: approval, parts, suspects: [], auto_hidden: autoHidden, hidden: Array.from(hidden), checks, blocked: [], demo_mode: true });
  }

  /** Sorting into sections (D-022): every material not sorted yet, one review. */
  private prepareSort(c: Case): Prepared {
    if (!c.meta.consent) fail("consent_missing", "לפני שליחה ל-Claude צריך לרשום בתיק את הסכמת ההורים.");
    const todo = c.inputs.filter((i) => R.needsSorting(this.routingOf(c, i.id), R.passages(i.content).length));
    if (!todo.length) fail("refused", "כל החומרים כבר ממוינים לסעיפים.");
    const parts: ReviewPart[] = [];
    const autoHidden: AutoHidden[] = [];
    const hidden = new Set<string>();
    const checks = { declared_names: 0, patterns: 0, name_suspects: 0, indirect_suspects: 0 };
    todo.forEach((i, n) => {
      const f = this.filterFor(c, i.content);
      parts.push({ label: `S${n + 1} · ${kindLabel[i.kind]} · ${i.title}`, original: f.original_segments, outgoing: f.tagged_segments });
      for (const a of f.auto_hidden) if (!autoHidden.some((x) => x.token === a.token)) autoHidden.push(a);
      f.hidden.forEach((h) => hidden.add(h));
      checks.declared_names += f.checks.declared_names;
      checks.patterns += f.checks.patterns;
      checks.name_suspects += f.checks.name_suspects;
    });
    const approval = newId("approval");
    this.pending.set(approval, { type: "sort", caseId: c.id, materials: todo.map((i) => ({ id: i.id, count: R.passages(i.content).length, content: i.content })) });
    return this.withKept(c, { approval_id: approval, parts, suspects: [], auto_hidden: autoHidden, hidden: Array.from(hidden), checks, blocked: [], demo_mode: true });
  }

  private sendSort(p: Extract<Pending, { type: "sort" }>): SortResult {
    const c = this.find(p.caseId);
    const keys = SORTABLE.map((s) => s.key);
    const result: SortResult = { sorted: 0, unchanged: 0, links: 0, ignored: 0, demo: true };
    for (const m of p.materials) {
      const i = c.inputs.find((x) => x.id === m.id);
      if (!i || i.content !== m.content) {
        result.unchanged += 1;
        continue;
      }
      const r = this.routingOf(c, m.id);
      const placed = R.sortLocally(R.passages(i.content), keys);
      if (!placed.length) {
        c.routing.set(m.id, { ...r, unplaced: m.count });
        result.unchanged += 1;
        continue;
      }
      c.routing.set(m.id, { ...r, suggestion: { byAi: false, passageCount: m.count, sections: placed } });
      result.sorted += 1;
      result.links += placed.length;
    }
    return result;
  }

  private sendSection(p: Extract<Pending, { type: "section" }>): SectionResult {
    const c = this.find(p.caseId);
    const paragraphs = p.refs.slice(0, 3).map((r) => ({ text: sentences(r.tagged, 2), source_refs: [r.sid], warnings: [] as string[] })).filter((x) => x.text);
    const approvedTarget = p.replaces ? c.drafts.find((d) => d.id === p.replaces && d.status === "approved") : undefined;
    const target = p.replaces ? c.drafts.find((d) => d.id === p.replaces && d.status === "proposed") : undefined;
    if (approvedTarget && paragraphs.length) {
      for (const d of c.drafts) if (d.replaces === approvedTarget.id && d.status === "proposed") d.status = "superseded";
      c.drafts.push({ id: newId("d"), section: p.section, text: `${paragraphs[0]?.text ?? approvedTarget.text} (ניסוח חדש)`, status: "proposed", byAi: true, sources: approvedTarget.sources, replaces: approvedTarget.id });
      paragraphs.splice(1);
    } else if (target && paragraphs.length) {
      // A rewrite of one paragraph stays where it is.
      const next = `${paragraphs[0]?.text ?? target.text} (ניסוח אחר)`;
      keepVersion(target, next);
      target.text = next;
      paragraphs.splice(1);
    } else {
      for (const d of c.drafts) if (d.section === p.section && d.status === "proposed" && d.byAi) d.status = "superseded";
      for (const x of paragraphs) {
        c.drafts.push({ id: newId("d"), section: p.section, text: x.text, status: "proposed", byAi: true, sources: p.refs.filter((r) => x.source_refs.includes(r.sid)).map((r) => r.label) });
      }
    }
    const reply = paragraphs.length
      ? `מצב הדגמה: ניסחתי ${paragraphs.length} פסקאות לדוגמה מתוך החומרים. בתוכנה, עם חיבור ל-Claude, הניסוח נעשה בסגנון שלך ומצליב בין החומרים.`
      : "מצב הדגמה: אין עדיין חומרים לסעיף הזה. אפשר להוסיף אינטייק, מפגש או מסמך.";
    const chat = (c.chat[p.section] ??= []);
    if (p.instruction.trim()) chat.push({ role: "user", text: p.instruction, hidden: [], demo: true });
    chat.push({ role: "assistant", text: reply, hidden: [], demo: true });
    c.updated = now();
    return {
      reply, paragraphs: paragraphs.map((x) => ({ ...x, text: restore(x.text, c.people, this.practitioner) })),
      questions: ["האם יש מידע נוסף מהגן על ההשתתפות במפגשי הבוקר?"], missing: [], contradictions: [], demo: true,
    };
  }

  private async importDocument(bytes: Uint8Array, headers: Record<string, string>): Promise<ImportPreview> {
    const c = this.find(decodeURIComponent(headers["x-case-id"] ?? ""));
    const name = decodeURIComponent(headers["x-file-name"] ?? "מסמך");
    const ext = name.toLowerCase().split(".").pop();
    let body = "";
    let margins: string[] = [];
    let author: string | null = null;
    if (ext === "docx") {
      try {
        ({ body, margins, author } = await readDocx(bytes));
      } catch {
        fail("corrupt", "הקובץ לא נפתח כמסמך Word תקין.");
      }
    } else if (ext === "txt") {
      body = new TextDecoder().decode(bytes);
    } else if (ext === "pdf" || ext === "odt") {
      fail("preview", "בהדמיה בדפדפן אפשר לייבא Word או טקסט. קובצי PDF ו-ODT נקראים בתוכנה המותקנת, בתהליך מבודד.");
    } else {
      fail("unsupported", "אפשר לייבא קובצי Word (docx), ODT, PDF או טקסט.");
    }
    if (!body.trim()) fail("empty", "לא נמצא טקסט במסמך.");
    // Names in the file's properties and margins are kept with the case without asking.
    const known = new Set(this.allPeople().flatMap((p) => [p.value, ...p.aliases]));
    const fromFile = [
      ...(author && !known.has(author) ? [{ value: author, reason: "מאפייני הקובץ · יוצר המסמך" }] : []),
      ...margins.flatMap((l) => (l.match(/[א-ת]+ בדוי[הא]?/g) ?? []).map((v) => ({ value: v, reason: "כותרת עליונה/תחתונה" }))),
    ].filter((s, i, all) => !known.has(s.value) && all.findIndex((x) => x.value === s.value) === i);
    this.keepFound(c, fromFile.map((s) => ({ ...s, role: "other" as Role, source: "metadata" as IdentitySource })));
    const f = this.filterFor(c, body);
    const first = body.split("\n")[0]?.trim() ?? "";
    const autoHidden: AutoHidden[] = [
      ...c.people.filter((p) => fromFile.some((s) => s.value === p.value)).map((p) => ({ token: p.value, tag: p.tag, role: p.role, reason: p.reason, uncertain: false, kind: "name" as const })),
      ...f.auto_hidden,
    ];
    const words = body.slice(0, 400);
    const kind: InputKind = /ציון|אחוזון|WPPSI|WISC/.test(words) && (words.match(/\d+/g)?.length ?? 0) > 6 ? "test_scores"
      : /אינטייק|ההורים סיפרו/.test(words) ? "intake" : /גננת|בגן/.test(words.slice(0, 80)) ? "kindergarten" : "prior_report";
    return {
      file_name: name, format: ext === "docx" ? "docx" : "text", pages: 1,
      title: first.length >= 3 && first.length <= 80 ? first : name.replace(/\.[^.]+$/, ""),
      suggested_kind: kind, body, preview: f.original_segments, suspects: [], auto_hidden: autoHidden, hidden: f.hidden,
      left_out: margins.length ? [...margins, "הכותרת העליונה/התחתונה לא יובאה (יש בה לרוב שם, ת\"ז ופרטי קשר)."] : [],
      name_suggestions: [], warnings: [],
    };
  }

  // ------------------------------------------------------------------ the IPC surface
  async handle(cmd: string, args: unknown, options?: { headers?: Record<string, string> }): Promise<unknown> {
    const a = (args ?? {}) as Args;
    if (!["ping", "app_status", "unlock", "unlock_with_recovery", "create_vault", "confirm_recovery_key", "score_instruments", "preview_scores", "choose_backup", "restore_backup", "forget_backup"].includes(cmd) && !this.unlocked) {
      fail("locked", "הכספת נעולה. יש לפתוח אותה מחדש.");
    }
    if (FakeStyle.handles(cmd)) return this.style.handle(cmd, a, args, options?.headers ?? {}, this.allPeople(), this.practitioner);
    switch (cmd) {
      case "ping":
        return { ipc_version: 1, core_version: "0.1.0", build_commit: "browser-preview", fips_active: false, platform: "browser" };
      case "app_status":
        return this.status();
      case "unlock":
      case "unlock_with_recovery":
        this.unlocked = true;
        this.logActivity("unlock", "access", cmd === "unlock" ? "כניסה" : "כניסה עם ערכת השחזור", cmd !== "unlock");
        return this.status();
      case "create_vault":
        this.vaultExists = true;
        this.unlocked = true;
        return { recovery_key: "DEMO-PREV-IEWX-KEYS-ONLY-4TST" };
      case "confirm_recovery_key":
        return true;
      case "touch":
        return null;
      case "hold_unsaved":
        return null;
      case "take_unsaved":
        return null;
      case "readiness":
      case "confirm_readiness": {
        if (cmd === "confirm_readiness") this.ready[a.key as string] = a.done ? 1_790_000_000 : null;
        const manual = ["zdr", "consent_form", "score_tables", "legal"].map((key) => ({
          key, done: this.ready[key] != null, checked_by_program: false, confirmed_at: this.ready[key] ?? null,
        }));
        const items = [...manual,
          { key: "api_key", done: false, checked_by_program: true, confirmed_at: null },
          { key: "backup", done: false, checked_by_program: true, confirmed_at: null },
          { key: "disk", done: true, checked_by_program: true, confirmed_at: null }];
        return { items, all_done: false };
      }
      case "usage_summary":
        return { month: "2026-10", requests: 0, input_tokens: 0, output_tokens: 0, estimated_cents: 0, cap_usd: this.capUsd, unpriced_models: [] };
      case "set_monthly_cap":
        this.capUsd = (a.capUsd as number | null) ?? null;
        return null;
      case "lock":
        this.unlocked = false;
        return null;
      case "set_api_key":
        return fail("preview", "בהדמיה בדפדפן אין חיבור ל-Claude. מפתח API מוזן רק בתוכנה המותקנת, ונשמר בה מוצפן.");
      case "set_model":
        this.model = String(a.model);
        return null;
      case "set_speed":
        this.speed = String(a.speed);
        return null;
      case "set_lock_minutes":
        this.lockMinutes = Number(a.minutes);
        return null;
      case "set_practitioner":
        this.practitioner = a.names as string[];
        return null;
      case "set_screen_protection":
        this.screenProtection = Boolean(a.on);
        return null;
      case "set_review_only_suspect":
        this.reviewOnlySuspect = Boolean(a.on);
        return null;
      case "report_settings":
        return this.report;
      case "set_report_settings":
        this.report = a.settings as ReportSettings;
        return null;
      case "list_cases":
        return this.cases.filter((c) => c.deletedAt === null).map((c) => this.summary(c));
      case "list_trash":
        return this.cases.filter((c) => c.deletedAt !== null).map((c) => this.summary(c));
      case "restore_case": {
        const c = this.find(a.caseId);
        c.deletedAt = null;
        if (c.folderId && !this.folderList.some((f) => f.id === c.folderId)) c.folderId = null;
        return null;
      }
      case "purge_case": {
        const c = this.find(a.caseId);
        if (c.deletedAt === null) fail("refused", "אפשר למחוק לצמיתות רק תיק שנמצא בסל המחזור.");
        if (!String(a.password ?? "")) fail("wrong_secret", "הסיסמה או ערכת השחזור לא נכונות.");
        this.cases = this.cases.filter((x) => x.id !== c.id);
        return null;
      }
      case "folders":
        return this.folderList;
      case "create_folder": {
        const name = String(a.name ?? "").trim();
        if (!name) fail("refused", "צריך לתת שם לתיקייה.");
        if (a.parentId) this.folderOf(a.parentId);
        return this.addFolder((a.parentId as string | null) ?? null, name);
      }
      case "rename_folder":
        this.folderOf(a.id).name = String(a.name ?? "").trim() || fail("refused", "צריך לתת שם לתיקייה.");
        return null;
      case "move_folder": {
        const f = this.folderOf(a.id);
        for (let p = (a.parentId as string | null) ?? null; p; p = this.folderOf(p).parent_id) {
          if (p === f.id) fail("refused", "אי אפשר להעביר תיקייה לתוך עצמה או לתוך תיקייה שבתוכה.");
        }
        f.parent_id = (a.parentId as string | null) ?? null;
        return null;
      }
      case "delete_folder": {
        const f = this.folderOf(a.id);
        for (const x of this.folderList) if (x.parent_id === f.id) x.parent_id = f.parent_id;
        for (const c of this.cases) if (c.folderId === f.id) c.folderId = f.parent_id;
        this.folderList = this.folderList.filter((x) => x.id !== f.id);
        return null;
      }
      case "move_case": {
        const c = this.find(a.caseId);
        if (a.folderId) this.folderOf(a.folderId);
        c.folderId = (a.folderId as string | null) ?? null;
        return null;
      }
      case "copy_secret":
        // The app does this natively (out of clipboard history, cleared after 60 s).
        await navigator.clipboard.writeText(String(a.text ?? "")).catch(() => undefined);
        return 60;
      case "find_name_matches":
        return this.nameMatches((a.caseId as string | null) ?? null, (a.names as string[]) ?? []);
      case "create_case":
        return this.newCase(a.meta as CaseMeta, a.identities as IdentityInput[]).id;
      case "create_follow_up": {
        const prev = this.find(a.caseId);
        const c = this.newCase({ code: `${prev.meta.code} · מעקב`, age: null, child_gender: prev.meta.child_gender, current_section: null, retention_until: null, consent: null, follows: prev.id },
          prev.people.map((p) => ({ id: null, role: p.role, value: p.value, aliases: p.aliases })));
        c.folderId = prev.folderId;
        return c.id;
      }
      case "follow_up": {
        const c = this.find(a.caseId);
        if (!c.meta.follows) return null;
        const prev = this.cases.find((x) => x.id === c.meta.follows && x.deletedAt === null);
        const inMaterials = c.inputs.some((i) => i.title === "השוואה לאבחון הקודם");
        if (!prev) return { previous_id: c.meta.follows, previous_code: "", previous_gone: true, rows: [], in_materials: inMaterials };
        return { previous_id: prev.id, previous_code: prev.meta.code, previous_gone: false, rows: compareSheets([...prev.sheets.values()], [...c.sheets.values()]), in_materials: inMaterials };
      }
      case "add_comparison_material": {
        const c = this.find(a.caseId);
        const prev = this.cases.find((x) => x.id === c.meta.follows);
        const rows = prev ? compareSheets([...prev.sheets.values()], [...c.sheets.values()]) : [];
        if (!rows.length) return fail("refused", "אין ציונים משותפים לשני האבחונים להשוואה.");
        const text = comparisonText(rows);
        const existing = c.inputs.find((i) => i.title === "השוואה לאבחון הקודם");
        return existing ? Object.assign(existing, { content: text }) : this.addInput(c, "test_scores", "השוואה לאבחון הקודם", text);
      }
      case "update_case": {
        const c = this.find(a.caseId);
        c.meta = a.meta as CaseMeta;
        c.updated = now();
        return null;
      }
      case "delete_case":
        this.find(a.caseId).deletedAt = now();
        return null;
      case "set_identities": {
        const c = this.find(a.caseId);
        this.setPeople(c, a.identities as IdentityInput[]);
        return this.detail(c).identities;
      }
      case "case_detail":
        return this.detail(this.find(a.caseId));
      case "add_input":
        return this.addInput(this.find(a.caseId), a.kind as InputKind, String(a.title), String(a.content));
      case "update_input": {
        const c = this.find(a.caseId);
        const i = c.inputs.find((x) => x.id === a.inputId) ?? fail("not_found", "החומר לא נמצא");
        i.title = String(a.title);
        if (i.content !== String(a.content)) {
          const r = this.routingOf(c, i.id);
          c.routing.set(i.id, { added: r.added, removed: r.removed });
        }
        i.content = String(a.content);
        return null;
      }
      case "delete_input": {
        const c = this.find(a.caseId);
        c.inputs = c.inputs.filter((i) => i.id !== a.inputId);
        return null;
      }
      case "score_instruments":
        return instruments;
      case "preview_scores":
        try {
          return formatSheet(a.sheet as ScoreSheet);
        } catch (e) {
          return fail("refused", (e as Error).message);
        }
      case "save_scores": {
        const c = this.find(a.caseId);
        const sheet = a.sheet as ScoreSheet;
        let text = "";
        try {
          text = formatSheet(sheet);
        } catch (e) {
          fail("refused", (e as Error).message);
        }
        const title = `ציוני ${instruments.find((i) => i.key === sheet.instrument)?.name ?? ""}`;
        const existing = c.inputs.find((i) => i.id === a.inputId);
        const input = existing ? Object.assign(existing, { title, content: text }) : this.addInput(c, "test_scores", title, text);
        c.sheets.set(input.id, sheet);
        return input;
      }
      case "paragraph_sources": {
        // The preview's "why": passages of the case's materials that share words with it.
        const c = this.find(a.caseId);
        const d = c.drafts.find((x) => x.id === String(a.draftId));
        if (!d?.byAi) return [];
        const words = (t: string) => new Set(t.split(/[^\p{L}\p{N}]+/u).filter((w) => w.length >= 3));
        const mine = words(restore(d.text, c.people, this.practitioner));
        return c.inputs
          .flatMap((i) => R.passages(i.content).map((text) => ({ label: `${kindLabel[i.kind]} · ${i.title}`, text })))
          .map((x) => ({ ...x, score: [...words(x.text)].filter((w) => mine.has(w)).length }))
          .filter((x) => x.score > 0)
          .sort((p, q) => q.score - p.score)
          .slice(0, 4)
          .map(({ label, text }) => ({ label, text }));
      }
      case "score_sheet":
        return this.find(a.caseId).sheets.get(String(a.inputId)) ?? null;
      case "import_document":
        return this.importDocument(args as Uint8Array, options?.headers ?? {});
      case "preview_filter":
        return this.filterFor(this.find(a.caseId), String(a.text));
      case "decide_suspect": {
        const c = this.find(a.caseId);
        const token = String(a.token);
        const d = a.decision as SuspectDecision;
        if (d.decision === "not_a_name") c.allowed.add(token);
        else this.setPeople(c, [...c.people.map((p) => ({ id: p.id, role: p.role, value: p.value, aliases: p.aliases })), { id: null, role: d.decision === "hide" ? d.role : "other", value: token, aliases: [] }]);
        return null;
      }
      case "restore_auto_hidden": {
        const c = this.find(a.caseId);
        const kept = c.people.find((p) => p.tag === String(a.tag) && p.source !== "manual");
        this.setPeople(c, c.people.filter((p) => p !== kept).map((p) => ({ id: p.id, role: p.role, value: p.value, aliases: p.aliases })));
        c.allowed.add(String(a.token));
        return null;
      }
      case "change_role": {
        const c = this.find(a.caseId);
        return this.setPeople(c, c.people.map((p) => ({ id: p.id, role: p.tag === String(a.tag) ? (a.role as Role) : p.role, value: p.value, aliases: p.aliases })));
      }
      case "prepare_section":
        return this.prepareSection(this.find(a.caseId), String(a.sectionKey), String(a.instruction ?? ""), a.replaces ? String(a.replaces) : null);
      case "prepare_full_draft": {
        const c = this.find(a.caseId);
        return SECTIONS.filter((s) => !DERIVED.includes(s.key) && c.inputs.some((i) => this.feedOf(c, i, s.key) !== null) && !c.drafts.some((d) => d.section === s.key && (d.status === "approved" || d.status === "proposed")))
          .map((s) => [s.key, this.prepareSection(c, s.key, "")]);
      }
      case "send_section": {
        const p = this.pending.get(String(a.approvalId));
        if (p?.type !== "section") return fail("refused", "האישור לא תקף. יש להכין את השליחה מחדש.");
        this.pending.delete(String(a.approvalId));
        await new Promise((r) => setTimeout(r, 2500));
        return this.sendSection(p);
      }
      case "prepare_sort":
        return this.prepareSort(this.find(a.caseId));
      case "send_sort": {
        const p = this.pending.get(String(a.approvalId));
        if (p?.type !== "sort") return fail("refused", "האישור לא תקף. יש להכין את השליחה מחדש.");
        this.pending.delete(String(a.approvalId));
        await new Promise((r) => setTimeout(r, 700));
        return this.sendSort(p);
      }
      case "set_input_sections": {
        const c = this.find(a.caseId);
        const i = c.inputs.find((x) => x.id === a.inputId) ?? fail("not_found", "החומר לא נמצא");
        const chosen = (a.sections as string[]).filter((k) => SORTABLE.some((s) => s.key === k));
        c.routing.set(i.id, R.choose(this.routingOf(c, i.id), chosen, this.tableFor(i.kind), R.passages(i.content).length));
        return null;
      }
      case "chat":
        return this.find(a.caseId).chat[String(a.sectionKey)] ?? [];
      case "approve_section": {
        const waiting = this.find(a.caseId).drafts.filter((d) => d.section === a.sectionKey && d.status === "proposed");
        waiting.forEach((d) => (d.status = "approved"));
        return waiting.length;
      }
      case "approve_paragraph":
      case "reject_paragraph": {
        const d = this.find(a.caseId).drafts.find((x) => x.id === a.draftId) ?? fail("not_found", "הפסקה לא נמצאה");
        d.status = cmd === "approve_paragraph" ? "approved" : "rejected";
        if (cmd === "approve_paragraph" && d.replaces) {
          const old = this.find(a.caseId).drafts.find((x) => x.id === d.replaces);
          if (old) old.status = "superseded";
        }
        return null;
      }
      case "edit_paragraph": {
        const c = this.find(a.caseId);
        const d = c.drafts.find((x) => x.id === a.draftId) ?? fail("not_found", "הפסקה לא נמצאה");
        if (!String(a.text).trim()) {
          c.drafts = c.drafts.filter((x) => x.id !== d.id);
          return null;
        }
        const tagged = this.filterFor(c, String(a.text)).tagged;
        keepVersion(d, tagged);
        d.text = tagged;
        d.byAi = false;
        d.status = "approved";
        return null;
      }
      case "paragraph_versions": {
        const c = this.find(a.caseId);
        const d = c.drafts.find((x) => x.id === a.draftId) ?? fail("not_found", "הפסקה לא נמצאה");
        return versionsOf(c.drafts, d).map((v) => ({ id: v.id, saved_at: v.at, by_ai: v.byAi, text: restore(v.text, c.people, this.practitioner) }));
      }
      case "restore_paragraph_version": {
        const c = this.find(a.caseId);
        const d = c.drafts.find((x) => x.id === a.draftId) ?? fail("not_found", "הפסקה לא נמצאה");
        const v = versionsOf(c.drafts, d).find((x) => x.id === a.versionId) ?? fail("not_found", "הגרסה לא נמצאה");
        keepVersion(d, v.text);
        d.text = v.text;
        d.byAi = v.byAi;
        d.status = "approved";
        for (const x of c.drafts) if (x.replaces === d.id && x.status === "proposed") x.status = "superseded";
        return null;
      }
      case "add_own_paragraph": {
        const c = this.find(a.caseId);
        const d = { id: newId("d"), section: String(a.sectionKey), text: this.filterFor(c, String(a.text)).tagged, status: "approved" as const, byAi: false, sources: [] };
        const after = a.after ? c.drafts.findIndex((x) => x.id === a.after) : -1;
        if (after >= 0) c.drafts.splice(after + 1, 0, d);
        else if (a.first) c.drafts.splice(Math.max(0, c.drafts.findIndex((x) => x.section === d.section)), 0, d);
        else c.drafts.push(d);
        return null;
      }
      case "prepare_consult": {
        const c = a.caseId ? this.find(a.caseId) : null;
        const text = String(a.message);
        const f = c ? this.filterFor(c, text) : filter(text, { caseId: "", people: this.allPeople(), practitioner: this.practitioner, allowed: new Set() });
        const approval = newId("approval");
        this.pending.set(approval, { type: "consult", question: f.tagged, shown: text, hidden: f.hidden, caseId: c?.id ?? null, conversationId: a.conversationId ? String(a.conversationId) : null });
        return this.withKept(c, { approval_id: approval, parts: [{ label: "השאלה", original: f.original_segments, outgoing: f.tagged_segments }], suspects: [], auto_hidden: f.auto_hidden, hidden: f.hidden, checks: f.checks, blocked: [], demo_mode: true });
      }
      case "send_consult": {
        const p = this.pending.get(String(a.approvalId));
        if (p?.type !== "consult") return fail("refused", "האישור לא תקף. יש להכין את השליחה מחדש.");
        this.pending.delete(String(a.approvalId));
        await new Promise((r) => setTimeout(r, 1400));
        const answer = "מצב הדגמה: כאן תופיע תשובה מקצועית של Claude, שמבחינה בין ידע מבוסס לדעה ומציינת אי-ודאות. ההחלטה המקצועית נשארת שלך.";
        const at = now();
        let conv = p.conversationId ? this.convs.find((x) => x.id === p.conversationId) : undefined;
        if (!conv) {
          conv = { id: newId("conv"), caseId: p.caseId, updated: at, turns: [] };
          this.convs.unshift(conv);
        }
        conv.turns.push({ role: "user", text: p.shown, hidden: p.hidden, demo: true, at }, { role: "assistant", text: answer, hidden: [], demo: true, at });
        conv.updated = at;
        return { answer, demo: true, conversation_id: conv.id };
      }
      case "consultations":
        return [...this.convs].sort((x, y) => y.updated - x.updated).map((c) => {
          const k = c.caseId ? this.cases.find((x) => x.id === c.caseId) : undefined;
          const first = c.turns.find((t) => t.role === "user")?.text ?? "שיחה";
          return {
            id: c.id, case_id: c.caseId, title: first.length > 60 ? `${first.slice(0, 60)}…` : first, updated_at: c.updated, turns: c.turns.length,
            case_label: k ? `${k.people.find((x) => x.role === "child")?.value ?? ""} · ${k.meta.code}` : null,
          };
        });
      case "consultation": {
        const c = this.convs.find((x) => x.id === String(a.id));
        if (!c) return fail("not_found", "לא נמצא: השיחה");
        return { id: c.id, case_id: c.caseId, turns: c.turns };
      }
      case "delete_consultation":
        this.convs = this.convs.filter((x) => x.id !== String(a.id));
        return null;
      case "check_export": {
        const c = this.find(a.caseId);
        const d = this.detail(c);
        const missing = d.sections.flatMap((s) => s.paragraphs.filter((p) => p.status === "approved").flatMap((p) => (p.text.match(/\[חסר[^\]]*\]/g) ?? []).map((m) => `${s.title}: ${m}`)));
        return {
          blocking: [], to_complete: missing, empty_sections: d.sections.filter((s) => !s.approved).map((s) => s.title),
          included_sections: d.sections.filter((s) => s.approved).length, score_tables: c.sheets.size, file_name: `דוח אבחון – ${c.meta.code}.docx`,
        } satisfies ExportCheck;
      }
      case "export_report":
        return "בהדמיה בדפדפן לא נוצר קובץ. בתוכנה המותקנת הדוח נשמר בתיקיית ההורדות, מוצפן בסיסמה.";
      case "set_auto_backup":
        this.autoBackup = Boolean(a.on);
        this.logActivity("settings_changed", "security", "הגיבוי האוטומטי הודלק או כובה");
        return this.handle("backup_status", {});
      case "backup_status": {
        const days = this.lastBackupAt === null ? null : Math.floor((now() - this.lastBackupAt) / 86_400);
        return {
          last_at: this.lastBackupAt, days_since: days, due: this.secretChanged || days === null || days >= 7,
          secret_changed: this.secretChanged, last_check_at: this.lastCheckAt, has_cases: this.cases.length > 0,
          auto: this.autoBackup,
        } satisfies BackupStatus;
      }
      case "write_backup": {
        this.lastBackupAt = now();
        this.secretChanged = false;
        this.logActivity("backup_written", "security", "גיבוי מוצפן נשמר");
        const day = new Date().toISOString().slice(0, 10);
        return { path: `E:\\גיבויים\\גיבוי כספת האבחון ${day}.vaultbak (בהדמיה לא נשמר קובץ)`, bytes: 1_843_200, created_at: this.lastBackupAt };
      }
      case "choose_backup": {
        const at = this.lastBackupAt ?? now() - 86_400;
        const day = new Date(at * 1000).toISOString().slice(0, 10);
        return { file_name: `גיבוי כספת האבחון ${day}.vaultbak`, created_at: at, same_vault: this.unlocked ? true : null } satisfies StagedBackup;
      }
      case "check_backup":
        if (!String(a.password ?? "")) fail("wrong_secret", "הסיסמה או ערכת השחזור לא נכונות.");
        this.lastCheckAt = now();
        this.logActivity("backup_checked", "security", `תרגול שחזור: הגיבוי נפתח ותקין (${this.cases.length === 1 ? "תיק אחד" : `${this.cases.length} תיקים`})`);
        return { created_at: this.lastBackupAt ?? now(), cases: this.cases.length, integrity_ok: true };
      case "restore_backup":
        if (!String(a.password ?? a.recoveryKey ?? "")) fail("wrong_secret", "הסיסמה או ערכת השחזור לא נכונות.");
        this.vaultExists = true;
        this.unlocked = true;
        return this.status();
      case "forget_backup":
        return null;
      case "send_progress":
        return 0;
      case "check_update":
        return {
          version: "0.2.0", current: "0.1.0", published: "2026-10-01", size_mb: 14, can_install: true,
          notes: ["כפתור ה-+ בחומרים מעלה גם קבצי Word ו-PDF", "זיהוי שמות חזק יותר לפני שליחה", "הדוח נכתב מול העיניים"],
        };
      case "prepare_update":
        return true;
      case "install_update":
        return fail("update", "בתצוגה המקדימה לא מתקינים. בתוכנה עצמה העדכון יורד, נבדק מול החתימה ומותקן.");
      case "change_password":
        if (!String(a.current ?? "")) fail("wrong_secret", "הסיסמה או ערכת השחזור לא נכונות.");
        if (String(a.newPassword ?? "").length < 12) fail("weak_password", "הסיסמה קצרה או נפוצה מדי. מומלץ משפט של כמה מילים (12 תווים לפחות).");
        this.secretChanged = true;
        this.logActivity("password_changed", "security", "הסיסמה הוחלפה");
        return null;
      case "new_recovery_kit":
        if (!String(a.current ?? "")) fail("wrong_secret", "הסיסמה או ערכת השחזור לא נכונות.");
        this.secretChanged = true;
        this.logActivity("recovery_key_rotated", "security", "נוצרה ערכת שחזור חדשה");
        return { recovery_key: "DEMO-NEWK-ITXX-ONLY-PREV-IEW7" };
      case "activity": {
        const before = typeof a.before === "number" ? a.before : Infinity;
        const page = [...this.activityLog].reverse().filter((e) => e.seq < before).slice(0, 50);
        return { entries: page, more: false, last_seq: page.at(-1)?.seq ?? null, intact: true, reviewed_at: this.reviewedAt };
      }
      case "mark_activity_reviewed":
        this.logActivity("audit_reviewed", "security", "היומן נבדק");
        this.reviewedAt = now();
        return null;
      case "retention_due":
        return [];
      case "keep_case_longer": {
        const c = this.find(a.caseId);
        c.meta = { ...c.meta, retention_until: `${new Date().getFullYear() + Number(a.years ?? 1)}-${new Date().toISOString().slice(5, 10)}` };
        return null;
      }
      default:
        return fail("preview", `הפעולה ${cmd} לא זמינה בהדמיה בדפדפן.`);
    }
  }
}
