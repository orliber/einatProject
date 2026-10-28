// D-134: the materials are read one request each, side by side. One request for all of them
// was slow, and past about nine materials its answer ran out of room and was lost whole.

import { ipc, type SortResult } from "../../ipc/client";
import { timed } from "../../components/Progress";
import type { CaseApi } from "../CaseScreen";

/** Requests on their way at once: under the per-minute limit of the sending side (20). */
export const SORT_PARALLEL = 4;

const empty = (): SortResult => ({ sorted: 0, unchanged: 0, links: 0, ignored: 0, demo: false, left_out: false });

/**
 * Every material not sorted yet, through `review` one after the other (each part goes once she
 * sends it, or at once when the gate cleared it), and sent `SORT_PARALLEL` at a time; the case
 * is read again as each one lands, so the materials fill in one by one. Resolves with whether
 * any part was sent and their results summed. A part that failed is in the strip with its own
 * "לנסות שוב"; only when every part failed is the first failure thrown.
 * `quiet`: the reading after a material was added; a part that was refused stays unsaid.
 */
export async function sortInParts(
  api: Pick<CaseApi, "caseId" | "review" | "track" | "reload">,
  opts: { title: string; label: string; quiet?: boolean; prepared?: () => void },
): Promise<{ sent: boolean; result: SortResult }> {
  const all = await timed("prepare", ipc.prepareSortParts(api.caseId), "sort");
  opts.prepared?.();
  const parts = all.filter(([, p]) => !opts.quiet || p.approval_id);
  const result = empty();
  const add = (r: SortResult) => {
    result.sorted += r.sorted;
    result.unchanged += r.unchanged;
    result.links += r.links;
    result.ignored += r.ignored;
    result.demo ||= r.demo;
    result.left_out ||= r.left_out;
  };
  const going: Promise<void>[] = [];
  const failures: unknown[] = [];
  let free = SORT_PARALLEL;
  const waiting: (() => void)[] = [];
  const slot = () => (free > 0 ? (free--, Promise.resolve()) : new Promise<void>((r) => waiting.push(r)));
  const release = () => {
    const next = waiting.shift();
    if (next) next();
    else free++;
  };
  let sent = false;
  let landed = 0;
  for (const [n, [inputId, prepared]] of parts.entries()) {
    const label = parts.length > 1 ? `${opts.label} (${n + 1} מתוך ${parts.length})` : opts.label;
    // The review returns as soon as the part is on its way, so the next one is not held up.
    const went = await api
      .review({
        title: opts.title,
        prepared,
        reprepare: () => ipc.prepareSortOne(api.caseId, inputId),
        onSend: async (id) => {
          await slot();
          going.push(
            api
              .track({ label, task: "sort", approval: id }, () => ipc.sendSort(id), async (r) => {
                add(r);
                landed++;
                await api.reload();
              })
              .then(
                () => undefined,
                (e: unknown) => void failures.push(e),
              )
              .finally(release),
          );
        },
      })
      .catch((e: unknown) => (failures.push(e), false));
    sent ||= went;
    // A cleared part not sent: she went back from its review, and so from the rest too.
    if (!went && prepared.approval_id) break;
  }
  await Promise.all(going);
  if (failures.length > 0 && landed === 0 && !opts.quiet) throw failures[0];
  return { sent, result };
}
