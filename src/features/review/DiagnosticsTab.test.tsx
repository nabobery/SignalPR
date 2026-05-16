import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DiagnosticsTab } from "./DiagnosticsTab";
import type { ContextPackSummary, LocalChecksSummary, PlatformCapabilities } from "../../lib/types";

const mockGetEnvironmentSummary = vi.fn();
const mockGetEventLog = vi.fn();
const mockRefreshPrMetadata = vi.fn();

vi.mock("../../lib/ipc", () => ({
  getEnvironmentSummary: (...args: unknown[]) => mockGetEnvironmentSummary(...args),
  getEventLog: (...args: unknown[]) => mockGetEventLog(...args),
  refreshPrMetadata: (...args: unknown[]) => mockRefreshPrMetadata(...args),
  parseError: (err: unknown) => ({ code: "unknown", message: String(err) }),
}));

describe("DiagnosticsTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetEnvironmentSummary.mockResolvedValue({
      can_review: true,
      can_submit: true,
      available_providers: ["codex"],
      warnings: [],
      tools: [
        {
          tool_name: "gitlab_token",
          status: "ready",
          version: null,
          message: "GITLAB_TOKEN set",
          checked_at: "2026-05-16T10:00:00Z",
        },
      ],
    });
  });

  it("renders event entries after loading", async () => {
    mockGetEventLog.mockResolvedValue([
      {
        timestamp: "2026-05-01T10:00:00Z",
        event_type: "lane_call_started",
        payload: { lane_id: "security", provider: "codex" },
      },
      {
        timestamp: "2026-05-01T10:00:05Z",
        event_type: "lane_call_completed",
        payload: { lane_id: "security", duration_ms: 5000 },
      },
    ]);

    render(<DiagnosticsTab runId="run-1" />);

    expect(await screen.findByText("lane_call_started")).toBeInTheDocument();
    expect(screen.getByText("lane_call_completed")).toBeInTheDocument();
    expect(screen.getByText("(2 events)")).toBeInTheDocument();
  });

  it("shows loading state initially", async () => {
    mockGetEventLog.mockReturnValue(new Promise(() => {}));

    render(<DiagnosticsTab runId="run-1" />);
    expect(document.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("shows error when event log fetch fails", async () => {
    mockGetEventLog.mockRejectedValue("Network error");

    render(<DiagnosticsTab runId="run-1" />);
    expect(await screen.findByText("Network error")).toBeInTheDocument();
  });

  it("filters events by event_type", async () => {
    mockGetEventLog.mockResolvedValue([
      {
        timestamp: "2026-05-01T10:00:00Z",
        event_type: "lane_call_started",
        payload: {},
      },
      {
        timestamp: "2026-05-01T10:00:05Z",
        event_type: "review_ready",
        payload: { surfaced_count: 3 },
      },
    ]);

    const user = userEvent.setup();
    render(<DiagnosticsTab runId="run-1" />);

    await screen.findByText("lane_call_started");

    const filter = screen.getByPlaceholderText(/filter/i);
    await user.type(filter, "ready");

    expect(screen.queryByText("lane_call_started")).not.toBeInTheDocument();
    expect(screen.getByText("review_ready")).toBeInTheDocument();
    expect(screen.getByText("(1 events)")).toBeInTheDocument();
  });

  it("shows empty state when no events exist", async () => {
    mockGetEventLog.mockResolvedValue([]);

    render(<DiagnosticsTab runId="run-1" />);
    expect(await screen.findByText("No events recorded for this run.")).toBeInTheDocument();
  });

  it("renders Context Pack section when summary is provided", async () => {
    mockGetEventLog.mockResolvedValue([]);

    const contextPack: ContextPackSummary = {
      total_bytes: 2048,
      item_count: 2,
      prompt_suffix: "",
      items: [
        { kind: "doc", label: "README.md", source: "/ws/README.md", bytes: 1024, included: true },
        {
          kind: "doc",
          label: "../../secret",
          source: "",
          bytes: 0,
          included: false,
          omit_reason: "outside_workspace",
        },
      ],
    };

    render(<DiagnosticsTab runId="run-1" contextPackSummary={contextPack} />);

    expect(await screen.findByText("Context pack evidence")).toBeInTheDocument();
    expect(screen.getByText(/1 item, 2\.0KB/)).toBeInTheDocument();
  });

  it("renders Local Checks section when summary is provided", async () => {
    mockGetEventLog.mockResolvedValue([]);

    const localChecks: LocalChecksSummary = {
      total_errors: 3,
      included_count: 3,
      tools_run: ["oxlint"],
      items: [
        {
          tool: "oxlint",
          file: "src/app.tsx",
          line: 10,
          column: 5,
          severity: "error",
          message: "Unused var",
          rule_id: "no-unused-vars",
        },
      ],
    };

    render(<DiagnosticsTab runId="run-1" localChecksSummary={localChecks} />);

    expect(await screen.findByText("Local check evidence")).toBeInTheDocument();
    expect(screen.getByText(/3 errors via oxlint/)).toBeInTheDocument();
  });

  it("expands event payload on click", async () => {
    mockGetEventLog.mockResolvedValue([
      {
        timestamp: "2026-05-01T10:00:00Z",
        event_type: "lane_call_completed",
        payload: { duration_ms: 5000, lane_id: "security" },
      },
    ]);

    const user = userEvent.setup();
    render(<DiagnosticsTab runId="run-1" />);

    await screen.findByText("lane_call_completed");
    await user.click(screen.getByText("lane_call_completed"));

    expect(screen.getByText(/"duration_ms": 5000/)).toBeInTheDocument();
  });

  it("renders Issue Context section when context pack has issue items", async () => {
    mockGetEventLog.mockResolvedValue([]);

    const contextPack: ContextPackSummary = {
      total_bytes: 3000,
      item_count: 3,
      prompt_suffix: "",
      items: [
        {
          kind: "issue",
          label: "Issue #42",
          source: "github:issue:https://github.com/acme/web/issues/42",
          bytes: 500,
          included: true,
          confidence: "high",
        },
        {
          kind: "issue",
          label: "Issue AUTH-123",
          source: "jira:issue:AUTH-123",
          bytes: 600,
          included: true,
          confidence: "medium",
        },
        {
          kind: "issue",
          label: "Issue ENG-99",
          source: "linear:issue:ENG-99",
          bytes: 0,
          included: false,
          omit_reason: "budget_exceeded",
          confidence: "high",
        },
      ],
    };

    render(<DiagnosticsTab runId="run-1" contextPackSummary={contextPack} />);

    expect(await screen.findByText("Issue context evidence")).toBeInTheDocument();
    expect(screen.getByText(/2 included/)).toBeInTheDocument();
    expect(screen.getByText(/1 omitted/)).toBeInTheDocument();
  });

  it("does not render Issue Context section when no issue items exist", async () => {
    mockGetEventLog.mockResolvedValue([]);

    const contextPack: ContextPackSummary = {
      total_bytes: 1000,
      item_count: 1,
      prompt_suffix: "",
      items: [
        { kind: "doc", label: "README.md", source: "/ws/README.md", bytes: 1000, included: true },
      ],
    };

    render(<DiagnosticsTab runId="run-1" contextPackSummary={contextPack} />);

    await screen.findByText("Context pack evidence");
    expect(screen.queryByText("Issue context evidence")).not.toBeInTheDocument();
  });

  it("renders evidence trail header", async () => {
    mockGetEventLog.mockResolvedValue([]);

    render(<DiagnosticsTab runId="run-1" />);

    expect(await screen.findByText("Evidence trail")).toBeInTheDocument();
  });

  it("renders platform capability diagnostics", async () => {
    mockGetEventLog.mockResolvedValue([]);

    const capabilities: PlatformCapabilities = {
      platform: "gitlab",
      capabilities: [
        {
          key: "pr_metadata",
          support: "full",
          constraints: [],
          fallback: null,
        },
        {
          key: "request_changes_review",
          support: "partial",
          constraints: [
            {
              code: "maps_to_unapprove",
              message: "Request changes currently maps to an approval removal flow.",
            },
          ],
          fallback: null,
        },
        {
          key: "pending_comment_batch",
          support: "none",
          constraints: [
            {
              code: "unsupported_pending_batch",
              message: "Pending review batches are not supported on this platform.",
            },
          ],
          fallback: {
            action: "body_only_summary",
            reason: "SignalPR will collapse these findings into the review body.",
          },
        },
      ],
    };

    const user = userEvent.setup();
    render(
      <DiagnosticsTab
        runId="run-1"
        platformCapabilities={capabilities}
        platformCapabilitiesFetchedAt="2026-05-16T10:00:00Z"
      />,
    );

    expect(await screen.findByText(/1 full, 1 partial, 1 blocked/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Platform capabilities/i }));

    expect(screen.getByText("Auth ready")).toBeInTheDocument();
    expect(screen.getByText("request_changes_review")).toBeInTheDocument();
    expect(
      screen.getByText("Request changes currently maps to an approval removal flow."),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Fallback: body_only_summary · SignalPR will collapse these findings/),
    ).toBeInTheDocument();
  });

  it("renders auth diagnostics when platform auth is not ready", async () => {
    mockGetEventLog.mockResolvedValue([]);
    mockGetEnvironmentSummary.mockResolvedValue({
      can_review: true,
      can_submit: false,
      available_providers: ["codex"],
      warnings: ["No submit path ready"],
      tools: [
        {
          tool_name: "gitlab_token",
          status: "missing",
          version: null,
          message: "Optional: Set GITLAB_TOKEN for GitLab MR submission",
          checked_at: "2026-05-16T10:00:00Z",
        },
      ],
    });

    render(
      <DiagnosticsTab
        runId="run-1"
        platformCapabilities={{
          platform: "gitlab",
          capabilities: [{ key: "pr_metadata", support: "full", constraints: [], fallback: null }],
        }}
      />,
    );

    await screen.findByText(/1 full, 0 partial, 0 blocked/);
    await userEvent.click(screen.getByRole("button", { name: /Platform capabilities/i }));

    expect(screen.getByText("Auth not ready")).toBeInTheDocument();
    expect(
      screen.getByText("Optional: Set GITLAB_TOKEN for GitLab MR submission"),
    ).toBeInTheDocument();
  });
});
