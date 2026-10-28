// D-134: the materials are read one request each, side by side. One request for all of them
// was slow, and past about nine materials its answer ran out of room and was lost whole.

import { timed } from "../../components/Progress";
import { ipc, type Prepared, type SortResult } from "../../ipc/client";
import type { CaseApi } from "../CaseScreen";

/** Requests on their way at once: under the per-minute limit of the sending side (20). */
export const SORT_PARALLEL = 4;

const empty = (): SortResult => ({ sorted: 0, unchanged: 0, links: 0, ignored: 0, demo: false, left_out: false });

/** The parts as one review: everything that leaves, one "send". It can be sent only when every
 *  part cleared the gate; its approval is the first part's (the send takes them all). */
export function mergeParts(parts: [string, Prepared][]): Prepared {
  const all = parts.map(([, p]) => p);
  const auto: Prepared["auto_hidden"] = [];
  for (const a of all.flatMap((p) => p.auto_hidden)) if (!auto.some((x) => x.tag === a.tag && x.token === a.token)) auto.push(a);
  const sum = (k: keyof Prepared["checks"]) => all.reduce((n, p) => n + p.checks[k], 0);
  return {
    approval_id: all.length > 0 && all.every((p) => p.approval_id) ? (all[0]?.approval_id ?? null) : null,
    // Each request numbers its own material S1: here they are told apart by their place.
    parts: all.flatMap((p, n) => p.parts.map((part) => (all.length > 1 ? { ...part, label: part.label.replace(/^S\d+(?= · )/, `חומר ${n + 1}`) } : part))),
    suspects: all.flatMap((p) => p.suspects),
    auto_hidden: auto,
    hidden: Array.from(new Set(all.flatMap((p) => p.hidden))),
    checks: { declared_names: sum("declared_names"), patterns: sum("patterns"), name_suspects: sum("name_suspects"), indirect_suspects: sum("indirect_suspects") },
    blocked: all.flatMap((p) => p.blocked),
    demo_mode: all.some((p) => p.demo_mode),
  };
}

/**
 * Every material not sorted yet, one request each, through one review (or at once when the gate
 * cleared them all), and sent `SORT_PARALLEL` at a time; the case is read again as each one
 * lands, so the materials fill in one by one. Resolves with whether they were sent and their
 * results summed. A part that failed is in the strip with its own "לנסות שוב"; only when every
 * part failed is the first failure thrown.
 * `quiet`: the reading after a material was added; nothing is said when it cannot go.
 */
export async function sortInParts(
  api: Pick<CaseApi, "caseId" | "review" | "track" | "reload">,
  opts: { title: string; label: string; quiet?: boolean; prepared?: () => void },
): Promise<{ sent: boolean; result: SortResult }> {
  let parts = await timed("prepare", ipc.prepareSortParts(api.caseId), "sort");
  opts.prepared?.();
  const prepared = mergeParts(parts);
  if (opts.quiet && !prepared.approval_id) return { sent: false, result: empty() };
  const result = empty();
  const add = (r: SortResult) => {
    result.sorted += r.sorted;
    result.unchanged += r.unchanged;
    result.links += r.links;
    result.ignored += r.ignored;
    result.demo ||= r.demo;
    result.left_out ||= r.left_out;
  };
  const sendAll = async () => {
    const queue = parts.flatMap(([, p], n) => (p.approval_id ? [[n, p.approval_id] as const] : []));
    const failures: unknown[] = [];
    let landed = 0;
    const worker = async () => {
      for (let next = queue.shift(); next; next = queue.shift()) {
        const [n, id] = next;
        const label = parts.length > 1 ? `${opts.label} (${n + 1} מתוך ${parts.length})` : opts.label;
        await api
          .track({ label, task: "sort", approval: id }, () => ipc.sendSort(id), async (r) => {
            add(r);
            landed++;
            await api.reload();
          })
          .catch((e: unknown) => void failures.push(e));
      }
    };
    await Promise.all(Array.from({ length: Math.min(SORT_PARALLEL, queue.length) }, worker));
    if (failures.length > 0 && landed === 0 && !opts.quiet) throw failures[0];
  };
  const sent = await api.review({
    title: opts.title,
    prepared,
    // Built again (after "להחזיר", or "לנסות שוב" on a refusal): the parts are those of now.
    reprepare: async () => {
      parts = await ipc.prepareSortParts(api.caseId);
      return mergeParts(parts);
    },
    onSend: sendAll,
  });
  return { sent, result };
}
