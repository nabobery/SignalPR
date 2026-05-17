import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SessionDrawer } from "./SessionDrawer";

vi.mock("../../lib/ipc", () => ({
  getAgentRunMetadata: vi.fn(),
  getProviderCapabilities: vi.fn(),
  parseError: (err: unknown) => ({ code: "unknown", message: String(err) }),
}));

import { getAgentRunMetadata, getProviderCapabilities } from "../../lib/ipc";

describe("SessionDrawer", () => {
  beforeEach(() => {
    vi.mocked(getAgentRunMetadata).mockResolvedValue({
      runs: [
        {
          id: "agent-run-1",
          review_run_id: "run-1",
          lane_id: "security",
          provider_name: "codex-app-server",
          governance_tier_at_run: "guarded_write",
          provider_session_id: "sess-123",
          resume_cursor: null,
          checkpoint_metadata_json: null,
          cost_usd: 0.25,
          started_at: "2026-05-16T10:00:00Z",
          completed_at: "2026-05-16T10:01:00Z",
          status: "completed",
          finding_count: 2,
        },
      ],
      provider_selection: {
        requested_provider: "claude",
        selected_provider: "codex_app_server",
        selection_mode: "fallback",
        checks: [
          {
            provider_id: "claude",
            available: false,
            reason: "unhealthy",
            message: "missing key",
          },
          {
            provider_id: "codex_app_server",
            available: true,
            reason: "selected",
            message: null,
          },
        ],
        warnings: [
          "Requested provider 'claude' could not run (missing key), so SignalPR selected 'codex_app_server'.",
        ],
      },
    });
    vi.mocked(getProviderCapabilities).mockResolvedValue([
      {
        provider_id: "codex_app_server",
        display_name: "Codex App Server",
        provider_family: "managed_local_agent",
        transport_family: "jsonrpc_stdio",
        fit_tags: ["balanced"],
        billing_risk: "included",
        setup_complexity: "moderate",
        install_source: "bundled_app",
        auth_mode: "desktop_session",
        permission_model: "interactive_approval",
        opt_in_only: false,
        in_auto_fallback: true,
        selection_eligibility: "auto_allowed",
        execution_support_tier: "supported",
        conformance_status: "covered",
        eval_status: "planned",
        credential_fields: [],
        interactive_permissions: true,
        default_governance_tier: "guarded_write",
        supports_session_resume: false,
        supports_checkpointing: false,
        paid_eval_eligible: false,
        supported_session_modes: [],
        supported_config_options: [],
        session_capabilities: { list: false, load: false, resume: false, close: false },
      },
    ]);
  });

  it("shows provider selection trace inside the drawer", async () => {
    const user = userEvent.setup();
    render(<SessionDrawer runId="run-1" />);

    await waitFor(() => expect(screen.getByText("Session Info")).toBeInTheDocument());
    await user.click(screen.getByText("Session Info"));

    expect(screen.getByText("Provider selection")).toBeInTheDocument();
    expect(screen.getAllByText(/Requested/)[0]).toHaveTextContent(
      "Requested claude, selected codex_app_server",
    );
    expect(screen.getByText(/could not run/)).toBeInTheDocument();
    expect(screen.getByText(/health check failed: missing key/)).toBeInTheDocument();
  });
});
