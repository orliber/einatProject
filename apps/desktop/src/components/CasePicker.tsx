// Picking a case by its place in the library (D-023): one group per folder, in tree order,
// each headed by its full path ("אבחונים פרטיים › 2026"). Cases outside any folder come first.
import { useQuery } from "@tanstack/react-query";
import { ipc, type CaseSummary, type Folder } from "../ipc/client";

export interface CaseGroup {
  label: string;
  cases: CaseSummary[];
}

/** Groups in the order a file explorer shows them: top level, then each folder depth-first. */
export function groupByFolder(cases: CaseSummary[], folders: Folder[]): CaseGroup[] {
  const byName = (a: Folder, b: Folder) => a.name.localeCompare(b.name, "he");
  const caseLabel = (c: CaseSummary) => c.child_name ?? c.meta.code;
  const sorted = (list: CaseSummary[]) => [...list].sort((a, b) => caseLabel(a).localeCompare(caseLabel(b), "he"));
  const known = new Set(folders.map((f) => f.id));
  const groups: CaseGroup[] = [];
  const top = cases.filter((c) => !c.folder_id || !known.has(c.folder_id));
  if (top.length) groups.push({ label: "כל התיקים", cases: sorted(top) });
  const walk = (parent: string | null, path: string[]) => {
    for (const f of folders.filter((x) => x.parent_id === parent).sort(byName)) {
      const here = [...path, f.name];
      const inside = cases.filter((c) => c.folder_id === f.id);
      if (inside.length) groups.push({ label: here.join(" › "), cases: sorted(inside) });
      walk(f.id, here);
    }
  };
  walk(null, []);
  return groups;
}

export function CasePicker(props: { id: string; value: string; onChange: (id: string) => void }) {
  const cases = useQuery({ queryKey: ["cases"], queryFn: ipc.listCases });
  const folders = useQuery({ queryKey: ["folders"], queryFn: ipc.folders });
  const groups = groupByFolder(cases.data ?? [], folders.data ?? []);
  return (
    <select id={props.id} className="select" value={props.value} onChange={(e) => props.onChange(e.target.value)}>
      {groups.map((g) => (
        <optgroup key={g.label} label={g.label}>
          {g.cases.map((c) => (
            <option key={c.id} value={c.id}>{c.child_name ? `${c.child_name} · ` : ""}{c.meta.code}</option>
          ))}
        </optgroup>
      ))}
    </select>
  );
}
