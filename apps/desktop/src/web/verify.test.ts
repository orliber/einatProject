// The check of a draft against its sources over the demo core: off by default, its own
// review (an approval of its own), and a verdict per proposed paragraph that changes nothing.
import { FakeCore } from "./fakeCore";

vi.setConfig({ testTimeout: 30_000 });

async function drafted() {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  const cases = (await core.handle("list_cases", {})) as { id: string }[];
  const caseId = cases[0]?.id ?? "";
  const detail = (await core.handle("case_detail", { caseId })) as {
    sections: { key: string; source_count: number }[];
  };
  const key = detail.sections.find((s) => s.source_count > 0 && !["summary", "diagnoses", "recommendations", "dsm"].includes(s.key))?.key ?? "";
  const prepared = (await core.handle("prepare_section", { caseId, sectionKey: key, instruction: "טיוטה" })) as { approval_id: string };
  await core.handle("send_section", { approvalId: prepared.approval_id });
  return { core, caseId, key };
}

describe("the check against the sources", () => {
  it("is off until switched on", async () => {
    const { core, caseId, key } = await drafted();
    expect(await core.handle("verify_stage_on", {})).toBe(false);
    await expect(core.handle("prepare_verify", { caseId, sectionKey: key })).rejects.toBeDefined();
  });

  it("has its own approval, answers per proposed paragraph and changes nothing", async () => {
    const { core, caseId, key } = await drafted();
    await core.handle("set_verify_stage", { on: true });
    const proposed = async () => {
      const d = (await core.handle("case_detail", { caseId })) as { sections: { key: string; paragraphs: { status: string }[] }[] };
      return d.sections.find((s) => s.key === key)?.paragraphs.filter((p) => p.status === "proposed").length ?? 0;
    };
    const before = await proposed();
    expect(before).toBeGreaterThan(0);
    const prepared = (await core.handle("prepare_verify", { caseId, sectionKey: key })) as { approval_id: string };
    const result = (await core.handle("send_verify", { approvalId: prepared.approval_id })) as {
      draft_ids: string[];
      checks: { verdict: string }[];
    };
    expect(result.draft_ids).toHaveLength(before);
    expect(result.checks.every((c) => c.verdict !== "supported")).toBe(true);
    expect(await proposed()).toBe(before);
    // An approval is used once.
    await expect(core.handle("send_verify", { approvalId: prepared.approval_id })).rejects.toBeDefined();
  });
});
