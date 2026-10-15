import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { OwnSectionPanel } from "./OwnSection";
import { SectionModesSettings } from "./SectionModes";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

function Wrap({ children }: { children: ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api()}>{children}</AppContext.Provider>
    </QueryClientProvider>
  );
}

// Fabricated report lines, kept with roles in place of names.
const HELP = {
  examples: [{ text: "[ילד] הגיע למפגש עם [אם] ונפרד ממנה בקלות.", report: "דוח בדוי", same_section: true }],
  openings: ["[ילד] הגיע למפגש עם [אם]."],
  closings: ["לאורך המפגש שיתף פעולה."],
  phrases: ["בשיח עימי"],
};

describe("OwnSectionPanel", () => {
  it("shows how she wrote the section before, as words, and words her points on request", async () => {
    mockIPC((cmd) => (cmd === "own_section_help" ? HELP : null));
    const word = vi.fn(async () => true);
    const user = userEvent.setup();
    render(
      <Wrap>
        <OwnSectionPanel caseId="c1" sectionKey="appearance" ai="Claude" working={false} onWord={word} />
      </Wrap>,
    );
    expect(await screen.findByText("ילד הגיע למפגש עם אם ונפרד ממנה בקלות.")).toBeInTheDocument();
    expect(screen.getByText("איך את פותחת את הסעיף")).toBeInTheDocument();
    expect(screen.getByText("בשיח עימי")).toBeInTheDocument();
    const send = screen.getByRole("button", { name: "לנסח מהנקודות שלי" });
    expect(send).toBeDisabled();
    const box = screen.getByLabelText("הנקודות שלך");
    await user.type(box, "נפרד בקלות{Enter}שיתף פעולה");
    await user.click(send);
    expect(word).toHaveBeenCalledWith("נפרד בקלות\nשיתף פעולה");
    expect(box).toHaveValue("");
  });
});

describe("SectionModesSettings", () => {
  it("marks the sections she writes and changes one for every case", async () => {
    const calls: [string, unknown][] = [];
    let modes = [
      { key: "appearance", title: "התרשמות", mode: "self", template_mode: "self" },
      { key: "cognitive", title: "התחום הקוגניטיבי", mode: "ai", template_mode: "ai" },
    ];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "set_section_mode") modes = modes.map((m) => (m.key === "cognitive" ? { ...m, mode: "self" } : m));
      return cmd === "section_modes" ? modes : null;
    });
    const user = userEvent.setup();
    render(<Wrap><SectionModesSettings /></Wrap>);
    expect(await screen.findByLabelText("התרשמות")).toBeChecked();
    expect(screen.getByLabelText("התחום הקוגניטיבי")).not.toBeChecked();
    await user.click(screen.getByLabelText("התחום הקוגניטיבי"));
    expect(calls).toContainEqual(["set_section_mode", { sectionKey: "cognitive", mode: "self" }]);
    expect(await screen.findByText("(שונה מהמכון)")).toBeInTheDocument();
  });
});
