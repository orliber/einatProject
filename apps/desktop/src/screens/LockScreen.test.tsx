import { render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { LockScreen } from "./LockScreen";

describe("LockScreen", () => {
  it("renders the Hebrew lock screen with an accessible password field", () => {
    mockIPC(() => ({ ipc_version: 1, core_version: "0.1.0", build_commit: "test", fips_active: false, platform: "linux" }));
    render(<LockScreen />);
    expect(screen.getByRole("heading", { name: "כספת האבחון" })).toBeInTheDocument();
    expect(screen.getByLabelText("סיסמה")).toHaveAttribute("type", "password");
    expect(screen.getByRole("button", { name: "פתיחה" })).toBeInTheDocument();
  });

  it("shows that the secure core is connected after ping succeeds", async () => {
    mockIPC((cmd) => {
      if (cmd === "ping") {
        return { ipc_version: 1, core_version: "0.1.0", build_commit: "test", fips_active: false, platform: "linux" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    render(<LockScreen />);
    expect(await screen.findByText(/הליבה המאובטחת מחוברת/)).toBeInTheDocument();
  });

  it("shows a clear error when the core does not answer", async () => {
    mockIPC(() => {
      throw new Error("down");
    });
    render(<LockScreen />);
    expect(await screen.findByText("אין חיבור לליבה המאובטחת")).toBeInTheDocument();
  });
});
