import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GeneralPanel } from "./GeneralPanel";

vi.mock("../../lib/ipc", () => ({
  getSettings: vi.fn(),
  updateSetting: vi.fn(),
  parseError: (err: unknown) => ({ code: "unknown", message: String(err) }),
  getProviderCredentialStatuses: vi.fn(),
  getProviderControlPlane: vi.fn(),
  storeProviderSecret: vi.fn(),
  deleteProviderSecret: vi.fn(),
}));

import { getSettings, getProviderCredentialStatuses, getProviderControlPlane } from "../../lib/ipc";
import { storeProviderSecret } from "../../lib/ipc";

describe("GeneralPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getSettings).mockResolvedValue({
      preferred_provider: "auto",
      max_surface_findings: "20",
      similarity_threshold: "0.85",
      drop_nitpicks: "false",
      min_confidence: "0.5",
      lane_timeout_secs: "120",
    });
    vi.mocked(getProviderCredentialStatuses).mockResolvedValue([]);
    vi.mocked(getProviderControlPlane).mockResolvedValue({
      providers: [
        {
          provider_id: "codex",
          display_name: "Codex CLI",
          status: "ready",
          status_reason: "Ready for review runs",
          credential_source: null,
          capabilities: {
            provider_id: "codex",
            display_name: "Codex CLI",
            provider_family: "local_cli",
            fit_tags: ["balanced", "fast_scan"],
            billing_risk: "included",
            setup_complexity: "moderate",
            opt_in_only: false,
            in_auto_fallback: true,
            credential_fields: [],
            interactive_permissions: false,
            default_governance_tier: "read_only",
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
          },
          recent_metrics: {
            sample_count: 3,
            avg_latency_ms: 900,
            avg_accept_rate: 0.7,
            avg_edit_rate: 0.1,
            avg_suppress_rate: 0.1,
            avg_anchor_validity: 1,
            avg_cost_usd: null,
          },
          fit_narrative: "Ready now.",
          recommended_default: true,
          warnings: [],
        },
        {
          provider_id: "gemini",
          display_name: "Gemini CLI",
          status: "degraded",
          status_reason: "Available but opt-in only",
          credential_source: "environment",
          capabilities: {
            provider_id: "gemini",
            display_name: "Gemini CLI",
            provider_family: "cli_bridge",
            fit_tags: ["fast_scan", "experimental"],
            billing_risk: "paid_api",
            setup_complexity: "moderate",
            opt_in_only: true,
            in_auto_fallback: false,
            credential_fields: [],
            interactive_permissions: false,
            default_governance_tier: "read_only",
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
          },
          recent_metrics: {
            sample_count: 1,
            avg_latency_ms: 1200,
            avg_accept_rate: 0.4,
            avg_edit_rate: 0,
            avg_suppress_rate: 0.2,
            avg_anchor_validity: 1,
            avg_cost_usd: 0.75,
          },
          fit_narrative: "Review setup details.",
          recommended_default: false,
          warnings: ["May incur paid usage."],
        },
      ],
      recommended_provider_id: "codex",
      recommendation_reason: "Codex CLI is ready now. It fits the current auto-routing policy.",
      preferred_provider: "auto",
      workspace_id: null,
      recent_window_size: 20,
      generated_at: "2026-05-16T10:00:00Z",
    });
  });

  it("renders recommended default and provider comparison data", async () => {
    render(<GeneralPanel />);

    await waitFor(() => expect(screen.getByText("Recommended default")).toBeInTheDocument());

    expect(
      screen.getByText("Codex CLI is ready now. It fits the current auto-routing policy."),
    ).toBeInTheDocument();
    expect(screen.getByText("Codex CLI")).toBeInTheDocument();
    expect(screen.getByText("Gemini CLI")).toBeInTheDocument();
    expect(screen.getByText("May incur paid usage.")).toBeInTheDocument();
  });

  it("refreshes provider control after storing a credential", async () => {
    const user = userEvent.setup();
    vi.mocked(getProviderCredentialStatuses)
      .mockResolvedValueOnce([
        {
          field: "anthropic_api_key",
          source: "none",
          provider_ids: ["claude"],
        },
      ])
      .mockResolvedValueOnce([
        {
          field: "anthropic_api_key",
          source: "keychain",
          provider_ids: ["claude"],
        },
      ]);
    vi.mocked(storeProviderSecret).mockResolvedValue(undefined);

    render(<GeneralPanel />);

    const input = await screen.findByPlaceholderText("sk-ant-...");
    await user.type(input, "sk-ant-test");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(storeProviderSecret).toHaveBeenCalled());
    await waitFor(() => expect(getProviderControlPlane).toHaveBeenCalledTimes(2));
  });
});
