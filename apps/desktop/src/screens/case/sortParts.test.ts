import { mockIPC } from "@tauri-apps/api/mocks";
import type { CaseApi } from "../CaseScreen";
import { prepared } from "../../test/fixtures";
import { SORT_PARALLEL, sortInParts } from "./sortParts";

/** A case screen whose review sends at once, like the default (no window before sending). */
function caseApi(back: string[] = []) {
  let reloads = 0;
  const api = {
    caseId: "c1",
    reload: async () => {
      reloads += 1;
    },
    review: async (r: { prepared: { approval_id: string | null }; onSend: (id: string) => Promise<void> }) => {
      const id = r.prepared.approval_id;
      if (!id || back.includes(id)) return false;
      await r.onSend(id);
      return true;
    },
    track: async <T,>(_job: unknown, run: () => Promise<T>, then?: (out: T) => Promise<void> | void) => {
      const out = await run();
      await then?.(out);
      return out;
    },
  } as unknown as Pick<CaseApi, "caseId" | "review" | "track" | "reload">;
  return { api, reloads: () => reloads };
}

const nine = Array.from({ length: 9 }, (_, n) => [`i${n}`, prepared({ approval_id: `s${n}` })]);
const one = { sorted: 1, unchanged: 0, links: 2, ignored: 0, demo: false, left_out: false };

describe("reading the materials one request each (D-134)", () => {
  it("sends every material, at most four at a time, sums the results and reads the case as each lands", async () => {
    let going = 0;
    let most = 0;
    const sent: string[] = [];
    mockIPC(async (cmd, args) => {
      if (cmd === "prepare_sort_parts") return nine;
      if (cmd === "send_sort") {
        sent.push((args as { approvalId: string }).approvalId);
        going += 1;
        most = Math.max(most, going);
        await new Promise((r) => setTimeout(r, 5));
        going -= 1;
        return one;
      }
      return null;
    });
    const { api, reloads } = caseApi();
    const { sent: any, result } = await sortInParts(api, { title: "מיון", label: "קורא" });
    expect(any).toBe(true);
    expect(sent).toHaveLength(9);
    expect(most).toBe(SORT_PARALLEL);
    expect(result).toMatchObject({ sorted: 9, links: 18 });
    expect(reloads()).toBe(9);
  });

  it("one part that fails leaves the rest in; only when all fail is it thrown", async () => {
    mockIPC(async (cmd, args) => {
      if (cmd === "prepare_sort_parts") return nine.slice(0, 3);
      if (cmd === "send_sort") {
        if ((args as { approvalId: string }).approvalId === "s1") throw { code: "egress", message: "נקטע", details: [] };
        return one;
      }
      return null;
    });
    const { result } = await sortInParts(caseApi().api, { title: "מיון", label: "קורא" });
    expect(result.sorted).toBe(2);

    mockIPC(async (cmd) => {
      if (cmd === "prepare_sort_parts") return nine.slice(0, 2);
      if (cmd === "send_sort") throw { code: "egress", message: "נקטע", details: [] };
      return null;
    });
    await expect(sortInParts(caseApi().api, { title: "מיון", label: "קורא" })).rejects.toMatchObject({ code: "egress" });
  });

  it("going back from one review stops the parts after it", async () => {
    const sent: string[] = [];
    mockIPC(async (cmd, args) => {
      if (cmd === "prepare_sort_parts") return nine.slice(0, 3);
      if (cmd === "send_sort") sent.push((args as { approvalId: string }).approvalId);
      return one;
    });
    await sortInParts(caseApi(["s1"]).api, { title: "מיון", label: "קורא" });
    expect(sent).toEqual(["s0"]);
  });
});
