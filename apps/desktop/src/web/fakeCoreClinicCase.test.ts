// The clinic's count of approved report sections in the demo core matches the real core and
// the case list (screens/cases/caseStatus.ts): the signature is not a section of the report's
// count, so approving it never moves "X מתוך Y" on the patient's card.
import { dataOf } from "../ipc/clinic";
import type { CaseSummary } from "../ipc/client";
import type { ClinicRequest } from "../ipc/generated/ClinicRequest";
import { approvedCount, totalSections } from "../screens/cases/caseStatus";
import { today as localToday } from "../screens/clinic/format";
import { FakeCore } from "./fakeCore";

describe("the demo clinic's link to a case", () => {
  it("counts approved sections as the case list does: the signature aside", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    const now = localToday();
    const clinic = async (request: ClinicRequest) => (await core.handle("clinic", { request })) as never;
    const summary = async (id: string) => ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.id === id) as CaseSummary;

    const rows = dataOf("patients", await clinic({ op: "patients", today: now }), "patients");
    const noam = rows.find((r) => r.patient.name === "נועם")?.patient;
    const caseId = noam?.case_id ?? "";
    expect(caseId).not.toBe("");
    const linkOf = async () => dataOf("patient_card", await clinic({ op: "patient_card", id: noam?.id ?? "", today: now }), "card").case;

    const before = await linkOf();
    expect(before?.approved).toBe(approvedCount(await summary(caseId)));
    expect(before?.total).toBe(totalSections(await summary(caseId)));

    // She approves the signature: the case holds one more approved section key, the count stays.
    await core.handle("add_own_paragraph", { caseId, sectionKey: "signature", text: "חתימה לדוגמה" });
    const withSignature = await summary(caseId);
    expect(withSignature.approved_sections).toContain("signature");
    const after = await linkOf();
    expect(after?.approved).toBe(before?.approved);
    expect(after?.approved).toBe(approvedCount(withSignature));
    expect(after?.approved).toBe(withSignature.approved_sections.length - 1);

    // A section of the report itself does count.
    const key = "background";
    await core.handle("add_own_paragraph", { caseId, sectionKey: key, text: "פסקה לדוגמה" });
    const more = await summary(caseId);
    const last = await linkOf();
    expect(last?.approved).toBe(approvedCount(more));
  });
});
