import { mockIPC } from "@tauri-apps/api/mocks";
import { getKept } from "../../kept";
import type { CaseApi } from "../CaseScreen";
import { prepared } from "../../test/fixtures";
import { resetRuns, runLine, runOf, runWholeReport, showingCase, stopRun } from "./reportRun";

/** A case screen whose review sends at once (the default: no window before sending). */
function caseApi(over: { unsorted?: boolean } = {}) {
  const sent: string[] = [];
  let reloads = 0;
  const api = {
    caseId: "c1",
    detail: {
      sections: [{ key: "a", title: "סעיף א" }, { key: "b", title: "סעיף ב" }, { key: "c", title: "סעיף ג" }, { key: "d", title: "סעיף ד" }],
      routing: over.unsorted ? [{ needs_sorting: true }] : [],
    },
    jobs: [],
    reload: async () => {
      reloads += 1;
    },
    review: async (r: { prepared: { approval_id: string | null }; onSend: (id: string) => Promise<void> }) => {
      if (!r.prepared.approval_id) return false;
      await r.onSend(r.prepared.approval_id);
      return true;
    },
    track: async <T,>(_job: unknown, run: () => Promise<T>, then?: (out: T) => Promise<void> | void) => {
      const out = await run();
      await then?.(out);
      return out;
    },
  } as unknown as CaseApi;
  return { api, sent, reloads: () => reloads };
}

const four = ["a", "b", "c", "d"].map((k) => [k, prepared({ approval_id: `ap-${k}` })]);

describe("writing the whole report in the background", () => {
  beforeEach(() => resetRuns());

  it("sorts first, writes every section at most three at a time, then checks each against its sources", async () => {
    const calls: string[] = [];
    let writing = 0;
    let most = 0;
    mockIPC(async (cmd, args) => {
      calls.push(cmd);
      const a = args as Record<string, string>;
      if (cmd === "prepare_sort_parts") return [["i1", prepared({ approval_id: "sort" })]];
      if (cmd === "send_sort") return { assigned: 1 };
      if (cmd === "prepare_full_draft") return four;
      if (cmd === "send_section") {
        writing += 1;
        most = Math.max(most, writing);
        await new Promise((r) => setTimeout(r, 5));
        writing -= 1;
        return {};
      }
      if (cmd === "prepare_verify") return prepared({ approval_id: `v-${a.sectionKey}` });
      if (cmd === "send_verify") return { draft_ids: ["p1"], checks: [{ paragraph: "P1", verdict: "supported", note: "" }], demo: false };
      return null;
    });
    const { api } = caseApi({ unsorted: true });
    await runWholeReport(api, { verify: true, ai: "Claude" });
    expect(calls.indexOf("send_sort")).toBeLessThan(calls.indexOf("prepare_full_draft"));
    expect(calls.filter((c) => c === "send_section")).toHaveLength(4);
    expect(most).toBe(3);
    expect(calls.filter((c) => c === "send_verify")).toHaveLength(4);
    // The section shows the check's note, as after its own check.
    expect(getKept<Map<string, unknown>>("case:c1:section:a:checks", new Map()).get("p1")).toMatchObject({ verdict: "supported" });
    const r = runOf("c1") ?? ({} as never);
    expect(r.phase).toBe("done");
    expect(runLine(r)).toBe("הדוח נכתב ונבדק מול המקורות. 4 סעיפים מחכים לאישורך");
  });

  it("stops: what is being written finishes, nothing new is sent, and nothing is checked", async () => {
    const calls: string[] = [];
    mockIPC(async (cmd) => {
      calls.push(cmd);
      if (cmd === "prepare_full_draft") return four;
      if (cmd === "send_section") {
        // Pressed while the first three are being written.
        await new Promise((r) => setTimeout(r, 5));
        stopRun("c1");
        return {};
      }
      return null;
    });
    await runWholeReport(caseApi().api, { verify: true, ai: "Claude" });
    expect(calls.filter((c) => c === "send_section")).toHaveLength(3);
    expect(calls).not.toContain("prepare_verify");
    expect(runOf("c1")?.phase).toBe("stopped");
  });

  it("a lock in the middle ends the run quietly, and the next one writes what is still missing", async () => {
    let locked = true;
    const sent: string[] = [];
    mockIPC(async (cmd, args) => {
      if (cmd === "prepare_full_draft") return four.filter(([k]) => !sent.includes(`ap-${k as string}`));
      if (cmd === "send_section") {
        const id = (args as { approvalId: string }).approvalId;
        if (locked && id === "ap-b") {
          locked = false;
          throw { code: "locked", message: "הכספת ננעלה באמצע.", details: [], action: { kind: "unlock" } };
        }
        sent.push(id);
        return {};
      }
      return null;
    });
    await runWholeReport(caseApi().api, { verify: false, ai: "Claude" });
    // Nothing is said about it: the page forgets what it showed when the vault locks.
    expect(runOf("c1")).toBeUndefined();
    await runWholeReport(caseApi().api, { verify: false, ai: "Claude" });
    expect(runOf("c1")?.phase).toBe("done");
    expect([...sent].sort()).toEqual(["ap-a", "ap-b", "ap-c", "ap-d"]);
  });

  it("a run cut by the lock never writes into the next one", async () => {
    let release: () => void = () => undefined;
    mockIPC(async (cmd) => {
      if (cmd === "prepare_full_draft") return [["a", prepared({ approval_id: "ap-a" })]];
      if (cmd === "send_section") await new Promise<void>((r) => (release = r));
      return {};
    });
    const first = runWholeReport(caseApi().api, { verify: false, ai: "Claude" });
    await vi.waitFor(() => expect(runOf("c1")?.phase).toBe("write"));
    resetRuns();
    release();
    await first;
    expect(runOf("c1")).toBeUndefined();
  });

  it("what stops it before any writing is said, with its reason", async () => {
    mockIPC(async (cmd) => {
      if (cmd === "prepare_full_draft") throw { code: "consent", message: "לפני שליחה צריך לרשום את הסכמת ההורים.", details: [] };
      return null;
    });
    await runWholeReport(caseApi().api, { verify: false, ai: "Claude" });
    const r = runOf("c1") ?? ({} as never);
    expect(runLine(r)).toBe("כתיבת הדוח לא התחילה: לפני שליחה צריך לרשום את הסכמת ההורים.");
  });

  it("a check that did not come back is never counted as checked", async () => {
    mockIPC(async (cmd, args) => {
      const a = args as Record<string, string>;
      if (cmd === "prepare_full_draft") return four.slice(0, 2);
      if (cmd === "prepare_verify") return prepared({ approval_id: a.sectionKey === "a" ? "v-a" : null });
      if (cmd === "send_verify") return { draft_ids: [], checks: [], demo: false };
      return {};
    });
    await runWholeReport(caseApi().api, { verify: true, ai: "Claude" });
    const r = runOf("c1") ?? ({} as never);
    expect([r.checked, r.uncheckedFailed]).toEqual([1, 1]);
    expect(runLine(r)).toBe("הדוח נכתב. 2 סעיפים מחכים לאישורך. סעיף אחד לא נבדק מול המקורות, ואפשר לבדוק בסעיף עצמו");
  });

  it("a section she wrote meanwhile is not written again", async () => {
    const calls: string[] = [];
    mockIPC(async (cmd, args) => {
      if (cmd === "prepare_full_draft") return four.slice(0, 2);
      if (cmd === "send_section") calls.push((args as { approvalId: string }).approvalId);
      return {};
    });
    const { api } = caseApi();
    // The screen showing the case now: "b" has her text.
    const now = { ...api, detail: { ...api.detail, sections: api.detail.sections.map((s) => (s.key === "b" ? { ...s, paragraphs: [{}] } : s)) } } as unknown as CaseApi;
    showingCase("c1", now);
    await runWholeReport(api, { verify: false, ai: "Claude" });
    showingCase("c1", null);
    expect(calls).toEqual(["ap-a"]);
    expect(runLine(runOf("c1") ?? ({} as never))).toBe("הדוח נכתב. סעיף אחד מחכה לאישורך");
  });

  it("a section the gate still refuses is counted, and the line says where to retry", async () => {
    mockIPC(async (cmd) => {
      if (cmd === "prepare_full_draft") return [["a", prepared({ approval_id: "ap-a" })], ["b", prepared({ approval_id: null })]];
      return {};
    });
    await runWholeReport(caseApi().api, { verify: false, ai: "Claude" });
    const r = runOf("c1") ?? ({} as never);
    expect([r.done, r.failed]).toEqual([1, 1]);
    expect(runLine(r)).toContain("לנסות שוב");
  });
});
