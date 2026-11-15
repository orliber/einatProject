// The case's consent keeps the form version it was signed on (D-115): saving other details does
// not relabel it, and only her "signed again on today's form" puts it on the current version.
// All names are invented.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../../App";
import { status } from "../../test/fixtures";
import { FakeCore } from "../../web/fakeCore";
import type { CaseDetail } from "../../ipc/client";
import type { CaseApi } from "../CaseScreen";
import { CONSENT_FORM_VERSION } from "../../consentForm";
import { DetailsView } from "./DetailsView";
import { isoToHe, todayIso } from "../../components/DateField";

vi.setConfig({ testTimeout: 30_000 });

async function openOnOldForm() {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  const caseId = (await core.handle("create_case", {
    meta: {
      code: "תיק-בדיקה", age: { years: 5, months: 2 }, child_gender: "male", current_section: null,
      retention_until: null, consent: { given_on: "2026-01-05", form_version: "v1", given_by: "שני ההורים" },
    },
    identities: [{ id: null, role: "child", value: "יונתן", aliases: [] }],
  })) as string;
  const updates: Record<string, unknown>[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "update_case") updates.push(args as Record<string, unknown>);
    return core.handle(cmd, args);
  });
  const detail = (await core.handle("case_detail", { caseId })) as CaseDetail;
  const app: AppApi = { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
  const api = { caseId, detail, reload: vi.fn(async () => undefined) } as unknown as CaseApi;
  render(<AppContext.Provider value={app}><DetailsView api={api} /></AppContext.Provider>);
  const version = () => (updates.at(-1)?.meta as { consent: { form_version: string } }).consent.form_version;
  return { updates, version, user: userEvent.setup() };
}

describe("consent on an older form", () => {
  it("says so quietly, and saving the details keeps the version it was signed on", async () => {
    const { updates, version, user } = await openOnOldForm();
    expect(screen.getByText(/נוסח v1 של הטופס/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    await waitFor(() => expect(updates).toHaveLength(1));
    expect(version()).toBe("v1");
  });

  it("records it on today's form once she says the parents signed again", async () => {
    const { updates, version, user } = await openOnOldForm();
    await user.click(screen.getByRole("button", { name: "ההורים חתמו מחדש על הנוסח הנוכחי" }));
    expect(screen.getByText(/תירשם לפי הנוסח הנוכחי/)).toBeInTheDocument();
    // The date of the new signature starts at today, shown in the field, and can be edited.
    expect(screen.getByLabelText("תאריך")).toHaveValue(isoToHe(todayIso()));
    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    await waitFor(() => expect(updates).toHaveLength(1));
    expect(version()).toBe(CONSENT_FORM_VERSION);
    expect((updates[0]?.meta as { consent: { given_on: string } }).consent.given_on).toBe(todayIso());
  });
});

/** The demo core's own case, saved through the real IPC path; every call recorded. */
async function openDemo() {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  const caseId = (await core.handle("create_case", {
    meta: { code: "תיק-בדיקה", age: { years: 5, months: 2 }, child_gender: "male", current_section: null, retention_until: null, consent: null },
    identities: [{ id: null, role: "child", value: "יונתן", aliases: [] }],
  })) as string;
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    return core.handle(cmd, args);
  });
  const detail = (await core.handle("case_detail", { caseId })) as CaseDetail;
  const app: AppApi = { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
  const api = { caseId, detail, reload: vi.fn(async () => undefined) } as unknown as CaseApi;
  render(<AppContext.Provider value={app}><DetailsView api={api} /></AppContext.Provider>);
  const identities = async () => ((await core.handle("case_detail", { caseId })) as CaseDetail).identities;
  return { calls, identities, user: userEvent.setup() };
}

describe("saving the details twice", () => {
  it("a person added once stays the same person: the second save sends her id, not a new row (regression)", async () => {
    const { calls, identities, user } = await openDemo();
    await user.click(screen.getByRole("button", { name: "+ הוספת אדם" }));
    const rows = screen.getAllByLabelText("שם");
    await user.type(rows[rows.length - 1] as HTMLElement, "רות");
    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "set_identities")).toHaveLength(1));
    const first = (await identities()).find((i) => i.value === "רות");
    expect(first?.id).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "set_identities")).toHaveLength(2));
    const sent = (calls.filter((c) => c.cmd === "set_identities")[1]?.args.identities as { id: string | null; value: string }[]).find((i) => i.value === "רות");
    expect(sent?.id).toBe(first?.id);
    expect((await identities()).find((i) => i.value === "רות")?.id).toBe(first?.id);
    expect((await identities()).filter((i) => i.value === "רות")).toHaveLength(1);
  });

  it("a person whose role she changed stays one person: the save after that sends her new id (regression)", async () => {
    const { calls, identities, user } = await openDemo();
    await user.click(screen.getByRole("button", { name: "+ הוספת אדם" }));
    const rows = screen.getAllByLabelText("שם");
    await user.type(rows[rows.length - 1] as HTMLElement, "רות");
    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "set_identities")).toHaveLength(1));
    const asMother = (await identities()).find((i) => i.value === "רות")?.id;
    expect(asMother).toBeTruthy();

    // The vault gives a changed role a new id (and a new tag).
    const roles = screen.getAllByLabelText("תפקיד");
    await user.selectOptions(roles[roles.length - 1] as HTMLElement, "teacher");
    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "set_identities")).toHaveLength(2));
    const asTeacher = (await identities()).find((i) => i.value === "רות");
    expect(asTeacher?.role).toBe("teacher");
    expect(asTeacher?.id).not.toBe(asMother);

    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "set_identities")).toHaveLength(3));
    const sent = (calls.filter((c) => c.cmd === "set_identities")[2]?.args.identities as { id: string | null; value: string }[]).find((i) => i.value === "רות");
    expect(sent?.id).toBe(asTeacher?.id);
    expect((await identities()).find((i) => i.value === "רות")?.id).toBe(asTeacher?.id);
    expect((await identities()).filter((i) => i.value === "רות")).toHaveLength(1);
  });

  it("months without years are refused, not saved as no age", async () => {
    const { calls, user } = await openDemo();
    await user.clear(screen.getByLabelText("שנים"));
    expect(screen.getByText("יש להזין שנים")).toBeInTheDocument();
    expect(screen.getByLabelText("שנים")).toHaveAttribute("aria-invalid", "true");
    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    expect(screen.getByRole("alert")).toHaveTextContent("יש להזין שנים");
    expect(calls.filter((c) => c.cmd === "update_case")).toHaveLength(0);
    // Both cleared: she means no age, and that is saved.
    await user.clear(screen.getByLabelText("חודשים"));
    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "update_case")).toHaveLength(1));
    expect((calls.find((c) => c.cmd === "update_case")?.args.meta as { age: unknown }).age).toBeNull();
  });

  it("an impossible age is refused with a reason, and the age on file is not erased", async () => {
    const { calls, user } = await openDemo();
    const months = screen.getByLabelText("חודשים");
    await user.clear(months);
    await user.type(months, "13");
    await user.click(screen.getByRole("button", { name: "שמירת השינויים" }));
    expect(screen.getByRole("alert")).toHaveTextContent(/גיל/);
    expect(calls.filter((c) => c.cmd === "update_case")).toHaveLength(0);
  });
});
