import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { VerifySettings } from "./VerifySettings";

describe("the switch for the check against the sources", () => {
  it("saves the moment it is flipped, with no save button", async () => {
    let on = false;
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "set_verify_stage") { on = (args as { on: boolean }).on; return null; }
      return on;
    });
    const notify = vi.fn();
    const api: AppApi = {
      status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify,
      fail: (e) => e.message, lockNow: vi.fn(async () => undefined),
    };
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={qc}>
        <AppContext.Provider value={api}>
          <VerifySettings />
        </AppContext.Provider>
      </QueryClientProvider>,
    );
    const sw = await screen.findByRole("switch", { name: /לאפשר בדיקה של טיוטה מול החומרים/ });
    await waitFor(() => expect(sw).toBeEnabled());
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("button", { name: /שמירה|לשמור/ })).not.toBeInTheDocument();
    fireEvent.click(sw);
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "true"));
    expect(calls.find(([c]) => c === "set_verify_stage")?.[1]).toMatchObject({ on: true });
    expect(notify).toHaveBeenCalledWith("הבדיקה מול המקורות פעילה.");
  });
});
