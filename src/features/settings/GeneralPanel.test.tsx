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
  getProviderSetupCatalog: vi.fn(),
  storeProviderSecret: vi.fn(),
  deleteProviderSecret: vi.fn(),
  probeProviderSetup: vi.fn(),
}));

import {
  getSettings,
  getProviderCredentialStatuses,
  getProviderControlPlane,
  getProviderSetupCatalog,
} from "../../lib/ipc";
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
          setup_state: "ready",
          execution_supported: true,
          release_gate_passed: false,
          currently_runnable: true,
          credential_source: null,
          capabilities: {
            provider_id: "codex",
            display_name: "Codex CLI",
            provider_family: "local_cli",
            transport_family: "shell_execute",
            fit_tags: ["balanced", "fast_scan"],
            billing_risk: "included",
            setup_complexity: "moderate",
            install_source: "manual_cli",
            auth_mode: "local_session",
            permission_model: "host_governed",
            opt_in_only: false,
            in_auto_fallback: true,
            execution_support_tier: "supported",
            conformance_status: "covered",
            eval_status: "planned",
            credential_fields: [],
            interactive_permissions: false,
            default_governance_tier: "read_only",
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: [],
            supported_config_options: [],
            session_capabilities: { list: false, load: false, resume: false, close: false },
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
          setup_state: "needs_auth",
          execution_supported: true,
          release_gate_passed: false,
          currently_runnable: false,
          credential_source: "environment",
          capabilities: {
            provider_id: "gemini",
            display_name: "Gemini CLI",
            provider_family: "cli_bridge",
            transport_family: "acp_stdio_ndjson",
            fit_tags: ["fast_scan", "experimental"],
            billing_risk: "paid_api",
            setup_complexity: "moderate",
            install_source: "acp_registry",
            auth_mode: "api_key",
            permission_model: "deny_by_default",
            opt_in_only: true,
            in_auto_fallback: false,
            execution_support_tier: "supported",
            conformance_status: "covered",
            eval_status: "planned",
            credential_fields: [],
            interactive_permissions: false,
            default_governance_tier: "read_only",
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: ["plan"],
            supported_config_options: [],
            session_capabilities: { list: false, load: false, resume: false, close: false },
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
    vi.mocked(getProviderSetupCatalog).mockResolvedValue({
      providers: [
        {
          provider_id: "gemini",
          display_name: "Gemini CLI",
          provider_family: "cli_bridge",
          setup_state: "needs_auth",
          readiness_reason: "Gemini still needs credentials.",
          support_tier: "supported",
          execution_supported: true,
          release_gate_passed: false,
          currently_runnable: false,
          credential_source: "environment",
          capabilities: {
            provider_id: "gemini",
            display_name: "Gemini CLI",
            provider_family: "cli_bridge",
            transport_family: "acp_stdio_ndjson",
            fit_tags: ["fast_scan", "experimental"],
            billing_risk: "paid_api",
            setup_complexity: "moderate",
            install_source: "acp_registry",
            auth_mode: "api_key",
            permission_model: "deny_by_default",
            opt_in_only: true,
            in_auto_fallback: false,
            execution_support_tier: "supported",
            conformance_status: "covered",
            eval_status: "planned",
            credential_fields: [],
            interactive_permissions: false,
            default_governance_tier: "read_only",
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: ["plan"],
            supported_config_options: [],
            session_capabilities: { list: false, load: false, resume: false, close: false },
          },
          registry: {
            registry_id: "gemini-cli",
            latest_version: "1.0.0",
            install_source: "acp_registry",
            distribution_channel: "acp_registry",
            install_command: "npm i -g @google/gemini-cli",
            install_url: "https://example.com/install",
            docs_url: "https://example.com/docs",
            auth_docs_url: "https://example.com/auth",
            config_options: [
              {
                id: "mode",
                name: "Session Mode",
                option_type: "select",
                current_value: null,
                options: [{ value: "plan", label: "plan", description: null }],
              },
            ],
            supported_modes: ["plan"],
            session_capabilities: { list: false, load: false, resume: false, close: false },
            setup_notes: [],
          },
          actions: [
            {
              id: "docs",
              label: "Provider docs",
              kind: "open_docs",
              enabled: true,
              command_preview: null,
              url: "https://example.com/docs",
            },
          ],
          warnings: [],
        },
      ],
      registry_fetched_at: "2026-05-16T10:00:00Z",
      registry_source: "seed",
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
    expect(screen.getAllByText("Gemini CLI").length).toBeGreaterThan(0);
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
