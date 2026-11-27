import { mockIPC } from "@tauri-apps/api/mocks";
import { getKept } from "../../kept";
import type { CaseApi } from "../CaseScreen";
import { prepared } from "../../test/fixtures";
import { PARALLEL, resetRuns, runInstruction, runLine, runOf, runWholeReport, showingCase, stopRun } from "./reportRun";

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

  it("checks each section as soon as it is in, beside the writing that is left, three checks at a time (D-146, D-161)", async () => {
    const events: string[] = [];
    let writing = 0;
    let most = 0;
    let checking = 0;
    let mostChecks = 0;
    mockIPC(async (cmd, args) => {
      const a = args as Record<string, string>;
      if (cmd === "prepare_full_draft") return four;
      if (cmd === "send_section") {
        writing += 1;
        most = Math.max(most, writing);
        // The last section takes long: the first ones are checked meanwhile.
        await new Promise((r) => setTimeout(r, a.approvalId === "ap-d" ? 60 : 5));
        writing -= 1;
        events.push(`written:${a.approvalId}`);
        return {};
      }
      if (cmd === "prepare_verify") return prepared({ approval_id: `v-${a.sectionKey}` });
      if (cmd === "send_verify") {
        checking += 1;
        mostChecks = Math.max(mostChecks, checking);
        events.push(`check:${a.approvalId}`);
        // Longer than the writing: the first three are out together, never more.
        await new Promise((r) => setTimeout(r, 30));
        checking -= 1;
        return { draft_ids: [], checks: [], demo: false };
      }
      return null;
    });
    await runWholeReport(caseApi().api, { verify: true, ai: "Claude" });
    expect(events.indexOf("check:v-a")).toBeGreaterThan(events.indexOf("written:ap-a"));
    expect(events.indexOf("check:v-a")).toBeLessThan(events.indexOf("written:ap-d"));
    expect(events.filter((e) => e.startsWith("check:"))).toHaveLength(4);
    expect(events.at(-1)).toBe("check:v-d");
    expect([most, mostChecks]).toEqual([3, 3]);
    const r = runOf("c1") ?? ({} as never);
    expect([r.checked, r.toCheck, r.phase]).toEqual([4, 4, "done"]);
  });

  it("never has more checks out than sections being written, however many wait", async () => {
    const six = ["a", "b", "c", "d", "e", "f"].map((k) => [k, prepared({ approval_id: `ap-${k}` })]);
    let checking = 0;
    let mostChecks = 0;
    mockIPC(async (cmd, args) => {
      const a = args as Record<string, string>;
      if (cmd === "prepare_full_draft") return six;
      if (cmd === "send_section") return {};
      if (cmd === "prepare_verify") return prepared({ approval_id: `v-${a.sectionKey}` });
      if (cmd === "send_verify") {
        checking += 1;
        mostChecks = Math.max(mostChecks, checking);
        await new Promise((r) => setTimeout(r, 20));
        checking -= 1;
        return { draft_ids: [], checks: [], demo: false };
      }
      return null;
    });
    await runWholeReport(caseApi().api, { verify: true, ai: "Claude" });
    expect(mostChecks).toBe(PARALLEL);
    const r = runOf("c1") ?? ({} as never);
    expect([r.checked, r.toCheck, r.phase]).toEqual([6, 6, "done"]);
  });

  it("a request built again goes with the wish she kept for the section (D-163)", async () => {
    const asked: string[] = [];
    mockIPC(async (cmd, args) => {
      const a = args as Record<string, string>;
      if (cmd === "draft_briefs") return { recent: [], sections: { a: "להתחיל מהגן" } };
      if (cmd === "prepare_full_draft") return four.slice(0, 2);
      if (cmd === "prepare_section") {
        asked.push(`${a.sectionKey}: ${a.instruction}`);
        return prepared({ approval_id: `again-${a.sectionKey}` });
      }
      return {};
    });
    const { api } = caseApi();
    // A review that builds every request again before sending it.
    api.review = async (r) => {
      const again = await r.reprepare();
      if (again.approval_id) await r.onSend(again.approval_id);
      return true;
    };
    await runWholeReport(api, { verify: false, ai: "Claude" });
    expect(asked.sort()).toEqual([
      `a: ${runInstruction("להתחיל מהגן")}`,
      `b: ${runInstruction(undefined)}`,
    ]);
    expect(runInstruction("להתחיל מהגן")).toMatch(/עם מקור לכל פסקה\. מה שחשוב לי שייכתב בסעיף: להתחיל מהגן$/);
    expect(runInstruction("  ")).toBe(runInstruction(undefined));
  });

  it("“לעצור” while a check is out: that check finishes, and no other section is checked", async () => {
    const calls: string[] = [];
    mockIPC(async (cmd, args) => {
      const a = args as Record<string, string>;
      calls.push(cmd);
      if (cmd === "prepare_full_draft") return four.slice(0, 1).concat(four.slice(1, 2));
      if (cmd === "send_section") {
        await new Promise((r) => setTimeout(r, a.approvalId === "ap-b" ? 40 : 5));
        return {};
      }
      if (cmd === "prepare_verify") return prepared({ approval_id: `v-${a.sectionKey}` });
      if (cmd === "send_verify") {
        stopRun("c1");
        await new Promise((r) => setTimeout(r, 5));
        return { draft_ids: [], checks: [], demo: false };
      }
      return null;
    });
    await runWholeReport(caseApi().api, { verify: true, ai: "Claude" });
    expect(calls.filter((c) => c === "send_section")).toHaveLength(2);
    expect(calls.filter((c) => c === "send_verify")).toHaveLength(1);
    expect(runOf("c1")?.phase).toBe("stopped");
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

  it("after the sorting, the line says the materials were sorted and names a section left with no material", async () => {
    let lineWhileWriting = "";
    mockIPC(async (cmd) => {
      if (cmd === "prepare_sort_parts") return [["i1", prepared({ approval_id: "sort" })]];
      if (cmd === "send_sort") return { assigned: 1 };
      // "ג" had material before the sorting and has none after it.
      if (cmd === "prepare_full_draft") return four.filter(([k]) => k !== "c");
      if (cmd === "send_section") {
        lineWhileWriting = runLine(runOf("c1") ?? ({} as never));
        return {};
      }
      return null;
    });
    const { api } = caseApi({ unsorted: true });
    (api.detail as { sections: unknown[] }).sections = ["a", "b", "c", "d"].map((k) => ({ key: k, title: `סעיף ${({ a: "א", b: "ב", c: "ג", d: "ד" } as Record<string, string>)[k]}`, paragraphs: [], source_count: 1, sortable: true }));
    // The summary is written later, from the approved sections: not named as lost.
    (api.detail as { sections: unknown[] }).sections.push({ key: "summary", title: "סיכום", paragraphs: [], source_count: 2, sortable: false });
    await runWholeReport(api, { verify: false, ai: "Claude" });
    expect(lineWhileWriting).toMatch(/^החומרים מוינו\. התוכנה עובדת: נכתבו \d מתוך 3 סעיפים\. אחרי המיון לא נמצא חומר לסעיף "סעיף ג", ולכן הוא לא נכתב$/);
    expect(runLine(runOf("c1") ?? ({} as never))).toBe("הדוח נכתב. 3 סעיפים מחכים לאישורך. אחרי המיון לא נמצא חומר לסעיף \"סעיף ג\", ולכן הוא לא נכתב");
  });
});
