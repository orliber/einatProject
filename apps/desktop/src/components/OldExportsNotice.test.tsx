// Reports an older version left in the app's own folder (D-120): shown while they are there,
// opened on request, never deleted, and only their number crosses the bridge.
import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { OldExportsNotice } from "./RetentionNotice";

const api = (): AppApi => ({
  status: status(),
  go: vi.fn(),
  refresh: vi.fn(async () => undefined),
  notify: vi.fn(),
  fail: (e) => e.message,
  lockNow: vi.fn(async () => undefined),
});

function wrap(children: ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api()}>{children}</AppContext.Provider>
    </QueryClientProvider>
  );
}

describe("files an older version left in the app's folder", () => {
  it("says how many, opens the folder on request, and offers no delete", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return cmd === "old_exports" ? 2 : null;
    });
    render(wrap(<OldExportsNotice />));
    expect(await screen.findByText(/2 קבצים שהופקו בגרסה קודמת נשמרו/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /מחיקה|למחוק/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "פתיחת התיקייה" }));
    await vi.waitFor(() => expect(calls).toContain("open_old_exports"));
    fireEvent.click(screen.getByRole("button", { name: "לא עכשיו" }));
    expect(screen.queryByText(/בגרסה קודמת/)).toBeNull();
  });

  it("says nothing when the folder is empty or gone", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return 0;
    });
    const { container } = render(wrap(<OldExportsNotice />));
    await vi.waitFor(() => expect(calls).toContain("old_exports"));
    expect(container).toBeEmptyDOMElement();
  });
});
