import { describe, expect, it } from "vitest";
import type { CaseSummary, Folder } from "../ipc/client";
import { groupByFolder } from "./CasePicker";

const folder = (id: string, name: string, parent: string | null = null): Folder => ({ id, name, parent_id: parent, created_at: 0 });
const kase = (id: string, child: string, folder_id: string | null): CaseSummary => ({
  id, child_name: child, folder_id, deleted_at: null, created_at: 0, updated_at: 0, approved_sections: [],
  meta: { code: id, age: null, child_gender: null, current_section: null, retention_until: null, consent: null },
});

describe("groupByFolder", () => {
  it("lists the top level first, then folders depth-first with their full path", () => {
    const folders = [folder("b", "פרטיים"), folder("c", "2026", "b"), folder("a", "מכון")];
    const cases = [kase("1", "נועם", null), kase("2", "מאיה", "c"), kase("3", "אורי", "a"), kase("4", "גיא", "missing")];
    expect(groupByFolder(cases, folders).map((g) => [g.label, g.cases.map((c) => c.child_name)])).toEqual([
      ["כל התיקים", ["גיא", "נועם"]],
      ["מכון", ["אורי"]],
      ["פרטיים › 2026", ["מאיה"]],
    ]);
  });
});
