import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { IntegrationsPanel } from "./IntegrationsPanel";

vi.mock("../../lib/ipc", () => ({
  getIntegrationStatuses: vi.fn(),
  storeIntegrationSecret: vi.fn(),
  deleteIntegrationSecret: vi.fn(),
  updateIntegrationSetting: vi.fn(),
  parseError: (err: unknown) => ({ code: "unknown", message: String(err) }),
}));

describe("IntegrationsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders loading state initially", async () => {
    const { getIntegrationStatuses } = await import("../../lib/ipc");
    (getIntegrationStatuses as ReturnType<typeof vi.fn>).mockReturnValue(new Promise(() => {}));

    render(<IntegrationsPanel />);
    expect(document.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("renders integration cards after loading", async () => {
    const { getIntegrationStatuses } = await import("../../lib/ipc");
    (getIntegrationStatuses as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        id: "jira",
        enabled: true,
        has_secret: true,
        settings: { base_url: "https://test.atlassian.net", email: "user@test.com" },
      },
      {
        id: "linear",
        enabled: false,
        has_secret: false,
        settings: { workspace: "" },
      },
    ]);

    render(<IntegrationsPanel />);

    expect(await screen.findByText("Jira")).toBeInTheDocument();
    expect(screen.getByText("Linear")).toBeInTheDocument();
  });

  it("shows credential status without exposing secrets", async () => {
    const { getIntegrationStatuses } = await import("../../lib/ipc");
    (getIntegrationStatuses as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        id: "jira",
        enabled: true,
        has_secret: true,
        settings: { base_url: "", email: "" },
      },
      {
        id: "linear",
        enabled: true,
        has_secret: false,
        settings: { workspace: "" },
      },
    ]);

    render(<IntegrationsPanel />);

    expect(await screen.findByText("Stored in keychain")).toBeInTheDocument();
    expect(screen.getByText("Not configured")).toBeInTheDocument();
  });

  it("calls updateIntegrationSetting on toggle", async () => {
    const { getIntegrationStatuses, updateIntegrationSetting } = await import("../../lib/ipc");
    (getIntegrationStatuses as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        id: "jira",
        enabled: false,
        has_secret: false,
        settings: { base_url: "", email: "" },
      },
      {
        id: "linear",
        enabled: false,
        has_secret: false,
        settings: { workspace: "" },
      },
    ]);
    (updateIntegrationSetting as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);

    const user = userEvent.setup();
    render(<IntegrationsPanel />);

    await screen.findByText("Jira");

    const jiraToggle = screen.getByRole("switch", { name: "Jira integration" });
    await user.click(jiraToggle);
    expect(updateIntegrationSetting).toHaveBeenCalledWith("integration_jira_enabled", "true");
  });

  it("shows error state when fetch fails", async () => {
    const { getIntegrationStatuses } = await import("../../lib/ipc");
    (getIntegrationStatuses as ReturnType<typeof vi.fn>).mockRejectedValue("Connection failed");

    render(<IntegrationsPanel />);
    expect(await screen.findByText("Connection failed")).toBeInTheDocument();
  });
});
