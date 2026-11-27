// The profile editor: an item she added herself has no id yet (""), so opening one for editing
// must open that one only. Every item is invented.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AppContext, type AppApi } from "../../App";
import type { StyleOverview, StyleProfileView } from "../../ipc/client";
import { status } from "../../test/fixtures";
import { ProfileEditor } from "./ProfileEditor";

const draft: StyleProfileView = {
  id: "p1", version: 1, status: "draft", created_at: 1_790_000_000, dropped: 0, demo: false,
  profile: {
    reports: 0,
    items: [
      { id: "", section: null, kind: "rule", text: "פסקת הסיכום פותחת בחוזקות", enabled: true, origin: "manual", support: 0 },
      { id: "", section: null, kind: "rule", text: "משפט אחד לכל ממצא", enabled: true, origin: "manual", support: 0 },
    ],
  },
};

const overview = { sections: [] } as unknown as StyleOverview;

describe("the profile editor", () => {
  it("editing one item she added opens a box on that item only (both have no id yet)", async () => {
    mockIPC(() => null);
    const app: AppApi = { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={qc}>
        <AppContext.Provider value={app}><ProfileEditor view={draft} o={overview} /></AppContext.Provider>
      </QueryClientProvider>,
    );
    const user = userEvent.setup();
    const region = screen.getByRole("region", { name: "טיוטת פרופיל הסגנון" });
    await user.click(within(region).getByRole("button", { name: "משפט אחד לכל ממצא" }));
    const boxes = within(region).getAllByRole("textbox", { name: "נוסח הפריט" });
    expect(boxes).toHaveLength(1);
    expect(boxes[0]).toHaveValue("משפט אחד לכל ממצא");
    // The other item is still shown as text, not as a box.
    expect(within(region).getByRole("button", { name: "פסקת הסיכום פותחת בחוזקות" })).toBeInTheDocument();
    // Her change goes to that item only.
    await user.clear(boxes[0] as HTMLElement);
    await user.type(boxes[0] as HTMLElement, "משפט קצר לכל ממצא");
    await user.tab();
    expect(within(region).queryByRole("textbox", { name: "נוסח הפריט" })).toBeNull();
    expect(within(region).getByRole("button", { name: "משפט קצר לכל ממצא" })).toBeInTheDocument();
    expect(within(region).getByRole("button", { name: "פסקת הסיכום פותחת בחוזקות" })).toBeInTheDocument();
  });
});
