// The IPC layer: the only module that talks to the Rust core. Tauri's `invoke` is replaced by a
// mock, so these tests check exactly what crosses the bridge and how failures come back.
import { invoke } from "@tauri-apps/api/core";
import { asUiError, ipc } from "./client";
import { replayFor } from "../errors";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mocked = vi.mocked(invoke);

beforeEach(() => {
  mocked.mockReset();
  mocked.mockResolvedValue(null);
});

/** A File whose bytes jsdom can read back (its File lacks `arrayBuffer` in some versions). */
function file(name: string, bytes: number[]): File {
  const f = new File([new Uint8Array(bytes)], name);
  Object.defineProperty(f, "arrayBuffer", { value: async () => new Uint8Array(bytes).buffer });
  return f;
}

describe("asUiError", () => {
  it("passes a core error through unchanged", () => {
    const e = { code: "wrong_secret", message: "הסיסמה לא נכונה.", details: [], action: { kind: "dismiss" } };
    expect(asUiError(e)).toBe(e);
    // From an older core without a next step: a report for Or, never a dead end (F6).
    expect(asUiError({ code: "x", message: "y" }).action).toEqual({ kind: "report" });
  });

  it("turns anything else into a Hebrew 'internal' error, keeping the text for Or", () => {
    expect(asUiError("panic at core")).toEqual({ code: "internal", message: "משהו השתבש. אפשר לנסות שוב.", details: ["panic at core"], action: { kind: "retry", after_seconds: 0 } });
    expect(asUiError(new Error("boom")).details).toEqual(["Error: boom"]);
    expect(asUiError(undefined).code).toBe("internal");
    // A message without a code is not a core error (it could be anything thrown in the webview).
    expect(asUiError({ message: "x" }).code).toBe("internal");
  });
});

describe("ipc", () => {
  it("sends each command with the argument names the core expects (camelCase)", async () => {
    await ipc.unlock("סיסמה בדויה");
    expect(mocked).toHaveBeenLastCalledWith("unlock", { password: "סיסמה בדויה" });
    await ipc.decideSuspect("c1", "דנה", { decision: "hide" } as never);
    expect(mocked).toHaveBeenLastCalledWith("decide_suspect", { caseId: "c1", token: "דנה", decision: { decision: "hide" } });
    await ipc.restoreAutoHidden("c1", "חן", "[אדם_1]");
    expect(mocked).toHaveBeenLastCalledWith("restore_auto_hidden", { caseId: "c1", token: "חן", tag: "[אדם_1]" });
    await ipc.exportReport("c1", null);
    expect(mocked).toHaveBeenLastCalledWith("export_report", { caseId: "c1", password: null });
    await ipc.setApiKey("anthropic", "k");
    expect(mocked).toHaveBeenLastCalledWith("set_api_key", { provider: "anthropic", key: "k", retentionAck: false });
  });

  it("a section request without a paragraph to replace says so explicitly (null, not missing)", async () => {
    await ipc.prepareSection("c1", "background", "קצר יותר");
    expect(mocked).toHaveBeenLastCalledWith("prepare_section", { caseId: "c1", sectionKey: "background", instruction: "קצר יותר", replaces: null });
    await ipc.prepareSection("c1", "background", "", "d7");
    expect(mocked).toHaveBeenLastCalledWith("prepare_section", { caseId: "c1", sectionKey: "background", instruction: "", replaces: "d7" });
  });

  it("her own paragraph goes at the end, first, or after a given paragraph", async () => {
    await ipc.addOwnParagraph("c1", "s", "טקסט");
    expect(mocked).toHaveBeenLastCalledWith("add_own_paragraph", { caseId: "c1", sectionKey: "s", text: "טקסט", first: false, after: null });
    await ipc.addOwnParagraph("c1", "s", "טקסט", "first");
    expect(mocked).toHaveBeenLastCalledWith("add_own_paragraph", { caseId: "c1", sectionKey: "s", text: "טקסט", first: true, after: null });
    await ipc.addOwnParagraph("c1", "s", "טקסט", "d3");
    expect(mocked).toHaveBeenLastCalledWith("add_own_paragraph", { caseId: "c1", sectionKey: "s", text: "טקסט", first: false, after: "d3" });
  });

  it("a command that answers nothing resolves to undefined, whatever the core returned", async () => {
    mocked.mockResolvedValueOnce({ unexpected: true });
    await expect(ipc.lock()).resolves.toBeUndefined();
  });

  it("returns the core's answer as is", async () => {
    mocked.mockResolvedValueOnce(["a", "b"]);
    await expect(ipc.listCases()).resolves.toEqual(["a", "b"]);
  });

  it("a core failure reaches the UI as the core's UiError", async () => {
    const e = { code: "consent_missing", message: "צריך לרשום הסכמה.", details: [], action: { kind: "consent" } };
    mocked.mockRejectedValueOnce(e);
    await expect(ipc.prepareSort("c1")).rejects.toBe(e);
  });

  it("an unknown failure reaches the UI as a Hebrew message, never as a raw exception", async () => {
    mocked.mockRejectedValueOnce("command import_document not allowed by the capability");
    const err = await ipc.listCases().catch((x: unknown) => x);
    expect(err).toEqual({ code: "internal", message: "משהו השתבש. אפשר לנסות שוב.", details: ["command import_document not allowed by the capability"], action: { kind: "retry", after_seconds: 0 } });
  });

  it("a document goes as raw bytes; the case id and file name only as encoded headers", async () => {
    await ipc.importDocument("תיק 1/2", file("דוח גן?.docx", [1, 2, 3]));
    const [cmd, body, options] = mocked.mock.lastCall ?? [];
    expect(cmd).toBe("import_document");
    expect(body).toBeInstanceOf(Uint8Array);
    expect(Array.from(body as Uint8Array)).toEqual([1, 2, 3]);
    // Hebrew and separators cannot travel in an HTTP header as is.
    expect(options).toEqual({ headers: { "x-case-id": "%D7%AA%D7%99%D7%A7%201%2F2", "x-file-name": "%D7%93%D7%95%D7%97%20%D7%92%D7%9F%3F.docx" } });
    expect(decodeURIComponent((options as { headers: Record<string, string> }).headers["x-file-name"] ?? "")).toBe("דוח גן?.docx");
  });

  it("a failure that offers a retry keeps the call itself, so it runs again exactly (F6)", async () => {
    const e = { code: "egress", message: "אין חיבור.", details: [], action: { kind: "retry", after_seconds: 0 } };
    mocked.mockRejectedValueOnce(e);
    const err = await ipc.sendSection("a7").catch((x: unknown) => x as typeof e);
    const again = replayFor(err as never);
    expect(again).not.toBeNull();
    mocked.mockResolvedValueOnce({ draft_ids: ["d1"], demo: false });
    await expect(again?.()).resolves.toEqual({ draft_ids: ["d1"], demo: false });
    expect(mocked).toHaveBeenLastCalledWith("send_section", { approvalId: "a7" });
    // A failure that is hers to fix keeps nothing to run again.
    mocked.mockRejectedValueOnce({ code: "refused", message: "לא.", details: [], action: { kind: "dismiss" } });
    const other = await ipc.listCases().catch((x: unknown) => x);
    expect(replayFor(other as never)).toBeNull();
  });

  it("raw-body commands also turn failures into a UiError", async () => {
    mocked.mockRejectedValueOnce("too large");
    const err = await ipc.importStyleSource(file("a.docx", [9])).catch((x: unknown) => x);
    expect(err).toMatchObject({ code: "internal", details: ["too large"] });
    mocked.mockRejectedValueOnce({ code: "refused", message: "הקובץ לא נקרא.", details: [] });
    await expect(ipc.checkOriginal(file("a.pdf", [1]))).rejects.toMatchObject({ code: "refused" });
    mocked.mockRejectedValueOnce(new Error("x"));
    await expect(ipc.setReportTemplate(file("t.docx", [1]))).rejects.toMatchObject({ code: "internal" });
  });

  it("the Word template goes as bytes, with no file name or path", async () => {
    await ipc.setReportTemplate(file("C:\\Users\\someone\\תבנית.docx", [7, 8]));
    expect(mocked.mock.lastCall?.length).toBe(2);
    const [cmd, body] = mocked.mock.lastCall ?? [];
    expect(cmd).toBe("set_report_template");
    expect(Array.from(body as Uint8Array)).toEqual([7, 8]);
  });
});
