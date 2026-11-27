import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { NameModelSettings } from "./NameModelSettings";

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

describe("name model download (UIA-5)", () => {
  it("a download still running when the page opens again is shown, and the switch cannot start another", async () => {
    let progress: [number, number] | null = [100, 700];
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "name_model_progress") return progress;
      if (cmd === "name_model_status") {
        return { enabled: progress === null, downloaded: progress === null, size_mb: 700, problem: null };
      }
      return null;
    });
    render(wrap(<NameModelSettings />));
    const toggle = await screen.findByRole("switch", { name: /זיהוי שמות/ });
    await vi.waitFor(() => expect(toggle).toBeDisabled());
    expect(screen.getByText(/מוריד את המודל|מתחיל להוריד/)).toBeInTheDocument();
    // It ends: the page reads the result and the switch is free again, now on.
    progress = null;
    await vi.waitFor(() => expect(toggle).not.toBeDisabled(), { timeout: 3000 });
    await vi.waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(calls).not.toContain("download_name_model");
  });
});
