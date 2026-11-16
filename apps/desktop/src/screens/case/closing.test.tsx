// D-163: the closing sections in the right order. The steps and their states, when the strip
// has a place on the page, what counts as not done, and the one button that drafts the DSM
// section and the recommendations side by side. Over the demo core; every detail is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../../App";
import { FakeCore } from "../../web/fakeCore";
import type { CaseDetail } from "../../ipc/client";
import { prepared } from "../../test/fixtures";
import { closingAnchor, closingRequest, closingSteps, emptyClosing, ownEmptyLine, showClosing } from "./closing";
import { DRAFT_FROM_APPROVED } from "./instructions";

configure({ asyncUtilTimeout: 6000 });
vi.setConfig({ testTimeout: 30_000 });

type Section = CaseDetail["sections"][number];
type Status = "approved" | "proposed";

function section(key: string, status: Status | null = null, extra: Partial<Section> = {}): Section {
  return {
    key, title: key, part: "א", source_count: 1, sortable: true, mode: "ai",
    paragraphs: status ? [{ id: `${key}-0`, text: "טקסט בדוי", status, by_ai: true, sources: [], warnings: [], polish: [], notes: [], replaces: null, has_versions: false, style_note: null, style: null }] : [],
    approved: status === "approved",
    ...extra,
  } as Section;
}

const body = (status: Status | null) => ["referral", "background", "kindergarten"].map((k) => section(k, status));
const states = (sections: Section[]) => closingSteps(sections).map((s) => `${s.id}:${s.state}`);

describe("the steps of the closing sections", () => {
  it("go in order: her diagnoses, then the DSM section and the recommendations, then the summary", () => {
    const empty = [...body("approved"), section("dsm"), section("summary"), section("diagnoses"), section("recommendations")];
    expect(states(empty)).toEqual(["diagnoses:next", "dsm_recommendations:later", "summary:later"]);
    expect(closingSteps(empty).map((s) => s.label)).toEqual(["אבחנות", "DSM והמלצות", "סיכום"]);
    // Both are drafted by the one button.
    expect(closingSteps(empty)[1]?.toDraft.map((s) => s.key)).toEqual(["dsm", "recommendations"]);

    const chosen = [...body("approved"), section("dsm"), section("summary"), section("diagnoses", "approved"), section("recommendations")];
    expect(states(chosen)).toEqual(["diagnoses:done", "dsm_recommendations:next", "summary:later"]);

    const drafted = [...body("approved"), section("dsm", "proposed"), section("summary"), section("diagnoses", "approved"), section("recommendations", "approved")];
    expect(states(drafted)).toEqual(["diagnoses:done", "dsm_recommendations:waiting", "summary:later"]);

    // The DSM section left empty does not hold the summary back: it is not for every child.
    const noDsm = [...body("approved"), section("dsm"), section("summary"), section("diagnoses", "approved"), section("recommendations", "approved")];
    expect(states(noDsm)).toEqual(["diagnoses:done", "dsm_recommendations:done", "summary:next"]);

    const all = [...body("approved"), section("dsm", "approved"), section("summary", "approved"), section("diagnoses", "approved"), section("recommendations", "approved")];
    expect(states(all)).toEqual(["diagnoses:done", "dsm_recommendations:done", "summary:done"]);
  });

  it("only the empty sections the AI drafts are drafted; a report without a section has no step for it", () => {
    const mine = [...body("approved"), section("dsm", "approved"), section("recommendations", null, { mode: "self" }), section("summary")];
    const steps = closingSteps(mine);
    expect(steps.map((s) => s.id)).toEqual(["dsm_recommendations", "summary"]);
    expect(steps[0]?.toDraft).toEqual([]);
    expect(closingSteps(body("approved"))).toEqual([]);
  });

  it("the strip shows when the body is mostly approved or she is on a closing section, and never once all is done", () => {
    const closing = [section("summary"), section("diagnoses"), section("recommendations")];
    const early = [...body(null), ...closing];
    expect(showClosing(early, "background")).toBe(false);
    expect(showClosing(early, "summary")).toBe(true);
    const two = [section("referral", "approved"), section("background", "approved"), section("kindergarten", "proposed"), ...closing];
    expect(showClosing(two, "kindergarten")).toBe(true);
    const one = [section("referral", "approved"), section("background"), section("kindergarten"), ...closing];
    expect(showClosing(one, "background")).toBe(false);
    // A section with no material and no text is not part of the count.
    const bare = [section("referral", "approved"), section("background", "approved"), section("extra", null, { source_count: 0 }), ...closing];
    expect(showClosing(bare, "referral")).toBe(true);
    const finished = [...body("approved"), ...closing.map((s) => section(s.key, "approved"))];
    expect(showClosing(finished, "summary")).toBe(false);
    expect(closingAnchor([...body("approved"), section("dsm"), ...closing])).toBe("summary");
  });

  it("an empty closing section is not done, the DSM section apart; her own empty sections are named", () => {
    const sections = [...body("approved"), section("dsm"), section("summary"), section("diagnoses", "proposed"), section("recommendations", "approved")];
    expect(emptyClosing(sections).map((s) => s.key)).toEqual(["summary", "diagnoses"]);
    expect(ownEmptyLine(sections)).toBeNull();
    const own = [...sections, section("impression", null, { mode: "self", title: "התרשמות" }), section("emotional", null, { mode: "self", title: "התחום הרגשי" })];
    expect(ownEmptyLine(own)).toBe("הסעיפים \"התרשמות\", \"התחום הרגשי\", שאת כותבת בעצמך, עוד לא נכתבו, ולכן לא יבואו לידי ביטוי בניסוח.");
    expect(ownEmptyLine([...sections, section("impression", null, { mode: "self", title: "התרשמות" })])).toMatch(/^הסעיף "התרשמות", שאת כותבת בעצמך, עוד לא נכתב/);
    expect(closingRequest(" קצר ")).toBe(`${DRAFT_FROM_APPROVED} מה שחשוב לי שייכתב בסעיף: קצר`);
    expect(closingRequest("")).toBeUndefined();
  });
});

async function openReport(reshape: (d: CaseDetail) => void) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("set_review_always", { on: false });
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC(async (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    // The closing sections' requests are answered here: the case on the page is reshaped.
    if (cmd === "draft_briefs") return { recent: [], sections: { recommendations: "לפי סדר חשיבות" } };
    if (cmd === "prepare_section") return prepared({ approval_id: `ap-${String(a.sectionKey)}` });
    if (cmd === "send_section") return { reply: "", paragraphs: [], questions: [], missing: [], contradictions: [], demo: true };
    const out = await core.handle(cmd, args);
    if (cmd !== "case_detail") return out;
    const d = structuredClone(out as CaseDetail);
    reshape(d);
    return d;
  });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
  const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
  return { calls, user, page };
}

/** The whole body approved, her diagnoses chosen, and one section of her own still empty. */
function bodyApproved(d: CaseDetail, diagnoses: boolean) {
  const model = d.sections.flatMap((s) => s.paragraphs).find((p) => p.status === "approved");
  if (!model) throw new Error("the demo case has an approved paragraph");
  for (const s of d.sections) {
    const closing = ["dsm", "summary", "diagnoses", "recommendations", "signature"].includes(s.key);
    if (!closing || (s.key === "diagnoses" && diagnoses)) {
      s.paragraphs = [{ ...model, id: `${s.key}-ok`, by_ai: !closing }];
      s.approved = true;
    }
  }
  const own = d.sections.find((s) => s.key === "appearance");
  if (own) {
    own.mode = "self";
    own.source_count = 0;
    own.paragraphs = [];
    own.approved = false;
  }
}

describe("the closing strip on the report page", () => {
  it("is not there while the body is still being written", async () => {
    const { page } = await openReport(() => undefined);
    await within(page).findAllByRole("heading", { level: 3 });
    expect(within(page).queryByRole("group", { name: "סעיפי הסיום, לפי הסדר" })).toBeNull();
  });

  it("shows each step's state, and one button drafts the DSM section and the recommendations side by side, each with her kept wish", async () => {
    const { page, user, calls } = await openReport((d) => bodyApproved(d, true));
    const strip = await within(page).findByRole("group", { name: "סעיפי הסיום, לפי הסדר" });
    const steps = within(strip).getAllByRole("listitem");
    expect(steps.map((li) => li.className)).toEqual(["closing-step st-done", "closing-step st-next", "closing-step st-later"]);
    expect(steps[0]).toHaveTextContent("אבחנות");
    expect(steps[0]).toHaveTextContent("נעשה");
    expect(steps[1]).toHaveTextContent("הבא בתור");
    // The summary waits for the recommendations, unless she says otherwise.
    expect(within(steps[2] as HTMLElement).queryByRole("button", { name: /לנסח את הסיכום/ })).toBeNull();
    expect(within(strip).getByText("הסיכום נכתב אחרי שההמלצות מאושרות, כדי שיכלול אותן.")).toBeInTheDocument();
    // Her own section still empty: said once, quietly (under the box of the closing section
    // in focus, else in the strip).
    expect(within(page).getAllByText(/שאת כותבת בעצמך, עוד לא נכתב/)).toHaveLength(1);
    expect(await within(page).findByText(/שאת כותבת בעצמך, עוד לא נכתב, ולכן לא יבוא לידי ביטוי בניסוח\.$/)).toBeInTheDocument();

    await user.click(within(steps[1] as HTMLElement).getByRole("button", { name: "✦ לנסח את שניהם" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "send_section")).toHaveLength(2));
    const asked = calls.filter((c) => c.cmd === "prepare_section").map((c) => `${String(c.args.sectionKey)}: ${String(c.args.instruction)}`).sort();
    expect(asked).toEqual([
      `dsm: ${DRAFT_FROM_APPROVED}`,
      `recommendations: ${DRAFT_FROM_APPROVED} מה שחשוב לי שייכתב בסעיף: לפי סדר חשיבות`,
    ]);

    await user.click(within(steps[2] as HTMLElement).getByRole("button", { name: "להמשיך בכל זאת" }));
    await user.click(within(strip).getByRole("button", { name: "✦ לנסח את הסיכום" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section" && c.args.sectionKey === "summary")).toBe(true));
  });

  it("her diagnoses come first: the step leads to the section, and what is drafted before them says so", async () => {
    const { page, user } = await openReport((d) => bodyApproved(d, false));
    const strip = await within(page).findByRole("group", { name: "סעיפי הסיום, לפי הסדר" });
    const steps = within(strip).getAllByRole("listitem");
    expect(steps.map((li) => li.className)).toEqual(["closing-step st-next", "closing-step st-later", "closing-step st-later"]);
    await user.click(within(steps[0] as HTMLElement).getByRole("button", { name: "לבחירת האבחנות" }));
    await waitFor(() => expect(document.querySelector(".a4-section.is-focus")?.id).toBe("sec-diagnoses"));
    // The contents say what is left, and the way to the file stays open.
    const contents = screen.getByRole("complementary", { name: "תוכן הדוח" });
    expect(within(contents).getByRole("button", { name: /^נשאר לכתוב: .*סיכום/ })).toBeInTheDocument();
    expect(within(contents).getByRole("button", { name: "להוצאת הדוח ←" })).not.toHaveClass("btn-primary");
  });

  it("before her diagnoses, the steps after them wait, and drafting them anyway is her choice, said quietly", async () => {
    const { page, user, calls } = await openReport((d) => bodyApproved(d, false));
    const strip = await within(page).findByRole("group", { name: "סעיפי הסיום, לפי הסדר" });
    const steps = within(strip).getAllByRole("listitem");
    expect(within(steps[1] as HTMLElement).queryByRole("button", { name: /^✦ לנסח/ })).toBeNull();
    expect(within(strip).getByText("שאר סעיפי הסיום נכתבים אחרי שהאבחנות שלך מאושרות, כדי שיתבססו עליהן.")).toBeInTheDocument();
    expect(within(strip).queryByText("הסיכום נכתב אחרי שההמלצות מאושרות, כדי שיכלול אותן.")).toBeNull();

    await user.click(within(steps[1] as HTMLElement).getByRole("button", { name: "להמשיך בכל זאת" }));
    expect(within(strip).getByText("האבחנות עוד לא אושרו: מה שינוסח עכשיו ייכתב בלעדיהן.")).toBeInTheDocument();
    // The summary still waits for the recommendations.
    expect(within(strip).getByText("הסיכום נכתב אחרי שההמלצות מאושרות, כדי שיכלול אותן.")).toBeInTheDocument();
    await user.click(within(steps[1] as HTMLElement).getByRole("button", { name: "✦ לנסח את שניהם" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "send_section")).toHaveLength(2));
  });

  it("with the recommendations approved and her diagnoses not, the summary still waits for her diagnoses", async () => {
    const { page, user, calls } = await openReport((d) => {
      bodyApproved(d, false);
      const model = d.sections.flatMap((s) => s.paragraphs).find((p) => p.status === "approved");
      for (const s of d.sections) {
        if (model && (s.key === "recommendations" || s.key === "dsm")) {
          s.paragraphs = [{ ...model, id: `${s.key}-ok` }];
          s.approved = true;
        }
      }
    });
    const strip = await within(page).findByRole("group", { name: "סעיפי הסיום, לפי הסדר" });
    expect(within(strip).queryByRole("button", { name: "✦ לנסח את הסיכום" })).toBeNull();
    expect(within(strip).getByText("שאר סעיפי הסיום נכתבים אחרי שהאבחנות שלך מאושרות, כדי שיתבססו עליהן.")).toBeInTheDocument();
    await user.click(within(strip).getByRole("button", { name: "להמשיך בכל זאת" }));
    expect(within(strip).getByText("האבחנות עוד לא אושרו: מה שינוסח עכשיו ייכתב בלעדיהן.")).toBeInTheDocument();
    await user.click(within(strip).getByRole("button", { name: "✦ לנסח את הסיכום" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section" && c.args.sectionKey === "summary")).toBe(true));
  });
});
